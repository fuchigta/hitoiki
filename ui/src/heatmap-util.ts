// 活動量ヒートマップの格子の組み立て。画面に依存しない純粋関数。
// 週は月曜始まり（core の `from_weekday` は月曜 = 0）。日付は core が返すローカル日付の文字列をそのまま使う。
import type { HeatmapDay } from "./api";

export const WEEKDAY_LABELS = ["月", "", "水", "", "金", "", ""];

/** 週ごとの 7 マス（月〜日）。範囲の外は null。 */
export function buildWeeks(days: HeatmapDay[], fromWeekday: number): (HeatmapDay | null)[][] {
  const lead = Math.max(0, Math.min(6, fromWeekday));
  const cells: (HeatmapDay | null)[] = [...Array<null>(lead).fill(null), ...days];
  while (cells.length % 7 !== 0) cells.push(null);
  const weeks: (HeatmapDay | null)[][] = [];
  for (let i = 0; i < cells.length; i += 7) weeks.push(cells.slice(i, i + 7));
  return weeks;
}

export interface MonthLabel {
  /** 何列目（0 始まり）に出すか。 */
  week: number;
  label: string;
}

/** 月が替わる週の列に「N月」を出す。前のラベルと近すぎるもの（minGap 列未満）は出さない。 */
export function monthLabels(weeks: (HeatmapDay | null)[][], minGap = 3): MonthLabel[] {
  const out: MonthLabel[] = [];
  let prev = "";
  let lastWeek = -minGap;
  weeks.forEach((w, i) => {
    const first = w.find((d) => d !== null);
    if (!first) return;
    const month = first.date.slice(5, 7);
    if (month === prev) return;
    prev = month;
    if (i - lastWeek < minGap) return;
    out.push({ week: i, label: `${Number(month)}月` });
    lastWeek = i;
  });
  return out;
}

/** 値の表示。`ms` は時間、`count` は件数。 */
export function fmtHeatValue(unit: "ms" | "count", v: number): string {
  if (unit === "count") return `${v}件`;
  const min = Math.round(Math.max(0, v) / 60000);
  if (min < 60) return `${min}分`;
  const h = Math.floor(min / 60);
  const m = min % 60;
  return m === 0 ? `${h}時間` : `${h}時間${m}分`;
}

const WD = ["月", "火", "水", "木", "金", "土", "日"];

/** `YYYY-MM-DD` の曜日（月曜 = 0）。 */
export function weekdayOf(date: string): number {
  const [y, m, d] = date.split("-").map(Number);
  return (new Date(Date.UTC(y, m - 1, d)).getUTCDay() + 6) % 7;
}

/** セルのツールチップ。値が 0 の日は「記録なし」。 */
export function cellTitle(d: HeatmapDay, unit: "ms" | "count"): string {
  const head = `${d.date}（${WD[weekdayOf(d.date)]}）`;
  return d.value > 0 ? `${head} ${fmtHeatValue(unit, d.value)}` : `${head} なし`;
}

export type HeatBy = "time" | "kind" | "task" | "skill" | "key_result" | "records";

/** 画面の選択から core の `by` 文字列を組み立てる。引数が要る集計の軸で未選択なら null。 */
export function buildBy(by: HeatBy, arg: string): string | null {
  switch (by) {
    case "time":
      return "time";
    case "records":
      return arg ? `records:${arg}` : "records";
    default:
      return arg ? `${by}:${arg}` : null;
  }
}
