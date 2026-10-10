//! 手入力の記録: `checkin` / `hassle` / `abc_record`。

use crate::model::{AbcRecord, Checkin, Hassle};
use crate::{Error, Result};
use rusqlite::{params, Connection, OptionalExtension};

pub fn insert_checkin(conn: &Connection, c: &Checkin) -> Result<()> {
    conn.execute(
        "INSERT INTO checkin (id, at, energy, mood, stress, erg_tag, note, push_id) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
        params![
            c.id,
            c.at,
            c.energy,
            c.mood,
            c.stress,
            c.erg_tag.map(|e| e.as_str()),
            c.note,
            c.push_id
        ],
    )?;
    Ok(())
}

pub fn list_checkins(conn: &Connection, start: i64, end: i64) -> Result<Vec<Checkin>> {
    let mut stmt = conn.prepare(
        "SELECT id, at, energy, mood, stress, erg_tag, note, push_id FROM checkin \
         WHERE at >= ?1 AND at < ?2 ORDER BY at, id",
    )?;
    let rows = stmt.query_map(params![start, end], |r| {
        let erg: Option<String> = r.get(5)?;
        Ok(Checkin {
            id: r.get(0)?,
            at: r.get(1)?,
            energy: r.get(2)?,
            mood: r.get(3)?,
            stress: r.get(4)?,
            erg_tag: erg.and_then(|s| s.parse().ok()),
            note: r.get(6)?,
            push_id: r.get(7)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub fn insert_hassle(conn: &Connection, h: &Hassle) -> Result<()> {
    conn.execute(
        "INSERT INTO hassle (id, at, text, intensity, segment_id, push_id) VALUES (?1,?2,?3,?4,?5,?6)",
        params![h.id, h.at, h.text, h.intensity, h.segment_id, h.push_id],
    )?;
    Ok(())
}

fn hassle_from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Hassle> {
    Ok(Hassle {
        id: r.get(0)?,
        at: r.get(1)?,
        text: r.get(2)?,
        intensity: r.get(3)?,
        segment_id: r.get(4)?,
        push_id: r.get(5)?,
    })
}

pub fn get_hassle(conn: &Connection, id: &str) -> Result<Hassle> {
    conn.query_row(
        "SELECT id, at, text, intensity, segment_id, push_id FROM hassle WHERE id = ?1",
        [id],
        hassle_from_row,
    )
    .optional()?
    .ok_or_else(|| Error::NotFound(format!("ハッスル {id}")))
}

pub fn resolve_hassle(conn: &Connection, prefix: &str) -> Result<Hassle> {
    get_hassle(conn, &super::resolve_prefix(conn, "hassle", prefix)?)
}

pub fn list_hassles(conn: &Connection, start: i64, end: i64) -> Result<Vec<Hassle>> {
    let mut stmt = conn.prepare(
        "SELECT id, at, text, intensity, segment_id, push_id FROM hassle WHERE at >= ?1 AND at < ?2 ORDER BY at, id",
    )?;
    let rows = stmt.query_map(params![start, end], hassle_from_row)?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub fn insert_abc(conn: &Connection, a: &AbcRecord) -> Result<()> {
    conn.execute(
        "INSERT INTO abc_record (id, at, activating_event, belief, consequence_emotion, \
         consequence_intensity, dispute, ai_dialog_md, hassle_id) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
        params![
            a.id,
            a.at,
            a.activating_event,
            a.belief,
            a.consequence_emotion,
            a.consequence_intensity,
            a.dispute,
            a.ai_dialog_md,
            a.hassle_id
        ],
    )?;
    Ok(())
}
