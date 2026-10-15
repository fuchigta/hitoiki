//! `task` と `task_doing_log` の読み書き。

use crate::model::{Task, TaskKind, TaskStatus};
use crate::{id::new_id, Error, Millis, Result};
use rusqlite::{params, Connection, OptionalExtension, Row};

const COLS: &str = "id, title, body_md, status, kind, urgent, important, estimate_min, \
                    planned_start_at, planned_end_at, key_result_id, external_url, \
                    created_at, updated_at, done_at";

fn flag(r: &Row<'_>, i: usize) -> rusqlite::Result<Option<bool>> {
    Ok(r.get::<_, Option<i64>>(i)?.map(|v| v != 0))
}

fn from_row(r: &Row<'_>) -> rusqlite::Result<Task> {
    Ok(Task {
        id: r.get(0)?,
        title: r.get(1)?,
        body_md: r.get(2)?,
        status: r.get(3)?,
        kind: r.get(4)?,
        urgent: flag(r, 5)?,
        important: flag(r, 6)?,
        estimate_min: r.get(7)?,
        planned_start_at: r.get(8)?,
        planned_end_at: r.get(9)?,
        key_result_id: r.get(10)?,
        external_url: r.get(11)?,
        created_at: r.get(12)?,
        updated_at: r.get(13)?,
        done_at: r.get(14)?,
    })
}

pub fn insert(conn: &Connection, t: &Task) -> Result<()> {
    conn.execute(
        &format!("INSERT INTO task ({COLS}) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)"),
        params![
            t.id,
            t.title,
            t.body_md,
            t.status,
            t.kind,
            t.urgent,
            t.important,
            t.estimate_min,
            t.planned_start_at,
            t.planned_end_at,
            t.key_result_id,
            t.external_url,
            t.created_at,
            t.updated_at,
            t.done_at
        ],
    )?;
    Ok(())
}

/// ID と作成日時以外の全列を更新する。
pub fn update(conn: &Connection, t: &Task) -> Result<()> {
    let n = conn.execute(
        "UPDATE task SET title=?2, body_md=?3, status=?4, kind=?5, urgent=?6, important=?7, \
         estimate_min=?8, planned_start_at=?9, planned_end_at=?10, key_result_id=?11, \
         external_url=?12, updated_at=?13, done_at=?14 WHERE id=?1",
        params![
            t.id,
            t.title,
            t.body_md,
            t.status,
            t.kind,
            t.urgent,
            t.important,
            t.estimate_min,
            t.planned_start_at,
            t.planned_end_at,
            t.key_result_id,
            t.external_url,
            t.updated_at,
            t.done_at
        ],
    )?;
    if n == 0 {
        return Err(Error::NotFound(format!("タスク {}", t.id)));
    }
    Ok(())
}

/// 成果の指標との紐付けだけを変える（`None` で外す）。
pub fn set_key_result(conn: &Connection, id: &str, key_result_id: Option<&str>, now: Millis) -> Result<()> {
    let n = conn.execute(
        "UPDATE task SET key_result_id = ?2, updated_at = ?3 WHERE id = ?1",
        params![id, key_result_id, now],
    )?;
    if n == 0 {
        return Err(Error::NotFound(format!("タスク {id}")));
    }
    Ok(())
}

/// 成果の指標に紐付いたタスク（状態を問わない）。作成順。
pub fn list_of_key_result(conn: &Connection, key_result_id: &str) -> Result<Vec<Task>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLS} FROM task WHERE key_result_id = ?1 ORDER BY created_at, id"
    ))?;
    let rows = stmt.query_map([key_result_id], from_row)?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub fn get(conn: &Connection, id: &str) -> Result<Task> {
    conn.query_row(&format!("SELECT {COLS} FROM task WHERE id = ?1"), [id], from_row)
        .optional()?
        .ok_or_else(|| Error::NotFound(format!("タスク {id}")))
}

pub fn resolve(conn: &Connection, prefix: &str) -> Result<Task> {
    get(conn, &super::resolve_prefix(conn, "task", prefix)?)
}

/// 状態（空なら全部）と種別で絞り込む。doing → 緊急 → 作成順。
pub fn list(conn: &Connection, statuses: &[TaskStatus], kind: Option<TaskKind>) -> Result<Vec<Task>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLS} FROM task ORDER BY (status = 'doing') DESC, COALESCE(urgent, 0) DESC, created_at, id"
    ))?;
    let rows = stmt.query_map([], from_row)?;
    let mut out = Vec::new();
    for t in rows {
        let t = t?;
        if (statuses.is_empty() || statuses.contains(&t.status)) && kind.is_none_or(|k| k == t.kind) {
            out.push(t);
        }
    }
    Ok(out)
}

pub fn list_doing(conn: &Connection) -> Result<Vec<Task>> {
    list(conn, &[TaskStatus::Doing], None)
}

pub fn open_doing_log(conn: &Connection, task_id: &str, at: Millis) -> Result<()> {
    conn.execute(
        "INSERT INTO task_doing_log (id, task_id, start_at, end_at) VALUES (?1, ?2, ?3, NULL)",
        params![new_id(), task_id, at],
    )?;
    Ok(())
}

/// 指定タスクの開いている doing ログを閉じる。閉じた件数を返す。
pub fn close_doing_logs(conn: &Connection, task_id: &str, at: Millis) -> Result<usize> {
    Ok(conn.execute(
        "UPDATE task_doing_log SET end_at = MAX(?2, start_at) WHERE task_id = ?1 AND end_at IS NULL",
        params![task_id, at],
    )?)
}
