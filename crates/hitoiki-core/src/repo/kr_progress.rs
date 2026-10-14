//! `kr_progress`（成果の指標の現在値の変更履歴）。

use crate::{Millis, Result};
use rusqlite::{params, Connection};
use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct KrProgress {
    pub id: String,
    pub kr_id: String,
    /// `text` の先頭の数値。数として読めなければ null。
    pub value: Option<f64>,
    /// 入力された現在値そのもの。消したときは null。
    pub text: Option<String>,
    pub note: Option<String>,
    pub at: Millis,
}

pub fn insert(conn: &Connection, p: &KrProgress) -> Result<()> {
    conn.execute(
        "INSERT INTO kr_progress (id, kr_id, value, text, note, at) VALUES (?1,?2,?3,?4,?5,?6)",
        params![p.id, p.kr_id, p.value, p.text, p.note, p.at],
    )?;
    Ok(())
}

/// 成果の指標の履歴を古い順に返す。
pub fn list_of_kr(conn: &Connection, kr_id: &str) -> Result<Vec<KrProgress>> {
    let mut stmt = conn.prepare(
        "SELECT id, kr_id, value, text, note, at FROM kr_progress WHERE kr_id = ?1 ORDER BY at, rowid",
    )?;
    let rows = stmt.query_map([kr_id], |r| {
        Ok(KrProgress {
            id: r.get(0)?,
            kr_id: r.get(1)?,
            value: r.get(2)?,
            text: r.get(3)?,
            note: r.get(4)?,
            at: r.get(5)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}
