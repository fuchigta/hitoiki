//! プッシュ評価の入力組み立てと 1 回分の評価（docs/dev/architecture.md「プッシュ通知」）。
//!
//! `build_context` は DB を触らない純粋関数で、時刻は引数で受ける。

use crate::sink::RuntimeSink;
use chrono_tz::Tz;
use hitoiki_core::config::Config;
use hitoiki_core::model::{
    ActivitySegment, CalendarEvent, IdleSpan, PushLog, SegmentStatus, TimeBlock, WindowSpan,
};
use hitoiki_core::push::{
    evaluate, store, ClosedIdle, DoingSpan, IdleState, KrIdleNudge, PushContext, PushDecision, TaskNudge,
    TaskRef,
};
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
    /// 緊急・重要のしるしが付いた未完了のタスク。
    pub tasks: Vec<TaskNudge>,
    /// 今週まだ時間が入っていない成果の指標（kr_idle の材料。`push_tick` が曜日に達したときだけ読む）。
    pub kr_idle: Vec<KrIdleNudge>,
    /// 今日の time_block と直近の doing ログが指すタスク。
    pub task_refs: Vec<TaskRef>,
    /// 直近の doing ログ（進行中を含む）。
    pub doing_spans: Vec<DoingSpan>,
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
    ctx.last_idle = last_closed_idle(&snap.idle_spans, now);
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
    ctx.tasks = snap.tasks;
    ctx.kr_idle = snap.kr_idle;
    ctx.task_refs = snap.task_refs;
    ctx.doing_spans = snap.doing_spans;
    ctx
}

/// 隣り合う無操作の記録（無操作 → ロックなど）をつなぐ最大の隙間。
const IDLE_JOIN_GAP: Millis = 10_000;

/// 終了済みの無操作区間を（重なり・ほぼ隣接なら）つないで、now までに終わった最も新しいものを返す。
fn last_closed_idle(spans: &[IdleSpan], now: Millis) -> Option<ClosedIdle> {
    let mut v: Vec<(Millis, Millis)> = spans
        .iter()
        .filter_map(|s| s.end_at.map(|e| (s.start_at, e)))
        .filter(|&(s, e)| s < e && e <= now)
        .collect();
    v.sort_unstable();
    let mut merged: Vec<(Millis, Millis)> = Vec::new();
    for (s, e) in v {
        match merged.last_mut() {
            Some(l) if s <= l.1 + IDLE_JOIN_GAP => l.1 = l.1.max(e),
            _ => merged.push((s, e)),
        }
    }
    merged
        .pop()
        .map(|(start_at, end_at)| ClosedIdle { start_at, end_at })
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
    let time_blocks = raw::time_blocks(conn, today)?;
    let doing_spans: Vec<DoingSpan> = raw::task_doing_logs(conn, Range::new(now - 24 * HOUR_MS, now + 1))?
        .into_iter()
        .map(|d| DoingSpan {
            task_id: d.task_id,
            start_at: d.start_at,
            end_at: d.end_at,
        })
        .collect();
    let task_refs = store::load_task_refs(conn, &time_blocks, &doing_spans)?;
    Ok(PushSnapshot {
        segments,
        idle_spans: raw::idle_spans(conn, Range::new(now - 24 * HOUR_MS, now + 1))?,
        window_spans: raw::window_spans(conn, Range::new(now - HOUR_MS, now + 1))?,
        time_blocks,
        calendar_events: calendar::list(conn, Range::new(today.start - HOUR_MS, today.end + HOUR_MS))?,
        push_log: store::load_recent(conn, week.start, 10)?,
        needs_review_count,
        tasks: store::load_task_nudges(conn)?,
        kr_idle: vec![],
        task_refs,
        doing_spans,
    })
}

/// kr_idle の材料を読む必要があるか（無効でなく、設定の曜日に達していて、今週まだ配信していない）。
/// 毎回の評価で集計し直さないための絞り込みで、最終的な判定は `evaluate`（トリガー側）が行う。
fn kr_idle_due(cfg: &Config, now: Millis, tz: Tz, push_log: &[PushLog]) -> bool {
    hitoiki_core::push::kr_idle_due(&cfg.push, tz, now, push_log)
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
    let mut snap = load_snapshot(conn, now, tz, cfg.review.min_sec)?;
    if kr_idle_due(cfg, now, tz, &snap.push_log) {
        snap.kr_idle = store::load_kr_idle(conn, tz, now)?;
    }
    let ctx = build_context(snap, cfg, now, tz);
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
    use hitoiki_core::model::{IdleReason, SegmentKind, Task, TaskKind, TaskStatus};
    use hitoiki_core::push::PushPayload;
    use hitoiki_core::repo::collect as rc;
    use hitoiki_core::repo::segment;
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
    fn last_idle_joins_adjacent_spans_and_ignores_open_and_future() {
        let mut lock = idle("b", DAY0 + 13 * MIN, Some(DAY0 + 40 * MIN));
        lock.reason = IdleReason::Locked;
        let snap = PushSnapshot {
            idle_spans: vec![
                idle("old", DAY0, Some(DAY0 + 2 * MIN)),
                idle("a", DAY0 + 10 * MIN, Some(DAY0 + 13 * MIN)),
                lock,
                // 未来の終了・進行中は対象外
                idle("fut", DAY0 + 50 * MIN, Some(DAY0 + 90 * MIN)),
            ],
            ..Default::default()
        };
        let ctx = build_context(snap, &cfg(), DAY0 + 45 * MIN, chrono_tz::UTC);
        let li = ctx.last_idle.unwrap();
        assert_eq!((li.start_at, li.end_at), (DAY0 + 10 * MIN, DAY0 + 40 * MIN));
        assert_eq!(
            build_context(PushSnapshot::default(), &cfg(), DAY0, chrono_tz::UTC).last_idle,
            None
        );
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

    fn task(id: &str, status: TaskStatus, urgent: Option<bool>, important: Option<bool>) -> Task {
        Task {
            id: id.into(),
            title: format!("タスク{id}"),
            body_md: String::new(),
            status,
            kind: TaskKind::Normal,
            urgent,
            important,
            estimate_min: None,
            planned_start_at: None,
            planned_end_at: None,
            key_result_id: None,
            external_url: None,
            created_at: DAY0 - 1000,
            updated_at: DAY0 - 1000,
            done_at: None,
        }
    }

    fn work_seg(id: &str, task: &str, kind: SegmentKind, start: Millis, end: Millis) -> ActivitySegment {
        ActivitySegment {
            id: id.into(),
            start_at: start,
            end_at: end,
            kind,
            task_id: Some(task.into()),
            activity_label: None,
            confidence: None,
            decided_by: None,
            status: SegmentStatus::Auto,
            window_span_ids: vec![],
            agent_session_id: None,
            exe_name: None,
            title: None,
            offline: false,
        }
    }

    #[test]
    fn snapshot_reads_flagged_open_tasks_with_last_worked() {
        let conn = db::open_in_memory().unwrap();
        for t in [
            task("urgent", TaskStatus::Todo, Some(true), None),
            task("imp", TaskStatus::Doing, Some(false), Some(true)),
            task("plain", TaskStatus::Todo, None, Some(false)),
            task("done", TaskStatus::Done, Some(true), Some(true)),
            task("dropped", TaskStatus::Dropped, None, Some(true)),
        ] {
            hitoiki_core::repo::task::insert(&conn, &t).unwrap();
        }
        let now = DAY0 + 10 * 60 * MIN;
        // imp に作業系 2 件（最後は 9:00）と休憩 1 件、done に作業。
        segment::replace_range(
            &conn,
            Range::new(DAY0, now),
            &[
                work_seg("s1", "imp", SegmentKind::Work, DAY0 + 60 * MIN, DAY0 + 90 * MIN),
                work_seg(
                    "s2",
                    "imp",
                    SegmentKind::AgentWatch,
                    DAY0 + 8 * 60 * MIN,
                    DAY0 + 9 * 60 * MIN,
                ),
                work_seg(
                    "s3",
                    "imp",
                    SegmentKind::Break,
                    DAY0 + 9 * 60 * MIN,
                    DAY0 + 9 * 60 * MIN + 30 * MIN,
                ),
                work_seg(
                    "s4",
                    "done",
                    SegmentKind::Work,
                    DAY0 + 2 * 60 * MIN,
                    DAY0 + 3 * 60 * MIN,
                ),
            ],
        )
        .unwrap();
        let snap = load_snapshot(&conn, now, chrono_tz::UTC, 0).unwrap();
        let mut ids: Vec<_> = snap.tasks.iter().map(|t| t.id.as_str()).collect();
        ids.sort_unstable();
        assert_eq!(ids, vec!["imp", "urgent"]);
        let imp = snap.tasks.iter().find(|t| t.id == "imp").unwrap();
        assert_eq!(imp.last_worked_at, Some(DAY0 + 9 * 60 * MIN));
        assert!(imp.doing && imp.important && !imp.urgent);
        let urgent = snap.tasks.iter().find(|t| t.id == "urgent").unwrap();
        assert_eq!(urgent.last_worked_at, None);
        assert!(urgent.urgent && !urgent.doing);
        assert_eq!(urgent.created_at, DAY0 - 1000);
        // コンテキストに渡る。
        let ctx = build_context(snap, &cfg(), now, chrono_tz::UTC);
        assert_eq!(ctx.tasks.len(), 2);
    }

    #[test]
    fn tick_delivers_urgent_task_with_task_id() {
        let conn = db::open_in_memory().unwrap();
        hitoiki_core::repo::task::insert(&conn, &task("u1", TaskStatus::Todo, Some(true), None)).unwrap();
        let sink = Rec::default();
        let now = DAY0 + 10 * 60 * MIN;
        push_tick(&conn, &cfg(), now, &sink).unwrap();
        let got = sink.0.lock().unwrap().clone();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].1, "urgent_task");
        let log = store::get_log(&conn, &got[0].0).unwrap();
        let p: PushPayload = serde_json::from_str(&log.payload_json).unwrap();
        assert_eq!(p.task_id.as_deref(), Some("u1"));
        // 間隔内の再評価では出ない。
        push_tick(&conn, &cfg(), now + 5 * MIN, &sink).unwrap();
        assert_eq!(sink.0.lock().unwrap().len(), 1);
    }

    #[test]
    fn tick_delivers_kr_idle_once_a_week_from_the_weekday() {
        use hitoiki_core::model::{Goal, KeyResult};
        use hitoiki_core::repo::goal;
        let conn = db::open_in_memory().unwrap();
        goal::insert_goal(
            &conn,
            &Goal {
                id: "O1".into(),
                title: "目標".into(),
                description_md: None,
                period_start: None,
                period_end: None,
                status: "active".into(),
            },
        )
        .unwrap();
        goal::insert_key_result(
            &conn,
            &KeyResult {
                id: "K1".into(),
                goal_id: "O1".into(),
                title: "英語".into(),
                target: None,
                current: None,
                status: "active".into(),
                measure: "time".into(),
                active_day_min: 15,
            },
        )
        .unwrap();
        let mut t = task("T1", TaskStatus::Todo, None, None);
        t.key_result_id = Some("K1".into());
        hitoiki_core::repo::task::insert(&conn, &t).unwrap();
        let sink = Rec::default();
        let delivered = |sink: &Rec| {
            sink.0
                .lock()
                .unwrap()
                .iter()
                .filter(|(_, t)| t == "kr_idle")
                .count()
        };
        // 月曜（DAY0）・火曜はまだ出ない。
        push_tick(&conn, &cfg(), DAY0 + 10 * 60 * MIN, &sink).unwrap();
        push_tick(&conn, &cfg(), DAY0 + 24 * 60 * MIN + 10 * 60 * MIN, &sink).unwrap();
        assert_eq!(delivered(&sink), 0);
        // 水曜に 1 回。
        let wed = DAY0 + 2 * 24 * 60 * MIN + 10 * 60 * MIN;
        push_tick(&conn, &cfg(), wed, &sink).unwrap();
        assert_eq!(delivered(&sink), 1);
        // 木曜以降も、今週はもう出ない。
        push_tick(&conn, &cfg(), wed + 24 * 60 * MIN, &sink).unwrap();
        assert_eq!(delivered(&sink), 1);
        // 今週時間が入っていれば（来週の水曜でも）出ない。
        segment::replace_range(
            &conn,
            Range::new(DAY0 + 7 * 24 * 60 * MIN, DAY0 + 9 * 24 * 60 * MIN),
            &[work_seg(
                "s1",
                "T1",
                SegmentKind::Work,
                DAY0 + 7 * 24 * 60 * MIN + 60 * MIN,
                DAY0 + 7 * 24 * 60 * MIN + 90 * MIN,
            )],
        )
        .unwrap();
        push_tick(&conn, &cfg(), wed + 7 * 24 * 60 * MIN, &sink).unwrap();
        assert_eq!(delivered(&sink), 1);
    }

    #[test]
    fn disabled_push_does_nothing() {
        let conn = db::open_in_memory().unwrap();
        let mut c = cfg();
        c.push.enabled = false;
        assert!(push_tick(&conn, &c, DAY0, &LogSink).unwrap().is_empty());
    }
}
