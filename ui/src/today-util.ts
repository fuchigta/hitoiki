// 今日の画面の純粋なロジック（日付の移動・サイドパネルの状態・「いま」の行のまとめ方・内訳の選び方）。画面に依存しない。
import type { BlockDraft } from "./block-form";
import type { Millis, ReportRow } from "./api";
import type { TimeSpan } from "./plan-util";
import { showStartGuide } from "./welcome-util";

const DATE_RE = /^\d{4}-\d{2}-\d{2}$/;

/** `?date=` の値。YYYY-MM-DD として読めなければ今日（`today`）。 */
export function dateFromParam(v: string | null | undefined, today: string): string {
  if (!v || !DATE_RE.test(v)) return today;
  const [y, m, d] = v.split("-").map(Number);
  const t = new Date(Date.UTC(y, m - 1, d));
  // 2/30 のような存在しない日付は今日にする。
  return t.getUTCFullYear() === y && t.getUTCMonth() === m - 1 && t.getUTCDate() === d ? v : today;
}

/** YYYY-MM-DD を `delta` 日ずらす（暦の上の日数。タイムゾーンの影響を受けない）。 */
export function shiftDate(date: string, delta: number): string {
  const [y, m, d] = date.split("-").map(Number);
  const t = new Date(Date.UTC(y, m - 1, d + delta));
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${t.getUTCFullYear()}-${pad(t.getUTCMonth() + 1)}-${pad(t.getUTCDate())}`;
}

/** 曜日（`水曜日`）。 */
export function weekdayLabel(date: string): string {
  const [y, m, d] = date.split("-").map(Number);
  return `${"日月火水木金土"[new Date(Date.UTC(y, m - 1, d)).getUTCDay()]}曜日`;
}

/** 実績画面の期間指定（`?period=`）で開ける日なら、その値。それ以外は undefined（既定の今週になる）。 */
export function actualPeriodFor(date: string, today: string): "today" | "yesterday" | undefined {
  if (date === today) return "today";
  if (date === shiftDate(today, -1)) return "yesterday";
  return undefined;
}

/** 今日の画面へのパラメータ。今日は省略する。 */
export function dateParams(date: string, today: string): Record<string, string | undefined> {
  return { date: date === today ? undefined : date };
}

// ===== サイドパネル =====

/** サイドパネルに出す内容（同時に 1 つ）。 */
export type SidePanel =
  | { kind: "block"; draft: BlockDraft }
  | { kind: "correct"; span: TimeSpan }
  | { kind: "segment"; id: string }
  | { kind: "design" }
  | { kind: "nudge" };

/**
 * サイドパネルの状態。デザインのモードは「完了」まで保つので、別のパネル（ブロックの編集など）を閉じると
 * デザインのパネルに戻る。
 */
export interface SideState {
  design: boolean;
  other: Exclude<SidePanel, { kind: "design" }> | null;
}

export type SideAction =
  | { type: "open"; panel: SidePanel }
  /** いま出ているパネルを閉じる（デザイン中の別パネルならデザインに戻る。デザインのパネル自体は閉じない）。 */
  | { type: "close" }
  /** デザインのパネルを畳む（モードは保つ）。 */
  | { type: "toggleDesign" }
  /** 「完了」。デザインを終え、パネルも閉じる。 */
  | { type: "finishDesign" }
  /** 表示する日付が変わった。日付に紐づくパネル（ブロック・補正・区間）は閉じる。 */
  | { type: "dateChanged" };

export const SIDE_INITIAL: SideState = { design: false, other: null };

export function sideReducer(s: SideState, a: SideAction): SideState {
  switch (a.type) {
    case "open":
      return a.panel.kind === "design" ? { design: true, other: null } : { ...s, other: a.panel };
    case "close":
      return { ...s, other: null };
    case "toggleDesign":
      return s.design && !s.other ? { design: false, other: null } : { design: true, other: null };
    case "finishDesign":
      return { design: false, other: null };
    case "dateChanged":
      return s.other && s.other.kind !== "nudge" ? { ...s, other: null } : s;
  }
}

/** 実際に出すパネル。別のパネルがあればそれ、なければデザイン。 */
export function visiblePanel(s: SideState): SidePanel | null {
  return s.other ?? (s.design ? { kind: "design" } : null);
}

/** 画面に出すパネル。ブロックの作成・編集は、その日の範囲（`hasRange`）が届くまで出さない。 */
export function shownPanel(panel: SidePanel | null, hasRange: boolean): SidePanel | null {
  if (!panel) return null;
  return panel.kind === "block" && !hasRange ? null : panel;
}

/** サイドパネルの内容のうち、タイムラインで強調するもの（編集中のブロック・補正の範囲・開いている区間）。 */
export interface TimelineSelection {
  draft: BlockDraft | null;
  actualSpan: TimeSpan | null;
  segmentId: string | null;
}

export function timelineSelection(panel: SidePanel | null): TimelineSelection {
  return {
    draft: panel?.kind === "block" ? panel.draft : null,
    actualSpan: panel?.kind === "correct" ? panel.span : null,
    segmentId: panel?.kind === "segment" ? panel.id : null,
  };
}

// ===== タイムライン =====

interface Spanned {
  start_at: Millis;
  end_at: Millis;
}

/** タイムラインの表示範囲に収めたい時刻（現在時刻・ブロック・区間・終日でない予定の両端）。`now` は今日だけ渡す。 */
export function viewAnchors(now: Millis | undefined, blocks: Spanned[], segments: Spanned[], events: (Spanned & { all_day: boolean })[]): Millis[] {
  return [
    ...(now === undefined ? [] : [now]),
    ...blocks.flatMap((b) => [b.start_at, b.end_at]),
    ...segments.flatMap((x) => [x.start_at, x.end_at]),
    ...events.filter((c) => !c.all_day).flatMap((c) => [c.start_at, c.end_at]),
  ];
}

/** 凡例に出す種別（区間の出現順。重複なし）。 */
export function legendKinds<K>(segments: { kind: K }[]): K[] {
  return [...new Set(segments.map((x) => x.kind))];
}

/** まだブロックにしていないタスク（デザインのパネルに並べる候補）。 */
export function unplacedTasks<T extends { id: string }>(tasks: T[], blocks: { task_id: string | null }[]): T[] {
  const placed = new Set(blocks.map((b) => b.task_id).filter(Boolean));
  return tasks.filter((t) => !placed.has(t.id));
}

// ===== 内訳・案内 =====

interface BreakdownStatus {
  today: { by_kind: ReportRow[]; top_tasks: ReportRow[]; total_ms: number };
}
interface BreakdownReport {
  rows: ReportRow[];
  total_ms: number;
}

/** 内訳の節に出すもの。 */
export interface Breakdown {
  kindRows: ReportRow[];
  /** タスクごと（上位 8 件）。 */
  taskRows: ReportRow[];
  total: number;
}

/**
 * 内訳。今日は status の集計、それ以外はレポートの集計（今日は null で届く）。
 * 元になる値がまだ届いていなければ null（節を出さない）。
 */
export function breakdownOf(
  isToday: boolean,
  status: BreakdownStatus | undefined,
  kinds: BreakdownReport | null | undefined,
  tasks: BreakdownReport | null | undefined,
): Breakdown | null {
  if (isToday) return status ? { kindRows: status.today.by_kind, taskRows: status.today.top_tasks, total: status.today.total_ms } : null;
  if (!kinds || !tasks) return null;
  return { kindRows: kinds.rows, taskRows: tasks.rows.slice(0, 8), total: kinds.total_ms };
}

/** 「次にやること」の案内に出す件数。今日の状況と実績が届いていて、デザイン中でなく、使い始めのときだけ。それ以外は null。 */
export function startGuideFor(
  isToday: boolean,
  status: { doing: unknown[] } | undefined,
  segments: unknown[] | undefined,
  designing: boolean,
): { doing: number; segments: number } | null {
  if (!isToday || !status || !segments || designing) return null;
  return showStartGuide(status.doing.length, segments.length) ? { doing: status.doing.length, segments: segments.length } : null;
}

// ===== 「いま」 =====

interface NowBlock {
  task_id?: string | null;
  task_title?: string | null;
  label: string | null;
}
interface NowDoing {
  task: { id: string; title: string };
}

/** いまの計画ブロックと進行中のタスクが同じものか（タスクが同じか、表示名が同じ）。 */
export function blockIsTask(b: NowBlock, d: NowDoing): boolean {
  if (b.task_id && b.task_id === d.task.id) return true;
  const title = b.task_title ?? b.label ?? "";
  return title !== "" && title === d.task.title;
}

/** 進行中のタスクのうち、いまのブロックと重なるもの（その行にブロックの時刻と「集中」を添える）。 */
export function mergedDoingIds(b: NowBlock | undefined, doing: NowDoing[]): Set<string> {
  const ids = new Set<string>();
  if (!b) return ids;
  for (const d of doing) if (blockIsTask(b, d)) ids.add(d.task.id);
  return ids;
}

// ===== 合計 =====

interface DaySeg {
  start_at: Millis;
  end_at: Millis;
  kind: string;
}

/** 合計に含めない種別（無操作・休憩・無視）。 */
const EXCLUDED = new Set(["idle", "break", "ignored"]);

/** 区間の合計（ミリ秒）。無操作・休憩・無視は含めない。今日以外の日の合計に使う。 */
export function daySum(segments: DaySeg[]): number {
  let ms = 0;
  for (const s of segments) if (!EXCLUDED.has(s.kind)) ms += Math.max(0, s.end_at - s.start_at);
  return ms;
}
