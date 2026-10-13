// モック用の時刻ユーティリティ。基準時刻とタイムゾーン（Asia/Tokyo, UTC+9 固定）を扱う。
// 開発専用。本番ビルドには含まれない（main.tsx の動的 import 経由でのみ読み込まれる）。

import type { Millis, RangeArgs, RangeView } from "../api/types";

export const MIN = 60_000;
export const HOUR = 60 * MIN;
export const DAY = 24 * HOUR;
/** Asia/Tokyo のオフセット（夏時間なし）。 */
export const OFFSET = 9 * HOUR;

/** 固定の基準時刻: 2026-10-13（火）10:30 JST。 */
export const BASE_NOW: Millis = Date.UTC(2026, 9, 13, 1, 30);

const pad = (n: number) => String(n).padStart(2, "0");

/** その時刻を含む日の 0:00（JST）。 */
export function dayStart(ms: Millis): Millis {
  return Math.floor((ms + OFFSET) / DAY) * DAY - OFFSET;
}

/** 月曜 = 0 … 日曜 = 6。 */
export function weekdayMon0(ms: Millis): number {
  return (new Date(ms + OFFSET).getUTCDay() + 6) % 7;
}

/** その時刻を含む週の月曜 0:00（JST）。 */
export function weekStart(ms: Millis): Millis {
  return dayStart(ms) - weekdayMon0(ms) * DAY;
}

/** `YYYY-MM-DD`（JST）。 */
export function ymd(ms: Millis): string {
  const d = new Date(ms + OFFSET);
  return `${d.getUTCFullYear()}-${pad(d.getUTCMonth() + 1)}-${pad(d.getUTCDate())}`;
}

/** `YYYY-MM-DDTHH:mm:ss+09:00` 形式（core の表示用文字列に相当）。 */
export function isoJst(ms: Millis): string {
  const d = new Date(ms + OFFSET);
  return `${ymd(ms)}T${pad(d.getUTCHours())}:${pad(d.getUTCMinutes())}:${pad(d.getUTCSeconds())}+09:00`;
}

/** `YYYY-MM-DD` の JST 0:00。 */
export function parseYmd(s: string): Millis {
  const [y, m, d] = s.split("-").map(Number);
  return Date.UTC(y, m - 1, d) - OFFSET;
}

/** 基準日から `days` 日ずらした日の `HH:MM`（JST）の時刻。 */
export function at(days: number, hhmm: string, base: Millis = BASE_NOW): Millis {
  const [h, m] = hhmm.split(":").map(Number);
  return dayStart(base) + days * DAY + h * HOUR + m * MIN;
}

export function rangeView(start_at: Millis, end_at: Millis): RangeView {
  return { start_at, end_at, start: isoJst(start_at), end: isoJst(end_at) };
}

const DATE_ONLY = /^\d{4}-\d{2}-\d{2}$/;

/** 範囲指定（today / yesterday / this-week / last-week / YYYY-MM-DD / from-to）を [開始, 終了) にする。 */
export function resolveRange(args: RangeArgs | undefined, now: Millis): [Millis, Millis] {
  const today = dayStart(now);
  const a = args ?? {};
  if (a.from || a.to) {
    const from = a.from ? (DATE_ONLY.test(a.from) ? parseYmd(a.from) : Date.parse(a.from)) : today;
    let to: Millis;
    if (!a.to) to = from + DAY;
    else if (DATE_ONLY.test(a.to)) to = parseYmd(a.to) + DAY;
    else to = Date.parse(a.to);
    return [from, to];
  }
  switch (a.date ?? "today") {
    case "today":
      return [today, today + DAY];
    case "yesterday":
      return [today - DAY, today];
    case "this-week": {
      const w = weekStart(now);
      return [w, w + 7 * DAY];
    }
    case "last-week": {
      const w = weekStart(now) - 7 * DAY;
      return [w, w + 7 * DAY];
    }
    default: {
      if (DATE_ONLY.test(a.date!)) {
        const d = parseYmd(a.date!);
        return [d, d + DAY];
      }
      return [today, today + DAY];
    }
  }
}
