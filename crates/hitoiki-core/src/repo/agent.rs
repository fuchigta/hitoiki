//! 生データ層 `agent_event` / `agent_ingest_offset` の読み書き。

use crate::model::AgentEvent;
use crate::timefmt::Range;
use crate::{Millis, Result};
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};

const COLS: &str =
    "id, at, agent, session_id, kind, cwd, git_branch, prompt_excerpt, source_file, source_offset";

fn from_row(r: &Row<'_>) -> rusqlite::Result<AgentEvent> {
    Ok(AgentEvent {
        id: r.get(0)?,
        at: r.get(1)?,
        agent: r.get(2)?,
        session_id: r.get(3)?,
        kind: r.get(4)?,
        cwd: r.get(5)?,
        git_branch: r.get(6)?,
        prompt_excerpt: r.get(7)?,
        source_file: r.get(8)?,
        source_offset: r.get(9)?,
    })
}

/// イベントを一括挿入する。一意キー (agent, session_id, kind, at) が重複するものは無視し、
/// 実際に挿入した件数を返す。
pub fn insert_events(conn: &Connection, events: &[AgentEvent]) -> Result<usize> {
    let mut stmt = conn.prepare_cached(&format!(
        "INSERT OR IGNORE INTO agent_event ({COLS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)"
    ))?;
    let mut n = 0;
    for e in events {
        n += stmt.execute(params![
            e.id,
            e.at,
            e.agent,
            e.session_id,
            e.kind,
            e.cwd,
            e.git_branch,
            e.prompt_excerpt,
            e.source_file,
            e.source_offset,
        ])?;
    }
    Ok(n)
}

/// 範囲 [start, end) のイベントを時刻順に返す。
pub fn list_events(conn: &Connection, range: Range) -> Result<Vec<AgentEvent>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLS} FROM agent_event WHERE at >= ?1 AND at < ?2 ORDER BY at, id"
    ))?;
    let rows = stmt.query_map(params![range.start, range.end], from_row)?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// あるセッションの全イベントを時刻順に返す。
pub fn list_session_events(conn: &Connection, agent: &str, session_id: &str) -> Result<Vec<AgentEvent>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLS} FROM agent_event WHERE agent = ?1 AND session_id = ?2 ORDER BY at, id"
    ))?;
    let rows = stmt.query_map(params![agent, session_id], from_row)?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// 取り込み済みオフセット。未登録なら None。
pub fn get_offset(conn: &Connection, source_file: &str) -> Result<Option<u64>> {
    let v: Option<i64> = conn
        .query_row(
            "SELECT offset FROM agent_ingest_offset WHERE source_file = ?1",
            [source_file],
            |r| r.get(0),
        )
        .optional()?;
    Ok(v.map(|v| v.max(0) as u64))
}

/// 取り込みオフセットを保存（上書き）。
pub fn set_offset(
    conn: &Connection,
    source_file: &str,
    agent: &str,
    offset: u64,
    updated_at: Millis,
) -> Result<()> {
    conn.execute(
        "INSERT INTO agent_ingest_offset (source_file, agent, offset, updated_at) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(source_file) DO UPDATE SET agent = ?2, offset = ?3, updated_at = ?4",
        params![source_file, agent, offset as i64, updated_at],
    )?;
    Ok(())
}

/// セッション一覧の 1 行。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionSummary {
    pub agent: String,
    pub session_id: String,
    pub start_at: Millis,
    pub end_at: Millis,
    pub cwd: Option<String>,
    pub git_branch: Option<String>,
    pub prompt_count: i64,
    pub first_prompt: Option<String>,
}

/// 範囲 [start, end) にイベントを持つセッションを、開始時刻順に返す。
/// 開始/終了時刻・prompt 数は範囲内のイベントのみで集計する。
pub fn list_sessions(conn: &Connection, range: Range) -> Result<Vec<SessionSummary>> {
    let mut stmt = conn.prepare(
        "SELECT e.agent, e.session_id, MIN(e.at), MAX(e.at),
            (SELECT cwd FROM agent_event x WHERE x.agent = e.agent AND x.session_id = e.session_id
               AND x.cwd IS NOT NULL AND x.at >= ?1 AND x.at < ?2 ORDER BY x.at LIMIT 1),
            (SELECT git_branch FROM agent_event x WHERE x.agent = e.agent AND x.session_id = e.session_id
               AND x.git_branch IS NOT NULL AND x.at >= ?1 AND x.at < ?2 ORDER BY x.at LIMIT 1),
            SUM(e.kind = 'prompt_submit'),
            (SELECT prompt_excerpt FROM agent_event x WHERE x.agent = e.agent AND x.session_id = e.session_id
               AND x.kind = 'prompt_submit' AND x.prompt_excerpt IS NOT NULL
               AND x.at >= ?1 AND x.at < ?2 ORDER BY x.at LIMIT 1)
         FROM agent_event e WHERE e.at >= ?1 AND e.at < ?2
         GROUP BY e.agent, e.session_id ORDER BY MIN(e.at), e.session_id",
    )?;
    let rows = stmt.query_map(params![range.start, range.end], |r| {
        Ok(SessionSummary {
            agent: r.get(0)?,
            session_id: r.get(1)?,
            start_at: r.get(2)?,
            end_at: r.get(3)?,
            cwd: r.get(4)?,
            git_branch: r.get(5)?,
            prompt_count: r.get(6)?,
            first_prompt: r.get(7)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::model::AgentEventKind;

    fn ev(id: &str, at: Millis, kind: AgentEventKind, excerpt: Option<&str>) -> AgentEvent {
        AgentEvent {
            id: id.into(),
            at,
            agent: "claude-code".into(),
            session_id: "S1".into(),
            kind,
            cwd: Some("/w".into()),
            git_branch: Some("main".into()),
            prompt_excerpt: excerpt.map(Into::into),
            source_file: Some("f.jsonl".into()),
            source_offset: Some(0),
        }
    }

    #[test]
    fn insert_dedup_and_sessions() {
        let conn = db::open_in_memory().unwrap();
        let evs = vec![
            ev("A", 10, AgentEventKind::SessionStart, None),
            ev("B", 20, AgentEventKind::PromptSubmit, Some("hello")),
            ev("C", 30, AgentEventKind::ResponseComplete, None),
            ev("D", 40, AgentEventKind::PromptSubmit, Some("again")),
        ];
        assert_eq!(insert_events(&conn, &evs).unwrap(), 4);
        // 同じ (agent, session, kind, at) は ID が違っても無視される
        let dup = vec![ev("Z", 20, AgentEventKind::PromptSubmit, Some("x"))];
        assert_eq!(insert_events(&conn, &dup).unwrap(), 0);
        assert_eq!(insert_events(&conn, &evs).unwrap(), 0);

        assert_eq!(list_events(&conn, Range::new(0, 100)).unwrap().len(), 4);
        assert_eq!(list_events(&conn, Range::new(20, 40)).unwrap().len(), 2);
        let s = list_sessions(&conn, Range::new(0, 100)).unwrap();
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].prompt_count, 2);
        assert_eq!((s[0].start_at, s[0].end_at), (10, 40));
        assert_eq!(s[0].first_prompt.as_deref(), Some("hello"));
        assert_eq!(s[0].cwd.as_deref(), Some("/w"));
    }

    #[test]
    fn offsets() {
        let conn = db::open_in_memory().unwrap();
        assert_eq!(get_offset(&conn, "f").unwrap(), None);
        set_offset(&conn, "f", "codex", 100, 1).unwrap();
        set_offset(&conn, "f", "codex", 250, 2).unwrap();
        assert_eq!(get_offset(&conn, "f").unwrap(), Some(250));
    }
}
