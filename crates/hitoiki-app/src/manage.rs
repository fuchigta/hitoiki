//! 編集・削除系の Tauri コマンド（目標・スキル・ルール・記録・カレンダーソース・活動）。
//! core の service 層を呼ぶだけの薄いラッパで、ロジックは持たない。
//! 削除は `mode` で dry_run（影響の確認）と force（確認後の削除）を指定する。

use crate::error::AppResult;
use crate::state::AppState;
use hitoiki_core::config::Config;
use hitoiki_core::model::{
    ActivityRule, Concern, Condition, Goal, KeyResult, Reframe, Skill, TaskAssignRule,
};
use hitoiki_core::paths;
use hitoiki_core::service::goal::{self, GoalEdit, KeyResultEdit};
use hitoiki_core::service::impact::{DeleteMode, DeleteOutcome};
use hitoiki_core::service::key_result_progress;
use hitoiki_core::service::link_suggestion;
use hitoiki_core::service::record::{self, ConcernInput, ConditionInput, ReframeEditInput};
use hitoiki_core::service::rule::{
    self, ActivityRuleEdit, ActivityRuleInput, ActivityRuleList, RuleDeleted, TaskRuleEdit,
};
use hitoiki_core::service::settings::{self, CalendarSourceList, CalendarSourceView, LabelList};
use hitoiki_core::service::skill::{self, SkillEdit};
use tauri::State;

type S<'a> = State<'a, AppState>;

// ===== 目標 =====

#[tauri::command(rename_all = "snake_case")]
pub fn goal_edit(state: S, id: String, input: GoalEdit) -> AppResult<Goal> {
    state.with_ctx(|c| goal::edit_goal(c, &id, &input))
}

#[tauri::command(rename_all = "snake_case")]
pub fn goal_delete(state: S, id: String, mode: Option<DeleteMode>) -> AppResult<DeleteOutcome> {
    let mode = mode.unwrap_or_default();
    let v = state.with_ctx(|c| goal::delete_goal(c, &id, mode))?;
    // 報告（成果の指標別・目標別の時間）はタスクの紐付けから算出するので、再計算は不要。
    Ok(v)
}

#[tauri::command(rename_all = "snake_case")]
pub fn key_result_edit(state: S, id: String, input: KeyResultEdit) -> AppResult<KeyResult> {
    state.with_ctx(|c| goal::edit_key_result(c, &id, &input))
}

/// 成果の指標にタスクを紐付ける（別の成果の指標に紐付いていたものは付け替える）。
#[tauri::command(rename_all = "snake_case")]
pub fn key_result_link(state: S, key_result: String, tasks: Vec<String>) -> AppResult<goal::LinkResult> {
    state.with_ctx(|c| goal::link_tasks(c, &key_result, &tasks))
}

/// タスクの成果の指標との紐付けを外す。
#[tauri::command(rename_all = "snake_case")]
pub fn key_result_unlink(state: S, tasks: Vec<String>) -> AppResult<Vec<goal::LinkedTask>> {
    state.with_ctx(|c| goal::unlink_tasks(c, &tasks))
}

/// 提案の候補を「付ける」（`candidates` は `task:<ID>` / `skill:<ID または名前>`）。
#[tauri::command(rename_all = "snake_case")]
pub fn key_result_suggest_accept(
    state: S,
    key_result: String,
    candidates: Vec<String>,
) -> AppResult<link_suggestion::DecisionResult> {
    let refs = parse_candidates(&candidates)?;
    state.with_ctx(|c| link_suggestion::accept(c, &key_result, &refs))
}

/// 提案の候補を「付けない」（以後この成果の指標には提案しない）。
#[tauri::command(rename_all = "snake_case")]
pub fn olink_suggestion_reject(
    state: S,
    key_result: String,
    candidates: Vec<String>,
) -> AppResult<link_suggestion::DecisionResult> {
    let refs = parse_candidates(&candidates)?;
    state.with_ctx(|c| link_suggestion::reject(c, &key_result, &refs))
}

fn parse_candidates(items: &[String]) -> hitoiki_core::Result<Vec<link_suggestion::CandidateRef>> {
    items.iter().map(|s| s.parse()).collect()
}

/// 成果の指標の集計（期間は目標の期間、無ければ直近 8 週）。
#[tauri::command(rename_all = "snake_case")]
pub fn key_result_show(state: S, key_result: String) -> AppResult<goal::KeyResultSummary> {
    state.with_ctx(|c| goal::key_result_summary(c, &key_result, None))
}

/// 成果の指標の現在値を記録する（現在値を更新し、履歴に 1 行残す）。
#[tauri::command(rename_all = "snake_case")]
pub fn key_result_progress(
    state: S,
    key_result: String,
    value: String,
    note: Option<String>,
) -> AppResult<key_result_progress::ProgressRecorded> {
    state.with_ctx(|c| key_result_progress::record(c, &key_result, &value, note.as_deref()))
}

/// 成果の指標の現在値の履歴と、週ごとの時間（期間は目標の期間、無ければ直近 8 週）。
#[tauri::command(rename_all = "snake_case")]
pub fn key_result_history(state: S, key_result: String) -> AppResult<key_result_progress::KeyResultHistory> {
    state.with_ctx(|c| key_result_progress::key_result_history(c, &key_result, None))
}

#[tauri::command(rename_all = "snake_case")]
pub fn key_result_delete(state: S, id: String, mode: Option<DeleteMode>) -> AppResult<DeleteOutcome> {
    let mode = mode.unwrap_or_default();
    state.with_ctx(|c| goal::delete_key_result(c, &id, mode))
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
pub fn task_rule_edit(state: S, id: String, input: TaskRuleEdit) -> AppResult<TaskAssignRule> {
    let v = state.with_ctx(|c| rule::task_rule_edit(c, &id, &input))?;
    state.request_rebuild();
    Ok(v)
}

#[tauri::command(rename_all = "snake_case")]
pub fn task_rule_delete(state: S, id: String) -> AppResult<RuleDeleted<TaskAssignRule>> {
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
pub fn condition_edit(state: S, id: String, input: ConditionInput) -> AppResult<Condition> {
    state.with_ctx(|c| record::condition_edit(c, &id, &input))
}

#[tauri::command(rename_all = "snake_case")]
pub fn condition_delete(state: S, id: String, mode: Option<DeleteMode>) -> AppResult<DeleteOutcome> {
    let mode = mode.unwrap_or(DeleteMode::FORCE);
    state.with_ctx(|c| record::condition_delete(c, &id, mode))
}

#[tauri::command(rename_all = "snake_case")]
pub fn concern_edit(state: S, id: String, input: ConcernInput) -> AppResult<Concern> {
    state.with_ctx(|c| record::concern_edit(c, &id, &input))
}

#[tauri::command(rename_all = "snake_case")]
pub fn concern_delete(state: S, id: String, mode: Option<DeleteMode>) -> AppResult<DeleteOutcome> {
    let mode = mode.unwrap_or_default();
    state.with_ctx(|c| record::concern_delete(c, &id, mode))
}

#[tauri::command(rename_all = "snake_case")]
pub fn reframe_edit(state: S, id: String, input: ReframeEditInput) -> AppResult<Reframe> {
    state.with_ctx(|c| record::reframe_edit(c, &id, &input))
}

#[tauri::command(rename_all = "snake_case")]
pub fn reframe_delete(state: S, id: String, mode: Option<DeleteMode>) -> AppResult<DeleteOutcome> {
    let mode = mode.unwrap_or(DeleteMode::FORCE);
    state.with_ctx(|c| record::reframe_delete(c, &id, mode))
}

// ===== カレンダーソース・活動 =====

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
pub fn activity_list() -> AppResult<LabelList> {
    Ok(settings::labels_list(&Config::load()?))
}

#[tauri::command(rename_all = "snake_case")]
pub fn activity_add(state: S, label: String) -> AppResult<LabelList> {
    let v = settings::labels_add(&paths::config_file(), &label)?;
    state.apply_config();
    Ok(v)
}

#[tauri::command(rename_all = "snake_case")]
pub fn activity_rename(state: S, from: String, to: String) -> AppResult<LabelList> {
    let v = state.with_ctx(|c| settings::labels_rename(c.conn, &paths::config_file(), &from, &to))?;
    state.apply_config();
    Ok(v)
}

#[tauri::command(rename_all = "snake_case")]
pub fn activity_remove(state: S, label: String) -> AppResult<LabelList> {
    let v = settings::labels_remove(&paths::config_file(), &label)?;
    state.apply_config();
    Ok(v)
}
