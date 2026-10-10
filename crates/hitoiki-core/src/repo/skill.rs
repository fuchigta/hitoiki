//! `skill` / `task_skill` とプリセット。

use crate::model::Skill;
use crate::{Error, Result};
use rusqlite::{params, Connection, OptionalExtension};

/// 同梱プリセット（名前, 親の名前）。親は必ず子より前に並べる。
pub const PRESETS: &[(&str, Option<&str>)] = &[
    ("プログラミング言語", None),
    ("Rust", Some("プログラミング言語")),
    ("TypeScript", Some("プログラミング言語")),
    ("Python", Some("プログラミング言語")),
    ("Go", Some("プログラミング言語")),
    ("Java", Some("プログラミング言語")),
    ("SQL", Some("プログラミング言語")),
    ("設計", None),
    ("コードレビュー", None),
    ("テスト", None),
    ("インフラ", None),
    ("ドキュメント", None),
    ("コミュニケーション", None),
    ("タスク管理", None),
];

fn from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Skill> {
    Ok(Skill {
        id: r.get(0)?,
        name: r.get(1)?,
        parent_id: r.get(2)?,
        description: r.get(3)?,
        preset: r.get::<_, i64>(4)? != 0,
    })
}

pub fn insert(conn: &Connection, s: &Skill) -> Result<()> {
    conn.execute(
        "INSERT INTO skill (id, name, parent_id, description, preset) VALUES (?1,?2,?3,?4,?5)",
        params![s.id, s.name, s.parent_id, s.description, s.preset],
    )?;
    Ok(())
}

pub fn list(conn: &Connection) -> Result<Vec<Skill>> {
    let mut stmt =
        conn.prepare("SELECT id, name, parent_id, description, preset FROM skill ORDER BY name")?;
    let rows = stmt.query_map([], from_row)?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub fn find_by_name(conn: &Connection, name: &str) -> Result<Option<Skill>> {
    Ok(conn
        .query_row(
            "SELECT id, name, parent_id, description, preset FROM skill WHERE name = ?1",
            [name],
            from_row,
        )
        .optional()?)
}

pub fn get_by_name(conn: &Connection, name: &str) -> Result<Skill> {
    find_by_name(conn, name)?.ok_or_else(|| {
        Error::NotFound(format!(
            "スキル「{name}」(先に `hitoiki skill add` で追加してください)"
        ))
    })
}

pub fn resolve(conn: &Connection, prefix: &str) -> Result<Skill> {
    let id = super::resolve_prefix(conn, "skill", prefix)?;
    Ok(conn.query_row(
        "SELECT id, name, parent_id, description, preset FROM skill WHERE id = ?1",
        [id],
        from_row,
    )?)
}

/// タスクのスキルを置き換える。
pub fn set_task_skills(conn: &Connection, task_id: &str, skill_ids: &[String]) -> Result<()> {
    conn.execute("DELETE FROM task_skill WHERE task_id = ?1", [task_id])?;
    for s in skill_ids {
        conn.execute(
            "INSERT OR IGNORE INTO task_skill (task_id, skill_id) VALUES (?1, ?2)",
            params![task_id, s],
        )?;
    }
    Ok(())
}

/// タスクのスキル名（名前順）。
pub fn task_skill_names(conn: &Connection, task_id: &str) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT s.name FROM task_skill ts JOIN skill s ON s.id = ts.skill_id WHERE ts.task_id = ?1 ORDER BY s.name",
    )?;
    let rows = stmt.query_map([task_id], |r| r.get(0))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}
