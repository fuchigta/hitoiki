//! テンプレート（`block_template` / `block_template_item` / `block_template_application`）。

use crate::{Millis, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Template {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    /// 割り当てた曜日（1=月 … 7=日、昇順）。
    pub weekdays: Vec<u8>,
    pub created_at: Millis,
    pub updated_at: Millis,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Item {
    pub id: String,
    pub template_id: String,
    /// ローカル時刻の 0 時からの分（0〜1439）。
    pub start_min: u32,
    /// 同（1〜1440）。
    pub end_min: u32,
    pub label: Option<String>,
    pub focus: bool,
    pub task_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Application {
    pub id: String,
    pub template_name: String,
    pub date: String,
    pub applied_at: Millis,
    pub block_ids: Vec<String>,
}

const COLS: &str = "id, name, description, weekdays, created_at, updated_at";

fn from_row(r: &rusqlite::Row) -> rusqlite::Result<Template> {
    let wd: String = r.get(3)?;
    Ok(Template {
        id: r.get(0)?,
        name: r.get(1)?,
        description: r.get(2)?,
        weekdays: serde_json::from_str(&wd).unwrap_or_default(),
        created_at: r.get(4)?,
        updated_at: r.get(5)?,
    })
}

pub fn insert(conn: &Connection, t: &Template) -> Result<()> {
    conn.execute(
        "INSERT INTO block_template (id, name, description, weekdays, created_at, updated_at) \
         VALUES (?1,?2,?3,?4,?5,?6)",
        params![
            t.id,
            t.name,
            t.description,
            serde_json::to_string(&t.weekdays).expect("json"),
            t.created_at,
            t.updated_at
        ],
    )?;
    Ok(())
}

pub fn update(conn: &Connection, t: &Template) -> Result<()> {
    conn.execute(
        "UPDATE block_template SET name = ?2, description = ?3, weekdays = ?4, updated_at = ?5 WHERE id = ?1",
        params![
            t.id,
            t.name,
            t.description,
            serde_json::to_string(&t.weekdays).expect("json"),
            t.updated_at
        ],
    )?;
    Ok(())
}

/// テンプレートと中のブロックを削除する。削除できたら true。
pub fn delete(conn: &Connection, id: &str) -> Result<bool> {
    conn.execute("DELETE FROM block_template_item WHERE template_id = ?1", [id])?;
    Ok(conn.execute("DELETE FROM block_template WHERE id = ?1", [id])? > 0)
}

pub fn get(conn: &Connection, id: &str) -> Result<Option<Template>> {
    Ok(conn
        .query_row(
            &format!("SELECT {COLS} FROM block_template WHERE id = ?1"),
            [id],
            from_row,
        )
        .optional()?)
}

pub fn find_by_name(conn: &Connection, name: &str) -> Result<Option<Template>> {
    Ok(conn
        .query_row(
            &format!("SELECT {COLS} FROM block_template WHERE name = ?1"),
            [name],
            from_row,
        )
        .optional()?)
}

pub fn list(conn: &Connection) -> Result<Vec<Template>> {
    let mut stmt = conn.prepare(&format!("SELECT {COLS} FROM block_template ORDER BY name, id"))?;
    let rows = stmt.query_map([], from_row)?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub fn items(conn: &Connection, template_id: &str) -> Result<Vec<Item>> {
    let mut stmt = conn.prepare(
        "SELECT id, template_id, start_min, end_min, label, focus, task_id FROM block_template_item \
         WHERE template_id = ?1 ORDER BY start_min, end_min, id",
    )?;
    let rows = stmt.query_map([template_id], |r| {
        Ok(Item {
            id: r.get(0)?,
            template_id: r.get(1)?,
            start_min: r.get(2)?,
            end_min: r.get(3)?,
            label: r.get(4)?,
            focus: r.get::<_, i64>(5)? != 0,
            task_id: r.get(6)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub fn insert_item(conn: &Connection, i: &Item) -> Result<()> {
    conn.execute(
        "INSERT INTO block_template_item (id, template_id, start_min, end_min, label, focus, task_id) \
         VALUES (?1,?2,?3,?4,?5,?6,?7)",
        params![
            i.id,
            i.template_id,
            i.start_min,
            i.end_min,
            i.label,
            i.focus,
            i.task_id
        ],
    )?;
    Ok(())
}

pub fn delete_items(conn: &Connection, template_id: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM block_template_item WHERE template_id = ?1",
        [template_id],
    )?;
    Ok(())
}

pub fn insert_application(conn: &Connection, a: &Application) -> Result<()> {
    conn.execute(
        "INSERT INTO block_template_application (id, template_name, date, applied_at, block_ids) \
         VALUES (?1,?2,?3,?4,?5)",
        params![
            a.id,
            a.template_name,
            a.date,
            a.applied_at,
            serde_json::to_string(&a.block_ids).expect("json")
        ],
    )?;
    Ok(())
}

/// 取り消していない直近の適用（日付を指定すればその日のもの）。
pub fn last_application(conn: &Connection, date: Option<&str>) -> Result<Option<Application>> {
    Ok(conn
        .query_row(
            "SELECT id, template_name, date, applied_at, block_ids FROM block_template_application \
             WHERE undone_at IS NULL AND (?1 IS NULL OR date = ?1) \
             ORDER BY applied_at DESC, rowid DESC LIMIT 1",
            [date],
            |r| {
                let ids: String = r.get(4)?;
                Ok(Application {
                    id: r.get(0)?,
                    template_name: r.get(1)?,
                    date: r.get(2)?,
                    applied_at: r.get(3)?,
                    block_ids: serde_json::from_str(&ids).unwrap_or_default(),
                })
            },
        )
        .optional()?)
}

pub fn mark_undone(conn: &Connection, id: &str, at: Millis) -> Result<()> {
    conn.execute(
        "UPDATE block_template_application SET undone_at = ?2 WHERE id = ?1",
        params![id, at],
    )?;
    Ok(())
}
