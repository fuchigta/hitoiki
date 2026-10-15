//! ユースケース層。CLI と将来の Tauri コマンドが共通で呼ぶ。
//!
//! 入力は構造体、出力は `Serialize` 可能な構造体。時刻は `Clock` 経由、日付境界は設定の
//! タイムゾーンで扱う。JSON 出力のスキーマ版は [`SCHEMA_VERSION`]（CLI が `"version"` として付与する）。

pub mod actual;
pub mod block_template;
pub mod context;
pub mod correction;
pub mod erg;
pub mod goal;
pub mod heatmap;
pub mod hint;
pub mod ignore;
pub mod impact;
pub mod ingest;
pub mod key_result_progress;
mod key_result_summary;
pub mod link_suggestion;
pub mod onboarding;
pub mod plan;
pub mod pushlog;
pub mod quick;
pub mod record;
pub mod reflect;
pub mod report;
pub mod review;
pub mod rule;
pub mod settings;
pub mod skill;
pub mod status;
pub mod structured;
pub mod task;
pub mod update;

use crate::config::Config;
use crate::model::{ActivitySegment, SegmentKind, SegmentStatus};
use crate::timefmt::{self, Range};
use crate::{Clock, Error, Millis, Result};
use chrono_tz::Tz;
use rusqlite::Connection;
use serde::Serialize;
use std::collections::HashMap;

/// JSON 出力のスキーマ版。
pub const SCHEMA_VERSION: u32 = 1;

/// サービス呼び出しの共通コンテキスト。
pub struct Ctx<'a> {
    pub conn: &'a Connection,
    pub cfg: &'a Config,
    pub clock: &'a dyn Clock,
}

impl<'a> Ctx<'a> {
    pub fn new(conn: &'a Connection, cfg: &'a Config, clock: &'a dyn Clock) -> Self {
        Self { conn, cfg, clock }
    }

    pub fn tz(&self) -> Result<Tz> {
        self.cfg.tz()
    }

    pub fn now(&self) -> Millis {
        self.clock.now_ms()
    }

    /// 範囲指定を解決する。指定が無ければ今日。
    pub fn range(&self, args: &RangeArgs) -> Result<Range> {
        let tz = self.tz()?;
        let now = self.now();
        if args.from.is_some() || args.to.is_some() {
            let from = match &args.from {
                Some(f) => timefmt::parse_instant(tz, f, now)?,
                None => return Err(Error::InvalidArgument("--to には --from も必要です".into())),
            };
            let to = match &args.to {
                Some(t) => parse_to(tz, t, now)?,
                None => now,
            };
            if to <= from {
                return Err(Error::InvalidArgument(
                    "範囲の終了は開始より後にしてください".into(),
                ));
            }
            return Ok(Range::new(from, to));
        }
        timefmt::parse_period(tz, args.date.as_deref().unwrap_or("today"), now)
    }
}

/// `--to` の解釈。日付のみ（`YYYY-MM-DD`/`today` 等）の場合はその日の終わりまでを含める。
pub fn parse_to(tz: Tz, s: &str, now: Millis) -> Result<Millis> {
    if let Ok(d) = timefmt::parse_date(tz, s, now) {
        return Ok(timefmt::day_range(tz, d).end);
    }
    timefmt::parse_instant(tz, s, now)
}

/// 範囲指定の引数（`--date` / `--period`、`--from` / `--to`）。
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(default)]
pub struct RangeArgs {
    /// `today` / `yesterday` / `this-week` / `last-week` / `YYYY-MM-DD`。
    pub date: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
}

/// 範囲の JSON 表現。
#[derive(Debug, Clone, Serialize)]
pub struct RangeView {
    pub start_at: Millis,
    pub end_at: Millis,
    pub start: String,
    pub end: String,
}

pub(crate) fn range_view(tz: Tz, r: Range) -> RangeView {
    RangeView {
        start_at: r.start,
        end_at: r.end,
        start: timefmt::fmt_rfc3339(tz, r.start),
        end: timefmt::fmt_rfc3339(tz, r.end),
    }
}

/// 区間の JSON 表現（タスク名と表示用の時刻を付与）。
#[derive(Debug, Clone, Serialize)]
pub struct SegmentView {
    pub id: String,
    pub start_at: Millis,
    pub end_at: Millis,
    pub start: String,
    pub end: String,
    pub duration_ms: i64,
    pub kind: SegmentKind,
    pub task_id: Option<String>,
    pub task_title: Option<String>,
    pub activity: Option<String>,
    pub confidence: Option<f64>,
    pub decided_by: Option<String>,
    pub status: SegmentStatus,
    pub agent_session_id: Option<String>,
    pub exe_name: Option<String>,
    pub title: Option<String>,
    /// PC 外で過ごした時間（実績の補正）。
    pub off_pc: bool,
}

pub(crate) fn task_titles(conn: &Connection) -> Result<HashMap<String, String>> {
    let mut stmt = conn.prepare("SELECT id, title FROM task")?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub(crate) fn segment_view(tz: Tz, s: &ActivitySegment, titles: &HashMap<String, String>) -> SegmentView {
    SegmentView {
        id: s.id.clone(),
        start_at: s.start_at,
        end_at: s.end_at,
        start: timefmt::fmt_rfc3339(tz, s.start_at),
        end: timefmt::fmt_rfc3339(tz, s.end_at),
        duration_ms: s.end_at - s.start_at,
        kind: s.kind,
        task_id: s.task_id.clone(),
        task_title: s.task_id.as_ref().and_then(|id| titles.get(id).cloned()),
        activity: s.activity.clone(),
        confidence: s.confidence,
        decided_by: s.decided_by.clone(),
        status: s.status,
        agent_session_id: s.agent_session_id.clone(),
        exe_name: s.exe_name.clone(),
        title: s.title.clone(),
        off_pc: s.off_pc,
    }
}

/// 1〜5 の範囲検証。
pub(crate) fn check_1_5(name: &str, v: Option<i64>) -> Result<()> {
    match v {
        Some(n) if !(1..=5).contains(&n) => Err(Error::InvalidArgument(format!(
            "{name} は 1〜5 で指定してください: {n}"
        ))),
        _ => Ok(()),
    }
}

#[cfg(test)]
pub(crate) mod testutil {
    use super::*;
    use crate::db;
    use crate::FixedClock;

    /// 2026-10-10 12:00 JST に固定した時刻。
    pub fn now() -> Millis {
        chrono::DateTime::parse_from_rfc3339("2026-10-10T12:00:00+09:00")
            .unwrap()
            .timestamp_millis()
    }

    pub struct Env {
        pub conn: Connection,
        pub cfg: Config,
        pub clock: FixedClock,
    }

    impl Env {
        pub fn new() -> Self {
            let mut cfg = Config::default();
            cfg.general.timezone = "Asia/Tokyo".into();
            Self {
                conn: db::open_in_memory().unwrap(),
                cfg,
                clock: FixedClock::new(now()),
            }
        }
        pub fn ctx(&self) -> Ctx<'_> {
            Ctx::new(&self.conn, &self.cfg, &self.clock)
        }
        /// 当日 00:00 JST。
        pub fn day_start(&self) -> Millis {
            let tz = self.cfg.tz().unwrap();
            timefmt::day_range(tz, timefmt::local_date_of(tz, now())).start
        }
    }

    pub fn insert_segment(
        conn: &Connection,
        id: &str,
        start: Millis,
        end: Millis,
        kind: SegmentKind,
        task_id: Option<&str>,
        status: SegmentStatus,
    ) {
        let seg = ActivitySegment {
            id: id.into(),
            start_at: start,
            end_at: end,
            kind,
            task_id: task_id.map(Into::into),
            activity: None,
            confidence: None,
            decided_by: None,
            status,
            window_span_ids: vec![],
            agent_session_id: None,
            exe_name: Some("Code.exe".into()),
            title: Some("main.rs".into()),
            off_pc: false,
        };
        conn.execute(
            "INSERT INTO activity_segment (id, start_at, end_at, kind, task_id, activity, confidence, \
             decided_by, status, window_span_ids, agent_session_id, exe_name, title) \
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,'[]',?10,?11,?12)",
            rusqlite::params![
                seg.id,
                seg.start_at,
                seg.end_at,
                seg.kind,
                seg.task_id,
                seg.activity,
                seg.confidence,
                seg.decided_by,
                seg.status,
                seg.agent_session_id,
                seg.exe_name,
                seg.title
            ],
        )
        .unwrap();
    }
}
