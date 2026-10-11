// 実績の補正（範囲指定の付け替え）の純粋関数。画面（correction.tsx、day-grid.tsx）から使う。
// 対象の判定・初期のチェック・切り出しは core（service::correction）が行い、ここは範囲選択（吸着）と、
// チェックの切り替えに応じた合計の再計算だけを持つ。
import type { CorrectionCandidate, Millis } from "./api";
import type { TimeSpan } from "./plan-util";

const MIN = 60_000;

/** 区間の境目（開始・終了）を、昇順・重複なしで返す。 */
export function boundaryTimes(spans: { start_at: Millis; end_at: Millis }[]): Millis[] {
  const set = new Set<Millis>();
  for (const s of spans) {
    set.add(s.start_at);
    set.add(s.end_at);
  }
  return [...set].sort((a, b) => a - b);
}

/** `t` に最も近い境目が `snapMs` 以内ならそこへ吸着する。同じ距離なら早い方。`snapMs` が 0 以下なら吸着しない。 */
export function snapToBoundary(t: Millis, boundaries: Millis[], snapMs: number): Millis {
  if (snapMs <= 0) return t;
  let best: Millis | null = null;
  let bestDist = Infinity;
  for (const b of boundaries) {
    const d = Math.abs(b - t);
    if (d < bestDist || (d === bestDist && best !== null && b < best)) {
      best = b;
      bestDist = d;
    }
  }
  return best !== null && bestDist <= snapMs ? best : t;
}

/** 縦位置（0〜height px）を、表示範囲内の 1 分刻みの時刻に変換する。 */
export function yToMinute(y: number, height: number, viewStart: Millis, viewEnd: Millis): Millis {
  if (height <= 0) return viewStart;
  const ratio = Math.min(1, Math.max(0, y / height));
  const t = viewStart + ratio * (viewEnd - viewStart);
  const m = Math.round(t / MIN) * MIN;
  return Math.min(viewEnd, Math.max(viewStart, m));
}

/**
 * ドラッグの始点・終点（縦位置）から、補正の範囲を作る。上下どちら向きでもよい。
 * 1 分刻みに丸めたうえで、両端を区間の境目へ吸着させる（`snapMs` 以内）。1 分に満たなければ null。
 */
export function pickRange(
  y1: number,
  y2: number,
  height: number,
  viewStart: Millis,
  viewEnd: Millis,
  boundaries: Millis[],
  snapMs: number,
): TimeSpan | null {
  const a = snapToBoundary(yToMinute(Math.min(y1, y2), height, viewStart, viewEnd), boundaries, snapMs);
  const b = snapToBoundary(yToMinute(Math.max(y1, y2), height, viewStart, viewEnd), boundaries, snapMs);
  return b - a >= MIN ? { start: a, end: b } : null;
}

// ===== チェックリスト =====

/** 絞り込み。`all` は絞り込まない。 */
export type CandidateFilter = "all" | "unconfirmed" | "no_task";

/** 絞り込みに合う候補か。記録なしはタスクを持たず、未確定でもあるので両方の絞り込みに合う。 */
export function matchesFilter(c: CorrectionCandidate, f: CandidateFilter): boolean {
  switch (f) {
    case "unconfirmed":
      return c.status !== "confirmed";
    case "no_task":
      return c.task_id === null;
    default:
      return true;
  }
}

/** 無操作・休憩・記録なし（割り当てると PC 外の時間になるもの）。 */
export function isIdleLike(c: CorrectionCandidate): boolean {
  return c.becomes_offline;
}

/** 候補のチェックを 1 件切り替える。 */
export function toggleChecked(cands: CorrectionCandidate[], id: string): CorrectionCandidate[] {
  return cands.map((c) => (c.id === id ? { ...c, checked: !c.checked } : c));
}

/** 「無操作・記録なしも含める」スイッチ: 無操作・記録なしのチェックを一括で切り替える（ほかは変えない）。 */
export function setIdleIncluded(cands: CorrectionCandidate[], on: boolean): CorrectionCandidate[] {
  return cands.map((c) => (isIdleLike(c) ? { ...c, checked: on } : c));
}

/** スイッチの状態: 無操作・記録なしの候補があり、すべてチェックされているとき ON。 */
export function idleIncluded(cands: CorrectionCandidate[]): boolean {
  const idle = cands.filter(isIdleLike);
  return idle.length > 0 && idle.every((c) => c.checked);
}

/** 実際に対象になる候補（チェックされていて、絞り込みにも合うもの）。 */
export function effectiveSelection(cands: CorrectionCandidate[], f: CandidateFilter): CorrectionCandidate[] {
  return cands.filter((c) => c.checked && matchesFilter(c, f));
}

export interface Totals {
  /** 対象にする区間の数。 */
  selected: number;
  total_ms: number;
  /** うち PC 外（無操作・記録なし）の時間。 */
  offline_ms: number;
  /** 外した（対象にならない）候補の数。絞り込みで隠れたものも含める。 */
  excluded: number;
}

/** 対象の合計（core の `Preview` と同じ数え方）。 */
export function sumSelected(cands: CorrectionCandidate[], f: CandidateFilter): Totals {
  const sel = effectiveSelection(cands, f);
  return {
    selected: sel.length,
    total_ms: sel.reduce((n, c) => n + c.duration_ms, 0),
    offline_ms: sel.filter(isIdleLike).reduce((n, c) => n + c.duration_ms, 0),
    excluded: cands.length - sel.length,
  };
}

/** 秒まで見る日本語の時間表示（`1時間12分`、`45分`、`30秒`）。core の `fmt_jp_duration` と同じ。 */
export function fmtJpDuration(ms: number): string {
  const secs = Math.floor((Math.max(0, ms) + 500) / 1000);
  if (secs < 60) return `${secs}秒`;
  const min = Math.floor((secs + 30) / 60);
  const h = Math.floor(min / 60);
  const m = min % 60;
  if (h === 0) return `${m}分`;
  return m === 0 ? `${h}時間` : `${h}時間${m}分`;
}

/** 「タスク X に 1時間12分を割り当て（うち PC 外 15分）。外した区間 3 件」。core の `Preview.summary` と同じ形。 */
export function describeTotals(targetLabel: string, t: Totals): string {
  if (t.selected === 0) return `${targetLabel} に割り当てる区間がありません。`;
  let s = `${targetLabel} に ${fmtJpDuration(t.total_ms)}を割り当て`;
  if (t.offline_ms > 0) s += `（うち PC 外 ${fmtJpDuration(t.offline_ms)}）`;
  s += "。";
  if (t.excluded > 0) s += `外した区間 ${t.excluded} 件`;
  return s;
}

/** 割り当て先の選択値。`none` はタスクなし、`rest` は休憩、それ以外はタスク ID。 */
export type TargetChoice = "none" | "rest" | (string & {});

/** 選択値を API の入力に変換する。 */
export function targetInput(choice: TargetChoice): { task?: string; no_task?: boolean; rest?: boolean } {
  if (choice === "none") return { no_task: true };
  if (choice === "rest") return { rest: true };
  return { task: choice };
}

/** 割り当て先の表示名（`describeTotals` の先頭）。 */
export function targetLabel(choice: TargetChoice, taskTitle?: string): string {
  if (choice === "none") return "「タスクなし」";
  if (choice === "rest") return "「休憩」";
  return `タスク ${taskTitle ?? ""}`.trim();
}
