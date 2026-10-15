// 成果の指標とタスクのつながり（選択欄・集計表示・紐付け候補）の純粋ロジック。画面に依存しない。
// 達成率の計算や評価はしない。選んだ数え方の値を大きく、残りを補助として並べるだけ。
import type { KeyResult, KeyResultMeasure, KeyResultSummary, KeyResultWeekBar, GoalView, TaskView } from "./api";
import { fmtDur } from "./lib";

/** 比較用に正規化する（全角半角・大文字小文字の差を無視）。 */
export function normKeyResult(s: string): string {
  return s.normalize("NFKC").trim().toLowerCase();
}

export interface KeyResultOption {
  id: string;
  title: string;
  goal_title: string;
  status: string;
}

/** 目標配下の成果の指標を選択欄の候補に平らにする（やめた成果の指標は除く。`keepId` は残す）。 */
export function keyResultOptions(goals: GoalView[], keepId?: string): KeyResultOption[] {
  const out: KeyResultOption[] = [];
  for (const o of goals) {
    for (const k of o.key_results) {
      if (k.status === "dropped" && k.id !== keepId) continue;
      out.push({ id: k.id, title: k.title, goal_title: o.title, status: k.status });
    }
  }
  return out;
}

/** 成果の指標のタイトル・目標名の部分一致で絞り込む。成果の指標タイトルの前方一致を先に（それぞれ元の順）。空なら全て。 */
export function filterKeyResults(all: KeyResultOption[], query: string): KeyResultOption[] {
  const q = normKeyResult(query);
  if (!q) return all;
  const prefix: KeyResultOption[] = [];
  const partial: KeyResultOption[] = [];
  for (const k of all) {
    const t = normKeyResult(k.title);
    if (t.startsWith(q)) prefix.push(k);
    else if (t.includes(q) || normKeyResult(k.goal_title).includes(q)) partial.push(k);
  }
  return [...prefix, ...partial];
}

/** 候補の表示名「目標 / 成果の指標」。 */
export function keyResultLabel(k: Pick<KeyResultOption, "title" | "goal_title">): string {
  return `${k.goal_title} / ${k.title}`;
}

export const MEASURE_LABEL: Record<KeyResultMeasure, string> = {
  time: "時間",
  active_days: "取り組んだ日数",
  done_tasks: "完了したタスク数",
};

export const MEASURES: KeyResultMeasure[] = ["time", "active_days", "done_tasks"];

/** 成果の指標の編集で選ぶ数え方。 */
export const MEASURE_OPTION: Record<KeyResultMeasure, string> = {
  time: "時間",
  active_days: "取り組んだ日数",
  done_tasks: "完了したタスク数",
};

/** 最小分数の入力欄を数に。1 以上の整数でなければ現在の値のまま。 */
export function parseActiveDayMin(s: string, current: number): number {
  const n = Number(s.trim());
  return Number.isInteger(n) && n >= 1 ? n : current;
}

export interface MeasureValue {
  measure: KeyResultMeasure;
  label: string;
  text: string;
}

/** 数え方ごとの表示用の値。 */
export function measureValues(
  s: Pick<KeyResultSummary, "time_ms" | "active_days" | "active_day_min" | "done_count" | "task_count">,
): Record<KeyResultMeasure, MeasureValue> {
  return {
    time: { measure: "time", label: "時間", text: fmtDur(s.time_ms) },
    active_days: { measure: "active_days", label: `取り組んだ日（1 日 ${s.active_day_min} 分以上）`, text: `${s.active_days} 日` },
    done_tasks: { measure: "done_tasks", label: "完了（期間内に完了 / 全体）", text: `${s.done_count} / ${s.task_count} 件` },
  };
}

/**
 * 時間・日数の内訳（小さく添える）。成果の指標にスキルが付いていない（かつスキルから数えた時間も無い）ときは null。
 * 合算した値は主表示で、内訳は「スキルから」「紐付けたタスクから」の 2 つ（両方に当てはまる区間はスキルから）。
 */
export function breakdownText(
  s: Pick<KeyResultSummary, "key_result_skills" | "from_skill_ms" | "from_task_ms">,
): string | null {
  if (s.key_result_skills.length === 0 && s.from_skill_ms === 0) return null;
  return `内訳: スキルから ${fmtDur(s.from_skill_ms)} ・ 紐付けたタスクから ${fmtDur(s.from_task_ms)}`;
}

/** 選んだ数え方の値（大きく出す）と、残り 2 つ（補助として小さく出す）。 */
export function splitMeasures(s: KeyResultSummary): { main: MeasureValue; aux: MeasureValue[] } {
  const all = measureValues(s);
  const order: KeyResultMeasure[] = ["time", "active_days", "done_tasks"];
  return { main: all[s.measure], aux: order.filter((m) => m !== s.measure).map((m) => all[m]) };
}

/** 週ごとの棒の高さ（最大を 100 とした割合。全て 0 なら全て 0）。 */
export function barHeights(weekly: KeyResultWeekBar[]): number[] {
  const max = Math.max(0, ...weekly.map((w) => w.duration_ms));
  return weekly.map((w) => (max === 0 ? 0 : Math.round((w.duration_ms / max) * 100)));
}

/** 「10/5」のような週の表示。 */
export function weekLabel(weekStart: string): string {
  return `${Number(weekStart.slice(5, 7))}/${Number(weekStart.slice(8, 10))}`;
}

export interface LinkCandidate {
  task: TaskView;
  /** 別の成果の指標に紐付いているとき、その成果の指標（付け替えになる）。 */
  fromKeyResult: string | null;
}

/**
 * 紐付けの候補。未完了（todo / doing）で、この成果の指標に紐付いていないタスクをタイトルの部分一致で絞る。
 * 他の成果の指標に紐付いていないもの（新規に紐付く）を先に、付け替えになるものを後に並べる。
 */
export function linkCandidates(tasks: TaskView[], keyResult: Pick<KeyResult, "id">, query: string): LinkCandidate[] {
  const q = normKeyResult(query);
  const list = tasks
    .filter((t) => (t.status === "todo" || t.status === "doing") && t.key_result_id !== keyResult.id)
    .filter((t) => !q || normKeyResult(t.title).includes(q))
    .map((t) => ({ task: t, fromKeyResult: t.key_result ? t.key_result.title : null }));
  return [...list.filter((c) => c.fromKeyResult === null), ...list.filter((c) => c.fromKeyResult !== null)];
}

/** 選んだ候補のうち、付け替えになる件数。 */
export function relinkCount(cands: LinkCandidate[], selected: Set<string>): number {
  return cands.filter((c) => selected.has(c.task.id) && c.fromKeyResult !== null).length;
}

/** 集計の期間の説明。 */
export function rangeSourceLabel(src: KeyResultSummary["range_source"]): string {
  switch (src) {
    case "goal":
      return "目標の期間";
    case "given":
      return "指定した期間";
    default:
      return "直近 8 週";
  }
}
