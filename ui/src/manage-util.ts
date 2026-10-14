// 編集・削除の画面で使う純粋関数。
import type { Impact, Millis, Objective, ObjectiveEdit } from "./api";
import { dateInput, toMillis } from "./lib";

/** 確認ダイアログに出す影響の行（「紐付いたタスク 3 件」）。件数 0 の行は core が出さない。 */
export function impactLines(impact: Impact): string[] {
  return [...impact.lines.map((l) => `${l.label} ${l.count} 件`), ...(impact.warnings ?? [])];
}

/** 影響がなく、確認の文言が「削除しますか？」だけで済むか。 */
export function hasImpact(impact: Impact): boolean {
  return impact.lines.some((l) => l.count > 0) || (impact.warnings ?? []).length > 0;
}

const DAY = 86_400_000;

/** 目標の期間を `<input type="date">` の値に。終了は「その日の終わり（翌日 0 時）」を保存するので 1 ミリ秒戻して日付にする。 */
export function periodInputs(o: Pick<Objective, "period_start" | "period_end">): { from: string; to: string } {
  return {
    from: o.period_start != null ? dateInput(o.period_start) : "",
    to: o.period_end != null ? dateInput(o.period_end - 1) : "",
  };
}

/** 期間の入力欄から編集内容を作る。両方空なら期間を消す（元から無ければ何もしない）。終了日はその日の終わりまで含める。 */
export function periodEdit(
  from: string,
  to: string,
  current: Pick<Objective, "period_start" | "period_end">,
): Pick<ObjectiveEdit, "from" | "to" | "clear_period"> {
  const f: Millis | undefined = from ? toMillis(from, "00:00") : undefined;
  const t: Millis | undefined = to ? toMillis(to, "00:00") + DAY : undefined;
  if (f === undefined && t === undefined) {
    return current.period_start != null || current.period_end != null ? { clear_period: true } : {};
  }
  // 片方だけ空のときは、空の側を消す（clear_period のあとで入力のある側だけ設定される）。
  return { clear_period: true, from: f, to: t };
}

/** 空白だけの入力は未入力として扱う。 */
export function blankToUndef(s: string): string | undefined {
  const t = s.trim();
  return t ? t : undefined;
}

/** 1〜5 の入力欄（文字列）を数値に。空は undefined、範囲外や非数は null。 */
export function parseScaleInput(s: string): number | undefined | null {
  const t = s.trim();
  if (!t) return undefined;
  const n = Number(t);
  return Number.isInteger(n) && n >= 1 && n <= 5 ? n : null;
}
