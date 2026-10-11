import type { Millis } from "./api";

/** スナップの刻み（分）。 */
export const SNAP_MIN = 15;
const MIN = 60_000;

/** `ms` を `origin` からの刻み（`stepMin` 分）に丸める（四捨五入）。 */
export function snapMillis(ms: Millis, origin: Millis, stepMin = SNAP_MIN): Millis {
  const step = stepMin * MIN;
  return origin + Math.round((ms - origin) / step) * step;
}

/** 縦位置（0〜height px）を、日の範囲内でスナップした時刻に変換する。 */
export function yToTime(
  y: number,
  height: number,
  dayStart: Millis,
  dayEnd: Millis,
  stepMin = SNAP_MIN,
): Millis {
  if (height <= 0) return dayStart;
  const ratio = Math.min(1, Math.max(0, y / height));
  const t = snapMillis(dayStart + ratio * (dayEnd - dayStart), dayStart, stepMin);
  return Math.min(dayEnd, Math.max(dayStart, t));
}

/**
 * ドラッグの始点・終点（縦位置）から、スナップ済みの範囲を作る。
 * 上下どちら向きのドラッグでも start < end にし、幅がない場合は 1 刻み分を確保する。
 */
export function dragRange(
  y1: number,
  y2: number,
  height: number,
  dayStart: Millis,
  dayEnd: Millis,
  stepMin = SNAP_MIN,
): { start: Millis; end: Millis } {
  const a = yToTime(Math.min(y1, y2), height, dayStart, dayEnd, stepMin);
  let b = yToTime(Math.max(y1, y2), height, dayStart, dayEnd, stepMin);
  if (b <= a) {
    b = a + stepMin * MIN;
    if (b > dayEnd) return { start: Math.max(dayStart, dayEnd - stepMin * MIN), end: dayEnd };
  }
  return { start: a, end: b };
}

/** 時刻を縦位置の割合（0〜100 %）に変換する。範囲外は端に寄せる。 */
export function timeToPercent(t: Millis, dayStart: Millis, dayEnd: Millis): number {
  if (dayEnd <= dayStart) return 0;
  return ((Math.min(dayEnd, Math.max(dayStart, t)) - dayStart) / (dayEnd - dayStart)) * 100;
}

// ===== 1 日のタイムラインの操作（計画画面・今日画面で共通） =====

export interface TimeSpan {
  start: Millis;
  end: Millis;
}

const HOUR = 60 * MIN;

/** クリックだけ（ドラッグなし）のときの範囲。クリック位置から `lenMin` 分、日の終わりを超えるなら手前にずらす。 */
export function clickRange(
  y: number,
  height: number,
  dayStart: Millis,
  dayEnd: Millis,
  lenMin = 60,
  stepMin = SNAP_MIN,
): TimeSpan {
  const len = lenMin * MIN;
  const start = Math.min(yToTime(y, height, dayStart, dayEnd, stepMin), Math.max(dayStart, dayEnd - len));
  return { start, end: Math.min(dayEnd, start + len) };
}

/** 既存の範囲を縦に `dy` px 動かす（長さはそのまま、刻みにスナップし、日の範囲に収める）。 */
export function moveRange(
  span: TimeSpan,
  dy: number,
  height: number,
  dayStart: Millis,
  dayEnd: Millis,
  stepMin = SNAP_MIN,
): TimeSpan {
  if (height <= 0) return span;
  const len = span.end - span.start;
  const shifted = span.start + (dy / height) * (dayEnd - dayStart);
  let start = snapMillis(shifted, dayStart, stepMin);
  start = Math.max(dayStart, Math.min(start, dayEnd - len));
  return { start, end: start + len };
}

/** 範囲の上端（`start`）または下端（`end`）を縦位置 `y` に動かす。最低 1 刻み分の長さを保つ。 */
export function resizeRange(
  span: TimeSpan,
  edge: "start" | "end",
  y: number,
  height: number,
  dayStart: Millis,
  dayEnd: Millis,
  stepMin = SNAP_MIN,
): TimeSpan {
  const step = stepMin * MIN;
  const t = yToTime(y, height, dayStart, dayEnd, stepMin);
  if (edge === "start") return { start: Math.min(t, span.end - step), end: span.end };
  return { start: span.start, end: Math.max(t, span.start + step) };
}

/** 範囲が重なるか（接しているだけは重ならない）。 */
export function overlaps(a: TimeSpan, b: TimeSpan): boolean {
  return a.start < b.end && b.start < a.end;
}

/** `span` と重なる項目（`exceptId` は除く）。 */
export function findOverlaps<T extends { id?: string; start_at: Millis; end_at: Millis }>(
  span: TimeSpan,
  items: T[],
  exceptId?: string,
): T[] {
  return items.filter((i) => i.id !== exceptId && overlaps(span, { start: i.start_at, end: i.end_at }));
}

/**
 * タスクを縦位置 `y` に置いたときの範囲。長さは見積もり（分、刻みに切り上げ）。無ければ `defaultMin` 分。
 */
export function placeTaskRange(
  y: number,
  height: number,
  dayStart: Millis,
  dayEnd: Millis,
  estimateMin: number | null,
  defaultMin = 60,
  stepMin = SNAP_MIN,
): TimeSpan {
  const base = estimateMin && estimateMin > 0 ? estimateMin : defaultMin;
  const len = Math.max(stepMin, Math.ceil(base / stepMin) * stepMin);
  return clickRange(y, height, dayStart, dayEnd, len, stepMin);
}

/** `from` 以降で、`busy` と重ならない最初の `lenMin` 分の空き。見つからなければ null。 */
export function nextFreeSlot(
  lenMin: number,
  from: Millis,
  dayEnd: Millis,
  busy: TimeSpan[],
  stepMin = SNAP_MIN,
): TimeSpan | null {
  const len = lenMin * MIN;
  const step = stepMin * MIN;
  let start = Math.ceil(from / step) * step;
  const sorted = [...busy].sort((a, b) => a.start - b.start);
  for (;;) {
    if (start + len > dayEnd) return null;
    const hit = sorted.find((b) => overlaps({ start, end: start + len }, b));
    if (!hit) return { start, end: start + len };
    start = Math.ceil(hit.end / step) * step;
  }
}

/**
 * タイムラインに表示する時間帯。既定は 7〜22 時で、予定・ブロック・実績・現在時刻がはみ出すときは
 * 1 時間単位で広げる。`all` なら 1 日全体。
 */
export function visibleWindow(
  dayStart: Millis,
  dayEnd: Millis,
  times: Millis[],
  all = false,
  fromHour = 7,
  toHour = 22,
): TimeSpan {
  if (all) return { start: dayStart, end: dayEnd };
  let lo = dayStart + fromHour * HOUR;
  let hi = dayStart + toHour * HOUR;
  for (const t of times) {
    if (t < dayStart || t > dayEnd) continue;
    lo = Math.min(lo, dayStart + Math.floor((t - dayStart) / HOUR) * HOUR);
    hi = Math.max(hi, dayStart + Math.ceil((t - dayStart) / HOUR) * HOUR);
  }
  return { start: Math.max(dayStart, lo), end: Math.min(dayEnd, hi) };
}

/** 現在時刻の線の位置（0〜100 %）。範囲外なら null。 */
export function nowPercent(now: Millis, start: Millis, end: Millis): number | null {
  if (now < start || now > end || end <= start) return null;
  return timeToPercent(now, start, end);
}

/** いま進行中のブロック（開始 <= now < 終了）。複数あれば先に始まったもの。 */
export function currentBlock<T extends { start_at: Millis; end_at: Millis }>(blocks: T[], now: Millis): T | undefined {
  return [...blocks].sort((a, b) => a.start_at - b.start_at).find((b) => b.start_at <= now && now < b.end_at);
}

interface SegLike {
  start_at: Millis;
  end_at: Millis;
  kind: string;
  status: string;
}

export interface TodayDigest {
  /** 要確認の件数と、その長さ（ミリ秒）。 */
  reviewCount: number;
  reviewMs: number;
  /** エージェント対話の時間（`agent_dialog` のみ。待機は含めない）。 */
  agentMs: number;
}

/** 今日の区間から、要確認とエージェント対話の要約を作る。 */
export function digestSegments(segments: SegLike[]): TodayDigest {
  let reviewCount = 0;
  let reviewMs = 0;
  let agentMs = 0;
  for (const s of segments) {
    const d = Math.max(0, s.end_at - s.start_at);
    if (s.status === "needs_review") {
      reviewCount++;
      reviewMs += d;
    }
    if (s.kind === "agent_dialog") agentMs += d;
  }
  return { reviewCount, reviewMs, agentMs };
}

/** 今日のデザインモードに入るか。明示的な指定（通知・振り返りから）か、ブロックが 0 件でまだ完了していないとき。 */
export function shouldDesign(blockCount: number, requested: boolean, finishedToday: boolean): boolean {
  if (requested) return true;
  return blockCount === 0 && !finishedToday;
}

/** 重なりの一覧を、短い文にする（3 件まで名前を出す）。 */
export function describeConflicts(titles: string[]): string {
  const uniq = [...new Set(titles.filter(Boolean))];
  if (uniq.length === 0) return "";
  const shown = uniq.slice(0, 3).join("、");
  return uniq.length > 3 ? `${shown} ほか ${uniq.length - 3} 件` : shown;
}
