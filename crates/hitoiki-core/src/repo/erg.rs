//! ERG ラベル（`erg_label`）。出所が `user` の行は AI からは上書きしない。

use crate::model::{ErgChoice, ErgLabel, ErgRecordKind, ErgSource};
use crate::{Millis, Result};
use rusqlite::{params, Connection, OptionalExtension};

const COLS: &str = "l.record_kind, l.record_id, l.tag, l.source, l.confidence, l.reason, l.at";

fn from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<ErgLabel> {
    Ok(ErgLabel {
        record_kind: r.get(0)?,
        record_id: r.get(1)?,
        tag: r.get(2)?,
        source: r.get(3)?,
        confidence: r.get(4)?,
        reason: r.get(5)?,
        at: r.get(6)?,
    })
}

pub fn get(conn: &Connection, kind: ErgRecordKind, id: &str) -> Result<Option<ErgLabel>> {
    Ok(conn
        .query_row(
            &format!("SELECT {COLS} FROM erg_label l WHERE l.record_kind = ?1 AND l.record_id = ?2"),
            params![kind, id],
            from_row,
        )
        .optional()?)
}

/// ユーザーが付けた・確定したタグを保存する（既存のラベルは出所を問わず置き換える）。
pub fn set_user(conn: &Connection, kind: ErgRecordKind, id: &str, tag: ErgChoice, at: Millis) -> Result<()> {
    conn.execute(
        "INSERT INTO erg_label (record_kind, record_id, tag, source, confidence, reason, at) \
         VALUES (?1, ?2, ?3, 'user', NULL, NULL, ?4) \
         ON CONFLICT(record_kind, record_id) DO UPDATE SET \
           tag = excluded.tag, source = 'user', confidence = NULL, reason = NULL, at = excluded.at",
        params![kind, id, tag, at],
    )?;
    Ok(())
}

/// AI の判定（`ai` か `unconfirmed`）を保存する。`user` のラベルがある記録は更新しない。
/// 更新したら true。
pub fn set_auto(conn: &Connection, l: &ErgLabel) -> Result<bool> {
    debug_assert!(l.source != ErgSource::User);
    let n = conn.execute(
        "INSERT INTO erg_label (record_kind, record_id, tag, source, confidence, reason, at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) \
         ON CONFLICT(record_kind, record_id) DO UPDATE SET \
           tag = excluded.tag, source = excluded.source, confidence = excluded.confidence, \
           reason = excluded.reason, at = excluded.at \
         WHERE erg_label.source <> 'user'",
        params![
            l.record_kind,
            l.record_id,
            l.tag,
            l.source,
            l.confidence,
            l.reason,
            l.at
        ],
    )?;
    Ok(n > 0)
}

/// ラベルを消す（記録の削除時）。
pub fn delete(conn: &Connection, kind: ErgRecordKind, id: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM erg_label WHERE record_kind = ?1 AND record_id = ?2",
        params![kind, id],
    )?;
    Ok(())
}

/// AI 由来のラベル（`ai` / `unconfirmed`）だけを消す。記録の本文を直したときに、判定をやり直せるようにする。
pub fn delete_auto(conn: &Connection, kind: ErgRecordKind, id: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM erg_label WHERE record_kind = ?1 AND record_id = ?2 AND source <> 'user'",
        params![kind, id],
    )?;
    Ok(())
}

/// 期間内の記録に付いたラベル。
pub fn list_range(conn: &Connection, start: Millis, end: Millis) -> Result<Vec<ErgLabel>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLS} FROM erg_label l JOIN condition t ON l.record_kind = 'condition' AND t.id = l.record_id \
           WHERE t.at >= ?1 AND t.at < ?2 \
         UNION ALL SELECT {COLS} FROM erg_label l JOIN concern t ON l.record_kind = 'concern' AND t.id = l.record_id \
           WHERE t.at >= ?1 AND t.at < ?2 \
         UNION ALL SELECT {COLS} FROM erg_label l JOIN reframe t ON l.record_kind = 'reframe' AND t.id = l.record_id \
           WHERE t.at >= ?1 AND t.at < ?2 \
         UNION ALL SELECT {COLS} FROM erg_label l JOIN reflection t ON l.record_kind = 'reflection' AND t.id = l.record_id \
           WHERE t.at >= ?1 AND t.at < ?2 \
         ORDER BY 7, 2"
    ))?;
    let rows = stmt.query_map(params![start, end], from_row)?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// 未確定（`unconfirmed`）の一覧（古い順）。
pub fn list_unconfirmed(conn: &Connection) -> Result<Vec<ErgLabel>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLS} FROM erg_label l WHERE l.source = 'unconfirmed' ORDER BY l.at, l.record_id"
    ))?;
    let rows = stmt.query_map([], from_row)?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}
