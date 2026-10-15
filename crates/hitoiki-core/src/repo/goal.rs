//! `goal` / `key_result`。

use crate::model::{Goal, KeyResult};
use crate::{Error, Result};
use rusqlite::{params, Connection, OptionalExtension};

pub fn insert_goal(conn: &Connection, o: &Goal) -> Result<()> {
    conn.execute(
        "INSERT INTO goal (id, title, description_md, period_start, period_end, status) VALUES (?1,?2,?3,?4,?5,?6)",
        params![o.id, o.title, o.description_md, o.period_start, o.period_end, o.status],
    )?;
    Ok(())
}

pub fn list_goals(conn: &Connection) -> Result<Vec<Goal>> {
    let mut stmt = conn.prepare(
        "SELECT id, title, description_md, period_start, period_end, status FROM goal \
         ORDER BY COALESCE(period_start, 0), id",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(Goal {
            id: r.get(0)?,
            title: r.get(1)?,
            description_md: r.get(2)?,
            period_start: r.get(3)?,
            period_end: r.get(4)?,
            status: r.get(5)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub fn resolve_goal(conn: &Connection, prefix: &str) -> Result<String> {
    super::resolve_prefix(conn, "goal", prefix)
}

pub fn insert_key_result(conn: &Connection, k: &KeyResult) -> Result<()> {
    conn.execute(
        "INSERT INTO key_result (id, goal_id, title, target, current, status, measure, active_day_min) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
        params![
            k.id,
            k.goal_id,
            k.title,
            k.target,
            k.current,
            k.status,
            k.measure,
            k.active_day_min
        ],
    )?;
    Ok(())
}

fn key_result_from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<KeyResult> {
    Ok(KeyResult {
        id: r.get(0)?,
        goal_id: r.get(1)?,
        title: r.get(2)?,
        target: r.get(3)?,
        current: r.get(4)?,
        status: r.get(5)?,
        measure: r.get(6)?,
        active_day_min: r.get(7)?,
    })
}

const KEY_RESULT_COLS: &str = "id, goal_id, title, target, current, status, measure, active_day_min";

pub fn list_key_results(conn: &Connection) -> Result<Vec<KeyResult>> {
    let mut stmt = conn.prepare(&format!("SELECT {KEY_RESULT_COLS} FROM key_result ORDER BY id"))?;
    let rows = stmt.query_map([], key_result_from_row)?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub fn get_key_result(conn: &Connection, id: &str) -> Result<KeyResult> {
    conn.query_row(
        &format!("SELECT {KEY_RESULT_COLS} FROM key_result WHERE id = ?1"),
        [id],
        key_result_from_row,
    )
    .optional()?
    .ok_or_else(|| Error::NotFound(format!("成果の指標 {id}")))
}

pub fn resolve_key_result(conn: &Connection, prefix: &str) -> Result<KeyResult> {
    get_key_result(conn, &super::resolve_prefix(conn, "key_result", prefix)?)
}
