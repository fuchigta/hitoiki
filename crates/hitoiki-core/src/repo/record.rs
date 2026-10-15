//! 手入力の記録: `condition` / `concern` / `reframe`。

use crate::model::{Concern, Condition, Reframe};
use crate::{Error, Result};
use rusqlite::{params, Connection, OptionalExtension};

pub fn insert_condition(conn: &Connection, c: &Condition) -> Result<()> {
    conn.execute(
        "INSERT INTO condition (id, at, energy, mood, stress, note, nudge_id) VALUES (?1,?2,?3,?4,?5,?6,?7)",
        params![c.id, c.at, c.energy, c.mood, c.stress, c.note, c.nudge_id],
    )?;
    Ok(())
}

pub fn list_conditions(conn: &Connection, start: i64, end: i64) -> Result<Vec<Condition>> {
    let mut stmt = conn.prepare(
        "SELECT c.id, c.at, c.energy, c.mood, c.stress, \
                (SELECT l.tag FROM erg_label l WHERE l.record_kind = 'condition' AND l.record_id = c.id \
                   AND l.source IN ('user', 'ai') AND l.tag <> 'none'), \
                c.note, c.nudge_id FROM condition c \
         WHERE c.at >= ?1 AND c.at < ?2 ORDER BY c.at, c.id",
    )?;
    let rows = stmt.query_map(params![start, end], |r| {
        let erg: Option<String> = r.get(5)?;
        Ok(Condition {
            id: r.get(0)?,
            at: r.get(1)?,
            energy: r.get(2)?,
            mood: r.get(3)?,
            stress: r.get(4)?,
            erg_tag: erg.and_then(|s| s.parse().ok()),
            note: r.get(6)?,
            nudge_id: r.get(7)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub fn insert_concern(conn: &Connection, h: &Concern) -> Result<()> {
    conn.execute(
        "INSERT INTO concern (id, at, text, intensity, segment_id, nudge_id) VALUES (?1,?2,?3,?4,?5,?6)",
        params![h.id, h.at, h.text, h.intensity, h.segment_id, h.nudge_id],
    )?;
    Ok(())
}

fn concern_from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Concern> {
    Ok(Concern {
        id: r.get(0)?,
        at: r.get(1)?,
        text: r.get(2)?,
        intensity: r.get(3)?,
        segment_id: r.get(4)?,
        nudge_id: r.get(5)?,
    })
}

pub fn get_concern(conn: &Connection, id: &str) -> Result<Concern> {
    conn.query_row(
        "SELECT id, at, text, intensity, segment_id, nudge_id FROM concern WHERE id = ?1",
        [id],
        concern_from_row,
    )
    .optional()?
    .ok_or_else(|| Error::NotFound(format!("気になったこと {id}")))
}

pub fn resolve_concern(conn: &Connection, prefix: &str) -> Result<Concern> {
    get_concern(conn, &super::resolve_prefix(conn, "concern", prefix)?)
}

pub fn list_concerns(conn: &Connection, start: i64, end: i64) -> Result<Vec<Concern>> {
    let mut stmt = conn.prepare(
        "SELECT id, at, text, intensity, segment_id, nudge_id FROM concern WHERE at >= ?1 AND at < ?2 ORDER BY at, id",
    )?;
    let rows = stmt.query_map(params![start, end], concern_from_row)?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub fn insert_reframe(conn: &Connection, a: &Reframe) -> Result<()> {
    conn.execute(
        "INSERT INTO reframe (id, at, event, belief, emotion, \
         emotion_intensity, alternative, deepening_md, concern_id) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
        params![
            a.id,
            a.at,
            a.event,
            a.belief,
            a.emotion,
            a.emotion_intensity,
            a.alternative,
            a.deepening_md,
            a.concern_id
        ],
    )?;
    Ok(())
}

pub fn list_reframe(conn: &Connection, start: i64, end: i64) -> Result<Vec<Reframe>> {
    let mut stmt = conn.prepare(
        "SELECT id, at, event, belief, emotion, emotion_intensity, alternative,          deepening_md, concern_id FROM reframe WHERE at >= ?1 AND at < ?2 ORDER BY at, id",
    )?;
    let rows = stmt.query_map(params![start, end], |r| {
        Ok(Reframe {
            id: r.get(0)?,
            at: r.get(1)?,
            event: r.get(2)?,
            belief: r.get(3)?,
            emotion: r.get(4)?,
            emotion_intensity: r.get(5)?,
            alternative: r.get(6)?,
            deepening_md: r.get(7)?,
            concern_id: r.get(8)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}
