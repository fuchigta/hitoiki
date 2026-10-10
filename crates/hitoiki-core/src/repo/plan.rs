//! `time_block`（タイムブロッキング）。

use crate::model::TimeBlock;
use crate::timefmt::Range;
use crate::Result;
use rusqlite::{params, Connection};

pub fn insert(conn: &Connection, b: &TimeBlock) -> Result<()> {
    conn.execute(
        "INSERT INTO time_block (id, start_at, end_at, task_id, label, focus) VALUES (?1,?2,?3,?4,?5,?6)",
        params![b.id, b.start_at, b.end_at, b.task_id, b.label, b.focus],
    )?;
    Ok(())
}

/// ID で削除する。削除できたら true。
pub fn delete(conn: &Connection, id: &str) -> Result<bool> {
    let n = conn.execute("DELETE FROM time_block WHERE id = ?1", params![id])?;
    Ok(n > 0)
}

pub fn list(conn: &Connection, range: Range) -> Result<Vec<TimeBlock>> {
    let mut stmt = conn.prepare(
        "SELECT id, start_at, end_at, task_id, label, focus FROM time_block \
         WHERE end_at > ?1 AND start_at < ?2 ORDER BY start_at, id",
    )?;
    let rows = stmt.query_map(params![range.start, range.end], |r| {
        Ok(TimeBlock {
            id: r.get(0)?,
            start_at: r.get(1)?,
            end_at: r.get(2)?,
            task_id: r.get(3)?,
            label: r.get(4)?,
            focus: r.get::<_, i64>(5)? != 0,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}
