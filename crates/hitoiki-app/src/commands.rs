//! Tauri コマンド（15.3）。core の service 層を呼ぶだけの薄いラッパで、ロジックは持たない。
//! 引数名は UI との契約なので snake_case のまま受ける。

use crate::dto::*;
use crate::error::{AppError, AppResult};
use crate::state::{AppState, PushEvent, RuntimeMode};
use crate::{events, tray, window};
use hitoiki_core::config::{self, Config};
use hitoiki_core::model::{AbcRecord, CalendarEvent, Checkin, Hassle, Reflection, TimeBlock};
use hitoiki_core::service::context::{self, ContextJson};
use hitoiki_core::service::log::{self as log_svc, Log, LogInput};
use hitoiki_core::service::okr::{self, OkrList};
use hitoiki_core::service::plan::{self, BlockList};
use hitoiki_core::service::record::{self as record_svc, AbcInput, CheckInput, HassleInput, Listing};
use hitoiki_core::service::reflect;
use hitoiki_core::service::report::{self, Report, ReportBy, ReportInput};
use hitoiki_core::service::review::{self, ReviewList, ReviewSetResult};
use hitoiki_core::service::skill::{self, PresetImport, SkillList};
use hitoiki_core::service::status::{self, Status};
use hitoiki_core::service::task::{self, RuleList, TaskChange, TaskInput, TaskList, TaskShow, TaskView};
use hitoiki_core::service::RangeArgs;
use hitoiki_core::{paths, Clock, SystemClock};
use serde_json::{json, Value};
use std::str::FromStr;
use tauri::{AppHandle, Manager, State};

type S<'a> = State<'a, AppState>;

/// doing タスクが変わりうる操作の後処理（UI へ通知し、再計算を要求する）。
fn doing_changed(app: &AppHandle) {
    events::emit_doing_changed(app);
    tray::refresh(app);
    app.state::<AppState>().request_rebuild();
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
pub fn task_add(app: AppHandle, state: S, input: TaskInput) -> AppResult<TaskView> {
    let v = state.with_ctx(|c| task::add(c, &input))?;
    tray::refresh(&app);
    state.request_rebuild();
    Ok(v)
}

#[tauri::command(rename_all = "snake_case")]
pub fn task_edit(app: AppHandle, state: S, id: String, input: TaskInput) -> AppResult<TaskView> {
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
    let v = state.with_ctx(|c| task::rule_add(c, &input))?;
    state.request_rebuild();
    Ok(v)
}

#[tauri::command(rename_all = "snake_case")]
pub fn rule_list(state: S, task: Option<String>) -> AppResult<RuleList> {
    state.with_ctx(|c| task::rule_list(c, task.as_deref()))
}

// ===== 区間・仕分け =====

#[tauri::command(rename_all = "snake_case")]
pub fn log(state: S, range: Option<RangeArgs>, raw: Option<bool>) -> AppResult<Log> {
    let input = LogInput {
        range: range.unwrap_or_default(),
        raw: raw.unwrap_or(false),
    };
    state.with_ctx(|c| log_svc::log(c, &input))
}

#[tauri::command(rename_all = "snake_case")]
pub fn review_list(state: S, range: Option<RangeArgs>) -> AppResult<ReviewList> {
    let args = range.unwrap_or_default();
    state.with_ctx(|c| review::list(c, &args))
}

#[tauri::command(rename_all = "snake_case")]
pub fn review_set(app: AppHandle, state: S, input: ReviewSetDto) -> AppResult<ReviewSetResult> {
    let input = input.into();
    let v = state.with_ctx(|c| review::set(c, &input))?;
    events::emit_segments_updated(&app);
    state.request_rebuild();
    Ok(v)
}

// ===== 計画（ブロック） =====

#[tauri::command(rename_all = "snake_case")]
pub fn block_add(state: S, input: BlockInputDto) -> AppResult<TimeBlock> {
    let input = input.into();
    let v = state.with_ctx(|c| plan::add(c, &input))?;
    state.request_rebuild();
    Ok(v)
}

#[tauri::command(rename_all = "snake_case")]
pub fn block_list(state: S, range: Option<RangeArgs>) -> AppResult<BlockList> {
    let args = range.unwrap_or_default();
    state.with_ctx(|c| plan::list(c, &args))
}

#[tauri::command(rename_all = "snake_case")]
pub fn block_delete(state: S, id: String) -> AppResult<()> {
    state.with_ctx(|c| plan::delete(c, &id))?;
    state.request_rebuild();
    Ok(())
}

/// 指定範囲のカレンダー予定（同期済みのもの）。
#[tauri::command(rename_all = "snake_case")]
pub fn calendar_list(state: S, range: Option<RangeArgs>) -> AppResult<Vec<CalendarEvent>> {
    let args = range.unwrap_or_default();
    state.with_ctx(|c| {
        let r = c.range(&args)?;
        hitoiki_core::repo::calendar::list(c.conn, r)
    })
}

// ===== 記録（チェックイン・ハッスル・ABC） =====

#[tauri::command(rename_all = "snake_case")]
pub fn checkin_add(state: S, input: CheckInput) -> AppResult<Checkin> {
    state.with_ctx(|c| record_svc::check(c, &input))
}

#[tauri::command(rename_all = "snake_case")]
pub fn checkin_list(state: S, range: Option<RangeArgs>) -> AppResult<Listing<Checkin>> {
    let args = range.unwrap_or_default();
    state.with_ctx(|c| record_svc::list_checkins(c, &args))
}

#[tauri::command(rename_all = "snake_case")]
pub fn hassle_add(state: S, input: HassleInput) -> AppResult<Hassle> {
    state.with_ctx(|c| record_svc::hassle(c, &input))
}

#[tauri::command(rename_all = "snake_case")]
pub fn hassle_list(state: S, range: Option<RangeArgs>) -> AppResult<Listing<Hassle>> {
    let args = range.unwrap_or_default();
    state.with_ctx(|c| record_svc::list_hassles(c, &args))
}

#[tauri::command(rename_all = "snake_case")]
pub fn abc_add(state: S, input: AbcInput) -> AppResult<AbcRecord> {
    state.with_ctx(|c| record_svc::abc_add(c, &input))
}

#[tauri::command(rename_all = "snake_case")]
pub fn abc_list(state: S, range: Option<RangeArgs>) -> AppResult<Listing<AbcRecord>> {
    let args = range.unwrap_or_default();
    state.with_ctx(|c| record_svc::list_abc(c, &args))
}

// ===== 振り返り（AI） =====

/// 振り返りに渡すコンテキスト（AI は呼ばない）。
#[tauri::command(rename_all = "snake_case")]
pub fn context(state: S, kind: String, date: Option<String>) -> AppResult<ContextJson> {
    let kind = context::parse_kind(&kind)?;
    state.with_ctx(|c| context::context(c, kind, date.as_deref()))
}

/// 時間のかかる処理を専用スレッド・専用の DB 接続で実行する（UI も他のコマンドも塞がない）。
async fn blocking<T: Send + 'static>(
    app: AppHandle,
    f: impl FnOnce(&AppState) -> AppResult<T> + Send + 'static,
) -> AppResult<T> {
    tauri::async_runtime::spawn_blocking(move || f(&app.state::<AppState>()))
        .await
        .map_err(|e| AppError::internal(format!("処理が異常終了しました: {e}")))?
}

/// 問いを AI で生成して保存する。
#[tauri::command(rename_all = "snake_case")]
pub async fn reflect_generate(app: AppHandle, kind: String, date: Option<String>) -> AppResult<Reflection> {
    let kind = context::parse_kind(&kind)?;
    blocking(app, move |st| {
        st.with_own_ctx(|c| reflect::reflect(c, kind, date.as_deref()))
    })
    .await
}

/// 回答を保存する。
#[tauri::command(rename_all = "snake_case")]
pub fn reflect_answer(state: S, id: String, answers_md: String) -> AppResult<Reflection> {
    state.with_ctx(|c| reflect::answer(c, &id, &answers_md))
}

/// 過去の振り返り（新しい順）。
#[tauri::command(rename_all = "snake_case")]
pub fn reflect_list(state: S, kind: Option<String>, limit: Option<i64>) -> AppResult<Vec<Reflection>> {
    let kind = kind.as_deref().map(context::parse_kind).transpose()?;
    state.with_ctx(|c| reflect::list(c, kind, limit.unwrap_or(30)))
}

#[tauri::command(rename_all = "snake_case")]
pub fn reflect_show(state: S, id: String) -> AppResult<Reflection> {
    state.with_ctx(|c| reflect::show(c, &id))
}

/// ABC 記録の B を掘り下げる問いを AI で生成し、記録に追記する。
#[tauri::command(rename_all = "snake_case")]
pub async fn abc_dispute(app: AppHandle, id: String) -> AppResult<AbcRecord> {
    blocking(app, move |st| st.with_own_ctx(|c| reflect::abc_dispute(c, &id))).await
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
    range: Option<RangeArgs>,
    by: String,
    include_idle: Option<bool>,
) -> AppResult<Report> {
    let input = ReportInput {
        range: range.unwrap_or_default(),
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
pub fn config_set(app: AppHandle, state: S, key: String, value: String) -> AppResult<Value> {
    let path = paths::config_file();
    config::set_value(&path, &key, &value)?;
    // 再計算・カレンダー・プッシュ・判定器に反映する（収集器の設定は再起動が必要）。
    state.apply_config();
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
pub fn pending_push(state: S) -> Option<PushEvent> {
    state.last_push()
}

/// ポップアップ／通知のボタン押下相当（UI から応答する）。`push_id` は `pending_push` が返した値。
#[tauri::command(rename_all = "snake_case")]
pub fn push_respond(
    app: AppHandle,
    push_id: Option<String>,
    action: Option<String>,
    value: Option<i64>,
    input: Option<String>,
    open_screen: Option<String>,
) {
    crate::runtime::handle_activation(
        &app,
        crate::notify::Activation {
            action,
            value,
            input,
            push_id,
            open_screen,
        },
    );
}

/// 常駐処理の状態（自前で収集中／別プロセスが収集中／起動失敗）。
#[tauri::command(rename_all = "snake_case")]
pub fn runtime_state(state: S) -> RuntimeMode {
    state.runtime_mode()
}

/// テスト通知（アクションボタン付きトースト）を出す。設定画面の動作確認用。
#[tauri::command(rename_all = "snake_case")]
pub fn notify_test(app: AppHandle) -> AppResult<()> {
    crate::notify::show_push(&app, None, &crate::notify::test_payload()).map_err(AppError::internal)
}
