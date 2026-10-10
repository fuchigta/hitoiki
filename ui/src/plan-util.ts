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
