//! Tauri コマンド（15.3）。core の service 層を呼ぶだけの薄いラッパで、ロジックは持たない。
//! 引数名は UI との契約なので snake_case のまま受ける。

use crate::dto::*;
use crate::error::{AppError, AppResult};
use crate::state::AppState;
use crate::{events, tray, window};
use hitoiki_core::config::{self, Config};
use hitoiki_core::model::{AbcRecord, Checkin, Hassle, TimeBlock};
use hitoiki_core::repo::record;
use hitoiki_core::service::log::{self as log_svc, Log, LogInput};
use hitoiki_core::service::okr::{self, OkrList};
use hitoiki_core::service::plan::{self, BlockList};
use hitoiki_core::service::report::{self, Report, ReportBy, ReportInput};
use hitoiki_core::service::review::{self, ReviewList, ReviewSetResult};
use hitoiki_core::service::skill::{self, PresetImport, SkillList};
use hitoiki_core::service::status::{self, Status};
use hitoiki_core::service::task::{self, RuleList, TaskChange, TaskList, TaskShow, TaskView};
use hitoiki_core::service::RangeArgs;
use hitoiki_core::{paths, Clock, Millis, SystemClock};
use serde::Serialize;
use serde_json::{json, Value};
use std::str::FromStr;
use tauri::{AppHandle, State};

type S<'a> = State<'a, AppState>;

/// doing タスクが変わりうる操作の後処理。
fn doing_changed(app: &AppHandle) {
    events::emit_doing_changed(app);
    tray::refresh(app);
}

// ===== 状況 =====

#[tauri::command(rename_all = "snake_case")]
pub fn status(state: S) -> AppResult<Status> {
    state.with_ctx(status::status)
}

// ===== タスク =====

#[tauri::command(rename_all = "snake_case")]
pub fn task_list(state: S, filter: Option<TaskFilterDto>) -> AppResult<TaskList> {
    let f = filter.unwrap_or_default().into();
    state.with_ctx(|c| task::list(c, &f))
}

#[tauri::command(rename_all = "snake_case")]
pub fn task_show(state: S, id: String) -> AppResult<TaskShow> {
    state.with_ctx(|c| task::show(c, &id))
}

#[tauri::command(rename_all = "snake_case")]
pub fn task_add(app: AppHandle, state: S, input: TaskInputDto) -> AppResult<TaskView> {
    let input = input.into();
    let v = state.with_ctx(|c| task::add(c, &input))?;
    tray::refresh(&app);
    Ok(v)
}

#[tauri::command(rename_all = "snake_case")]
pub fn task_edit(app: AppHandle, state: S, id: String, input: TaskInputDto) -> AppResult<TaskView> {
    let input = input.into();
    let v = state.with_ctx(|c| task::edit(c, &id, &input))?;
    doing_changed(&app);
    Ok(v)
}

#[tauri::command(rename_all = "snake_case")]
pub fn task_start(app: AppHandle, state: S, id: String) -> AppResult<TaskChange> {
    let v = state.with_ctx(|c| task::start(c, &id))?;
    doing_changed(&app);
    Ok(v)
}

#[tauri::command(rename_all = "snake_case")]
pub fn task_done(app: AppHandle, state: S, id: String) -> AppResult<TaskView> {
    let v = state.with_ctx(|c| task::done(c, &id))?;
    doing_changed(&app);
    Ok(v)
}

#[tauri::command(rename_all = "snake_case")]
pub fn task_drop(app: AppHandle, state: S, id: String) -> AppResult<TaskView> {
    let v = state.with_ctx(|c| task::drop_task(c, &id))?;
    doing_changed(&app);
    Ok(v)
}

#[tauri::command(rename_all = "snake_case")]
pub fn rule_add(state: S, input: RuleInputDto) -> AppResult<hitoiki_core::model::TaskMatchRule> {
    let input = input.into();
    state.with_ctx(|c| task::rule_add(c, &input))
}

#[tauri::command(rename_all = "snake_case")]
pub fn rule_list(state: S, task: Option<String>) -> AppResult<RuleList> {
    state.with_ctx(|c| task::rule_list(c, task.as_deref()))
}

// ===== 区間・仕分け =====

#[tauri::command(rename_all = "snake_case")]
pub fn log(state: S, range: Option<RangeDto>, raw: Option<bool>) -> AppResult<Log> {
    let input = LogInput {
        range: range.unwrap_or_default().into(),
        raw: raw.unwrap_or(false),
    };
    state.with_ctx(|c| log_svc::log(c, &input))
}

#[tauri::command(rename_all = "snake_case")]
pub fn review_list(state: S, range: Option<RangeDto>) -> AppResult<ReviewList> {
    let args: RangeArgs = range.unwrap_or_default().into();
    state.with_ctx(|c| review::list(c, &args))
}

#[tauri::command(rename_all = "snake_case")]
pub fn review_set(app: AppHandle, state: S, input: ReviewSetDto) -> AppResult<ReviewSetResult> {
    let input = input.into();
    let v = state.with_ctx(|c| review::set(c, &input))?;
    events::emit_segments_updated(&app);
    Ok(v)
}

// ===== 計画（ブロック） =====

#[tauri::command(rename_all = "snake_case")]
pub fn block_add(state: S, input: BlockInputDto) -> AppResult<TimeBlock> {
    let input = input.into();
    state.with_ctx(|c| plan::add(c, &input))
}

#[tauri::command(rename_all = "snake_case")]
pub fn block_list(state: S, range: Option<RangeDto>) -> AppResult<BlockList> {
    let args: RangeArgs = range.unwrap_or_default().into();
    state.with_ctx(|c| plan::list(c, &args))
}

// ===== 記録（チェックイン・ハッスル・ABC） =====

/// 期間付きの一覧。
#[derive(Debug, Serialize)]
pub struct Items<T> {
    pub start_at: Millis,
    pub end_at: Millis,
    pub items: Vec<T>,
}

#[tauri::command(rename_all = "snake_case")]
pub fn checkin_add(state: S, input: CheckInputDto) -> AppResult<Checkin> {
    let input = input.into();
    state.with_ctx(|c| hitoiki_core::service::record::check(c, &input))
}

#[tauri::command(rename_all = "snake_case")]
pub fn checkin_list(state: S, range: Option<RangeDto>) -> AppResult<Items<Checkin>> {
    let args: RangeArgs = range.unwrap_or_default().into();
    state.with_ctx(|c| {
        let r = c.range(&args)?;
        Ok(Items {
            start_at: r.start,
            end_at: r.end,
            items: record::list_checkins(c.conn, r.start, r.end)?,
        })
    })
}

#[tauri::command(rename_all = "snake_case")]
pub fn hassle_add(state: S, input: HassleInputDto) -> AppResult<Hassle> {
    let input = input.into();
    state.with_ctx(|c| hitoiki_core::service::record::hassle(c, &input))
}

#[tauri::command(rename_all = "snake_case")]
pub fn hassle_list(state: S, range: Option<RangeDto>) -> AppResult<Items<Hassle>> {
    let args: RangeArgs = range.unwrap_or_default().into();
    state.with_ctx(|c| {
        let r = c.range(&args)?;
        Ok(Items {
            start_at: r.start,
            end_at: r.end,
            items: record::list_hassles(c.conn, r.start, r.end)?,
        })
    })
}

#[tauri::command(rename_all = "snake_case")]
pub fn abc_add(state: S, input: AbcInputDto) -> AppResult<AbcRecord> {
    let input = input.into();
    state.with_ctx(|c| hitoiki_core::service::record::abc_add(c, &input))
}

/// ABC 記録の一覧。
/// TODO(core): `repo::record::list_abc` が追加されたらそちらを呼ぶ（今は core に一覧関数が無いため直接読む）。
#[tauri::command(rename_all = "snake_case")]
pub fn abc_list(state: S, range: Option<RangeDto>) -> AppResult<Items<AbcRecord>> {
    let args: RangeArgs = range.unwrap_or_default().into();
    state.with_ctx(|c| {
        let r = c.range(&args)?;
        let mut stmt = c.conn.prepare(
            "SELECT id, at, activating_event, belief, consequence_emotion, consequence_intensity, \
             dispute, ai_dialog_md, hassle_id FROM abc_record WHERE at >= ?1 AND at < ?2 ORDER BY at, id",
        )?;
        let items = stmt
            .query_map([r.start, r.end], |row| {
                Ok(AbcRecord {
                    id: row.get(0)?,
                    at: row.get(1)?,
                    activating_event: row.get(2)?,
                    belief: row.get(3)?,
                    consequence_emotion: row.get(4)?,
                    consequence_intensity: row.get(5)?,
                    dispute: row.get(6)?,
                    ai_dialog_md: row.get(7)?,
                    hassle_id: row.get(8)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(Items {
            start_at: r.start,
            end_at: r.end,
            items,
        })
    })
}

// ===== OKR・スキル =====

#[tauri::command(rename_all = "snake_case")]
pub fn okr_list(state: S) -> AppResult<OkrList> {
    state.with_ctx(okr::list)
}

#[tauri::command(rename_all = "snake_case")]
pub fn okr_add_objective(state: S, input: ObjectiveInputDto) -> AppResult<hitoiki_core::model::Objective> {
    let input = input.into();
    state.with_ctx(|c| okr::add_objective(c, &input))
}

#[tauri::command(rename_all = "snake_case")]
pub fn okr_add_kr(state: S, input: KrInputDto) -> AppResult<hitoiki_core::model::KeyResult> {
    let input = input.into();
    state.with_ctx(|c| okr::add_kr(c, &input))
}

#[tauri::command(rename_all = "snake_case")]
pub fn skill_list(state: S) -> AppResult<SkillList> {
    state.with_ctx(skill::list)
}

#[tauri::command(rename_all = "snake_case")]
pub fn skill_add(state: S, input: SkillInputDto) -> AppResult<hitoiki_core::model::Skill> {
    let input = input.into();
    state.with_ctx(|c| skill::add(c, &input))
}

#[tauri::command(rename_all = "snake_case")]
pub fn skill_import_preset(state: S, name: Option<String>) -> AppResult<PresetImport> {
    state.with_ctx(|c| skill::import_preset(c, name.as_deref()))
}

// ===== レポート =====

#[tauri::command(rename_all = "snake_case")]
pub fn report(
    state: S,
    range: Option<RangeDto>,
    by: String,
    include_idle: Option<bool>,
) -> AppResult<Report> {
    let input = ReportInput {
        range: range.unwrap_or_default().into(),
        by: ReportBy::from_str(&by)?,
        include_idle: include_idle.unwrap_or(false),
    };
    state.with_ctx(|c| report::report(c, &input))
}

// ===== 設定 =====

/// 設定全体（設定画面用）。
#[tauri::command(rename_all = "snake_case")]
pub fn config_all() -> AppResult<Value> {
    let cfg = Config::load()?;
    serde_json::to_value(&cfg).map_err(|e| AppError::internal(e.to_string()))
}

#[tauri::command(rename_all = "snake_case")]
pub fn config_get(key: String) -> AppResult<Value> {
    let v = Config::load()?.get_value(&key)?;
    let jv = serde_json::to_value(&v).map_err(|e| AppError::internal(e.to_string()))?;
    Ok(json!({ "key": key, "value": jv }))
}

/// `value` は TOML の値として解釈される（`true` / `"Asia/Tokyo"` / `[...]`）。
#[tauri::command(rename_all = "snake_case")]
pub fn config_set(app: AppHandle, key: String, value: String) -> AppResult<Value> {
    let path = paths::config_file();
    config::set_value(&path, &key, &value)?;
    if key == "general.autostart" {
        crate::apply_autostart(&app);
    }
    Ok(json!({ "key": key, "path": path }))
}

#[tauri::command(rename_all = "snake_case")]
pub fn config_path() -> Value {
    json!({ "path": paths::config_file(), "db": paths::db_file() })
}

// ===== ウインドウ・通知 =====

/// 画面を開く（`hitoiki ui <screen>`、トレイ、通知から）。
#[tauri::command(rename_all = "snake_case")]
pub fn open_screen(app: AppHandle, screen: String) {
    window::open_screen(&app, &screen);
}

/// 通知の一時停止（分）。0 で解除。
#[tauri::command(rename_all = "snake_case")]
pub fn snooze(app: AppHandle, state: S, minutes: i64) -> Value {
    let now = SystemClock.now_ms();
    let until = if minutes > 0 { now + minutes * 60_000 } else { 0 };
    state.set_snooze_until(until);
    tray::refresh(&app);
    json!({ "snooze_until": until })
}

#[tauri::command(rename_all = "snake_case")]
pub fn snooze_state(state: S) -> Value {
    json!({ "snooze_until": state.snooze_until() })
}

/// 起動引数 `--screen` で指定された画面を一度だけ返す（UI の初期表示用）。
#[tauri::command(rename_all = "snake_case")]
pub fn take_initial_screen(state: S) -> Option<String> {
    state.take_initial_screen()
}

/// ポップアップ窓用: 直近のプッシュを返す。
#[tauri::command(rename_all = "snake_case")]
pub fn pending_push(state: S) -> Option<Value> {
    state.last_push()
}

/// ポップアップ／通知のボタン押下相当（UI から応答する）。
#[tauri::command(rename_all = "snake_case")]
pub fn push_respond(app: AppHandle, action_id: String, input: Option<String>) {
    crate::runtime::handle_activation(
        &app,
        crate::notify::Activation {
            action_id: Some(action_id),
            input,
            push_id: None,
        },
    );
}

/// テスト通知（アクションボタン付きトースト）を出す。設定画面の動作確認用。
#[tauri::command(rename_all = "snake_case")]
pub fn notify_test(app: AppHandle) -> AppResult<()> {
    crate::notify::show_push(&app, &crate::notify::PushPayload::checkin()).map_err(AppError::internal)
}
