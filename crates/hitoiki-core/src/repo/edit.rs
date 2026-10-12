//! 編集・削除系の DB アクセス（OKR・スキル・ルール・記録）。
//!
//! 外部キーが有効なので、参照されている行を消す前に参照側を外す（NULL 化または削除）。

use crate::model::{AbcRecord, ActivityRule, Checkin, Hassle, KeyResult, Objective, Skill, TaskMatchRule};
use crate::{Error, Result};
use rusqlite::{params, Connection, OptionalExtension};

fn count(conn: &Connection, sql: &str, id: &str) -> Result<i64> {
    Ok(conn.query_row(sql, [id], |r| r.get(0))?)
}

// ===== OKR =====

pub fn get_objective(conn: &Connection, id: &str) -> Result<Objective> {
    conn.query_row(
        "SELECT id, title, description_md, period_start, period_end, status FROM objective WHERE id = ?1",
        [id],
        |r| {
            Ok(Objective {
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
    .ok_or_else(|| Error::NotFound(format!("Objective {id}")))
}

pub fn update_objective(conn: &Connection, o: &Objective) -> Result<()> {
    conn.execute(
        "UPDATE objective SET title = ?2, description_md = ?3, period_start = ?4, period_end = ?5, status = ?6 WHERE id = ?1",
        params![o.id, o.title, o.description_md, o.period_start, o.period_end, o.status],
    )?;
    Ok(())
}

pub fn update_kr(conn: &Connection, k: &KeyResult) -> Result<()> {
    conn.execute(
        "UPDATE key_result SET objective_id = ?2, title = ?3, target = ?4, current = ?5, status = ?6 WHERE id = ?1",
        params![k.id, k.objective_id, k.title, k.target, k.current, k.status],
    )?;
    Ok(())
}

pub fn count_krs_of(conn: &Connection, objective_id: &str) -> Result<i64> {
    count(
        conn,
        "SELECT COUNT(*) FROM key_result WHERE objective_id = ?1",
        objective_id,
    )
}

pub fn count_tasks_of_objective(conn: &Connection, objective_id: &str) -> Result<i64> {
    count(
        conn,
        "SELECT COUNT(*) FROM task WHERE key_result_id IN (SELECT id FROM key_result WHERE objective_id = ?1)",
        objective_id,
    )
}

pub fn count_tasks_of_kr(conn: &Connection, kr_id: &str) -> Result<i64> {
    count(conn, "SELECT COUNT(*) FROM task WHERE key_result_id = ?1", kr_id)
}

/// KR を消す。紐付いたタスクの `key_result_id` は NULL に戻す（タスクは残す）。
pub fn delete_kr(conn: &Connection, kr_id: &str) -> Result<()> {
    conn.execute(
        "UPDATE task SET key_result_id = NULL WHERE key_result_id = ?1",
        [kr_id],
    )?;
    conn.execute("DELETE FROM key_result WHERE id = ?1", [kr_id])?;
    Ok(())
}

/// Objective と配下の KR を消す。KR に紐付いたタスクの参照は NULL に戻す。
pub fn delete_objective(conn: &Connection, objective_id: &str) -> Result<()> {
    conn.execute(
        "UPDATE task SET key_result_id = NULL WHERE key_result_id IN (SELECT id FROM key_result WHERE objective_id = ?1)",
        [objective_id],
    )?;
    conn.execute("DELETE FROM key_result WHERE objective_id = ?1", [objective_id])?;
    conn.execute("DELETE FROM objective WHERE id = ?1", [objective_id])?;
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

/// スキル 1 件を消す（`task_skill` と `activity_skill_rule` の参照も消す）。子の付け替えは呼び出し側で先に行う。
pub fn delete_skill_row(conn: &Connection, skill_id: &str) -> Result<()> {
    conn.execute("DELETE FROM task_skill WHERE skill_id = ?1", [skill_id])?;
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

pub fn get_task_rule(conn: &Connection, id: &str) -> Result<TaskMatchRule> {
    conn.query_row(
        "SELECT id, task_id, field, pattern, pattern_type, priority, enabled FROM task_match_rule WHERE id = ?1",
        [id],
        |r| {
            Ok(TaskMatchRule {
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

pub fn update_task_rule(conn: &Connection, r: &TaskMatchRule) -> Result<()> {
    conn.execute(
        "UPDATE task_match_rule SET field = ?2, pattern = ?3, pattern_type = ?4, priority = ?5, enabled = ?6 WHERE id = ?1",
        params![r.id, r.field, r.pattern, r.pattern_type, r.priority, r.enabled],
    )?;
    Ok(())
}

pub fn delete_task_rule(conn: &Connection, id: &str) -> Result<()> {
    conn.execute("DELETE FROM task_match_rule WHERE id = ?1", [id])?;
    Ok(())
}

pub fn get_activity_rule(conn: &Connection, id: &str) -> Result<ActivityRule> {
    conn.query_row(
        "SELECT id, field, pattern, pattern_type, activity_label, priority, enabled FROM activity_rule WHERE id = ?1",
        [id],
        |r| {
            Ok(ActivityRule {
                id: r.get(0)?,
                field: r.get(1)?,
                pattern: r.get(2)?,
                pattern_type: r.get(3)?,
                activity_label: r.get(4)?,
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
        "UPDATE activity_rule SET field = ?2, pattern = ?3, pattern_type = ?4, activity_label = ?5, priority = ?6, enabled = ?7 WHERE id = ?1",
        params![r.id, r.field, r.pattern, r.pattern_type, r.activity_label, r.priority, r.enabled],
    )?;
    Ok(())
}

pub fn delete_activity_rule(conn: &Connection, id: &str) -> Result<()> {
    conn.execute("DELETE FROM activity_rule WHERE id = ?1", [id])?;
    Ok(())
}

// ===== 記録 =====

pub fn get_checkin(conn: &Connection, id: &str) -> Result<Checkin> {
    let (s, e) = (0, i64::MAX);
    super::record::list_checkins(conn, s, e)?
        .into_iter()
        .find(|c| c.id == id)
        .ok_or_else(|| Error::NotFound(format!("チェックイン {id}")))
}

pub fn update_checkin(conn: &Connection, c: &Checkin) -> Result<()> {
    conn.execute(
        "UPDATE checkin SET energy = ?2, mood = ?3, stress = ?4, note = ?5 WHERE id = ?1",
        params![c.id, c.energy, c.mood, c.stress, c.note],
    )?;
    Ok(())
}

pub fn delete_checkin(conn: &Connection, id: &str) -> Result<()> {
    conn.execute("DELETE FROM checkin WHERE id = ?1", [id])?;
    conn.execute(
        "DELETE FROM erg_label WHERE record_kind = 'checkin' AND record_id = ?1",
        [id],
    )?;
    Ok(())
}

pub fn update_hassle(conn: &Connection, h: &Hassle) -> Result<()> {
    conn.execute(
        "UPDATE hassle SET text = ?2, intensity = ?3 WHERE id = ?1",
        params![h.id, h.text, h.intensity],
    )?;
    Ok(())
}

/// ハッスルを元にした ABC 記録の数。
pub fn count_abc_of_hassle(conn: &Connection, hassle_id: &str) -> Result<i64> {
    count(
        conn,
        "SELECT COUNT(*) FROM abc_record WHERE hassle_id = ?1",
        hassle_id,
    )
}

/// ハッスルを消す。元にした ABC 記録は残し、`hassle_id` だけ NULL に戻す。
pub fn delete_hassle(conn: &Connection, id: &str) -> Result<()> {
    conn.execute(
        "UPDATE abc_record SET hassle_id = NULL WHERE hassle_id = ?1",
        [id],
    )?;
    conn.execute("DELETE FROM hassle WHERE id = ?1", [id])?;
    conn.execute(
        "DELETE FROM erg_label WHERE record_kind = 'hassle' AND record_id = ?1",
        [id],
    )?;
    Ok(())
}

pub fn get_abc(conn: &Connection, id: &str) -> Result<AbcRecord> {
    super::record::list_abc(conn, 0, i64::MAX)?
        .into_iter()
        .find(|a| a.id == id)
        .ok_or_else(|| Error::NotFound(format!("ABC 記録 {id}")))
}

pub fn update_abc(conn: &Connection, a: &AbcRecord) -> Result<()> {
    conn.execute(
        "UPDATE abc_record SET activating_event = ?2, belief = ?3, consequence_emotion = ?4, \
         consequence_intensity = ?5, dispute = ?6 WHERE id = ?1",
        params![
            a.id,
            a.activating_event,
            a.belief,
            a.consequence_emotion,
            a.consequence_intensity,
            a.dispute
        ],
    )?;
    Ok(())
}

pub fn delete_abc(conn: &Connection, id: &str) -> Result<()> {
    conn.execute("DELETE FROM abc_record WHERE id = ?1", [id])?;
    conn.execute(
        "DELETE FROM erg_label WHERE record_kind = 'abc' AND record_id = ?1",
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
