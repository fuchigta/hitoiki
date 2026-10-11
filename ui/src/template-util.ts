// ブロックのテンプレートまわりの純粋なロジック。

import type { Overlap, PreviewBlock, TemplateInput, TemplateItemInput, TemplateView } from "./api";

/** 曜日の表示名（1=月 … 7=日）。 */
export const WEEKDAY_LABELS = ["月", "火", "水", "木", "金", "土", "日"] as const;

/** `1,3` → 「月・水」。割り当てがなければ空文字。 */
export function weekdaysLabel(days: number[]): string {
  return [...days]
    .sort((a, b) => a - b)
    .map((d) => WEEKDAY_LABELS[d - 1])
    .filter(Boolean)
    .join("・");
}

/** 曜日の選択を切り替える（昇順を保つ）。 */
export function toggleWeekday(days: number[], d: number): number[] {
  return days.includes(d) ? days.filter((x) => x !== d) : [...days, d].sort((a, b) => a - b);
}

/** 日付（YYYY-MM-DD）の曜日（1=月 … 7=日）。タイムゾーンに依存しない暦の計算。 */
export function weekdayOf(date: string): number {
  const [y, m, d] = date.split("-").map(Number);
  const dow = new Date(Date.UTC(y, m - 1, d)).getUTCDay(); // 0=日
  return dow === 0 ? 7 : dow;
}

/** `HH:MM`（24:00 は終了のみ）を 0 時からの分にする。解釈できなければ null。 */
export function parseHm(s: string, allow24 = false): number | null {
  const m = /^(\d{1,2}):(\d{2})$/.exec(s.trim());
  if (!m) return null;
  const h = Number(m[1]);
  const mi = Number(m[2]);
  if (mi >= 60) return null;
  const total = h * 60 + mi;
  if (total > 24 * 60 || (total === 24 * 60 && !allow24)) return null;
  return total;
}

export function emptyItem(): TemplateItemInput {
  return { start: "09:00", end: "10:00", label: "", focus: false, task: null };
}

/** 保存済みのテンプレートを編集フォームの入力にする。 */
export function toInput(t: TemplateView): TemplateInput {
  return {
    name: t.name,
    description: t.description ?? "",
    weekdays: [...t.weekdays],
    items: t.blocks.map((b) => ({
      start: b.start,
      end: b.end,
      label: b.label ?? "",
      focus: b.focus,
      task: b.task_id,
    })),
  };
}

/** 保存前の入力チェック（core と同じ規則）。問題があればメッセージ、なければ null。 */
export function validateTemplate(t: TemplateInput): string | null {
  if (!t.name.trim()) return "名前を入れてください。";
  for (const [i, it] of t.items.entries()) {
    const s = parseHm(it.start);
    const e = parseHm(it.end, true);
    if (s === null || e === null) return `${i + 1} 番目のブロックの時刻を HH:MM で入れてください。`;
    if (e <= s) return `${i + 1} 番目のブロックは、終了を開始より後にしてください。`;
    if (!it.task && !(it.label ?? "").trim()) return `${i + 1} 番目のブロックにはタスクかラベルが必要です。`;
  }
  return null;
}

/** 開始時刻の順に並べる（元の配列は変えない）。 */
export function sortItems(items: TemplateItemInput[]): TemplateItemInput[] {
  return [...items].sort((a, b) => (parseHm(a.start) ?? 0) - (parseHm(b.start) ?? 0) || (parseHm(a.end, true) ?? 0) - (parseHm(b.end, true) ?? 0));
}

export interface PreviewSummary {
  /** 作られるブロック数。 */
  create: number;
  /** 重なりのために作られないブロック数（skip のとき）。 */
  skip: number;
  /** 何かと重なる項目の数。 */
  conflicting: number;
}

/** プレビューの件数。`skip` では重なる項目は作らず、`overlay` では全部作る。 */
export function summarizePreview(blocks: PreviewBlock[], overlap: Overlap): PreviewSummary {
  const conflicting = blocks.filter((b) => b.conflicts.length > 0).length;
  return overlap === "skip"
    ? { create: blocks.length - conflicting, skip: conflicting, conflicting }
    : { create: blocks.length, skip: 0, conflicting };
}

/** 曜日の既定（その日の曜日に割り当てたテンプレート）の案内文。 */
export function suggestionText(names: string[], weekday: number): string {
  const wd = WEEKDAY_LABELS[weekday - 1] ?? "";
  if (names.length === 0) return "";
  return `${wd}曜日のパターン「${names.join("」「")}」を適用しますか？`;
}
