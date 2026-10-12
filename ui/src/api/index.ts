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
  ruleAdd: (input: T.RuleInput) => call<T.TaskMatchRule>("rule_add", { input }),
  // クイック入力（割り込みなどで素早くタスクを切り替える）
  quickPrevious: () => call<T.TaskView | null>("quick_previous"),
  quickSwitch: (input: { query: string; new?: boolean; interrupt?: boolean }) =>
    call<T.SwitchResult>("quick_switch", { input }),
  quickBack: () => call<T.SwitchResult>("quick_back"),
  quickClose: () => call<void>("quick_close"),
  quickShortcutState: () => call<T.QuickShortcutState>("quick_shortcut_state"),
  quickShortcutSet: (shortcut: string) => call<{ shortcut: string; registered: string | null }>("quick_shortcut_set", { shortcut }),

  ruleList: (task?: string) => call<{ rules: T.TaskMatchRule[] }>("rule_list", { task }),

  // 区間・仕分け
  log: (range?: T.RangeArgs, raw = false) => call<T.Log>("log", { range, raw }),
  reviewList: (range?: T.RangeArgs, all = false) => call<T.ReviewList>("review_list", { range, all }),
  reviewSet: (input: T.ReviewSetInput) => call<T.ReviewSetResult>("review_set", { input }),
  reviewSetMany: (input: T.ReviewBatchInput) => call<T.ReviewBatchResult>("review_set_many", { input }),
  reviewIdle: (segments: string[], preset: T.IdlePreset) =>
    call<T.ReviewBatchResult>("review_idle", { segments, preset }),
  reviewSuggestions: () => call<T.RuleSuggestion[]>("review_suggestions"),
  reviewSuggestAccept: (input: T.RuleSuggestionRef) => call<T.CreatedRule>("review_suggest_accept", { input }),
  reviewSuggestDismiss: (input: T.RuleSuggestionRef) => call<void>("review_suggest_dismiss", { input }),

  // 無視するアプリ・区間
  reviewIgnore: (segments: string[]) => call<T.ReviewBatchResult>("review_ignore", { segments }),
  reviewUnignore: (segments: string[]) => call<T.UnignoreResult>("review_unignore", { segments }),
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
  blockAdd: (input: T.BlockInput) => call<T.TimeBlock>("block_add", { input }),
  blockList: (range?: T.RangeArgs) => call<T.BlockList>("block_list", { range }),
  blockUpdate: (id: string, input: T.BlockInput) => call<T.TimeBlock>("block_update", { id, input }),
  blockDelete: (id: string) => call<null>("block_delete", { id }),

  // ブロックのテンプレート
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
  checkinAdd: (input: T.CheckInput) => call<T.Checkin>("checkin_add", { input }),
  checkinList: (range?: T.RangeArgs) => call<T.Items<T.Checkin>>("checkin_list", { range }),
  hassleAdd: (text: string, intensity?: number) =>
    call<T.Hassle>("hassle_add", { input: { text, intensity } }),
  hassleList: (range?: T.RangeArgs) => call<T.Items<T.Hassle>>("hassle_list", { range }),
  abcAdd: (input: T.AbcInput) => call<T.AbcRecord>("abc_add", { input }),
  abcList: (range?: T.RangeArgs) => call<T.Items<T.AbcRecord>>("abc_list", { range }),

  // 振り返り（AI）。generate / dispute は時間がかかる。
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
  abcDispute: (id: string) => call<T.AbcRecord>("abc_dispute", { id }),

  // OKR・スキル
  okrList: () => call<{ objectives: T.ObjectiveView[] }>("okr_list"),
  okrAddObjective: (title: string) => call<T.Objective>("okr_add_objective", { input: { title } }),
  okrAddKr: (objective: string, title: string, target?: string) =>
    call<T.KeyResult>("okr_add_kr", { input: { objective, title, target } }),
  skillList: () => call<{ skills: T.SkillView[] }>("skill_list"),
  skillAdd: (name: string, parent?: string) => call<T.Skill>("skill_add", { input: { name, parent } }),
  skillImportPreset: (name?: string) => call<T.PresetImport>("skill_import_preset", { name }),

  // OKR・スキルの編集と削除。削除は mode で dry_run（影響の確認）と force（削除）を指定する。
  okrEditObjective: (id: string, input: T.ObjectiveEdit) => call<T.Objective>("okr_edit_objective", { id, input }),
  okrDeleteObjective: (id: string, mode: T.DeleteMode) => call<T.DeleteOutcome>("okr_delete_objective", { id, mode }),
  okrEditKr: (id: string, input: T.KrEdit) => call<T.KeyResult>("okr_edit_kr", { id, input }),
  okrDeleteKr: (id: string, mode: T.DeleteMode) => call<T.DeleteOutcome>("okr_delete_kr", { id, mode }),
  skillEdit: (key: string, input: T.SkillEdit) => call<T.Skill>("skill_edit", { key, input }),
  skillDelete: (key: string, children: T.ChildPolicy, mode: T.DeleteMode) =>
    call<T.DeleteOutcome>("skill_delete", { key, children, mode }),

  // ルール
  ruleEdit: (id: string, input: T.TaskRuleEdit) => call<T.TaskMatchRule>("rule_edit", { id, input }),
  ruleDelete: (id: string) => call<unknown>("rule_delete", { id }),
  activityRuleList: () => call<{ rules: T.ActivityRule[] }>("activity_rule_list"),
  activityRuleAdd: (input: T.ActivityRuleInput) => call<T.ActivityRule>("activity_rule_add", { input }),
  activityRuleEdit: (id: string, input: T.ActivityRuleEdit) =>
    call<T.ActivityRule>("activity_rule_edit", { id, input }),
  activityRuleDelete: (id: string) => call<unknown>("activity_rule_delete", { id }),

  // 記録の編集と削除
  checkinEdit: (id: string, input: T.CheckInput) => call<T.Checkin>("checkin_edit", { id, input }),
  checkinDelete: (id: string) => call<T.DeleteOutcome>("checkin_delete", { id }),
  hassleEdit: (id: string, text: string, intensity?: number) =>
    call<T.Hassle>("hassle_edit", { id, input: { text, intensity } }),
  hassleDelete: (id: string, mode: T.DeleteMode) => call<T.DeleteOutcome>("hassle_delete", { id, mode }),
  abcEdit: (id: string, input: T.AbcEditInput) => call<T.AbcRecord>("abc_edit", { id, input }),
  abcDelete: (id: string) => call<T.DeleteOutcome>("abc_delete", { id }),

  // カレンダーソース・活動ラベル（設定）
  calendarSourceList: () => call<{ sources: T.CalendarSourceView[] }>("calendar_source_list"),
  calendarSourceAdd: (name: string, url: string) =>
    call<T.CalendarSourceView>("calendar_source_add", { name, url }),
  calendarSourceEdit: (name: string, newName?: string, url?: string) =>
    call<T.CalendarSourceView>("calendar_source_edit", { name, new_name: newName, url }),
  calendarSourceDelete: (name: string, mode: T.DeleteMode) =>
    call<T.DeleteOutcome>("calendar_source_delete", { name, mode }),
  activityLabelList: () => call<{ labels: string[] }>("activity_label_list"),
  activityLabelAdd: (label: string) => call<{ labels: string[] }>("activity_label_add", { label }),
  activityLabelRename: (from: string, to: string) =>
    call<{ labels: string[] }>("activity_label_rename", { from, to }),
  activityLabelRemove: (label: string) => call<{ labels: string[] }>("activity_label_remove", { label }),

  // レポート
  report: (by: T.ReportBy, range?: T.RangeArgs, includeIdle = false) =>
    call<T.Report>("report", { range, by, include_idle: includeIdle }),

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
  pendingPush: () => call<T.PushEvent | null>("pending_push"),
  /** ポップアップからの応答。`push_id` が無ければテスト通知として扱われる。 */
  pushRespond: (r: {
    push_id?: string | null;
    action?: string | null;
    value?: number | null;
    input?: string;
    open_screen?: string | null;
  }) => call<void>("push_respond", r),
  runtimeState: () => call<T.RuntimeMode>("runtime_state"),
  notifyTest: () => call<void>("notify_test"),

  // オンボーディング
  onboardingStatus: () => call<{ completed: boolean; completed_at: number | null }>("onboarding_status"),
  onboardingComplete: () => call<{ completed: boolean; completed_at: number | null }>("onboarding_complete"),
  onboardingReset: () => call<{ completed: boolean; completed_at: number | null }>("onboarding_reset"),
};

// ===== イベント =====

export type EventName = "segments-updated" | "push" | "doing-changed" | "import-completed" | "open-screen" | "quick-open";

export function on<P = unknown>(name: EventName, handler: (payload: P) => void): Promise<UnlistenFn> {
  return listen<P>(name, (e) => handler(e.payload));
}
