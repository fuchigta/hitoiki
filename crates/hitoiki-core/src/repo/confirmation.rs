//! `segment_confirmation`（ユーザー確定。再構築しても残る）。

use crate::model::SegmentConfirmation;
use crate::timefmt::Range;
use crate::Result;
use rusqlite::{params, Connection};

pub fn insert(conn: &Connection, o: &SegmentConfirmation) -> Result<()> {
    conn.execute(
        "INSERT INTO segment_confirmation (id, start_at, end_at, kind, task_id, activity, created_at, off_pc, correction_id) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
        params![
            o.id,
            o.start_at,
            o.end_at,
            o.kind,
            o.task_id,
            o.activity,
            o.created_at,
            o.off_pc as i64,
            o.correction_id
        ],
    )?;
    Ok(())
}

/// ID を指定して削除する。削除した数を返す。
pub fn delete_many(conn: &Connection, ids: &[String]) -> Result<usize> {
    let mut n = 0;
    for id in ids {
        n += conn.execute("DELETE FROM segment_confirmation WHERE id = ?1", [id])?;
    }
    Ok(n)
}

/// 範囲と重なる確定（作成順）。
pub fn list(conn: &Connection, range: Range) -> Result<Vec<SegmentConfirmation>> {
    let mut stmt = conn.prepare(
        "SELECT id, start_at, end_at, kind, task_id, activity, created_at, off_pc, correction_id FROM segment_confirmation \
         WHERE end_at > ?1 AND start_at < ?2 ORDER BY created_at, id",
    )?;
    let rows = stmt.query_map(params![range.start, range.end], |r| {
        Ok(SegmentConfirmation {
            id: r.get(0)?,
            start_at: r.get(1)?,
            end_at: r.get(2)?,
            kind: r.get(3)?,
            task_id: r.get(4)?,
            activity: r.get(5)?,
            created_at: r.get(6)?,
            off_pc: r.get::<_, i64>(7)? != 0,
            correction_id: r.get(8)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}
