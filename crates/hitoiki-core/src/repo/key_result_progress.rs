//! `key_result_progress`（成果の指標の現在値の変更履歴）。

use crate::{Millis, Result};
use rusqlite::{params, Connection};
use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct KeyResultProgress {
    pub id: String,
    pub key_result_id: String,
    /// `text` の先頭の数値。数として読めなければ null。
    pub value: Option<f64>,
    /// 入力された現在値そのもの。消したときは null。
    pub text: Option<String>,
    pub note: Option<String>,
    pub at: Millis,
}

pub fn insert(conn: &Connection, p: &KeyResultProgress) -> Result<()> {
    conn.execute(
        "INSERT INTO key_result_progress (id, key_result_id, value, text, note, at) VALUES (?1,?2,?3,?4,?5,?6)",
        params![p.id, p.key_result_id, p.value, p.text, p.note, p.at],
    )?;
    Ok(())
}

/// 成果の指標の履歴を古い順に返す。
pub fn list_of_key_result(conn: &Connection, key_result_id: &str) -> Result<Vec<KeyResultProgress>> {
    let mut stmt = conn.prepare(
        "SELECT id, key_result_id, value, text, note, at FROM key_result_progress WHERE key_result_id = ?1 ORDER BY at, rowid",
    )?;
    let rows = stmt.query_map([key_result_id], |r| {
        Ok(KeyResultProgress {
            id: r.get(0)?,
            key_result_id: r.get(1)?,
            value: r.get(2)?,
            text: r.get(3)?,
            note: r.get(4)?,
            at: r.get(5)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}
