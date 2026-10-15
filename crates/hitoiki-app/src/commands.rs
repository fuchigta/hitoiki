//! Tauri コマンド（docs/dev/architecture.md「GUI」）。core の service 層を呼ぶだけの薄いラッパで、ロジックは持たない。
//! 引数名は UI との契約なので snake_case のまま受ける。

use crate::dto::*;
use crate::error::{AppError, AppResult};
use crate::state::{AppState, PushEvent, RuntimeMode};
use crate::{events, tray, window};
use hitoiki_core::config::{self, Config};
use hitoiki_core::model::{
    AbcRecord, CalendarEvent, Checkin, ErgChoice, ErgLabel, ErgRecordKind, Hassle, Reflection, TimeBlock,
};
use hitoiki_core::service::block_template::{
    self as btpl, ApplyInput, ApplyResult, Preview, SaveFromInput, Suggestion, TemplateInput, TemplateList,
    TemplateView, UndoResult,
};
use hitoiki_core::service::context::{self, ContextJson};
use hitoiki_core::service::correction::{
    self, ApplyResult as CorrectionApplied, CandidateList, CorrectionFilter, CorrectionInput, CorrectionList,
    Include, Preview as CorrectionPreview, UndoResult as CorrectionUndone,
};
use hitoiki_core::service::erg;
use hitoiki_core::service::goal::{self, GoalList};
use hitoiki_core::service::heatmap::{self, Heatmap, HeatmapInput};
use hitoiki_core::service::hint;
use hitoiki_core::service::ignore;
use hitoiki_core::service::link_suggestion;
use hitoiki_core::service::log::{self as log_svc, Log, LogInput};
use hitoiki_core::service::onboarding::{self, OnboardingStatus};
use hitoiki_core::service::plan::{self, BlockList};
use hitoiki_core::service::record::{self as record_svc, AbcInput, CheckInput, HassleInput, Listing};
use hitoiki_core::service::reflect;
use hitoiki_core::service::report::{self, Report, ReportBy, ReportInput};
use hitoiki_core::service::review::{
    self, CreatedRule, IdlePreset, ReviewBatchInput, ReviewBatchResult, ReviewList, ReviewSetResult,
    RuleSuggestion, RuleSuggestionRef,
};
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
pub fn task_delete_preview(state: S, id: String) -> AppResult<task::DeletePreview> {
    state.with_ctx(|c| task::delete_preview(c, &id))
}

/// 誤入力のタスクを削除する。UI は確認ダイアログで影響を見せたあとに呼ぶので `force` で実行する。
#[tauri::command(rename_all = "snake_case")]
pub fn task_delete(app: AppHandle, state: S, id: String) -> AppResult<task::DeleteResult> {
    let v = state.with_ctx(|c| {
        let day = c.range(&RangeArgs::default())?;
        let judge = hitoiki_daemon::build_judge(c.conn, c.cfg, day);
        task::delete(c, &id, true, judge.as_deref())
    })?;
    events::emit_segments_updated(&app);
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
pub fn review_list(state: S, range: Option<RangeArgs>, all: Option<bool>) -> AppResult<ReviewList> {
    let args = range.unwrap_or_default();
    state.with_ctx(|c| review::list_with(c, &args, all.unwrap_or(false)))
}

#[tauri::command(rename_all = "snake_case")]
pub fn review_set(app: AppHandle, state: S, input: ReviewSetDto) -> AppResult<ReviewSetResult> {
    let input = input.into();
    let v = state.with_ctx(|c| review::set(c, &input))?;
    events::emit_segments_updated(&app);
    state.request_rebuild();
    Ok(v)
}

/// 複数の区間にまとめて同じ割り当てをする。
#[tauri::command(rename_all = "snake_case")]
pub fn review_set_many(app: AppHandle, state: S, input: ReviewBatchInput) -> AppResult<ReviewBatchResult> {
    let v = state.with_ctx(|c| review::set_many(c, &input))?;
    events::emit_segments_updated(&app);
    state.request_rebuild();
    Ok(v)
}

/// 無操作の区間に分類（休憩・会議・考えごと・離席）をつける。
#[tauri::command(rename_all = "snake_case")]
pub fn review_idle(
    app: AppHandle,
    state: S,
    segments: Vec<String>,
    preset: IdlePreset,
) -> AppResult<ReviewBatchResult> {
    let v = state.with_ctx(|c| review::set_idle(c, &segments, preset))?;
    events::emit_segments_updated(&app);
    state.request_rebuild();
    Ok(v)
}

/// ルール化の提案の一覧。
#[tauri::command(rename_all = "snake_case")]
pub fn review_suggestions(state: S) -> AppResult<Vec<RuleSuggestion>> {
    state.with_ctx(review::suggest_rules)
}

/// 提案を受け入れてルールを追加する。
#[tauri::command(rename_all = "snake_case")]
pub fn review_suggest_accept(app: AppHandle, state: S, input: RuleSuggestionRef) -> AppResult<CreatedRule> {
    let v = state.with_ctx(|c| review::accept_suggestion(c, &input))?;
    events::emit_segments_updated(&app);
    state.request_rebuild();
    Ok(v)
}

/// 提案を断る（一定期間は同じ提案を出さない）。
#[tauri::command(rename_all = "snake_case")]
pub fn review_suggest_dismiss(state: S, input: RuleSuggestionRef) -> AppResult<()> {
    state.with_ctx(|c| review::dismiss_suggestion(c, &input))
}

// ===== 無視するアプリ・区間 =====

/// 区間（まとめて指定可）を無視にする。
#[tauri::command(rename_all = "snake_case")]
pub fn review_ignore(app: AppHandle, state: S, segments: Vec<String>) -> AppResult<ReviewBatchResult> {
    let v = state.with_ctx(|c| review::ignore(c, &segments))?;
    events::emit_segments_updated(&app);
    state.request_rebuild();
    Ok(v)
}

/// 無視にした区間を元に戻す（確定を削除して再計算する）。
#[tauri::command(rename_all = "snake_case")]
pub fn review_unignore(app: AppHandle, state: S, segments: Vec<String>) -> AppResult<review::UnignoreResult> {
    let v = state.with_ctx(|c| {
        let day = c.range(&RangeArgs::default())?;
        let judge = hitoiki_daemon::build_judge(c.conn, c.cfg, day);
        review::unignore(c, &segments, judge.as_deref())
    })?;
    events::emit_segments_updated(&app);
    state.request_rebuild();
    Ok(v)
}

#[tauri::command(rename_all = "snake_case")]
pub fn ignore_list(state: S) -> AppResult<ignore::IgnoreList> {
    state.with_ctx(|c| Ok(ignore::list(c.cfg)))
}

/// アプリを無視リストに追加し、直近 7 日を再計算する。
#[tauri::command(rename_all = "snake_case")]
pub fn ignore_add(app: AppHandle, state: S, exe: String) -> AppResult<ignore::IgnoreChange> {
    ignore_change(app, state, |c, judge| {
        ignore::add(c, &paths::config_file(), &exe, judge)
    })
}

/// アプリを無視リストから外し、直近 7 日を再計算する。
#[tauri::command(rename_all = "snake_case")]
pub fn ignore_remove(app: AppHandle, state: S, exe: String) -> AppResult<ignore::IgnoreChange> {
    ignore_change(app, state, |c, judge| {
        ignore::remove(c, &paths::config_file(), &exe, judge)
    })
}

fn ignore_change(
    app: AppHandle,
    state: S,
    f: impl FnOnce(
        &hitoiki_core::service::Ctx,
        Option<&dyn hitoiki_core::pipeline::SegmentJudge>,
    ) -> hitoiki_core::Result<ignore::IgnoreChange>,
) -> AppResult<ignore::IgnoreChange> {
    let v = state.with_own_ctx(|c| {
        let day = c.range(&RangeArgs::default())?;
        let judge = hitoiki_daemon::build_judge(c.conn, c.cfg, day);
        f(c, judge.as_deref())
    })?;
    state.apply_config();
    events::emit_segments_updated(&app);
    Ok(v)
}

/// 無視の提案（既定は直近 7 日）。
#[tauri::command(rename_all = "snake_case")]
pub fn ignore_suggestions(state: S, days: Option<i64>) -> AppResult<Vec<ignore::IgnoreSuggestion>> {
    state.with_ctx(|c| ignore::suggest(c, days.unwrap_or(ignore::SUGGEST_DAYS)))
}

/// 無視の提案を断る（30 日間は同じアプリを提案しない）。
#[tauri::command(rename_all = "snake_case")]
pub fn ignore_suggest_dismiss(state: S, exe: String) -> AppResult<()> {
    state.with_ctx(|c| ignore::dismiss(c, &exe))
}

// ===== 実績の補正 =====

/// 範囲の候補（既定のチェックつき）。
#[tauri::command(rename_all = "snake_case")]
pub fn correction_candidates(state: S, start_at: i64, end_at: i64) -> AppResult<CandidateList> {
    state.with_ctx(|c| correction::candidates(c, start_at, end_at, Include::from_config(c)))
}

/// 確定前のプレビュー（DB は変更しない）。
#[tauri::command(rename_all = "snake_case")]
pub fn correction_preview(state: S, input: CorrectionInput) -> AppResult<CorrectionPreview> {
    state.with_ctx(|c| correction::preview(c, &input))
}

/// 補正を確定する。
#[tauri::command(rename_all = "snake_case")]
pub fn correction_apply(app: AppHandle, state: S, input: CorrectionInput) -> AppResult<CorrectionApplied> {
    let v = state.with_ctx(|c| correction::apply(c, &input))?;
    events::emit_segments_updated(&app);
    state.request_rebuild();
    Ok(v)
}

/// 補正の一覧。
#[tauri::command(rename_all = "snake_case")]
pub fn correction_list(state: S, filter: Option<CorrectionFilter>) -> AppResult<CorrectionList> {
    let f = filter.unwrap_or_default();
    state.with_ctx(|c| correction::list(c, &f))
}

/// 補正を取り消す（その補正で作った確定を削除して、範囲を再計算する）。
#[tauri::command(rename_all = "snake_case")]
pub fn correction_undo(app: AppHandle, state: S, id: String) -> AppResult<CorrectionUndone> {
    let v = state.with_ctx(|c| {
        let day = c.range(&RangeArgs::default())?;
        let judge = hitoiki_daemon::build_judge(c.conn, c.cfg, day);
        correction::undo(c, &id, judge.as_deref())
    })?;
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

#[tauri::command(rename_all = "snake_case")]
pub fn block_update(state: S, id: String, input: BlockInputDto) -> AppResult<TimeBlock> {
    let input = input.into();
    let v = state.with_ctx(|c| plan::update(c, &id, &input))?;
    state.request_rebuild();
    Ok(v)
}

// ===== ブロックのテンプレート =====

#[tauri::command(rename_all = "snake_case")]
pub fn block_template_list(state: S) -> AppResult<TemplateList> {
    state.with_ctx(btpl::list)
}

#[tauri::command(rename_all = "snake_case")]
pub fn block_template_add(state: S, input: TemplateInput) -> AppResult<TemplateView> {
    state.with_ctx(|c| btpl::add(c, &input))
}

#[tauri::command(rename_all = "snake_case")]
pub fn block_template_edit(state: S, id: String, input: TemplateInput) -> AppResult<TemplateView> {
    state.with_ctx(|c| btpl::edit(c, &id, &input))
}

#[tauri::command(rename_all = "snake_case")]
pub fn block_template_delete(state: S, id: String) -> AppResult<TemplateView> {
    state.with_ctx(|c| btpl::delete(c, &id))
}

/// 指定日（省略は今日）の曜日に割り当てたテンプレート。
#[tauri::command(rename_all = "snake_case")]
pub fn block_template_suggest(state: S, date: Option<String>) -> AppResult<Suggestion> {
    state.with_ctx(|c| btpl::suggest(c, date.as_deref()))
}

#[tauri::command(rename_all = "snake_case")]
pub fn block_template_preview(state: S, input: ApplyInput) -> AppResult<Preview> {
    state.with_ctx(|c| btpl::preview(c, &input))
}

#[tauri::command(rename_all = "snake_case")]
pub fn block_template_apply(state: S, input: ApplyInput) -> AppResult<ApplyResult> {
    let v = state.with_ctx(|c| btpl::apply(c, &input))?;
    state.request_rebuild();
    Ok(v)
}

#[tauri::command(rename_all = "snake_case")]
pub fn block_template_undo(state: S, date: Option<String>) -> AppResult<UndoResult> {
    let v = state.with_ctx(|c| btpl::undo(c, date.as_deref()))?;
    state.request_rebuild();
    Ok(v)
}

#[tauri::command(rename_all = "snake_case")]
pub fn block_template_save_from(state: S, input: SaveFromInput) -> AppResult<TemplateView> {
    state.with_ctx(|c| btpl::save_from(c, &input))
}

// ===== 1 回だけ出す案内 =====

#[tauri::command(rename_all = "snake_case")]
pub fn hint_dismissed(state: S, name: String) -> AppResult<bool> {
    state.with_ctx(|c| hint::is_dismissed(c, &name))
}

#[tauri::command(rename_all = "snake_case")]
pub fn hint_dismiss(state: S, name: String) -> AppResult<()> {
    state.with_ctx(|c| hint::dismiss(c, &name))
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

/// 構造化された問いへの回答を、種類ごとの保存先（チェックイン・ハッスル・ABC・自由記述）に保存する。
#[tauri::command(rename_all = "snake_case")]
pub fn reflect_answer_structured(
    state: S,
    id: String,
    answers: reflect::AnswersInput,
) -> AppResult<Reflection> {
    state.with_ctx(|c| reflect::answer_structured(c, &id, &answers))
}

/// 回答済みの問いを、保存した記録（チェックイン・ハッスル・ABC）の中身つきで返す。
#[tauri::command(rename_all = "snake_case")]
pub fn reflect_answer_details(state: S, id: String) -> AppResult<Vec<reflect::AnswerDetail>> {
    state.with_ctx(|c| {
        let r = reflect::show(c, &id)?;
        reflect::answer_details(c, &r)
    })
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

// ===== ERG（記録の内容から AI が判定し、ユーザーが確認・修正する） =====

/// 期間内の記録の ERG ラベルと分布、確認待ちの数。
#[tauri::command(rename_all = "snake_case")]
pub fn erg_overview(state: S, range: Option<RangeArgs>) -> AppResult<erg::ErgOverview> {
    let args = range.unwrap_or_default();
    state.with_ctx(|c| erg::overview(c, &args))
}

/// 確認待ち（AI の提案で未確定）の一覧。
#[tauri::command(rename_all = "snake_case")]
pub fn erg_review(state: S) -> AppResult<erg::ErgReview> {
    state.with_ctx(erg::review)
}

/// 未判定の記録を AI で判定する。時間がかかるので専用スレッドで実行する。
#[tauri::command(rename_all = "snake_case")]
pub async fn erg_classify(
    app: AppHandle,
    range: Option<RangeArgs>,
    dry_run: Option<bool>,
) -> AppResult<erg::ClassifyOutcome> {
    let args = range.unwrap_or_default();
    blocking(app, move |st| {
        st.with_own_ctx(|c| erg::classify(c, &args, dry_run.unwrap_or(false)))
    })
    .await
}

/// 成果の指標への紐づけの候補（名前の近さ + AI が設定されていれば AI）。AI は時間がかかるので専用スレッドで実行する。
#[tauri::command(rename_all = "snake_case")]
pub async fn key_result_suggest(
    app: AppHandle,
    key_result: String,
    use_ai: Option<bool>,
) -> AppResult<link_suggestion::Suggestions> {
    blocking(app, move |st| {
        st.with_own_ctx(|c| link_suggestion::suggest(c, &key_result, use_ai.unwrap_or(true)))
    })
    .await
}

/// ユーザーがタグを付ける・直す（`tag` は existence / relatedness / growth / none）。
#[tauri::command(rename_all = "snake_case")]
pub fn erg_set(state: S, record_kind: String, record_id: String, tag: String) -> AppResult<ErgLabel> {
    let kind = ErgRecordKind::from_str(&record_kind)?;
    let tag = ErgChoice::from_str(&tag)?;
    state.with_ctx(|c| erg::set_for(c, kind, &record_id, tag))
}

/// 確認待ちの提案をそのまま確定する。
#[tauri::command(rename_all = "snake_case")]
pub fn erg_confirm(state: S, record_kind: String, record_id: String) -> AppResult<ErgLabel> {
    let kind = ErgRecordKind::from_str(&record_kind)?;
    state.with_ctx(|c| erg::confirm_for(c, kind, &record_id))
}

// ===== 目標・スキル =====

#[tauri::command(rename_all = "snake_case")]
pub fn goal_list(state: S) -> AppResult<GoalList> {
    state.with_ctx(goal::list)
}

#[tauri::command(rename_all = "snake_case")]
pub fn goal_add(state: S, input: GoalInputDto) -> AppResult<hitoiki_core::model::Goal> {
    let input = input.into();
    state.with_ctx(|c| goal::add_goal(c, &input))
}

#[tauri::command(rename_all = "snake_case")]
pub fn key_result_add(state: S, input: KeyResultInputDto) -> AppResult<hitoiki_core::model::KeyResult> {
    let input = input.into();
    state.with_ctx(|c| goal::add_key_result(c, &input))
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

/// 活動量ヒートマップ（日別の濃淡）。
#[tauri::command(rename_all = "snake_case")]
pub fn heatmap(state: S, by: Option<String>, from: Option<String>, to: Option<String>) -> AppResult<Heatmap> {
    let input = HeatmapInput { by, from, to };
    state.with_ctx(|c| heatmap::heatmap(c, &input))
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
    if key == "general.quick_entry_shortcut" {
        // 保存した値を OS に登録し直す（失敗しても値は保存済み。理由を返す）。
        let setting = Config::load()
            .map(|c| c.general.quick_entry_shortcut)
            .unwrap_or_default();
        crate::quick::apply(&app, &setting).map_err(AppError::internal)?;
        tray::refresh(&app);
    }
    if key == "general.update_check" {
        tray::refresh(&app);
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

/// オンボーディングの完了状態。
#[tauri::command(rename_all = "snake_case")]
pub fn onboarding_status(state: S) -> AppResult<OnboardingStatus> {
    state.with_ctx(onboarding::status)
}

/// オンボーディングを完了済みにする。
#[tauri::command(rename_all = "snake_case")]
pub fn onboarding_complete(state: S) -> AppResult<OnboardingStatus> {
    state.with_ctx(onboarding::complete)
}

/// オンボーディングを未完了に戻す（設定画面の「もう一度見る」）。
#[tauri::command(rename_all = "snake_case")]
pub fn onboarding_reset(state: S) -> AppResult<OnboardingStatus> {
    state.with_ctx(onboarding::reset)
}

/// テスト通知（アクションボタン付きトースト）を出す。設定画面の動作確認用。
#[tauri::command(rename_all = "snake_case")]
pub fn notify_test(app: AppHandle) -> AppResult<()> {
    crate::notify::show_push(&app, None, &crate::notify::test_payload()).map_err(AppError::internal)
}
