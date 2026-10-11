//! `segment_override`（ユーザー確定。再構築しても残る）。

use crate::model::SegmentOverride;
use crate::timefmt::Range;
use crate::Result;
use rusqlite::{params, Connection};

pub fn insert(conn: &Connection, o: &SegmentOverride) -> Result<()> {
    conn.execute(
        "INSERT INTO segment_override (id, start_at, end_at, kind, task_id, activity_label, created_at, offline, correction_id) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
        params![
            o.id,
            o.start_at,
            o.end_at,
            o.kind,
            o.task_id,
            o.activity_label,
            o.created_at,
            o.offline as i64,
            o.correction_id
        ],
    )?;
    Ok(())
}

/// 範囲と重なるオーバーライド（作成順）。
pub fn list(conn: &Connection, range: Range) -> Result<Vec<SegmentOverride>> {
    let mut stmt = conn.prepare(
        "SELECT id, start_at, end_at, kind, task_id, activity_label, created_at, offline, correction_id FROM segment_override \
         WHERE end_at > ?1 AND start_at < ?2 ORDER BY created_at, id",
    )?;
    let rows = stmt.query_map(params![range.start, range.end], |r| {
        Ok(SegmentOverride {
            id: r.get(0)?,
            start_at: r.get(1)?,
            end_at: r.get(2)?,
            kind: r.get(3)?,
            task_id: r.get(4)?,
            activity_label: r.get(5)?,
            created_at: r.get(6)?,
            offline: r.get::<_, i64>(7)? != 0,
            correction_id: r.get(8)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}
