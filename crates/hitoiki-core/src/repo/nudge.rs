//! `nudge_log` の参照（書き込みは常駐側）。

use crate::model::NudgeLog;
use crate::Result;
use rusqlite::{params, Connection, Row};

const COLS: &str =
    "id, at, trigger, payload_json, delivered, responded_at, response_json, snoozed_until, suppressed_reason";

fn row(r: &Row<'_>) -> rusqlite::Result<NudgeLog> {
    Ok(NudgeLog {
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
}

/// 範囲内で配信済みかつ未応答の問いかけ。「見送り」（`dismiss`。応答時刻は付けない）にしたものは除く。
pub fn list_unanswered(conn: &Connection, start: i64, end: i64) -> Result<Vec<NudgeLog>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLS} FROM nudge_log
         WHERE delivered = 1 AND responded_at IS NULL AND at >= ?1 AND at < ?2
           AND COALESCE(json_extract(response_json, '$.action'), '') <> 'dismiss'
         ORDER BY at, id"
    ))?;
    let rows = stmt.query_map(params![start, end], row)?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// 範囲内で配信済みの問いかけすべて（応答済み・見送りを含む）。
pub fn list_delivered(conn: &Connection, start: i64, end: i64) -> Result<Vec<NudgeLog>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLS} FROM nudge_log WHERE delivered = 1 AND at >= ?1 AND at < ?2 ORDER BY at, id"
    ))?;
    let rows = stmt.query_map(params![start, end], row)?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}
