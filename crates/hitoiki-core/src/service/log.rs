//! `hitoiki log`: 区間一覧と、`--raw` での生データの時系列表示（12.2）。

use super::{range_view, segment_view, task_titles, Ctx, RangeArgs, RangeView, SegmentView};
use crate::repo;
use crate::{Millis, Result};
use rusqlite::params;
use serde::Serialize;

#[derive(Debug, Clone, Default)]
pub struct LogInput {
    pub range: RangeArgs,
    pub raw: bool,
}

/// 生データ 1 件。`source` は window_span / idle_span / agent_event。
#[derive(Debug, Clone, Serialize)]
pub struct RawEntry {
    pub at: Millis,
    pub end_at: Option<Millis>,
    pub at_local: String,
    pub source: &'static str,
    pub summary: String,
    pub detail: serde_json::Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct Log {
    pub range: RangeView,
    pub segments: Vec<SegmentView>,
    /// `raw` 指定時のみ。時刻順。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw: Option<Vec<RawEntry>>,
}

pub fn log(ctx: &Ctx, input: &LogInput) -> Result<Log> {
    let tz = ctx.tz()?;
    let range = ctx.range(&input.range)?;
    let titles = task_titles(ctx.conn)?;
    let segments = repo::segment::list(ctx.conn, range)?
        .iter()
        .map(|s| segment_view(tz, s, &titles))
        .collect();
    let raw = if input.raw {
        Some(raw_entries(ctx, range.start, range.end)?)
    } else {
        None
    };
    Ok(Log {
        range: range_view(tz, range),
        segments,
        raw,
    })
}

fn raw_entries(ctx: &Ctx, start: Millis, end: Millis) -> Result<Vec<RawEntry>> {
    let tz = ctx.tz()?;
    let local = |ms| crate::timefmt::fmt_rfc3339(tz, ms);
    let mut out: Vec<RawEntry> = Vec::new();

    let mut stmt = ctx.conn.prepare(
        "SELECT id, start_at, end_at, exe_name, title, excluded FROM window_span \
         WHERE start_at < ?2 AND COALESCE(end_at, ?3) > ?1 ORDER BY start_at, id",
    )?;
    let now = ctx.now();
    for row in stmt.query_map(params![start, end, now], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, Millis>(1)?,
            r.get::<_, Option<Millis>>(2)?,
            r.get::<_, Option<String>>(3)?,
            r.get::<_, Option<String>>(4)?,
            r.get::<_, i64>(5)?,
        ))
    })? {
        let (id, s, e, exe, title, excluded) = row?;
        out.push(RawEntry {
            at: s,
            end_at: e,
            at_local: local(s),
            source: "window_span",
            summary: format!(
                "{} {}",
                exe.clone().unwrap_or_default(),
                title.clone().unwrap_or_default()
            )
            .trim()
            .to_string(),
            detail: serde_json::json!({"id": id, "exe_name": exe, "title": title, "excluded": excluded != 0}),
        });
    }

    let mut stmt = ctx.conn.prepare(
        "SELECT id, start_at, end_at, reason FROM idle_span \
         WHERE start_at < ?2 AND COALESCE(end_at, ?3) > ?1 ORDER BY start_at, id",
    )?;
    for row in stmt.query_map(params![start, end, now], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, Millis>(1)?,
            r.get::<_, Option<Millis>>(2)?,
            r.get::<_, String>(3)?,
        ))
    })? {
        let (id, s, e, reason) = row?;
        out.push(RawEntry {
            at: s,
            end_at: e,
            at_local: local(s),
            source: "idle_span",
            summary: format!("無操作 ({reason})"),
            detail: serde_json::json!({"id": id, "reason": reason}),
        });
    }

    let mut stmt = ctx.conn.prepare(
        "SELECT id, at, agent, session_id, kind, cwd, git_branch, prompt_excerpt FROM agent_event \
         WHERE at >= ?1 AND at < ?2 ORDER BY at, id",
    )?;
    for row in stmt.query_map(params![start, end], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, Millis>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, String>(4)?,
            r.get::<_, Option<String>>(5)?,
            r.get::<_, Option<String>>(6)?,
            r.get::<_, Option<String>>(7)?,
        ))
    })? {
        let (id, at, agent, session, kind, cwd, branch, excerpt) = row?;
        out.push(RawEntry {
            at,
            end_at: None,
            at_local: local(at),
            source: "agent_event",
            summary: format!(
                "{agent} {kind} {}",
                excerpt.clone().or_else(|| cwd.clone()).unwrap_or_default()
            )
            .trim()
            .to_string(),
            detail: serde_json::json!({
                "id": id, "agent": agent, "session_id": session, "kind": kind,
                "cwd": cwd, "git_branch": branch, "prompt_excerpt": excerpt
            }),
        });
    }

    out.sort_by(|a, b| a.at.cmp(&b.at).then_with(|| a.source.cmp(b.source)));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{SegmentKind, SegmentStatus};
    use crate::service::testutil::{insert_segment, Env};

    #[test]
    fn raw_is_merged_in_time_order() {
        let env = Env::new();
        let ctx = env.ctx();
        let d = env.day_start();
        insert_segment(
            ctx.conn,
            "S1",
            d,
            d + 1000,
            SegmentKind::Work,
            None,
            SegmentStatus::Auto,
        );
        ctx.conn
            .execute(
                "INSERT INTO window_span (id, start_at, end_at, exe_name, title) VALUES ('W1', ?1, ?2, 'Code.exe', 't')",
                params![d + 500, d + 900],
            )
            .unwrap();
        ctx.conn
            .execute(
                "INSERT INTO idle_span (id, start_at, end_at, reason) VALUES ('I1', ?1, NULL, 'no_input')",
                params![d + 100],
            )
            .unwrap();
        ctx.conn
            .execute(
                "INSERT INTO agent_event (id, at, agent, session_id, kind) VALUES ('E1', ?1, 'claude-code', 'S', 'prompt_submit')",
                params![d + 300],
            )
            .unwrap();
        let plain = log(&ctx, &LogInput::default()).unwrap();
        assert!(plain.raw.is_none());
        assert_eq!(plain.segments.len(), 1);
        let l = log(
            &ctx,
            &LogInput {
                raw: true,
                ..Default::default()
            },
        )
        .unwrap();
        let raw = l.raw.unwrap();
        let order: Vec<_> = raw.iter().map(|r| r.source).collect();
        assert_eq!(order, ["idle_span", "agent_event", "window_span"]);
    }
}
