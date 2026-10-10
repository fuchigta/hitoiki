//! `push_log` の参照（書き込みは常駐側）。

use crate::model::PushLog;
use crate::Result;
use rusqlite::{params, Connection};

/// 範囲内で配信済みかつ未応答のプッシュ。
pub fn list_unanswered(conn: &Connection, start: i64, end: i64) -> Result<Vec<PushLog>> {
    let mut stmt = conn.prepare(
        "SELECT id, at, trigger, payload_json, delivered, responded_at, response_json, snoozed_until, suppressed_reason \
         FROM push_log WHERE delivered = 1 AND responded_at IS NULL AND at >= ?1 AND at < ?2 ORDER BY at, id",
    )?;
    let rows = stmt.query_map(params![start, end], |r| {
        Ok(PushLog {
            id: r.get(0)?,
            at: r.get(1)?,
            trigger: r.get(2)?,
            payload_json: r.get(3)?,
            delivered: r.get::<_, i64>(4)? != 0,
            responded_at: r.get(5)?,
            response_json: r.get(6)?,
            snoozed_until: r.get(7)?,
            suppressed_reason: r.get(8)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}
