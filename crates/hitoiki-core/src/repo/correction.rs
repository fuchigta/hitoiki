//! `segment_correction`（実績の補正 1 回分。ユーザー層）。補正が作った override は `segment_override.correction_id` で結ぶ。

use crate::model::SegmentKind;
use crate::timefmt::Range;
use crate::{Error, Millis, Result};
use rusqlite::{params, Connection, OptionalExtension, Row};

#[derive(Debug, Clone, PartialEq)]
pub struct Correction {
    pub id: String,
    pub start_at: Millis,
    pub end_at: Millis,
    pub task_id: Option<String>,
    pub kind: Option<SegmentKind>,
    pub activity_label: Option<String>,
    pub total_ms: i64,
    pub offline_ms: i64,
    pub segment_count: i64,
    pub created_at: Millis,
}

const COLS: &str =
    "id, start_at, end_at, task_id, kind, activity_label, total_ms, offline_ms, segment_count, created_at";

fn from_row(r: &Row<'_>) -> rusqlite::Result<Correction> {
    Ok(Correction {
        id: r.get(0)?,
        start_at: r.get(1)?,
        end_at: r.get(2)?,
        task_id: r.get(3)?,
        kind: r.get(4)?,
        activity_label: r.get(5)?,
        total_ms: r.get(6)?,
        offline_ms: r.get(7)?,
        segment_count: r.get(8)?,
        created_at: r.get(9)?,
    })
}

pub fn insert(conn: &Connection, c: &Correction) -> Result<()> {
    conn.execute(
        &format!("INSERT INTO segment_correction ({COLS}) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)"),
        params![
            c.id,
            c.start_at,
            c.end_at,
            c.task_id,
            c.kind,
            c.activity_label,
            c.total_ms,
            c.offline_ms,
            c.segment_count,
            c.created_at
        ],
    )?;
    Ok(())
}

pub fn get(conn: &Connection, id: &str) -> Result<Correction> {
    conn.query_row(
        &format!("SELECT {COLS} FROM segment_correction WHERE id = ?1"),
        [id],
        from_row,
    )
    .optional()?
    .ok_or_else(|| Error::NotFound(format!("補正 {id}")))
}

/// ID の前方一致で 1 件に解決する。
pub fn resolve(conn: &Connection, prefix: &str) -> Result<Correction> {
    get(conn, &super::resolve_prefix(conn, "segment_correction", prefix)?)
}

/// 新しい順。`range` があればその期間と重なるもの、`task_id` があればそのタスクのものだけ。
pub fn list(conn: &Connection, range: Option<Range>, task_id: Option<&str>) -> Result<Vec<Correction>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLS} FROM segment_correction \
         WHERE (?1 IS NULL OR (end_at > ?1 AND start_at < ?2)) AND (?3 IS NULL OR task_id = ?3) \
         ORDER BY created_at DESC, id DESC"
    ))?;
    let rows = stmt.query_map(
        params![range.map(|r| r.start), range.map(|r| r.end), task_id],
        from_row,
    )?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// 補正と、それが作った override を削除する。削除した override の数を返す。
pub fn delete(conn: &Connection, id: &str) -> Result<usize> {
    let n = conn.execute("DELETE FROM segment_override WHERE correction_id = ?1", [id])?;
    conn.execute("DELETE FROM segment_correction WHERE id = ?1", [id])?;
    Ok(n)
}
