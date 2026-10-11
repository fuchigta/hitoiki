//! プッシュ評価の入力組み立てと 1 回分の評価（docs/dev/architecture.md「プッシュ通知」）。
//!
//! `build_context` は DB を触らない純粋関数で、時刻は引数で受ける。

use crate::sink::RuntimeSink;
use chrono_tz::Tz;
use hitoiki_core::config::Config;
use hitoiki_core::model::{
    ActivitySegment, CalendarEvent, IdleSpan, PushLog, SegmentStatus, TimeBlock, WindowSpan,
};
use hitoiki_core::push::{evaluate, store, IdleState, PushContext, PushDecision};
use hitoiki_core::repo::{calendar, raw, segment};
use hitoiki_core::timefmt::{day_range, local_date_of, week_range, Range};
use hitoiki_core::{Millis, Result};
use rusqlite::Connection;

const HOUR_MS: Millis = 3_600_000;

/// `PushContext` の材料（DB から読んだ生の行）。
#[derive(Debug, Clone, Default)]
pub struct PushSnapshot {
    /// 当日分（少なくとも直近 3 時間）の区間。
    pub segments: Vec<ActivitySegment>,
    /// 直近の無操作区間（進行中を含む）。
    pub idle_spans: Vec<IdleSpan>,
    /// 直近の前面ウインドウ区間（進行中を含む）。
    pub window_spans: Vec<WindowSpan>,
    pub time_blocks: Vec<TimeBlock>,
    pub calendar_events: Vec<CalendarEvent>,
    pub push_log: Vec<PushLog>,
    pub needs_review_count: u32,
}

/// スナップショットから `PushContext` を作る（純粋関数）。
///
/// - idle: 進行中（end_at なし）の無操作区間があれば無操作中。経過は区間の開始（最終入力）から。
/// - resumed_at: 終了済みの無操作区間（ロック・スリープ含む）の最も新しい終了時刻。
/// - 前面 exe: 進行中の window_span のうち最も新しいものの exe 名。
pub fn build_context(snap: PushSnapshot, cfg: &Config, now: Millis, tz: Tz) -> PushContext {
    let mut ctx = PushContext::new(now, tz);

    let open_idle = snap
        .idle_spans
        .iter()
        .filter(|s| s.end_at.is_none() && s.start_at <= now)
        .max_by_key(|s| s.start_at);
    ctx.idle = match open_idle {
        Some(s) => IdleState {
            is_idle: true,
            since_last_input_sec: ((now - s.start_at).max(0) / 1000) as u64,
        },
        None => IdleState::default(),
    };
    ctx.resumed_at = snap
        .idle_spans
        .iter()
        .filter_map(|s| s.end_at)
        .filter(|&e| e <= now)
        .max();
    ctx.foreground_exe = snap
        .window_spans
        .iter()
        .filter(|w| w.end_at.is_none() && w.start_at <= now)
        .max_by_key(|w| w.start_at)
        .and_then(|w| w.exe_name.clone());
    ctx.meeting_exes = cfg.collect.meeting_exes.clone();
    ctx.recent_segments = snap.segments;
    ctx.time_blocks = snap.time_blocks;
    ctx.calendar_events = snap.calendar_events;
    ctx.push_log = snap.push_log;
    ctx.needs_review_count = snap.needs_review_count;
    ctx
}

/// DB から評価に必要な行を読む。
/// `review_min_sec` 未満の短い要確認区間は `needs_review_count` に含めない（仕分け一覧の既定表示と同じ数え方）。
pub fn load_snapshot(conn: &Connection, now: Millis, tz: Tz, review_min_sec: u64) -> Result<PushSnapshot> {
    let today = day_range(tz, local_date_of(tz, now));
    // 「当日分」かつ少なくとも直近 3 時間。
    let seg_range = Range::new(today.start.min(now - 3 * HOUR_MS), now + 1);
    let segments = segment::list(conn, seg_range)?;
    let review_min_ms = review_min_sec as i64 * 1000;
    let needs_review_count = segment::list_by_status(conn, today, SegmentStatus::NeedsReview)?
        .iter()
        .filter(|s| s.end_at - s.start_at >= review_min_ms)
        .count() as u32;
    let week = week_range(tz, local_date_of(tz, now));
    Ok(PushSnapshot {
        segments,
        idle_spans: raw::idle_spans(conn, Range::new(now - 24 * HOUR_MS, now + 1))?,
        window_spans: raw::window_spans(conn, Range::new(now - HOUR_MS, now + 1))?,
        time_blocks: raw::time_blocks(conn, today)?,
        calendar_events: calendar::list(conn, Range::new(today.start - HOUR_MS, today.end + HOUR_MS))?,
        push_log: store::load_recent(conn, week.start, 10)?,
        needs_review_count,
    })
}

/// 評価を 1 回行い、結果を push_log に残し、Deliver を sink に渡す。
/// Defer は記録せず、次回の評価で再判定される（`evaluate` は毎回再判定する）。
pub fn push_tick(
    conn: &Connection,
    cfg: &Config,
    now: Millis,
    sink: &dyn RuntimeSink,
) -> Result<Vec<PushDecision>> {
    if !cfg.push.enabled {
        return Ok(vec![]);
    }
    let tz = cfg.tz()?;
    let ctx = build_context(load_snapshot(conn, now, tz, cfg.review.min_sec)?, cfg, now, tz);
    let decisions = evaluate(&ctx, &cfg.push);
    for d in &decisions {
        let id = store::record_decision(conn, d, now)?;
        if let (PushDecision::Deliver(payload), Some(id)) = (d, id) {
            sink.push(&id, payload);
        }
    }
    Ok(decisions)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sink::LogSink;
    use hitoiki_core::db;
    use hitoiki_core::model::IdleReason;
    use hitoiki_core::push::PushPayload;
    use hitoiki_core::repo::collect as rc;
    use std::sync::Mutex;

    const MIN: Millis = 60_000;
    /// 2025-01-06 (月) 00:00 UTC。
    const DAY0: Millis = 1_736_121_600_000;

    fn cfg() -> Config {
        let mut c = Config::default();
        c.general.timezone = "UTC".into();
        c
    }

    fn idle(id: &str, start: Millis, end: Option<Millis>) -> IdleSpan {
        IdleSpan {
            id: id.into(),
            start_at: start,
            end_at: end,
            reason: IdleReason::NoInput,
        }
    }

    fn win(id: &str, start: Millis, end: Option<Millis>, exe: &str) -> WindowSpan {
        WindowSpan {
            id: id.into(),
            start_at: start,
            end_at: end,
            hwnd: Some("1".into()),
            pid: None,
            exe_path: None,
            exe_name: Some(exe.into()),
            title: None,
            excluded: false,
        }
    }

    #[test]
    fn idle_open_span_means_idle() {
        let snap = PushSnapshot {
            idle_spans: vec![idle("a", DAY0 + 10 * MIN, None)],
            ..Default::default()
        };
        let ctx = build_context(snap, &cfg(), DAY0 + 15 * MIN, chrono_tz::UTC);
        assert!(ctx.idle.is_idle);
        assert_eq!(ctx.idle.since_last_input_sec, 300);
        assert_eq!(ctx.resumed_at, None);
    }

    #[test]
    fn closed_idle_gives_resumed_at_latest() {
        let snap = PushSnapshot {
            idle_spans: vec![
                idle("a", DAY0, Some(DAY0 + 5 * MIN)),
                idle("b", DAY0 + 20 * MIN, Some(DAY0 + 30 * MIN)),
                // 未来の終了は無視する
                idle("c", DAY0 + 40 * MIN, Some(DAY0 + 90 * MIN)),
            ],
            ..Default::default()
        };
        let ctx = build_context(snap, &cfg(), DAY0 + 60 * MIN, chrono_tz::UTC);
        assert!(!ctx.idle.is_idle);
        assert_eq!(ctx.resumed_at, Some(DAY0 + 30 * MIN));
    }

    #[test]
    fn foreground_is_newest_open_window() {
        let snap = PushSnapshot {
            window_spans: vec![
                win("a", DAY0, Some(DAY0 + MIN), "old.exe"),
                win("b", DAY0 + 2 * MIN, None, "Zoom.exe"),
            ],
            ..Default::default()
        };
        let ctx = build_context(snap, &cfg(), DAY0 + 10 * MIN, chrono_tz::UTC);
        assert_eq!(ctx.foreground_exe.as_deref(), Some("Zoom.exe"));
        assert!(ctx.meeting_exes.iter().any(|e| e == "Zoom.exe"));
    }

    #[derive(Default)]
    struct Rec(Mutex<Vec<(String, String)>>);
    impl RuntimeSink for Rec {
        fn segments_updated(&self, _: Range) {}
        fn push(&self, id: &str, p: &PushPayload) {
            self.0.lock().unwrap().push((id.into(), p.trigger.to_string()));
        }
        fn doing_changed(&self, _: Option<&str>) {}
        fn import_completed(&self, _: &hitoiki_agents::ImportStats) {}
        fn open_ui(&self, _: Option<&str>) {}
        fn shutdown_requested(&self) {}
    }

    #[test]
    fn tick_delivers_day_start_once_and_logs() {
        let conn = db::open_in_memory().unwrap();
        // 09:00 UTC に今日のロック解除（無操作明け）があった状況。
        rc::open_idle_span(&conn, &idle("i1", DAY0 + 8 * 60 * MIN, None)).unwrap();
        rc::close_idle_span(&conn, "i1", DAY0 + 9 * 60 * MIN).unwrap();
        let sink = Rec::default();
        let now = DAY0 + 9 * 60 * MIN + 5 * MIN;

        let d = push_tick(&conn, &cfg(), now, &sink).unwrap();
        assert!(d.iter().any(|d| matches!(d, PushDecision::Deliver(_))));
        let got = sink.0.lock().unwrap().clone();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].1, "day_start");
        // push_log に配信済みで残り、push_id で引ける。
        assert!(store::get_log(&conn, &got[0].0).unwrap().delivered);

        // 直後の再評価では出ない。
        let before = sink.0.lock().unwrap().len();
        push_tick(&conn, &cfg(), now + MIN, &sink).unwrap();
        assert_eq!(sink.0.lock().unwrap().len(), before);
    }

    #[test]
    fn disabled_push_does_nothing() {
        let conn = db::open_in_memory().unwrap();
        let mut c = cfg();
        c.push.enabled = false;
        assert!(push_tick(&conn, &c, DAY0, &LogSink).unwrap().is_empty());
    }
}
