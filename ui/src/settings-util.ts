// 設定画面の純粋ロジック（タブの解決、ルールの統合一覧）。表示に依存しない。
import type { ActivityRule, MatchField, PatternType, TaskMatchRule, TaskView } from "./api";

export const SETTINGS_TABS = [
  { id: "general", label: "全般" },
  { id: "notify", label: "通知" },
  { id: "review", label: "判定" },
  { id: "rules", label: "ルール" },
  { id: "labels", label: "活動ラベル" },
  { id: "calendar", label: "カレンダー" },
  { id: "templates", label: "テンプレート" },
  { id: "data", label: "設定ファイル" },
] as const;

export type SettingsTabId = (typeof SETTINGS_TABS)[number]["id"];
const TAB_IDS: readonly string[] = SETTINGS_TABS.map((t) => t.id);

/** `#/settings/<tab>` の tab を解決する。未指定・知らない値は `general`。 */
export function settingsTab(seg: string | undefined): SettingsTabId {
  return (seg && TAB_IDS.includes(seg) ? seg : "general") as SettingsTabId;
}

// ===== ルールの統合一覧 =====

export type RuleKind = "task" | "activity" | "ignore";
export type RuleFilter = "all" | RuleKind;

/** 対象（MatchField）の表示名。value は識別子のまま、画面にはこの名前を出す。 */
export const FIELD_LABEL: Record<MatchField, string> = {
  exe: "アプリ（実行ファイル名）",
  title: "ウインドウ名",
  cwd: "作業フォルダ",
  repo: "リポジトリ",
  url: "URL",
  git_branch: "ブランチ",
};
/** 種類（PatternType）の表示名。 */
export const PTYPE_LABEL: Record<PatternType, string> = {
  glob: "ワイルドカード（* と ?）",
  regex: "正規表現",
  contains: "含む",
};

export const RULE_KIND_LABEL: Record<RuleKind, string> = { task: "タスク", activity: "活動", ignore: "無視" };
export const RULE_FILTERS: { id: RuleFilter; label: string }[] = [
  { id: "all", label: "すべて" },
  { id: "task", label: RULE_KIND_LABEL.task },
  { id: "activity", label: RULE_KIND_LABEL.activity },
  { id: "ignore", label: RULE_KIND_LABEL.ignore },
];

const KIND_ORDER: Record<RuleKind, number> = { task: 0, activity: 1, ignore: 2 };

/** 一覧の 1 行。タスクは task_id / 対象タスク名、活動は活動ラベル、無視は exe 名だけ。 */
export interface RuleItem {
  key: string;
  kind: RuleKind;
  /** タスク・活動はルールの ID、無視は exe 名。 */
  id: string;
  field?: MatchField;
  type?: PatternType;
  pattern: string;
  priority: number;
  enabled: boolean;
  taskId?: string;
  /** 行に出す「行き先」（タスク名・活動ラベル）。無視は空。 */
  target: string;
}

export function mergeRules(
  taskRules: TaskMatchRule[],
  activityRules: ActivityRule[],
  ignoreExes: string[],
  tasks: Pick<TaskView, "id" | "title">[],
): RuleItem[] {
  const title = new Map(tasks.map((t) => [t.id, t.title]));
  const items: RuleItem[] = [
    ...taskRules.map(
      (r): RuleItem => ({
        key: `task:${r.id}`,
        kind: "task",
        id: r.id,
        field: r.field,
        type: r.pattern_type,
        pattern: r.pattern,
        priority: r.priority,
        enabled: r.enabled,
        taskId: r.task_id,
        target: title.get(r.task_id) ?? "（不明なタスク）",
      }),
    ),
    ...activityRules.map(
      (r): RuleItem => ({
        key: `activity:${r.id}`,
        kind: "activity",
        id: r.id,
        field: r.field,
        type: r.pattern_type,
        pattern: r.pattern,
        priority: r.priority,
        enabled: r.enabled,
        target: r.activity_label,
      }),
    ),
    ...ignoreExes.map(
      (e): RuleItem => ({ key: `ignore:${e}`, kind: "ignore", id: e, pattern: e, priority: 0, enabled: true, target: "" }),
    ),
  ];
  // 種類（タスク → 活動 → 無視）、優先度の大きい順、パターン順。
  return items.sort(
    (a, b) => KIND_ORDER[a.kind] - KIND_ORDER[b.kind] || b.priority - a.priority || a.pattern.localeCompare(b.pattern),
  );
}

/** 種類とタスクで絞る。タスクで絞ると、そのタスクの割り当てルールだけになる（空文字は絞らない）。 */
export function filterRules(items: RuleItem[], kind: RuleFilter, taskId = ""): RuleItem[] {
  return items.filter((i) => (kind === "all" || i.kind === kind) && (!taskId || (i.kind === "task" && i.taskId === taskId)));
}
