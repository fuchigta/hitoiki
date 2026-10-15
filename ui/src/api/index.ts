// Tauri コマンドの薄いクライアント。コア呼び出しはここに集約する。
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type * as T from "./types";

export * from "./types";

/** Tauri から `{code, message}` で返るエラーを AppError に正規化する。 */
export function toAppError(e: unknown): T.AppError {
  if (e && typeof e === "object" && "message" in e) {
    const o = e as { code?: unknown; message: unknown };
    return { code: typeof o.code === "string" ? o.code : "internal", message: String(o.message) };
  }
  return { code: "internal", message: String(e) };
}

/** ブラウザ（Tauri 外）で開いたときは呼び出しが失敗する。画面は落とさずエラー表示にする。 */
async function call<R>(cmd: string, args?: Record<string, unknown>): Promise<R> {
  try {
    return await invoke<R>(cmd, args);
  } catch (e) {
    throw toAppError(e);
  }
}

export const api = {
  status: () => call<T.Status>("status"),

  // タスク
  taskList: (statuses?: T.TaskStatus[], kind?: T.TaskKind) =>
    call<{ tasks: T.TaskView[] }>("task_list", { filter: { statuses: statuses ?? [], kind } }),
  taskShow: (id: string) => call<T.TaskShow>("task_show", { id }),
  taskAdd: (input: T.TaskInput) => call<T.TaskView>("task_add", { input }),
  taskEdit: (id: string, input: T.TaskInput) => call<T.TaskView>("task_edit", { id, input }),
  taskStart: (id: string) => call<T.TaskChange>("task_start", { id }),
  taskDone: (id: string) => call<T.TaskView>("task_done", { id }),
  taskDrop: (id: string) => call<T.TaskView>("task_drop", { id }),
  taskDeletePreview: (id: string) => call<T.TaskDeletePreview>("task_delete_preview", { id }),
  taskDelete: (id: string) => call<T.TaskDeleteResult>("task_delete", { id }),
  taskRuleAdd: (input: T.RuleInput) => call<T.TaskAssignRule>("task_rule_add", { input }),
  // クイック入力（割り込みなどで素早くタスクを切り替える）
  quickPrevious: () => call<T.TaskView | null>("quick_previous"),
  quickSwitch: (input: { query: string; new?: boolean; interrupt?: boolean }) =>
    call<T.SwitchResult>("quick_switch", { input }),
  quickBack: () => call<T.SwitchResult>("quick_back"),
  quickClose: () => call<void>("quick_close"),
  quickShortcutState: () => call<T.QuickShortcutState>("quick_shortcut_state"),
  quickShortcutSet: (shortcut: string) => call<{ shortcut: string; registered: string | null }>("quick_shortcut_set", { shortcut }),

  taskRuleList: (task?: string) => call<{ rules: T.TaskAssignRule[] }>("task_rule_list", { task }),

  // 区間・要確認
  actualList: (range?: T.RangeArgs, raw = false) => call<T.ActualList>("actual_list", { range, raw }),
  actualReview: (range?: T.RangeArgs, all = false) => call<T.ReviewList>("actual_review", { range, all }),
  actualConfirm: (input: T.ConfirmInput) => call<T.ConfirmResult>("actual_confirm", { input }),
  actualConfirmMany: (input: T.ConfirmBatchInput) => call<T.ConfirmBatchResult>("actual_confirm_many", { input }),
  actualIdle: (segments: string[], preset: T.IdlePreset) =>
    call<T.ConfirmBatchResult>("actual_idle", { segments, preset }),
  actualSuggestions: () => call<T.RuleSuggestion[]>("actual_suggestions"),
  actualSuggestAccept: (input: T.RuleSuggestionRef) => call<T.CreatedRule>("actual_suggest_accept", { input }),
  actualSuggestDismiss: (input: T.RuleSuggestionRef) => call<void>("actual_suggest_dismiss", { input }),

  // 無視するアプリ・区間
  actualIgnore: (segments: string[]) => call<T.ConfirmBatchResult>("actual_ignore", { segments }),
  actualUnignore: (segments: string[]) => call<T.UnignoreResult>("actual_unignore", { segments }),
  ignoreList: () => call<T.IgnoreList>("ignore_list"),
  ignoreAdd: (exe: string) => call<T.IgnoreChange>("ignore_add", { exe }),
  ignoreRemove: (exe: string) => call<T.IgnoreChange>("ignore_remove", { exe }),
  ignoreSuggestions: (days?: number) => call<T.IgnoreSuggestion[]>("ignore_suggestions", { days }),
  ignoreSuggestDismiss: (exe: string) => call<void>("ignore_suggest_dismiss", { exe }),

  // 実績の補正（範囲指定の付け替え）
  correctionCandidates: (start_at: number, end_at: number) =>
    call<T.CorrectionCandidates>("correction_candidates", { start_at, end_at }),
  correctionPreview: (input: T.CorrectionInput) => call<T.CorrectionPreview>("correction_preview", { input }),
  correctionApply: (input: T.CorrectionInput) => call<T.CorrectionApplied>("correction_apply", { input }),
  correctionList: (filter?: { task?: string; date?: string }) =>
    call<T.CorrectionList>("correction_list", { filter }),
  correctionUndo: (id: string) => call<T.CorrectionUndone>("correction_undo", { id }),

  // 計画
  blockAdd: (input: T.BlockInput) => call<T.PlanBlock>("block_add", { input }),
  blockList: (range?: T.RangeArgs) => call<T.BlockList>("block_list", { range }),
  blockUpdate: (id: string, input: T.BlockInput) => call<T.PlanBlock>("block_update", { id, input }),
  blockDelete: (id: string) => call<null>("block_delete", { id }),

  // テンプレート
  templateList: () => call<{ templates: T.TemplateView[] }>("block_template_list"),
  templateAdd: (input: T.TemplateInput) => call<T.TemplateView>("block_template_add", { input }),
  templateEdit: (id: string, input: T.TemplateInput) => call<T.TemplateView>("block_template_edit", { id, input }),
  templateDelete: (id: string) => call<T.TemplateView>("block_template_delete", { id }),
  templateSuggest: (date?: string) => call<T.Suggestion>("block_template_suggest", { date }),
  templatePreview: (input: T.ApplyInput) => call<T.TemplatePreview>("block_template_preview", { input }),
  templateApply: (input: T.ApplyInput) => call<T.ApplyResult>("block_template_apply", { input }),
  templateUndo: (date?: string) => call<T.UndoResult>("block_template_undo", { date }),
  templateSaveFrom: (input: { name: string; description?: string; date?: string; weekdays?: number[] }) =>
    call<T.TemplateView>("block_template_save_from", { input }),

  // 1 回だけ出す案内
  hintDismissed: (name: string) => call<boolean>("hint_dismissed", { name }),
  hintDismiss: (name: string) => call<null>("hint_dismiss", { name }),
  calendarList: (range?: T.RangeArgs) => call<T.CalendarEvent[]>("calendar_list", { range }),

  // 記録
  conditionAdd: (input: T.ConditionInput) => call<T.Condition>("condition_add", { input }),
  conditionList: (range?: T.RangeArgs) => call<T.Items<T.Condition>>("condition_list", { range }),
  concernAdd: (text: string, intensity?: number) =>
    call<T.Concern>("concern_add", { input: { text, intensity } }),
  concernList: (range?: T.RangeArgs) => call<T.Items<T.Concern>>("concern_list", { range }),
  reframeAdd: (input: T.ReframeInput) => call<T.Reframe>("reframe_add", { input }),
  reframeList: (range?: T.RangeArgs) => call<T.Items<T.Reframe>>("reframe_list", { range }),

  // 振り返り（AI）。generate / alternative は時間がかかる。
  context: (kind: T.ReflectionKind, date?: string) => call<unknown>("context", { kind, date }),
  reflectGenerate: (kind: T.ReflectionKind, date?: string) =>
    call<T.Reflection>("reflect_generate", { kind, date }),
  reflectAnswer: (id: string, answersMd: string) =>
    call<T.Reflection>("reflect_answer", { id, answers_md: answersMd }),
  reflectAnswerStructured: (id: string, answers: T.AnswersInput) =>
    call<T.Reflection>("reflect_answer_structured", { id, answers }),
  reflectAnswerDetails: (id: string) => call<T.AnswerDetail[]>("reflect_answer_details", { id }),
  reflectList: (kind?: T.ReflectionKind, limit = 30) =>
    call<T.Reflection[]>("reflect_list", { kind, limit }),
  reflectShow: (id: string) => call<T.Reflection>("reflect_show", { id }),
  reframeDeepen: (id: string) => call<T.Reframe>("reframe_deepen", { id }),

  // ERG
  ergOverview: (range?: T.RangeArgs) => call<T.ErgOverview>("erg_overview", { range }),
  ergReview: () => call<T.ErgReview>("erg_review"),
  ergClassify: (range?: T.RangeArgs) => call<T.ErgClassifyOutcome>("erg_classify", { range }),
  ergSet: (record_kind: T.ErgRecordKind, record_id: string, tag: T.ErgChoice) =>
    call<T.ErgLabel>("erg_set", { record_kind, record_id, tag }),
  ergConfirm: (record_kind: T.ErgRecordKind, record_id: string) =>
    call<T.ErgLabel>("erg_confirm", { record_kind, record_id }),

  // 目標・スキル
  goalList: () => call<{ goals: T.GoalView[] }>("goal_list"),
  goalAdd: (title: string) => call<T.Goal>("goal_add", { input: { title } }),
  keyResultAdd: (goal: string, title: string, target?: string, skills?: string[]) =>
    call<T.KeyResult>("key_result_add", { input: { goal, title, target, skills } }),
  skillList: () => call<{ skills: T.SkillView[] }>("skill_list"),
  skillAdd: (name: string, parent?: string) => call<T.Skill>("skill_add", { input: { name, parent } }),
  skillImportPreset: (name?: string) => call<T.PresetImport>("skill_import_preset", { name }),

  // 目標・スキルの編集と削除。削除は mode で dry_run（影響の確認）と force（削除）を指定する。
  goalEdit: (id: string, input: T.GoalEdit) => call<T.Goal>("goal_edit", { id, input }),
  goalDelete: (id: string, mode: T.DeleteMode) => call<T.DeleteOutcome>("goal_delete", { id, mode }),
  keyResultEdit: (id: string, input: T.KeyResultEdit) => call<T.KeyResult>("key_result_edit", { id, input }),
  keyResultDelete: (id: string, mode: T.DeleteMode) => call<T.DeleteOutcome>("key_result_delete", { id, mode }),
  // 成果の指標とタスクのつながり。紐付けは別の成果の指標に紐付いていたタスクを付け替える。
  keyResultLink: (keyResult: string, tasks: string[]) => call<T.LinkResult>("key_result_link", { key_result: keyResult, tasks }),
  keyResultUnlink: (tasks: string[]) => call<T.LinkedTask[]>("key_result_unlink", { tasks }),
  keyResultShow: (keyResult: string) => call<T.KeyResultSummary>("key_result_show", { key_result: keyResult }),
  // 成果の指標への紐づけの提案。自動では紐付けない。AI は設定されているときだけ（use_ai=false なら名前の近い候補だけ）。
  keyResultSuggest: (keyResult: string, useAi = false) => call<T.KeyResultSuggestions>("key_result_suggest", { key_result: keyResult, use_ai: useAi }),
  keyResultSuggestAccept: (keyResult: string, candidates: string[]) =>
    call<T.KeyResultSuggestDecision>("key_result_suggest_accept", { key_result: keyResult, candidates }),
  keyResultSuggestReject: (keyResult: string, candidates: string[]) =>
    call<T.KeyResultSuggestDecision>("key_result_suggest_reject", { key_result: keyResult, candidates }),
  // 成果の指標の現在値の記録と履歴（値の推移と週ごとの時間）。
  keyResultProgress: (keyResult: string, value: string, note?: string) =>
    call<T.ProgressRecorded>("key_result_progress", { key_result: keyResult, value, note }),
  keyResultHistory: (keyResult: string) => call<T.KeyResultHistory>("key_result_history", { key_result: keyResult }),
  skillEdit: (key: string, input: T.SkillEdit) => call<T.Skill>("skill_edit", { key, input }),
  skillDelete: (key: string, children: T.ChildPolicy, mode: T.DeleteMode) =>
    call<T.DeleteOutcome>("skill_delete", { key, children, mode }),

  // ルール
  taskRuleEdit: (id: string, input: T.TaskRuleEdit) => call<T.TaskAssignRule>("task_rule_edit", { id, input }),
  taskRuleDelete: (id: string) => call<unknown>("task_rule_delete", { id }),
  activityRuleList: () => call<{ rules: T.ActivityRule[] }>("activity_rule_list"),
  activityRuleAdd: (input: T.ActivityRuleInput) => call<T.ActivityRule>("activity_rule_add", { input }),
  activityRuleEdit: (id: string, input: T.ActivityRuleEdit) =>
    call<T.ActivityRule>("activity_rule_edit", { id, input }),
  activityRuleDelete: (id: string) => call<unknown>("activity_rule_delete", { id }),

  // 記録の編集と削除
  conditionEdit: (id: string, input: T.ConditionInput) => call<T.Condition>("condition_edit", { id, input }),
  conditionDelete: (id: string, mode?: T.DeleteMode) => call<T.DeleteOutcome>("condition_delete", { id, mode }),
  concernEdit: (id: string, text: string, intensity?: number) =>
    call<T.Concern>("concern_edit", { id, input: { text, intensity } }),
  concernDelete: (id: string, mode: T.DeleteMode) => call<T.DeleteOutcome>("concern_delete", { id, mode }),
  reframeEdit: (id: string, input: T.ReframeEditInput) => call<T.Reframe>("reframe_edit", { id, input }),
  reframeDelete: (id: string, mode?: T.DeleteMode) => call<T.DeleteOutcome>("reframe_delete", { id, mode }),

  // カレンダーソース・活動（設定）
  calendarSourceList: () => call<{ sources: T.CalendarSourceView[] }>("calendar_source_list"),
  calendarSourceAdd: (name: string, url: string) =>
    call<T.CalendarSourceView>("calendar_source_add", { name, url }),
  calendarSourceEdit: (name: string, newName?: string, url?: string) =>
    call<T.CalendarSourceView>("calendar_source_edit", { name, new_name: newName, url }),
  calendarSourceDelete: (name: string, mode: T.DeleteMode) =>
    call<T.DeleteOutcome>("calendar_source_delete", { name, mode }),
  activityList: () => call<{ labels: string[] }>("activity_list"),
  activityAdd: (label: string) => call<{ labels: string[] }>("activity_add", { label }),
  activityRename: (from: string, to: string) =>
    call<{ labels: string[] }>("activity_rename", { from, to }),
  activityRemove: (label: string) => call<{ labels: string[] }>("activity_remove", { label }),

  // レポート
  report: (by: T.ReportBy, range?: T.RangeArgs, includeIdle = false) =>
    call<T.Report>("report", { range, by, include_idle: includeIdle }),

  heatmap: (by?: string, from?: string, to?: string) => call<T.Heatmap>("heatmap", { by, from, to }),

  // 設定
  configAll: () => call<Record<string, unknown>>("config_all"),
  configGet: (key: string) => call<{ key: string; value: unknown }>("config_get", { key }),
  /** `value` は TOML の値（`true`、`"Asia/Tokyo"`、`[...]`）。 */
  configSet: (key: string, value: string) => call<{ key: string; path: string }>("config_set", { key, value }),
  configPath: () => call<{ path: string; db: string }>("config_path"),

  // ウインドウ・通知
  openScreen: (screen: string) => call<void>("open_screen", { screen }),
  snooze: (minutes: number) => call<{ snooze_until: number }>("snooze", { minutes }),
  snoozeState: () => call<{ snooze_until: number }>("snooze_state"),
  takeInitialScreen: () => call<string | null>("take_initial_screen"),
  pendingNudge: () => call<T.NudgeEvent | null>("pending_nudge"),
  /** ポップアップからの応答。`nudge_id` が無ければテスト通知として扱われる。 */
  nudgeRespond: (r: {
    nudge_id?: string | null;
    action?: string | null;
    value?: number | null;
    input?: string;
    open_screen?: string | null;
  }) => call<void>("nudge_respond", r),
  runtimeState: () => call<T.RuntimeMode>("runtime_state"),
  notifyTest: () => call<void>("notify_test"),

  // はじめに
  welcomeStatus: () => call<{ completed: boolean; completed_at: number | null }>("welcome_status"),
  welcomeComplete: () => call<{ completed: boolean; completed_at: number | null }>("welcome_complete"),
  welcomeReset: () => call<{ completed: boolean; completed_at: number | null }>("welcome_reset"),
};

// ===== イベント =====

export type EventName = "segments-updated" | "nudge" | "doing-changed" | "import-completed" | "open-screen" | "quick-open";

export function on<P = unknown>(name: EventName, handler: (payload: P) => void): Promise<UnlistenFn> {
  return listen<P>(name, (e) => handler(e.payload));
}
