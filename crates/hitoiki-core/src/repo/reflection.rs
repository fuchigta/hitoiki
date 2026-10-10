//! `reflection`（AI 振り返り）と `abc_record` の AI 対話列の読み書き。

use crate::model::{AbcRecord, Reflection, ReflectionKind};
use crate::{Error, Result};
use rusqlite::{params, Connection, OptionalExtension};

pub fn insert(conn: &Connection, r: &Reflection) -> Result<()> {
    conn.execute(
        "INSERT INTO reflection (id, at, kind, context_json, questions_md, answers_md, summary_md) \
         VALUES (?1,?2,?3,?4,?5,?6,?7)",
        params![
            r.id,
            r.at,
            r.kind,
            r.context_json,
            r.questions_md,
            r.answers_md,
            r.summary_md
        ],
    )?;
    Ok(())
}

fn from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Reflection> {
    Ok(Reflection {
        id: r.get(0)?,
        at: r.get(1)?,
        kind: r.get(2)?,
        context_json: r.get(3)?,
        questions_md: r.get(4)?,
        answers_md: r.get(5)?,
        summary_md: r.get(6)?,
    })
}

const COLS: &str = "id, at, kind, context_json, questions_md, answers_md, summary_md";

pub fn get(conn: &Connection, id: &str) -> Result<Reflection> {
    conn.query_row(
        &format!("SELECT {COLS} FROM reflection WHERE id = ?1"),
        [id],
        from_row,
    )
    .optional()?
    .ok_or_else(|| Error::NotFound(format!("振り返り {id}")))
}

pub fn resolve(conn: &Connection, prefix: &str) -> Result<Reflection> {
    get(conn, &super::resolve_prefix(conn, "reflection", prefix)?)
}

/// 新しい順。`kind` 指定で絞り込み。
pub fn list(conn: &Connection, kind: Option<ReflectionKind>, limit: i64) -> Result<Vec<Reflection>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLS} FROM reflection WHERE (?1 IS NULL OR kind = ?1) ORDER BY at DESC, id DESC LIMIT ?2"
    ))?;
    let rows = stmt.query_map(params![kind, limit], from_row)?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub fn set_answers(conn: &Connection, id: &str, answers_md: &str) -> Result<()> {
    conn.execute(
        "UPDATE reflection SET answers_md = ?2 WHERE id = ?1",
        params![id, answers_md],
    )?;
    Ok(())
}

pub fn get_abc(conn: &Connection, id: &str) -> Result<AbcRecord> {
    conn.query_row(
        "SELECT id, at, activating_event, belief, consequence_emotion, consequence_intensity, dispute, \
         ai_dialog_md, hassle_id FROM abc_record WHERE id = ?1",
        [id],
        |r| {
            Ok(AbcRecord {
                id: r.get(0)?,
                at: r.get(1)?,
                activating_event: r.get(2)?,
                belief: r.get(3)?,
                consequence_emotion: r.get(4)?,
                consequence_intensity: r.get(5)?,
                dispute: r.get(6)?,
                ai_dialog_md: r.get(7)?,
                hassle_id: r.get(8)?,
            })
        },
    )
    .optional()?
    .ok_or_else(|| Error::NotFound(format!("ABC 記録 {id}")))
}

pub fn resolve_abc(conn: &Connection, prefix: &str) -> Result<AbcRecord> {
    get_abc(conn, &super::resolve_prefix(conn, "abc_record", prefix)?)
}

pub fn set_abc_dialog(conn: &Connection, id: &str, ai_dialog_md: &str) -> Result<()> {
    conn.execute(
        "UPDATE abc_record SET ai_dialog_md = ?2 WHERE id = ?1",
        params![id, ai_dialog_md],
    )?;
    Ok(())
}
