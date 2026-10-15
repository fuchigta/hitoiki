//! 編集・削除系の DB アクセス（目標・スキル・ルール・記録）。
//!
//! 外部キーが有効なので、参照されている行を消す前に参照側を外す（NULL 化または削除）。

use crate::model::{ActivityRule, Concern, Condition, Goal, KeyResult, Reframe, Skill, TaskAssignRule};
use crate::{Error, Result};
use rusqlite::{params, Connection, OptionalExtension};

fn count(conn: &Connection, sql: &str, id: &str) -> Result<i64> {
    Ok(conn.query_row(sql, [id], |r| r.get(0))?)
}

// ===== 目標 =====

pub fn get_goal(conn: &Connection, id: &str) -> Result<Goal> {
    conn.query_row(
        "SELECT id, title, description_md, period_start, period_end, status FROM goal WHERE id = ?1",
        [id],
        |r| {
            Ok(Goal {
                id: r.get(0)?,
                title: r.get(1)?,
                description_md: r.get(2)?,
                period_start: r.get(3)?,
                period_end: r.get(4)?,
                status: r.get(5)?,
            })
        },
    )
    .optional()?
    .ok_or_else(|| Error::NotFound(format!("目標 {id}")))
}

pub fn update_goal(conn: &Connection, o: &Goal) -> Result<()> {
    conn.execute(
        "UPDATE goal SET title = ?2, description_md = ?3, period_start = ?4, period_end = ?5, status = ?6 WHERE id = ?1",
        params![o.id, o.title, o.description_md, o.period_start, o.period_end, o.status],
    )?;
    Ok(())
}

pub fn update_key_result(conn: &Connection, k: &KeyResult) -> Result<()> {
    conn.execute(
        "UPDATE key_result SET goal_id = ?2, title = ?3, target = ?4, current = ?5, status = ?6, measure = ?7, active_day_min = ?8 WHERE id = ?1",
        params![k.id, k.goal_id, k.title, k.target, k.current, k.status, k.measure, k.active_day_min],
    )?;
    Ok(())
}

pub fn count_key_results_of(conn: &Connection, goal_id: &str) -> Result<i64> {
    count(
        conn,
        "SELECT COUNT(*) FROM key_result WHERE goal_id = ?1",
        goal_id,
    )
}

pub fn count_tasks_of_goal(conn: &Connection, goal_id: &str) -> Result<i64> {
    count(
        conn,
        "SELECT COUNT(*) FROM task WHERE key_result_id IN (SELECT id FROM key_result WHERE goal_id = ?1)",
        goal_id,
    )
}

pub fn count_tasks_of_key_result(conn: &Connection, key_result_id: &str) -> Result<i64> {
    count(
        conn,
        "SELECT COUNT(*) FROM task WHERE key_result_id = ?1",
        key_result_id,
    )
}

/// 成果の指標を消す。紐付いたタスクの `key_result_id` は NULL に戻す（タスクは残す）。
pub fn delete_key_result(conn: &Connection, key_result_id: &str) -> Result<()> {
    conn.execute(
        "UPDATE task SET key_result_id = NULL WHERE key_result_id = ?1",
        [key_result_id],
    )?;
    conn.execute(
        "DELETE FROM key_result_skill WHERE key_result_id = ?1",
        [key_result_id],
    )?;
    conn.execute(
        "DELETE FROM key_result_progress WHERE key_result_id = ?1",
        [key_result_id],
    )?;
    conn.execute(
        "DELETE FROM link_suggestion_reject WHERE key_result_id = ?1",
        [key_result_id],
    )?;
    conn.execute("DELETE FROM key_result WHERE id = ?1", [key_result_id])?;
    Ok(())
}

/// 目標と配下の成果の指標を消す。成果の指標に紐付いたタスクの参照は NULL に戻す。
pub fn delete_goal(conn: &Connection, goal_id: &str) -> Result<()> {
    conn.execute(
        "UPDATE task SET key_result_id = NULL WHERE key_result_id IN (SELECT id FROM key_result WHERE goal_id = ?1)",
        [goal_id],
    )?;
    for table in [
        "key_result_skill",
        "key_result_progress",
        "link_suggestion_reject",
    ] {
        conn.execute(
            &format!(
                "DELETE FROM {table} WHERE key_result_id IN (SELECT id FROM key_result WHERE goal_id = ?1)"
            ),
            [goal_id],
        )?;
    }
    conn.execute("DELETE FROM key_result WHERE goal_id = ?1", [goal_id])?;
    conn.execute("DELETE FROM goal WHERE id = ?1", [goal_id])?;
    Ok(())
}

// ===== スキル =====

pub fn update_skill(conn: &Connection, s: &Skill) -> Result<()> {
    conn.execute(
        "UPDATE skill SET name = ?2, parent_id = ?3, description = ?4 WHERE id = ?1",
        params![s.id, s.name, s.parent_id, s.description],
    )?;
    Ok(())
}

pub fn children_of(conn: &Connection, skill_id: &str) -> Result<Vec<String>> {
    let mut stmt = conn.prepare("SELECT id FROM skill WHERE parent_id = ?1 ORDER BY name")?;
    let rows = stmt.query_map([skill_id], |r| r.get(0))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// 指定スキルが参照されている `task_skill` の行数。
pub fn count_task_skill(conn: &Connection, skill_id: &str) -> Result<i64> {
    count(
        conn,
        "SELECT COUNT(*) FROM task_skill WHERE skill_id = ?1",
        skill_id,
    )
}

pub fn count_activity_skill_rules(conn: &Connection, skill_id: &str) -> Result<i64> {
    count(
        conn,
        "SELECT COUNT(*) FROM activity_skill_rule WHERE skill_id = ?1",
        skill_id,
    )
}

/// 指定スキルが付いた成果の指標の数（`key_result_skill` の行数）。
pub fn count_key_result_skill(conn: &Connection, skill_id: &str) -> Result<i64> {
    count(
        conn,
        "SELECT COUNT(*) FROM key_result_skill WHERE skill_id = ?1",
        skill_id,
    )
}

/// スキル 1 件を消す（`task_skill`・`key_result_skill`・`activity_skill_rule` の参照も消す）。子の付け替えは呼び出し側で先に行う。
pub fn delete_skill_row(conn: &Connection, skill_id: &str) -> Result<()> {
    conn.execute("DELETE FROM task_skill WHERE skill_id = ?1", [skill_id])?;
    conn.execute("DELETE FROM key_result_skill WHERE skill_id = ?1", [skill_id])?;
    conn.execute("DELETE FROM activity_skill_rule WHERE skill_id = ?1", [skill_id])?;
    conn.execute("DELETE FROM skill WHERE id = ?1", [skill_id])?;
    Ok(())
}

pub fn set_skill_parent(conn: &Connection, skill_id: &str, parent: Option<&str>) -> Result<()> {
    conn.execute(
        "UPDATE skill SET parent_id = ?2 WHERE id = ?1",
        params![skill_id, parent],
    )?;
    Ok(())
}

// ===== ルール =====

pub fn get_task_rule(conn: &Connection, id: &str) -> Result<TaskAssignRule> {
    conn.query_row(
        "SELECT id, task_id, field, pattern, pattern_type, priority, enabled FROM task_assign_rule WHERE id = ?1",
        [id],
        |r| {
            Ok(TaskAssignRule {
                id: r.get(0)?,
                task_id: r.get(1)?,
                field: r.get(2)?,
                pattern: r.get(3)?,
                pattern_type: r.get(4)?,
                priority: r.get(5)?,
                enabled: r.get::<_, i64>(6)? != 0,
            })
        },
    )
    .optional()?
    .ok_or_else(|| Error::NotFound(format!("ルール {id}")))
}

pub fn update_task_rule(conn: &Connection, r: &TaskAssignRule) -> Result<()> {
    conn.execute(
        "UPDATE task_assign_rule SET field = ?2, pattern = ?3, pattern_type = ?4, priority = ?5, enabled = ?6 WHERE id = ?1",
        params![r.id, r.field, r.pattern, r.pattern_type, r.priority, r.enabled],
    )?;
    Ok(())
}

pub fn delete_task_rule(conn: &Connection, id: &str) -> Result<()> {
    conn.execute("DELETE FROM task_assign_rule WHERE id = ?1", [id])?;
    Ok(())
}

pub fn get_activity_rule(conn: &Connection, id: &str) -> Result<ActivityRule> {
    conn.query_row(
        "SELECT id, field, pattern, pattern_type, activity, priority, enabled FROM activity_rule WHERE id = ?1",
        [id],
        |r| {
            Ok(ActivityRule {
                id: r.get(0)?,
                field: r.get(1)?,
                pattern: r.get(2)?,
                pattern_type: r.get(3)?,
                activity: r.get(4)?,
                priority: r.get(5)?,
                enabled: r.get::<_, i64>(6)? != 0,
            })
        },
    )
    .optional()?
    .ok_or_else(|| Error::NotFound(format!("活動ルール {id}")))
}

pub fn update_activity_rule(conn: &Connection, r: &ActivityRule) -> Result<()> {
    conn.execute(
        "UPDATE activity_rule SET field = ?2, pattern = ?3, pattern_type = ?4, activity = ?5, priority = ?6, enabled = ?7 WHERE id = ?1",
        params![r.id, r.field, r.pattern, r.pattern_type, r.activity, r.priority, r.enabled],
    )?;
    Ok(())
}

pub fn delete_activity_rule(conn: &Connection, id: &str) -> Result<()> {
    conn.execute("DELETE FROM activity_rule WHERE id = ?1", [id])?;
    Ok(())
}

// ===== 記録 =====

pub fn get_condition(conn: &Connection, id: &str) -> Result<Condition> {
    let (s, e) = (0, i64::MAX);
    super::record::list_conditions(conn, s, e)?
        .into_iter()
        .find(|c| c.id == id)
        .ok_or_else(|| Error::NotFound(format!("調子 {id}")))
}

pub fn update_condition(conn: &Connection, c: &Condition) -> Result<()> {
    conn.execute(
        "UPDATE condition SET energy = ?2, mood = ?3, stress = ?4, note = ?5 WHERE id = ?1",
        params![c.id, c.energy, c.mood, c.stress, c.note],
    )?;
    Ok(())
}

pub fn delete_condition(conn: &Connection, id: &str) -> Result<()> {
    conn.execute("DELETE FROM condition WHERE id = ?1", [id])?;
    conn.execute(
        "DELETE FROM erg_label WHERE record_kind = 'condition' AND record_id = ?1",
        [id],
    )?;
    Ok(())
}

pub fn update_concern(conn: &Connection, h: &Concern) -> Result<()> {
    conn.execute(
        "UPDATE concern SET text = ?2, intensity = ?3 WHERE id = ?1",
        params![h.id, h.text, h.intensity],
    )?;
    Ok(())
}

/// 気になったことを元にした捉え直しの数。
pub fn count_reframe_of_concern(conn: &Connection, concern_id: &str) -> Result<i64> {
    count(
        conn,
        "SELECT COUNT(*) FROM reframe WHERE concern_id = ?1",
        concern_id,
    )
}

/// 気になったことを消す。元にした捉え直しは残し、`concern_id` だけ NULL に戻す。
pub fn delete_concern(conn: &Connection, id: &str) -> Result<()> {
    conn.execute("UPDATE reframe SET concern_id = NULL WHERE concern_id = ?1", [id])?;
    conn.execute("DELETE FROM concern WHERE id = ?1", [id])?;
    conn.execute(
        "DELETE FROM erg_label WHERE record_kind = 'concern' AND record_id = ?1",
        [id],
    )?;
    Ok(())
}

pub fn get_reframe(conn: &Connection, id: &str) -> Result<Reframe> {
    super::record::list_reframe(conn, 0, i64::MAX)?
        .into_iter()
        .find(|a| a.id == id)
        .ok_or_else(|| Error::NotFound(format!("捉え直し {id}")))
}

pub fn update_reframe(conn: &Connection, a: &Reframe) -> Result<()> {
    conn.execute(
        "UPDATE reframe SET event = ?2, belief = ?3, emotion = ?4, \
         emotion_intensity = ?5, alternative = ?6 WHERE id = ?1",
        params![
            a.id,
            a.event,
            a.belief,
            a.emotion,
            a.emotion_intensity,
            a.alternative
        ],
    )?;
    Ok(())
}

pub fn delete_reframe(conn: &Connection, id: &str) -> Result<()> {
    conn.execute("DELETE FROM reframe WHERE id = ?1", [id])?;
    conn.execute(
        "DELETE FROM erg_label WHERE record_kind = 'reframe' AND record_id = ?1",
        [id],
    )?;
    Ok(())
}

// ===== カレンダー =====

pub fn count_calendar_events(conn: &Connection, source: &str) -> Result<i64> {
    count(
        conn,
        "SELECT COUNT(*) FROM calendar_event WHERE source = ?1",
        source,
    )
}

pub fn delete_calendar_events(conn: &Connection, source: &str) -> Result<()> {
    conn.execute("DELETE FROM calendar_event WHERE source = ?1", [source])?;
    Ok(())
}

pub fn rename_calendar_source(conn: &Connection, from: &str, to: &str) -> Result<()> {
    conn.execute(
        "UPDATE calendar_event SET source = ?2 WHERE source = ?1",
        params![from, to],
    )?;
    Ok(())
}
