//! `objective` / `key_result`。

use crate::model::{KeyResult, Objective};
use crate::{Error, Result};
use rusqlite::{params, Connection, OptionalExtension};

pub fn insert_objective(conn: &Connection, o: &Objective) -> Result<()> {
    conn.execute(
        "INSERT INTO objective (id, title, description_md, period_start, period_end, status) VALUES (?1,?2,?3,?4,?5,?6)",
        params![o.id, o.title, o.description_md, o.period_start, o.period_end, o.status],
    )?;
    Ok(())
}

pub fn list_objectives(conn: &Connection) -> Result<Vec<Objective>> {
    let mut stmt = conn.prepare(
        "SELECT id, title, description_md, period_start, period_end, status FROM objective \
         ORDER BY COALESCE(period_start, 0), id",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(Objective {
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

pub fn resolve_objective(conn: &Connection, prefix: &str) -> Result<String> {
    super::resolve_prefix(conn, "objective", prefix)
}

pub fn insert_kr(conn: &Connection, k: &KeyResult) -> Result<()> {
    conn.execute(
        "INSERT INTO key_result (id, objective_id, title, target, current, status, measure, day_min) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
        params![
            k.id,
            k.objective_id,
            k.title,
            k.target,
            k.current,
            k.status,
            k.measure,
            k.day_min
        ],
    )?;
    Ok(())
}

fn kr_from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<KeyResult> {
    Ok(KeyResult {
        id: r.get(0)?,
        objective_id: r.get(1)?,
        title: r.get(2)?,
        target: r.get(3)?,
        current: r.get(4)?,
        status: r.get(5)?,
        measure: r.get(6)?,
        day_min: r.get(7)?,
    })
}

const KR_COLS: &str = "id, objective_id, title, target, current, status, measure, day_min";

pub fn list_krs(conn: &Connection) -> Result<Vec<KeyResult>> {
    let mut stmt = conn.prepare(&format!("SELECT {KR_COLS} FROM key_result ORDER BY id"))?;
    let rows = stmt.query_map([], kr_from_row)?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub fn get_kr(conn: &Connection, id: &str) -> Result<KeyResult> {
    conn.query_row(
        &format!("SELECT {KR_COLS} FROM key_result WHERE id = ?1"),
        [id],
        kr_from_row,
    )
    .optional()?
    .ok_or_else(|| Error::NotFound(format!("成果の指標 {id}")))
}

pub fn resolve_kr(conn: &Connection, prefix: &str) -> Result<KeyResult> {
    get_kr(conn, &super::resolve_prefix(conn, "key_result", prefix)?)
}
