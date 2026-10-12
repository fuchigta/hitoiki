//! SQLite の接続とマイグレーション（WAL + busy_timeout、適用前にバックアップ。docs/dev/architecture.md「データモデル」）。

use crate::Result;
use rusqlite::Connection;
use std::path::Path;
use std::time::Duration;

/// (バージョン, SQL)。追加時は末尾に足すこと。
const MIGRATIONS: &[(i64, &str)] = &[
    (1, include_str!("../migrations/0001_init.sql")),
    (2, include_str!("../migrations/0002_judge_cache.sql")),
    (3, include_str!("../migrations/0003_block_template.sql")),
    (4, include_str!("../migrations/0004_correction.sql")),
    (5, include_str!("../migrations/0005_reflection_structured.sql")),
    (7, include_str!("../migrations/0007_erg_label.sql")),
];

/// DB を開き、未適用のマイグレーションを適用する。
pub fn open(path: &Path) -> Result<Connection> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let existed = path.exists();
    let mut conn = Connection::open(path)?;
    configure(&conn)?;
    let pending = pending_versions(&conn)?;
    if existed && !pending.is_empty() && has_user_tables(&conn)? {
        backup(&conn, path)?;
    }
    migrate(&mut conn)?;
    Ok(conn)
}

/// テスト用のインメモリ DB（マイグレーション適用済み）。
pub fn open_in_memory() -> Result<Connection> {
    let mut conn = Connection::open_in_memory()?;
    conn.execute_batch("PRAGMA foreign_keys = ON;")?;
    migrate(&mut conn)?;
    Ok(conn)
}

fn configure(conn: &Connection) -> Result<()> {
    conn.busy_timeout(Duration::from_secs(5))?;
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA synchronous = NORMAL;
         PRAGMA foreign_keys = ON;",
    )?;
    Ok(())
}

fn ensure_migration_table(conn: &Connection) -> Result<()> {
    conn.execute_batch("CREATE TABLE IF NOT EXISTS schema_migration (version INTEGER PRIMARY KEY);")?;
    Ok(())
}

fn has_user_tables(conn: &Connection) -> Result<bool> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name <> 'schema_migration'",
        [],
        |r| r.get(0),
    )?;
    Ok(n > 0)
}

fn pending_versions(conn: &Connection) -> Result<Vec<i64>> {
    ensure_migration_table(conn)?;
    let current: i64 = conn.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM schema_migration",
        [],
        |r| r.get(0),
    )?;
    Ok(MIGRATIONS
        .iter()
        .map(|(v, _)| *v)
        .filter(|v| *v > current)
        .collect())
}

fn migrate(conn: &mut Connection) -> Result<()> {
    let pending = pending_versions(conn)?;
    for (version, sql) in MIGRATIONS.iter().filter(|(v, _)| pending.contains(v)) {
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.execute("INSERT INTO schema_migration(version) VALUES (?1)", [version])?;
        tx.commit()?;
        tracing::info!(version, "マイグレーションを適用しました");
    }
    Ok(())
}

fn backup(conn: &Connection, path: &Path) -> Result<()> {
    let stamp = chrono::Local::now().format("%Y%m%d%H%M%S");
    let dest = path.with_extension(format!("db.bak-{stamp}"));
    conn.execute("VACUUM INTO ?1", [dest.to_string_lossy()])?;
    tracing::info!(dest = %dest.display(), "マイグレーション前にバックアップしました");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrate_in_memory() {
        let conn = open_in_memory().unwrap();
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM schema_migration", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n as usize, MIGRATIONS.len());
    }

    #[test]
    fn 従来のチェックインのerg_tagはユーザーのラベルに引き継がれる() {
        let mut conn = Connection::open_in_memory().unwrap();
        ensure_migration_table(&conn).unwrap();
        for (v, sql) in MIGRATIONS.iter().filter(|(v, _)| *v < 7) {
            conn.execute_batch(sql).unwrap();
            conn.execute("INSERT INTO schema_migration(version) VALUES (?1)", [v])
                .unwrap();
        }
        conn.execute(
            "INSERT INTO checkin (id, at, erg_tag) VALUES ('C1', 1000, 'growth'), ('C2', 2000, NULL)",
            [],
        )
        .unwrap();
        migrate(&mut conn).unwrap();
        let rows: Vec<(String, String, String, i64)> = conn
            .prepare("SELECT record_kind, record_id, tag || '/' || source, at FROM erg_label")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(
            rows,
            vec![("checkin".into(), "C1".into(), "growth/user".into(), 1000)]
        );
    }

    #[test]
    fn reopen_file_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("t.db");
        drop(open(&p).unwrap());
        drop(open(&p).unwrap());
    }
}
