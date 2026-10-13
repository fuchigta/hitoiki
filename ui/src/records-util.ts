// 記録画面（チェックイン・ハッスル・ABC・振り返りを 1 本の時系列に）の純粋ロジック。画面に依存しない。
// 文言は事実を並べるだけにして、記録の内容や件数の偏りに良し悪しの評価は付けない。

import type {
  AbcRecord,
  Checkin,
  ErgChoice,
  ErgDistribution,
  ErgLabel,
  ErgRecordKind,
  Hassle,
  Millis,
  Reflection,
  ReflectionKind,
} from "./api";
import { ergKey, ergState } from "./erg-util";
import { ALL_PERIODS, type PeriodId } from "./ui/period-util";

export type RecordKind = "checkin" | "hassle" | "abc" | "reflection";

export const RECORD_KIND_LABEL: Record<RecordKind, string> = {
  checkin: "チェックイン",
  hassle: "ハッスル",
  abc: "ABC",
  reflection: "振り返り",
};

/** 一覧の種類の絞り込み（先頭は「すべて」）。 */
export const KIND_FILTERS: { id: RecordKind | "all"; label: string }[] = [
  { id: "all", label: "すべて" },
  { id: "checkin", label: RECORD_KIND_LABEL.checkin },
  { id: "hassle", label: RECORD_KIND_LABEL.hassle },
  { id: "abc", label: RECORD_KIND_LABEL.abc },
  { id: "reflection", label: RECORD_KIND_LABEL.reflection },
];

/** `?kind=` から種類の絞り込みを得る。知らない値は「すべて」。 */
export function kindFilter(param: string | null | undefined): RecordKind | "all" {
  return KIND_FILTERS.some((k) => k.id === param) ? (param as RecordKind | "all") : "all";
}

export const REFLECTION_KIND_LABEL: Record<ReflectionKind, string> = {
  day_start: "始業",
  day_end: "終業",
  weekly: "週次",
};

/** 「＋ 記録」で作れる種類。 */
export type NewKind = "checkin" | "hassle" | "abc" | ReflectionKind;

export const NEW_KINDS: { id: NewKind; label: string }[] = [
  { id: "checkin", label: "チェックイン" },
  { id: "hassle", label: "ハッスル" },
  { id: "abc", label: "ABC" },
  { id: "day_start", label: "始業の振り返り" },
  { id: "day_end", label: "終業の振り返り" },
  { id: "weekly", label: "週次の振り返り" },
];

export function isReflectionKind(k: string): k is ReflectionKind {
  return k === "day_start" || k === "day_end" || k === "weekly";
}

export type RecordsSelection =
  | { mode: "detail"; kind: RecordKind; id: string }
  | { mode: "new"; kind: NewKind };

/** `#/records/<種類>/<ID>`・`#/records/new/<種類>` のパスから選択を得る。形が合わなければ null。 */
export function recordsSelection(path: string[]): RecordsSelection | null {
  const [a, b] = path;
  if (a === "new") {
    return b && NEW_KINDS.some((k) => k.id === b) ? { mode: "new", kind: b as NewKind } : null;
  }
  if ((a === "checkin" || a === "hassle" || a === "abc" || a === "reflection") && b) {
    return { mode: "detail", kind: a, id: b };
  }
  return null;
}

/** 期間の既定は今週。`?period=` が知らない値でも今週。 */
export const DEFAULT_PERIOD: PeriodId = "this-week";

export function recordsPeriod(param: string | null | undefined): PeriodId {
  return ALL_PERIODS.includes(param as PeriodId) ? (param as PeriodId) : DEFAULT_PERIOD;
}

// ===== 時系列 =====

export type RecordSource =
  | { kind: "checkin"; rec: Checkin }
  | { kind: "hassle"; rec: Hassle }
  | { kind: "abc"; rec: AbcRecord }
  | { kind: "reflection"; rec: Reflection };

export interface TimelineEntry {
  kind: RecordKind;
  id: string;
  at: Millis;
  /** 一覧の行に出す 1 行の要約。 */
  summary: string;
  /** この記録の ERG（無ければ未判定）。 */
  erg?: ErgLabel;
  source: RecordSource;
}

/** チェックインの要約。入力のある値だけを並べ、一言メモがあれば添える。 */
export function checkinSummary(c: Checkin): string {
  const parts: string[] = [];
  if (c.energy != null) parts.push(`エネルギー ${c.energy}`);
  if (c.mood != null) parts.push(`気分 ${c.mood}`);
  if (c.stress != null) parts.push(`ストレス ${c.stress}`);
  const note = c.note?.trim();
  const head = parts.join("・");
  if (!note) return head || "チェックイン";
  return head ? `${head}　${note}` : note;
}

export function reflectionAnswered(r: Reflection): boolean {
  return !!(r.answers_md?.trim() || r.answers_json);
}

/** 振り返りの要約（「終業の振り返り・回答あり」）。 */
export function reflectionSummary(r: Reflection): string {
  return `${REFLECTION_KIND_LABEL[r.kind]}の振り返り・${reflectionAnswered(r) ? "回答あり" : "未回答"}`;
}

export function oneLine(s: string): string {
  return s.replace(/\s+/g, " ").trim();
}

export interface TimelineInput {
  checkins: Checkin[];
  hassles: Hassle[];
  abcs: AbcRecord[];
  reflections: Reflection[];
}

/** 期間 `[start, end)` に入っているか。 */
export function inRange(at: Millis, start: Millis, end: Millis): boolean {
  return at >= start && at < end;
}

/**
 * 4 種類の記録を、時刻の降順で 1 本の時系列にする（同時刻は ID の降順で安定させる）。
 * 振り返りは API が期間で絞らないので、`range` で絞る。
 */
export function buildTimeline(
  input: TimelineInput,
  labels: Map<string, ErgLabel>,
  range?: { start: Millis; end: Millis },
): TimelineEntry[] {
  const out: TimelineEntry[] = [];
  const push = (kind: RecordKind, rec: { id: string; at: Millis }, summary: string, source: RecordSource) =>
    out.push({ kind, id: rec.id, at: rec.at, summary, erg: labels.get(ergKey(kind as ErgRecordKind, rec.id)), source });
  for (const c of input.checkins) push("checkin", c, checkinSummary(c), { kind: "checkin", rec: c });
  for (const h of input.hassles) push("hassle", h, oneLine(h.text), { kind: "hassle", rec: h });
  for (const a of input.abcs) push("abc", a, oneLine(a.activating_event), { kind: "abc", rec: a });
  for (const r of input.reflections) {
    if (range && !inRange(r.at, range.start, range.end)) continue;
    push("reflection", r, reflectionSummary(r), { kind: "reflection", rec: r });
  }
  return out.sort((x, y) => y.at - x.at || (x.id < y.id ? 1 : x.id > y.id ? -1 : 0));
}

/** 種類と「ERG 確認待ち」で絞る。 */
export function filterTimeline(entries: TimelineEntry[], kind: RecordKind | "all", pendingOnly: boolean): TimelineEntry[] {
  return entries.filter((e) => (kind === "all" || e.kind === kind) && (!pendingOnly || ergState(e.erg) === "pending"));
}

/** ERG が確認待ちの件数。 */
export function pendingCount(entries: TimelineEntry[]): number {
  return entries.filter((e) => ergState(e.erg) === "pending").length;
}

// ===== ERG の表示 =====

const ERG_SHORT: Record<ErgChoice, string> = {
  existence: "存在",
  relatedness: "関係",
  growth: "成長",
  none: "該当なし",
};

/**
 * 一覧の行に付ける ERG の印。ユーザー確定は観点の名前、AI の判定は名前に「AI」を添える。
 * 確認待ち（AI の提案で未確定）は「確認待ち」。未判定は無し。
 */
export function ergBadge(label: ErgLabel | undefined): string | undefined {
  switch (ergState(label)) {
    case "user":
      return ERG_SHORT[label!.tag];
    case "ai":
      return `${ERG_SHORT[label!.tag]} AI`;
    case "pending":
      return "確認待ち";
    default:
      return undefined;
  }
}

/** 期間内の ERG の分布の 1 行要約（「存在 3・関係 5・成長 2・該当なし 1（未判定 2）」）。件数を並べるだけ。 */
export function ergCountLine(d: ErgDistribution): string {
  if (d.total === 0) return "この期間に、文章のある記録はありません。";
  let s = `存在 ${d.existence}・関係 ${d.relatedness}・成長 ${d.growth}・該当なし ${d.none}`;
  const rest: string[] = [];
  if (d.pending > 0) rest.push(`確認待ち ${d.pending}`);
  if (d.unlabeled > 0) rest.push(`未判定 ${d.unlabeled}`);
  if (rest.length > 0) s += `（${rest.join("、")}）`;
  return s;
}
