// KR とタスクのつながり（選択欄・集計表示・紐付け候補）の純粋ロジック。画面に依存しない。
// 達成率の計算や評価はしない。選んだ軸の値を大きく、残りを補助として並べるだけ。
import type { KeyResult, KrMeasure, KrSummary, KrWeekBar, ObjectiveView, TaskView } from "./api";
import { fmtDur } from "./lib";

/** 比較用に正規化する（全角半角・大文字小文字の差を無視）。 */
export function normKr(s: string): string {
  return s.normalize("NFKC").trim().toLowerCase();
}

export interface KrOption {
  id: string;
  title: string;
  objective_title: string;
  status: string;
}

/** Objective 配下の KR を選択欄の候補に平らにする（やめた KR は除く。`keepId` は残す）。 */
export function krOptions(objectives: ObjectiveView[], keepId?: string): KrOption[] {
  const out: KrOption[] = [];
  for (const o of objectives) {
    for (const k of o.key_results) {
      if (k.status === "dropped" && k.id !== keepId) continue;
      out.push({ id: k.id, title: k.title, objective_title: o.title, status: k.status });
    }
  }
  return out;
}

/** KR のタイトル・Objective 名の部分一致で絞り込む。KR タイトルの前方一致を先に（それぞれ元の順）。空なら全て。 */
export function filterKrs(all: KrOption[], query: string): KrOption[] {
  const q = normKr(query);
  if (!q) return all;
  const prefix: KrOption[] = [];
  const partial: KrOption[] = [];
  for (const k of all) {
    const t = normKr(k.title);
    if (t.startsWith(q)) prefix.push(k);
    else if (t.includes(q) || normKr(k.objective_title).includes(q)) partial.push(k);
  }
  return [...prefix, ...partial];
}

/** 候補の表示名「Objective / KR」。 */
export function krLabel(k: Pick<KrOption, "title" | "objective_title">): string {
  return `${k.objective_title} / ${k.title}`;
}

export const MEASURE_LABEL: Record<KrMeasure, string> = {
  time: "時間",
  days: "回数",
  done: "完了数",
};

export const MEASURES: KrMeasure[] = ["time", "days", "done"];

/** KR の編集で選ぶ軸の説明。 */
export const MEASURE_OPTION: Record<KrMeasure, string> = {
  time: "時間（量が要るもの）",
  days: "回数（「週 3 回」のような習慣）",
  done: "完了数（成果物の数）",
};

/** 最小分数の入力欄を数に。1 以上の整数でなければ現在の値のまま。 */
export function parseDayMin(s: string, current: number): number {
  const n = Number(s.trim());
  return Number.isInteger(n) && n >= 1 ? n : current;
}

export interface MeasureValue {
  measure: KrMeasure;
  label: string;
  text: string;
}

/** 軸ごとの表示用の値。 */
export function measureValues(
  s: Pick<KrSummary, "time_ms" | "active_days" | "day_min" | "done_count" | "task_count">,
): Record<KrMeasure, MeasureValue> {
  return {
    time: { measure: "time", label: "時間", text: fmtDur(s.time_ms) },
    days: { measure: "days", label: `取り組んだ日（1 日 ${s.day_min} 分以上）`, text: `${s.active_days} 日` },
    done: { measure: "done", label: "完了（期間内に完了 / 全体）", text: `${s.done_count} / ${s.task_count} 件` },
  };
}

/** 選んだ軸の値（大きく出す）と、残り 2 つ（補助として小さく出す）。 */
export function splitMeasures(s: KrSummary): { main: MeasureValue; aux: MeasureValue[] } {
  const all = measureValues(s);
  const order: KrMeasure[] = ["time", "days", "done"];
  return { main: all[s.measure], aux: order.filter((m) => m !== s.measure).map((m) => all[m]) };
}

/** 週ごとの棒の高さ（最大を 100 とした割合。全て 0 なら全て 0）。 */
export function barHeights(weekly: KrWeekBar[]): number[] {
  const max = Math.max(0, ...weekly.map((w) => w.duration_ms));
  return weekly.map((w) => (max === 0 ? 0 : Math.round((w.duration_ms / max) * 100)));
}

/** 「10/5」のような週の表示。 */
export function weekLabel(weekStart: string): string {
  return `${Number(weekStart.slice(5, 7))}/${Number(weekStart.slice(8, 10))}`;
}

export interface LinkCandidate {
  task: TaskView;
  /** 別の KR に紐付いているとき、その KR（付け替えになる）。 */
  fromKr: string | null;
}

/**
 * 紐付けの候補。未完了（todo / doing）で、この KR に紐付いていないタスクをタイトルの部分一致で絞る。
 * 他の KR に紐付いていないもの（新規に紐付く）を先に、付け替えになるものを後に並べる。
 */
export function linkCandidates(tasks: TaskView[], kr: Pick<KeyResult, "id">, query: string): LinkCandidate[] {
  const q = normKr(query);
  const list = tasks
    .filter((t) => (t.status === "todo" || t.status === "doing") && t.key_result_id !== kr.id)
    .filter((t) => !q || normKr(t.title).includes(q))
    .map((t) => ({ task: t, fromKr: t.key_result ? t.key_result.title : null }));
  return [...list.filter((c) => c.fromKr === null), ...list.filter((c) => c.fromKr !== null)];
}

/** 選んだ候補のうち、付け替えになる件数。 */
export function relinkCount(cands: LinkCandidate[], selected: Set<string>): number {
  return cands.filter((c) => selected.has(c.task.id) && c.fromKr !== null).length;
}

/** 集計の期間の説明。 */
export function rangeSourceLabel(src: KrSummary["range_source"]): string {
  switch (src) {
    case "objective":
      return "Objective の期間";
    case "given":
      return "指定した期間";
    default:
      return "直近 8 週";
  }
}
