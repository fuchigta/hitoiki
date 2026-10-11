// 無視するアプリ・区間の純粋関数。画面（Review.tsx、correction.tsx、Settings.tsx）から使う。
import type { CorrectionCandidate, IgnoreSuggestion } from "./api";

/** 入力から exe 名だけを取り出す（パスが付いていればファイル名、前後の空白は除く）。空なら空文字列。 */
export function normalizeExe(input: string): string {
  const parts = input.trim().split(/[\\/]/);
  return (parts[parts.length - 1] ?? "").trim();
}

/** 大文字小文字を区別せずに一覧にあるか。 */
export function hasExe(list: string[], exe: string): boolean {
  const e = exe.toLowerCase();
  return list.some((x) => x.toLowerCase() === e);
}

/** 付け替えの候補のうち、チェックされた（記録なしを除く）区間の ID。 */
export function ignorableIds(cands: CorrectionCandidate[]): string[] {
  return cands.filter((c) => c.checked && !c.gap).map((c) => c.id);
}

/** 合計時間の表記（1 分未満は秒、それ以上は小数 1 桁の分）。 */
export function fmtTotal(ms: number): string {
  if (ms < 60_000) return `${Math.round(ms / 1000)}秒`;
  return `${(Math.round(ms / 6000) / 10).toFixed(1)}分`;
}

/** 「explorer.exe は短い区間が 214 件（合計 16.5分）。」。短い区間が無ければ区間数で言う。 */
export function suggestionLine(sg: IgnoreSuggestion): string {
  const main = sg.short_count > 0 ? `短い区間が ${sg.short_count} 件` : `区間が ${sg.span_count} 件`;
  return `${sg.exe} は${main}（合計 ${fmtTotal(sg.total_ms)}）。`;
}
