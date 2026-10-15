//! `task_assign_rule` / `activity_rule`。

use crate::model::{ActivityRule, MatchField, PatternType, TaskAssignRule};
use crate::{Error, Result};
use rusqlite::{params, Connection, Row};

fn task_rule_from_row(r: &Row<'_>) -> rusqlite::Result<TaskAssignRule> {
    Ok(TaskAssignRule {
        id: r.get(0)?,
        task_id: r.get(1)?,
        field: r.get(2)?,
        pattern: r.get(3)?,
        pattern_type: r.get(4)?,
        priority: r.get(5)?,
        enabled: r.get::<_, i64>(6)? != 0,
    })
}

pub fn insert_task_rule(conn: &Connection, r: &TaskAssignRule) -> Result<()> {
    conn.execute(
        "INSERT INTO task_assign_rule (id, task_id, field, pattern, pattern_type, priority, enabled) \
         VALUES (?1,?2,?3,?4,?5,?6,?7)",
        params![
            r.id,
            r.task_id,
            r.field,
            r.pattern,
            r.pattern_type,
            r.priority,
            r.enabled
        ],
    )?;
    Ok(())
}

/// タスク指定があればそのタスクのルールだけ。
pub fn list_task_rules(conn: &Connection, task_id: Option<&str>) -> Result<Vec<TaskAssignRule>> {
    let mut stmt = conn.prepare(
        "SELECT id, task_id, field, pattern, pattern_type, priority, enabled FROM task_assign_rule \
         WHERE (?1 IS NULL OR task_id = ?1) ORDER BY priority DESC, id",
    )?;
    let rows = stmt.query_map([task_id], task_rule_from_row)?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub fn find_task_rule(
    conn: &Connection,
    task_id: &str,
    field: MatchField,
    pattern: &str,
    pattern_type: PatternType,
) -> Result<Option<TaskAssignRule>> {
    Ok(list_task_rules(conn, Some(task_id))?
        .into_iter()
        .find(|r| r.field == field && r.pattern == pattern && r.pattern_type == pattern_type))
}

fn activity_rule_from_row(r: &Row<'_>) -> rusqlite::Result<ActivityRule> {
    Ok(ActivityRule {
        id: r.get(0)?,
        field: r.get(1)?,
        pattern: r.get(2)?,
        pattern_type: r.get(3)?,
        activity: r.get(4)?,
        priority: r.get(5)?,
        enabled: r.get::<_, i64>(6)? != 0,
    })
}

pub fn insert_activity_rule(conn: &Connection, r: &ActivityRule) -> Result<()> {
    conn.execute(
        "INSERT INTO activity_rule (id, field, pattern, pattern_type, activity, priority, enabled) \
         VALUES (?1,?2,?3,?4,?5,?6,?7)",
        params![
            r.id,
            r.field,
            r.pattern,
            r.pattern_type,
            r.activity,
            r.priority,
            r.enabled
        ],
    )?;
    Ok(())
}

pub fn list_activity_rules(conn: &Connection) -> Result<Vec<ActivityRule>> {
    let mut stmt = conn.prepare(
        "SELECT id, field, pattern, pattern_type, activity, priority, enabled FROM activity_rule \
         ORDER BY priority DESC, id",
    )?;
    let rows = stmt.query_map([], activity_rule_from_row)?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub fn find_activity_rule(
    conn: &Connection,
    field: MatchField,
    pattern: &str,
    pattern_type: PatternType,
    label: &str,
) -> Result<Option<ActivityRule>> {
    Ok(list_activity_rules(conn)?.into_iter().find(|r| {
        r.field == field && r.pattern == pattern && r.pattern_type == pattern_type && r.activity == label
    }))
}

/// パターンを事前検証する（空、不正な glob / 正規表現）。
pub fn validate_pattern(pattern: &str, ty: PatternType) -> Result<()> {
    if pattern.is_empty() {
        return Err(Error::InvalidArgument("パターンが空です".into()));
    }
    match ty {
        PatternType::Glob => {
            globset::Glob::new(pattern)
                .map_err(|e| Error::InvalidArgument(format!("glob パターンが不正です: {e}")))?;
        }
        PatternType::Regex => {
            regex::Regex::new(pattern)
                .map_err(|e| Error::InvalidArgument(format!("正規表現が不正です: {e}")))?;
        }
        PatternType::Contains => {}
    }
    Ok(())
}
