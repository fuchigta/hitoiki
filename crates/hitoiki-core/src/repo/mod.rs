//! DB アクセス。テーブル群ごとにファイルを分ける。

pub mod agent;
pub mod collect;
pub mod raw;
pub mod segment;
pub mod task;

use crate::{Error, Result};
use rusqlite::Connection;

/// `table` の `id` を前方一致で 1 件に解決する（12.1）。
/// 0 件は `NotFound`、複数件は `Ambiguous`（候補を列挙）。完全一致があればそれを優先する。
pub fn resolve_prefix(conn: &Connection, table: &str, prefix: &str) -> Result<String> {
    if prefix.is_empty() {
        return Err(Error::InvalidArgument("ID が空です".into()));
    }
    let upper = prefix.to_ascii_uppercase();
    let mut stmt = conn.prepare(&format!(
        "SELECT id FROM {table} WHERE id = ?1 OR substr(id, 1, length(?1)) = ?1 ORDER BY id LIMIT 11"
    ))?;
    let ids: Vec<String> = stmt
        .query_map([&upper], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    if ids.iter().any(|i| *i == upper) {
        return Ok(upper);
    }
    match ids.len() {
        0 => Err(Error::NotFound(format!("{table} の ID {prefix}"))),
        1 => Ok(ids.into_iter().next().expect("one")),
        _ => Err(Error::Ambiguous(format!("{prefix}: {}", ids.join(", ")))),
    }
}
