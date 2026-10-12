//! 編集・削除系の Tauri コマンド（OKR・スキル・ルール・記録・カレンダーソース・活動ラベル）。
//! core の service 層を呼ぶだけの薄いラッパで、ロジックは持たない。
//! 削除は `mode` で dry_run（影響の確認）と force（確認後の削除）を指定する。

use crate::error::AppResult;
use crate::state::AppState;
use hitoiki_core::config::Config;
use hitoiki_core::model::{
    AbcRecord, ActivityRule, Checkin, Hassle, KeyResult, Objective, Skill, TaskMatchRule,
};
use hitoiki_core::paths;
use hitoiki_core::service::impact::{DeleteMode, DeleteOutcome};
use hitoiki_core::service::okr::{self, KrEdit, ObjectiveEdit};
use hitoiki_core::service::record::{self, AbcEditInput, CheckInput, HassleInput};
use hitoiki_core::service::rule::{
    self, ActivityRuleEdit, ActivityRuleInput, ActivityRuleList, RuleDeleted, TaskRuleEdit,
};
use hitoiki_core::service::settings::{self, CalendarSourceList, CalendarSourceView, LabelList};
use hitoiki_core::service::skill::{self, SkillEdit};
use tauri::State;

type S<'a> = State<'a, AppState>;

// ===== OKR =====

#[tauri::command(rename_all = "snake_case")]
pub fn okr_edit_objective(state: S, id: String, input: ObjectiveEdit) -> AppResult<Objective> {
    state.with_ctx(|c| okr::edit_objective(c, &id, &input))
}

#[tauri::command(rename_all = "snake_case")]
pub fn okr_delete_objective(state: S, id: String, mode: Option<DeleteMode>) -> AppResult<DeleteOutcome> {
    let mode = mode.unwrap_or_default();
    let v = state.with_ctx(|c| okr::delete_objective(c, &id, mode))?;
    // 報告（okr）はタスクの紐付けから算出するので、再計算は不要。
    Ok(v)
}

#[tauri::command(rename_all = "snake_case")]
pub fn okr_edit_kr(state: S, id: String, input: KrEdit) -> AppResult<KeyResult> {
    state.with_ctx(|c| okr::edit_kr(c, &id, &input))
}

/// KR にタスクを紐付ける（別の KR に紐付いていたものは付け替える）。
#[tauri::command(rename_all = "snake_case")]
pub fn okr_link(state: S, kr: String, tasks: Vec<String>) -> AppResult<okr::LinkResult> {
    state.with_ctx(|c| okr::link_tasks(c, &kr, &tasks))
}

/// タスクの KR との紐付けを外す。
#[tauri::command(rename_all = "snake_case")]
pub fn okr_unlink(state: S, tasks: Vec<String>) -> AppResult<Vec<okr::LinkedTask>> {
    state.with_ctx(|c| okr::unlink_tasks(c, &tasks))
}

/// KR の集計（期間は Objective の期間、無ければ直近 8 週）。
#[tauri::command(rename_all = "snake_case")]
pub fn okr_show(state: S, kr: String) -> AppResult<okr::KrSummary> {
    state.with_ctx(|c| okr::kr_summary(c, &kr, None))
}

#[tauri::command(rename_all = "snake_case")]
pub fn okr_delete_kr(state: S, id: String, mode: Option<DeleteMode>) -> AppResult<DeleteOutcome> {
    let mode = mode.unwrap_or_default();
    state.with_ctx(|c| okr::delete_kr(c, &id, mode))
}

// ===== スキル =====

#[tauri::command(rename_all = "snake_case")]
pub fn skill_edit(state: S, key: String, input: SkillEdit) -> AppResult<Skill> {
    state.with_ctx(|c| skill::edit(c, &key, &input))
}

/// `children` は `reparent`（既定）か `delete`。
#[tauri::command(rename_all = "snake_case")]
pub fn skill_delete(
    state: S,
    key: String,
    children: Option<String>,
    mode: Option<DeleteMode>,
) -> AppResult<DeleteOutcome> {
    let policy = match children.as_deref() {
        Some(s) => s.parse()?,
        None => skill::ChildPolicy::default(),
    };
    let mode = mode.unwrap_or_default();
    state.with_ctx(|c| skill::delete(c, &key, policy, mode))
}

// ===== ルール =====

#[tauri::command(rename_all = "snake_case")]
pub fn rule_edit(state: S, id: String, input: TaskRuleEdit) -> AppResult<TaskMatchRule> {
    let v = state.with_ctx(|c| rule::task_rule_edit(c, &id, &input))?;
    state.request_rebuild();
    Ok(v)
}

#[tauri::command(rename_all = "snake_case")]
pub fn rule_delete(state: S, id: String) -> AppResult<RuleDeleted<TaskMatchRule>> {
    let v = state.with_ctx(|c| rule::task_rule_delete(c, &id, false))?;
    state.request_rebuild();
    Ok(v)
}

#[tauri::command(rename_all = "snake_case")]
pub fn activity_rule_list(state: S) -> AppResult<ActivityRuleList> {
    state.with_ctx(rule::activity_rule_list)
}

#[tauri::command(rename_all = "snake_case")]
pub fn activity_rule_add(state: S, input: ActivityRuleInput) -> AppResult<ActivityRule> {
    let v = state.with_ctx(|c| rule::activity_rule_add(c, &input))?;
    state.request_rebuild();
    Ok(v)
}

#[tauri::command(rename_all = "snake_case")]
pub fn activity_rule_edit(state: S, id: String, input: ActivityRuleEdit) -> AppResult<ActivityRule> {
    let v = state.with_ctx(|c| rule::activity_rule_edit(c, &id, &input))?;
    state.request_rebuild();
    Ok(v)
}

#[tauri::command(rename_all = "snake_case")]
pub fn activity_rule_delete(state: S, id: String) -> AppResult<RuleDeleted<ActivityRule>> {
    let v = state.with_ctx(|c| rule::activity_rule_delete(c, &id, false))?;
    state.request_rebuild();
    Ok(v)
}

// ===== 記録 =====

#[tauri::command(rename_all = "snake_case")]
pub fn checkin_edit(state: S, id: String, input: CheckInput) -> AppResult<Checkin> {
    state.with_ctx(|c| record::check_edit(c, &id, &input))
}

#[tauri::command(rename_all = "snake_case")]
pub fn checkin_delete(state: S, id: String) -> AppResult<DeleteOutcome> {
    state.with_ctx(|c| record::check_delete(c, &id, DeleteMode::FORCE))
}

#[tauri::command(rename_all = "snake_case")]
pub fn hassle_edit(state: S, id: String, input: HassleInput) -> AppResult<Hassle> {
    state.with_ctx(|c| record::hassle_edit(c, &id, &input))
}

#[tauri::command(rename_all = "snake_case")]
pub fn hassle_delete(state: S, id: String, mode: Option<DeleteMode>) -> AppResult<DeleteOutcome> {
    let mode = mode.unwrap_or_default();
    state.with_ctx(|c| record::hassle_delete(c, &id, mode))
}

#[tauri::command(rename_all = "snake_case")]
pub fn abc_edit(state: S, id: String, input: AbcEditInput) -> AppResult<AbcRecord> {
    state.with_ctx(|c| record::abc_edit(c, &id, &input))
}

#[tauri::command(rename_all = "snake_case")]
pub fn abc_delete(state: S, id: String) -> AppResult<DeleteOutcome> {
    state.with_ctx(|c| record::abc_delete(c, &id, DeleteMode::FORCE))
}

// ===== カレンダーソース・活動ラベル =====

#[tauri::command(rename_all = "snake_case")]
pub fn calendar_source_list() -> AppResult<CalendarSourceList> {
    Ok(settings::calendar_list(&Config::load()?))
}

#[tauri::command(rename_all = "snake_case")]
pub fn calendar_source_add(state: S, name: String, url: String) -> AppResult<CalendarSourceView> {
    let v = settings::calendar_add(&paths::config_file(), &name, &url)?;
    state.apply_config();
    Ok(v)
}

#[tauri::command(rename_all = "snake_case")]
pub fn calendar_source_edit(
    state: S,
    name: String,
    new_name: Option<String>,
    url: Option<String>,
) -> AppResult<CalendarSourceView> {
    let v = state.with_ctx(|c| {
        settings::calendar_edit(
            c.conn,
            &paths::config_file(),
            &name,
            new_name.as_deref(),
            url.as_deref(),
        )
    })?;
    state.apply_config();
    Ok(v)
}

#[tauri::command(rename_all = "snake_case")]
pub fn calendar_source_delete(state: S, name: String, mode: Option<DeleteMode>) -> AppResult<DeleteOutcome> {
    let mode = mode.unwrap_or_default();
    let v = state.with_ctx(|c| settings::calendar_delete(c.conn, &paths::config_file(), &name, mode))?;
    if v.deleted {
        state.apply_config();
    }
    Ok(v)
}

#[tauri::command(rename_all = "snake_case")]
pub fn activity_label_list() -> AppResult<LabelList> {
    Ok(settings::labels_list(&Config::load()?))
}

#[tauri::command(rename_all = "snake_case")]
pub fn activity_label_add(state: S, label: String) -> AppResult<LabelList> {
    let v = settings::labels_add(&paths::config_file(), &label)?;
    state.apply_config();
    Ok(v)
}

#[tauri::command(rename_all = "snake_case")]
pub fn activity_label_rename(state: S, from: String, to: String) -> AppResult<LabelList> {
    let v = state.with_ctx(|c| settings::labels_rename(c.conn, &paths::config_file(), &from, &to))?;
    state.apply_config();
    Ok(v)
}

#[tauri::command(rename_all = "snake_case")]
pub fn activity_label_remove(state: S, label: String) -> AppResult<LabelList> {
    let v = settings::labels_remove(&paths::config_file(), &label)?;
    state.apply_config();
    Ok(v)
}
