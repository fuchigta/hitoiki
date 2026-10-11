// core の Serialize 出力に対応する型。時刻は UTC の Unix ミリ秒。

export type Millis = number;

export type SegmentKind =
  | "work"
  | "agent_dialog"
  | "agent_watch"
  | "meeting"
  | "idle"
  | "break"
  | "interrupt";
export type SegmentStatus = "auto" | "needs_review" | "confirmed";
export type TaskStatus = "todo" | "doing" | "done" | "dropped";
export type TaskKind = "normal" | "interrupt";
export type MatchField = "exe" | "title" | "cwd" | "repo" | "url" | "git_branch";
export type PatternType = "glob" | "regex" | "contains";
export type ErgTag = "existence" | "relatedness" | "growth";
export type ReportBy = "task" | "activity" | "quadrant" | "okr" | "skill" | "agent" | "kind";

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
  objective_id: string;
  objective_title: string;
}

/** Task にスキルと KR を足したもの（フラットに展開される）。 */
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

export interface TaskChange {
  task: TaskView;
  demoted: TaskView[];
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
  excluded: { idle_ms: number; break_ms: number };
  include_idle: boolean;
  multi_attribution: boolean;
  note: string | null;
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
  by_kind: ReportRow[];
  top_tasks: ReportRow[];
}

export interface PushView {
  id: string;
  at: Millis;
  trigger: string;
  payload: unknown;
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
  erg?: ErgTag;
  note?: string;
}

export interface AbcInput {
  activating_event?: string;
  belief?: string;
  consequence_emotion?: string;
  consequence_intensity?: number;
  from_hassle?: string;
}

export interface Objective {
  id: string;
  title: string;
  description_md: string | null;
  period_start: Millis | null;
  period_end: Millis | null;
  status: string;
}

export interface KeyResult {
  id: string;
  objective_id: string;
  title: string;
  target: string | null;
  current: string | null;
  status: string;
}

export interface ObjectiveView extends Objective {
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
