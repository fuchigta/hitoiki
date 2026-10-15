// core の Serialize 出力に対応する型。時刻は UTC の Unix ミリ秒。

export type Millis = number;

export type SegmentKind =
  | "work"
  | "agent_dialog"
  | "agent_watch"
  | "meeting"
  | "idle"
  | "break"
  | "interrupt"
  | "ignored";
export type SegmentStatus = "auto" | "needs_review" | "confirmed";
export type TaskStatus = "todo" | "doing" | "done" | "dropped";
export type TaskKind = "normal" | "interrupt";
export type MatchField = "exe" | "title" | "cwd" | "repo" | "url" | "git_branch";
export type PatternType = "glob" | "regex" | "contains";
export type ErgTag = "existence" | "relatedness" | "growth";
export type ReportBy = "task" | "activity" | "urgent_important" | "key_result" | "skill" | "agent" | "kind";

export interface AppError {
  code: string;
  message: string;
}

export interface RangeArgs {
  /** today / yesterday / this-week / last-week / YYYY-MM-DD */
  date?: string;
  from?: string;
  to?: string;
}

export interface RangeView {
  start_at: Millis;
  end_at: Millis;
  start: string;
  end: string;
}

export interface SegmentView {
  id: string;
  start_at: Millis;
  end_at: Millis;
  start: string;
  end: string;
  duration_ms: number;
  kind: SegmentKind;
  task_id: string | null;
  task_title: string | null;
  activity_label: string | null;
  confidence: number | null;
  decided_by: string | null;
  status: SegmentStatus;
  agent_session_id: string | null;
  exe_name: string | null;
  title: string | null;
  /** PC 外で過ごした時間（実績の補正で、無操作・記録なしを割り当てたもの）。 */
  offline?: boolean;
}

export interface Task {
  id: string;
  title: string;
  body_md: string;
  status: TaskStatus;
  kind: TaskKind;
  urgent: boolean | null;
  important: boolean | null;
  estimate_min: number | null;
  planned_start_at: Millis | null;
  planned_end_at: Millis | null;
  key_result_id: string | null;
  external_url: string | null;
  created_at: Millis;
  updated_at: Millis;
  done_at: Millis | null;
}

export interface KeyResultRef {
  id: string;
  title: string;
  goal_id: string;
  goal_title: string;
}

/** Task にスキルと成果の指標を足したもの（フラットに展開される）。 */
export interface TaskView extends Task {
  skills: string[];
  key_result: KeyResultRef | null;
}

export interface TaskMatchRule {
  id: string;
  task_id: string;
  field: MatchField;
  pattern: string;
  pattern_type: PatternType;
  priority: number;
  enabled: boolean;
}

export interface TaskInput {
  title?: string;
  body?: string;
  urgent?: boolean;
  important?: boolean;
  estimate_min?: number;
  key_result?: string;
  /** 編集で成果の指標との紐付けを外す。 */
  clear_key_result?: boolean;
  skills?: string[];
  /** 編集でスキルを全て外す。 */
  clear_skills?: boolean;
  external_url?: string;
  planned_start_at?: Millis;
  planned_end_at?: Millis;
  interrupt?: boolean;
}

export interface TaskShow {
  task: TaskView;
  rules: TaskMatchRule[];
  tracked_ms: number;
  tracked_segments: number;
}

/** タスク削除の影響の見積もり。 */
export interface TaskDeletePreview {
  task_id: string;
  title: string;
  status: TaskStatus;
  segments: number;
  tracked_ms: number;
  overrides: number;
  corrections: number;
  rules: number;
  skills: number;
  blocks: number;
  template_items: number;
  doing_logs: number;
  is_doing: boolean;
  needs_force: boolean;
  /** 影響の日本語の説明。 */
  summary: string;
}

export interface TaskDeleteResult {
  deleted: TaskDeletePreview;
  range: RangeView | null;
}

export interface TaskChange {
  task: TaskView;
  demoted: TaskView[];
}

export interface SwitchResult extends TaskChange {
  /** 新規に作成したタスクか。 */
  created: boolean;
}

export interface QuickShortcutState {
  /** 設定値（空は無効）。 */
  shortcut: string;
  /** OS に登録できている正規形。 */
  registered: string | null;
  /** 直近の登録失敗の理由。 */
  error: string | null;
}

export interface RuleInput {
  task: string;
  field: MatchField;
  pattern: string;
  pattern_type?: PatternType;
  priority?: number;
}

export interface ReportRow {
  key: string | null;
  label: string;
  duration_ms: number;
  ratio: number;
  segments: number;
  breakdown?: Record<string, number>;
}

export interface Report {
  range: RangeView;
  by: ReportBy;
  total_ms: number;
  rows: ReportRow[];
  excluded: { idle_ms: number; break_ms: number; ignored_ms: number };
  include_idle: boolean;
  multi_attribution: boolean;
  note: string | null;
}

export interface HeatmapDay {
  date: string;
  value: number;
  /** 濃淡の段階（0〜4）。 */
  level: 0 | 1 | 2 | 3 | 4;
}

export interface Heatmap {
  from: string;
  to: string;
  /** `from` の曜日（月曜 = 0）。 */
  from_weekday: number;
  week_start: "monday";
  by: string;
  label: string;
  unit: "ms" | "count";
  days: HeatmapDay[];
  total: number;
  active_days: number;
  max: number;
  thresholds: number[];
}

export interface DoingView {
  task: TaskView;
  since: Millis | null;
}

export interface TodaySummary {
  range: RangeView;
  total_ms: number;
  idle_ms: number;
  break_ms: number;
  /** 無視した区間の合計（合計には含めない）。 */
  ignored_ms: number;
  by_kind: ReportRow[];
  top_tasks: ReportRow[];
}

export type PushState = "unanswered" | "answered" | "dismissed";

/** 配信された通知（`status` の未回答と `push list`）。 */
export interface PushView {
  id: string;
  at: Millis;
  at_local: string;
  trigger: string;
  /** トリガーの日本語表示名（始業など）。 */
  trigger_label: string;
  title: string;
  body: string;
  state: PushState;
  /** 通知の中身（ボタン・入力欄・対象期間）。読み取れなければ null。 */
  payload: PushPayload | null;
}

export interface Status {
  now: Millis;
  now_local: string;
  doing: DoingView[];
  recent_segments: SegmentView[];
  today: TodaySummary;
  needs_review_count: number;
  needs_review_short_count: number;
  unanswered_pushes: PushView[];
  update_available?: string | null;
}

/** 記録なしの時間帯（収集器が動いていなかった区間。無操作・ロック・スリープとは別）。 */
export interface GapView {
  start_at: Millis;
  end_at: Millis;
  start: string;
  end: string;
  duration_ms: number;
}

export interface Log {
  range: RangeView;
  segments: SegmentView[];
  /** 記録なしの時間帯（時刻順）。 */
  gaps: GapView[];
  raw?: unknown[];
}

export interface ReviewList {
  range: RangeView;
  count: number;
  segments: SegmentView[];
  /** `review.min_sec` 未満のため隠した件数（全件表示のときは 0）。 */
  hidden_short: number;
  min_sec: number;
}

export interface ReviewSetInput {
  segment: string;
  task?: string;
  kind?: SegmentKind;
  activity?: string;
  rule?: boolean;
}

/** 無操作区間のワンタップ分類。休憩=break、会議=meeting、考えごと=作業+活動「考えごと」、離席=休憩+活動「離席」。 */
export type IdlePreset = "break" | "meeting" | "thinking" | "away";

export interface ReviewBatchInput {
  segments: string[];
  task?: string;
  kind?: SegmentKind;
  activity?: string;
  rule?: boolean;
}

export interface ReviewBatchResult {
  count: number;
  segments: SegmentView[];
  override_ids: string[];
  rules: CreatedRule[];
}

/** 無視リスト（collect.ignore_exes）。 */
export interface IgnoreList {
  exes: string[];
  default_exes: string[];
  customized: boolean;
}

export interface IgnoreChange {
  exe: string;
  changed: boolean;
  exes: string[];
  recomputed: RangeView | null;
}

export interface IgnoreReason {
  code: "system" | "flicker" | "review" | "user_ignored" | "user_no_task";
  text: string;
}

/** 「これは無視しますか？」という提案。 */
export interface IgnoreSuggestion {
  exe: string;
  reasons: IgnoreReason[];
  span_count: number;
  total_ms: number;
  short_count: number;
  short_ratio: number;
  review_count: number;
  user_ignored_count: number;
  user_no_task_count: number;
  summary: string;
}

export interface UnignoreResult {
  count: number;
  overrides_removed: number;
  range: RangeView;
}

/** ルール化の提案の識別（受け入れ・却下に渡す）。 */
export interface RuleSuggestionRef {
  task_id: string;
  field: MatchField;
  pattern: string;
  pattern_type: PatternType;
}

export interface RuleSuggestion extends RuleSuggestionRef {
  task_title: string | null;
  /** 直近に同じ割り当てを確定した回数。 */
  count: number;
  /** 表示用の条件名（exe 名、または作業ディレクトリ）。 */
  target: string;
}

export interface CreatedRule {
  rule_type: "task_match_rule" | "activity_rule";
  id: string;
  field: MatchField;
  pattern: string;
  pattern_type: PatternType;
  task_id: string | null;
  activity_label: string | null;
  reused: boolean;
}

export interface ReviewSetResult {
  segment: SegmentView;
  override_id: string;
  rule: CreatedRule | null;
}

export interface TimeBlock {
  id: string;
  start_at: Millis;
  end_at: Millis;
  task_id: string | null;
  label: string | null;
  focus: boolean;
}

export interface BlockView extends TimeBlock {
  task_title: string | null;
}

export interface BlockList {
  range: RangeView;
  blocks: BlockView[];
}

export interface BlockInput {
  start: Millis;
  end: Millis;
  task?: string;
  label?: string;
  focus?: boolean;
}

export interface TemplateItemInput {
  /** ローカル時刻 HH:MM */
  start: string;
  /** ローカル時刻 HH:MM（24:00 も可） */
  end: string;
  label?: string | null;
  focus?: boolean;
  task?: string | null;
}

export interface TemplateInput {
  name: string;
  description?: string | null;
  /** 1=月 … 7=日 */
  weekdays: number[];
  items: TemplateItemInput[];
}

export interface TemplateItemView {
  id: string;
  start: string;
  end: string;
  start_min: number;
  end_min: number;
  label: string | null;
  focus: boolean;
  task_id: string | null;
  task_title: string | null;
}

export interface TemplateView {
  id: string;
  name: string;
  description: string | null;
  weekdays: number[];
  created_at: Millis;
  updated_at: Millis;
  blocks: TemplateItemView[];
}

export type Overlap = "skip" | "overlay";

export interface ApplyInput {
  template: string;
  date?: string;
  overlap?: Overlap;
}

export interface Conflict {
  kind: "block" | "calendar";
  title: string;
  start_at: Millis;
  end_at: Millis;
}

export interface PreviewBlock {
  start_at: Millis;
  end_at: Millis;
  label: string | null;
  focus: boolean;
  task_id: string | null;
  task_title: string | null;
  conflicts: Conflict[];
}

export interface TemplatePreview {
  template: string;
  date: string;
  overlap: Overlap;
  blocks: PreviewBlock[];
}

export interface ApplyResult {
  application_id: string;
  template: string;
  date: string;
  created: BlockView[];
  skipped: PreviewBlock[];
}

export interface UndoResult {
  template: string;
  date: string;
  removed: number;
}

export interface Suggestion {
  date: string;
  weekday: number;
  templates: TemplateView[];
}

export interface Checkin {
  id: string;
  at: Millis;
  energy: number | null;
  mood: number | null;
  stress: number | null;
  erg_tag: ErgTag | null;
  note: string | null;
  push_id: string | null;
}

export interface Hassle {
  id: string;
  at: Millis;
  text: string;
  intensity: number;
  segment_id: string | null;
  push_id: string | null;
}

export interface AbcRecord {
  id: string;
  at: Millis;
  activating_event: string;
  belief: string | null;
  consequence_emotion: string | null;
  consequence_intensity: number | null;
  dispute: string | null;
  ai_dialog_md: string | null;
  hassle_id: string | null;
}

export interface Items<T> {
  start_at: Millis;
  end_at: Millis;
  items: T[];
}

export interface CheckInput {
  energy?: number;
  mood?: number;
  stress?: number;
  note?: string;
}

export interface AbcInput {
  activating_event?: string;
  belief?: string;
  consequence_emotion?: string;
  consequence_intensity?: number;
  from_hassle?: string;
}

export interface Goal {
  id: string;
  title: string;
  description_md: string | null;
  period_start: Millis | null;
  period_end: Millis | null;
  status: string;
}

export interface KeyResult {
  id: string;
  goal_id: string;
  title: string;
  target: string | null;
  current: string | null;
  status: string;
  /** 数え方。 */
  measure: KeyResultMeasure;
  /** 数え方が「取り組んだ日数」のとき「取り組んだ日」とみなす 1 日の最小分数。 */
  active_day_min: number;
}

export type KeyResultMeasure = "time" | "active_days" | "done_tasks";

export interface KeyResultWeekBar {
  /** 週の月曜（YYYY-MM-DD）。 */
  week_start: string;
  duration_ms: number;
}

export interface KeyResultSkillTime {
  skill_id: string | null;
  name: string;
  duration_ms: number;
}

export interface KeyResultTask {
  id: string;
  title: string;
  status: TaskStatus;
  estimate_min: number | null;
  tracked_ms: number;
  done_at: Millis | null;
}

/** 成果の指標の集計（core の `key_result_summary`）。 */
export interface KeyResultSummary {
  key_result: KeyResult;
  goal: { id: string; title: string };
  range: RangeView;
  range_source: "given" | "goal" | "recent_weeks";
  measure: KeyResultMeasure;
  time_ms: number;
  active_days: number;
  active_day_min: number;
  /** 成果の指標に付けたスキルの名前。時間・日数はこのスキルが主で、紐付けたタスクも重ならないように合算する。 */
  key_result_skills: string[];
  /** 内訳: 成果の指標のスキル（子スキルを含む）が付いたタスク・活動の区間の時間。`time_ms` の一部。 */
  from_skill_ms: number;
  /** 内訳: 上に当てはまらない、紐付けたタスクの区間の時間（両方に当てはまる区間は上に入る）。 */
  from_task_ms: number;
  done_count: number;
  task_count: number;
  weekly: KeyResultWeekBar[];
  skills: KeyResultSkillTime[];
  estimate: { estimate_min_total: number; estimated_tasks: number; actual_ms_of_estimated: number };
  tasks: KeyResultTask[];
}

/** 成果の指標の現在値の履歴 1 件（core の `KeyResultProgress`）。 */
export interface KeyResultProgressEntry {
  id: string;
  key_result_id: string;
  /** `text` の先頭の数値。数として読めなければ null。 */
  value: number | null;
  /** 入力された現在値。消したときは null。 */
  text: string | null;
  note: string | null;
  at: Millis;
}

export interface KeyResultHistoryWeek {
  week_start: string;
  duration_ms: number;
  /** その週の終わり時点の現在値の数値。 */
  value: number | null;
  text: string | null;
  /** その週に残した履歴の数。 */
  changes: number;
}

/** 成果の指標の時系列（core の `key_result_history`）。 */
export interface KeyResultHistory {
  key_result: KeyResult;
  range: RangeView;
  range_source: "given" | "goal" | "recent_weeks";
  weeks: KeyResultHistoryWeek[];
  entries: KeyResultProgressEntry[];
}

export interface ProgressRecorded {
  key_result: KeyResult;
  entry: KeyResultProgressEntry;
}

export interface LinkedTask {
  id: string;
  title: string;
  /** 付け替え前の成果の指標（無かったときは null）。 */
  previous_key_result: { id: string; title: string } | null;
}

export interface LinkResult {
  key_result: KeyResult;
  linked: LinkedTask[];
}

/** 成果の指標への紐づけの候補の種類。 */
export type KeyResultCandidateKind = "task" | "skill";

/** 紐づけの候補 1 件。`source` は名前の近さ（local）/ AI（ai）/ 両方（both）。 */
export interface KeyResultCandidate {
  kind: KeyResultCandidateKind;
  id: string;
  name: string;
  score: number;
  source: "name_match" | "ai" | "both";
}

/** `key_result_suggest` の結果。スキルか紐付けタスクが既にある成果の指標は `eligible` が false。 */
export interface KeyResultSuggestions {
  key_result: { id: string; title: string };
  eligible: boolean;
  candidates: KeyResultCandidate[];
  ai: { used: boolean; note: string | null };
}

/** `key_result_suggest_accept` / `key_result_suggest_reject` の結果。 */
export interface KeyResultSuggestDecision {
  key_result: { id: string; title: string };
  items: { kind: KeyResultCandidateKind; id: string; name: string }[];
}

export interface GoalView extends Goal {
  key_results: KeyResult[];
}

export interface Skill {
  id: string;
  name: string;
  parent_id: string | null;
  description: string | null;
  preset: boolean;
}

export interface SkillView extends Skill {
  parent_name: string | null;
}

// ===== 編集・削除 =====

export interface ImpactLine {
  label: string;
  count: number;
}

/** 削除の影響（core の `Impact`）。 */
export interface Impact {
  entity: string;
  id: string;
  name: string;
  lines: ImpactLine[];
  note: string | null;
  /** 警告（振り返りの回答から参照されている、など）。あると CLI は `--force` が必要。 */
  warnings?: string[];
}

export interface DeleteOutcome {
  dry_run: boolean;
  deleted: boolean;
  impact: Impact;
}

export interface DeleteMode {
  dry_run?: boolean;
  force?: boolean;
}

export type ChildPolicy = "reparent" | "delete";

export interface GoalEdit {
  title?: string;
  /** 空文字で説明を消す。 */
  description?: string;
  from?: Millis;
  to?: Millis;
  clear_period?: boolean;
  status?: string;
}

export interface KeyResultEdit {
  title?: string;
  /** 空文字で消す。 */
  target?: string;
  current?: string;
  /** 現在値を変えたときに履歴へ残すメモ。 */
  current_note?: string;
  status?: string;
  goal?: string;
  measure?: KeyResultMeasure;
  active_day_min?: number;
  /** 成果の指標のスキル（名前）。空でなければ今のスキルを置き換える。 */
  skills?: string[];
  /** 成果の指標のスキルを全て外す。 */
  clear_skills?: boolean;
}

export interface SkillEdit {
  name?: string;
  parent?: string;
  clear_parent?: boolean;
  description?: string;
}

export interface TaskRuleEdit {
  field?: MatchField;
  pattern?: string;
  pattern_type?: PatternType;
  priority?: number;
  enabled?: boolean;
}

export interface ActivityRule {
  id: string;
  field: MatchField;
  pattern: string;
  pattern_type: PatternType;
  activity_label: string;
  priority: number;
  enabled: boolean;
}

export interface ActivityRuleInput {
  field: MatchField;
  pattern: string;
  pattern_type?: PatternType;
  activity_label: string;
  priority?: number;
}

export interface ActivityRuleEdit {
  field?: MatchField;
  pattern?: string;
  pattern_type?: PatternType;
  activity_label?: string;
  priority?: number;
  enabled?: boolean;
}

export interface AbcEditInput {
  activating_event: string;
  belief?: string;
  consequence_emotion?: string;
  consequence_intensity?: number;
  dispute?: string;
}

export interface CalendarSourceView {
  name: string;
  type: string;
  /** URL は秘密情報なのでホスト名だけ。 */
  host: string;
}

export interface PresetImport {
  [key: string]: unknown;
}

/** プッシュのボタン（core の `PushAction`）。押すと `{action: id, value}` で応答する。 */
export interface PushAction {
  /** "energy" | "stress" | "snooze_15" | "snooze_60" | "snooze_today" | "open" など。 */
  id: string;
  label: string;
  value: number | null;
}

export interface PushInput {
  /** 入力の保存先を示す ID（"hassle"）。 */
  id: string;
  placeholder: string;
}

/** core の `PushPayload`。 */
export interface PushPayload {
  trigger: string;
  title: string;
  body: string;
  actions: PushAction[];
  input: PushInput | null;
  /** 通知クリックで開く画面（例 "reflection/day_start"）。 */
  open_screen: string | null;
  /** idle_return の対象期間 [開始, 終了]（無ければ省略）。 */
  idle_range?: [Millis, Millis];
  /** urgent_task / important_task の対象タスク（無ければ省略）。「始める」でこのタスクを始める。 */
  task_id?: string;
  /** block_start の対象ブロック（無ければ省略）。 */
  block_id?: string;
}

/** `push` イベント／`pending_push` の中身。`push_id` が無いのはテスト通知。 */
export interface PushEvent {
  push_id: string | null;
  payload: PushPayload;
}

export type ReflectionKind = "day_start" | "day_end" | "weekly";

export interface Reflection {
  id: string;
  at: Millis;
  kind: ReflectionKind;
  context_json: string;
  questions_md: string;
  answers_md: string | null;
  summary_md: string | null;
  /** 構造化出力（`StructuredOutput` の JSON 文字列）。null は従来の Markdown のみ。 */
  structured_json: string | null;
  /** 構造化された問いへの回答の対応表（`AnswersRecord` の JSON 文字列）。 */
  answers_json: string | null;
}

export type QuestionKind = "checkin" | "hassle" | "abc" | "free";
export type Scale = "energy" | "mood" | "stress";

/** core の `service::structured::Question`。使わない項目は省略される。 */
export interface Question {
  kind: QuestionKind;
  prompt: string;
  scales?: Scale[];
  examples?: string[];
  activating_event?: string;
  belief_prompt?: string;
}

export interface StructuredOutput {
  summary?: string;
  questions: Question[];
}

/** 保存済みの回答 1 件（どの問いに答え、どのレコードを作ったか）。 */
export interface AnsweredItem {
  index: number;
  kind: QuestionKind;
  record_id?: string;
  text?: string;
}

export interface AnswersRecord {
  items: AnsweredItem[];
}

/** core の `reflect::AnswerDetail`。回答済みの問いと、保存した記録の中身。記録が削除されていれば無い。 */
export interface AnswerDetail {
  index: number;
  kind: QuestionKind;
  prompt: string;
  record_id?: string;
  text?: string;
  checkin?: Checkin;
  hassle?: Hassle;
  abc?: AbcRecord;
  /** 回答が作った記録が削除されていて、もう無い。 */
  missing?: boolean;
}

/** core の `reflect::AnswerItem`（問いの種類で使う項目が決まる）。 */
export interface AnswerItem {
  index: number;
  energy?: number;
  mood?: number;
  stress?: number;
  note?: string;
  text?: string;
  intensity?: number;
  activating_event?: string;
  belief?: string;
  emotion?: string;
  emotion_intensity?: number;
  dispute?: string;
}

export interface AnswersInput {
  answers: AnswerItem[];
}

/** 常駐処理の状態。 */
export type RuntimeMode =
  | { mode: "starting" }
  | { mode: "own" }
  | { mode: "external"; message: string }
  | { mode: "failed"; message: string };

export interface CalendarEvent {
  id: string;
  source: string;
  external_id: string;
  start_at: Millis;
  end_at: Millis;
  all_day: boolean;
  title: string;
  location: string | null;
  is_busy: boolean;
  fetched_at: Millis;
}

// ===== 実績の補正 =====

export type CorrectionCategory = "work" | "agent_dialog" | "confirmed" | "meeting" | "idle" | "gap";

/** 補正の対象になりうる区間（範囲で切った後の部分）。記録なしの `id` は `gap:<開始のミリ秒>`。 */
export interface CorrectionCandidate {
  id: string;
  gap: boolean;
  start_at: Millis;
  end_at: Millis;
  start: string;
  end: string;
  duration_ms: number;
  full_duration_ms: number;
  kind: SegmentKind | null;
  status: SegmentStatus | null;
  task_id: string | null;
  task_title: string | null;
  activity_label: string | null;
  exe_name: string | null;
  title: string | null;
  category: CorrectionCategory;
  offline: boolean;
  /** 割り当てると PC 外の時間として計上されるか（無操作・休憩・記録なし）。 */
  becomes_offline: boolean;
  checked: boolean;
}

export interface CorrectionCandidates {
  range: RangeView;
  /** 範囲の端の吸着の幅（秒、`review.snap_sec`）。 */
  snap_sec: number;
  candidates: CorrectionCandidate[];
}

export interface CorrectionInput {
  start_at: Millis;
  end_at: Millis;
  task?: string;
  no_task?: boolean;
  rest?: boolean;
  activity?: string;
  /** 対象にする候補の ID。省略すると既定のチェック。 */
  selected?: string[];
}

export interface CorrectionPreview {
  range: RangeView;
  target: string;
  selected_count: number;
  total_ms: number;
  offline_ms: number;
  excluded_count: number;
  summary: string;
  candidates: CorrectionCandidate[];
}

export interface CorrectionView {
  id: string;
  start_at: Millis;
  end_at: Millis;
  start: string;
  end: string;
  task_id: string | null;
  task_title: string | null;
  kind: SegmentKind | null;
  activity_label: string | null;
  total_ms: number;
  offline_ms: number;
  segment_count: number;
  created_at: Millis;
  created: string;
}

export interface CorrectionApplied {
  correction: CorrectionView;
  preview: CorrectionPreview;
  override_ids: string[];
}

export interface CorrectionList {
  count: number;
  corrections: CorrectionView[];
}

export interface CorrectionUndone {
  correction: CorrectionView;
  overrides_removed: number;
  range: RangeView;
}

// ===== ERG（記録の内容から AI が判定し、ユーザーが確認・修正する） =====

/** 3 つの観点か、どれにも関わらない（none）。 */
export type ErgChoice = ErgTag | "none";
export type ErgRecordKind = "checkin" | "hassle" | "abc" | "reflection";
/** user = ユーザーが付けた・確定した / ai = 確信度が閾値以上で AI が採用 / suggested = 確認待ち（AI の提案で未確定） */
export type ErgSource = "user" | "ai" | "suggested";

/** core の `model::ErgLabel`。 */
export interface ErgLabel {
  record_kind: ErgRecordKind;
  record_id: string;
  tag: ErgChoice;
  source: ErgSource;
  confidence: number | null;
  reason: string | null;
  at: Millis;
}

/** 期間内の記録の分布（件数。事実の集計で、評価は含まない）。 */
export interface ErgDistribution {
  existence: number;
  relatedness: number;
  growth: number;
  none: number;
  pending: number;
  unlabeled: number;
  ai_decided: number;
  total: number;
  legend: string;
}

export interface ErgOverview {
  range: RangeView;
  distribution: ErgDistribution;
  labels: ErgLabel[];
  pending_total: number;
}

export interface ErgItem extends ErgLabel {
  record_at: Millis;
  excerpt: string;
}

export interface ErgReview {
  items: ErgItem[];
}

export interface ErgClassifiedItem {
  record_kind: ErgRecordKind;
  record_id: string;
  at: Millis;
  excerpt: string;
  tag: ErgChoice;
  confidence: number | null;
  reason: string;
  source: ErgSource;
}

export interface ErgClassifyOutcome {
  range: RangeView;
  dry_run: boolean;
  /** 判定しなかった理由（AI が未設定など）。 */
  skipped: string | null;
  targets: number;
  adopted: number;
  pending: number;
  unread: number;
  items: ErgClassifiedItem[];
}
