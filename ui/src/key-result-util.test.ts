import { describe, expect, it } from "vitest";
import type { KeyResultSummary, GoalView, TaskView } from "./api";
import {
  barHeights,
  breakdownText,
  filterKeyResults,
  keyResultLabel,
  keyResultOptions,
  linkCandidates,
  measureValues,
  parseActiveDayMin,
  relinkCount,
  splitMeasures,
  weekLabel,
} from "./key-result-util";

const keyResult = (id: string, title: string, status = "active") => ({ id, title, status }) as never;
const goals = [
  { id: "o1", title: "読書習慣", key_results: [keyResult("k1", "週 3 回読書"), keyResult("k2", "技術書を 5 冊", "dropped")] },
  { id: "o2", title: "ＲＵＳＴ習得", key_results: [keyResult("k3", "写経を進める")] },
] as unknown as GoalView[];

describe("keyResultOptions / filterKeyResults", () => {
  it("やめた成果の指標は除くが、選択中なら残す", () => {
    expect(keyResultOptions(goals).map((k) => k.id)).toEqual(["k1", "k3"]);
    expect(keyResultOptions(goals, "k2").map((k) => k.id)).toEqual(["k1", "k2", "k3"]);
  });
  it("空入力なら全て、目標名も対象、全角半角を吸収する", () => {
    const all = keyResultOptions(goals);
    expect(filterKeyResults(all, "")).toHaveLength(2);
    expect(filterKeyResults(all, "rust").map((k) => k.id)).toEqual(["k3"]);
    expect(filterKeyResults(all, "読書").map((k) => k.id)).toEqual(["k1"]);
    expect(filterKeyResults(all, "zzz")).toEqual([]);
  });
  it("成果の指標タイトルの前方一致を先に出す", () => {
    const all = keyResultOptions(goals);
    // 「写」は k3 のタイトル前方一致。「読」は k1 のタイトル中・目標名にも含まれる。
    expect(filterKeyResults(all, "写").map((k) => k.id)).toEqual(["k3"]);
    expect(keyResultLabel(all[0])).toBe("読書習慣 / 週 3 回読書");
  });
});

const summary = (over: Partial<KeyResultSummary> = {}): KeyResultSummary =>
  ({
    measure: "time",
    time_ms: 90 * 60_000,
    active_days: 4,
    active_day_min: 15,
    done_count: 2,
    task_count: 5,
    ...over,
  }) as KeyResultSummary;

describe("measureValues / splitMeasures", () => {
  it("3 つの軸の値を作る", () => {
    const v = measureValues(summary());
    expect(v.time.text).toBe("1時間30分");
    expect(v.active_days.text).toBe("4 日");
    expect(v.active_days.label).toContain("15 分");
    expect(v.done_tasks.text).toBe("2 / 5 件");
  });
  it("選んだ軸が main、残りが補助", () => {
    expect(splitMeasures(summary()).main.measure).toBe("time");
    expect(splitMeasures(summary()).aux.map((a) => a.measure)).toEqual(["active_days", "done_tasks"]);
    const d = splitMeasures(summary({ measure: "active_days" }));
    expect([d.main.measure, ...d.aux.map((a) => a.measure)]).toEqual(["active_days", "time", "done_tasks"]);
    const n = splitMeasures(summary({ measure: "done_tasks" }));
    expect([n.main.measure, ...n.aux.map((a) => a.measure)]).toEqual(["done_tasks", "time", "active_days"]);
  });
});

describe("breakdownText", () => {
  it("スキルが無く、スキルからの時間も無ければ出さない", () => {
    expect(breakdownText({ key_result_skills: [], from_skill_ms: 0, from_task_ms: 30 * 60_000 })).toBeNull();
  });
  it("スキルがあれば、スキルから・タスクからの内訳を出す", () => {
    expect(breakdownText({ key_result_skills: ["Rust"], from_skill_ms: 40 * 60_000, from_task_ms: 60 * 60_000 })).toBe(
      "内訳: スキルから 40分 ・ 紐付けたタスクから 1時間",
    );
  });
});

describe("parseActiveDayMin", () => {
  it("1 以上の整数だけ受け付け、それ以外は現在の値のまま", () => {
    expect(parseActiveDayMin("20", 15)).toBe(20);
    expect(parseActiveDayMin(" 1 ", 15)).toBe(1);
    expect(parseActiveDayMin("0", 15)).toBe(15);
    expect(parseActiveDayMin("-5", 15)).toBe(15);
    expect(parseActiveDayMin("1.5", 15)).toBe(15);
    expect(parseActiveDayMin("", 15)).toBe(15);
    expect(parseActiveDayMin("abc", 15)).toBe(15);
  });
});

describe("barHeights / weekLabel", () => {
  it("最大を 100 とする。全て 0 なら 0", () => {
    const w = (ms: number[]) => ms.map((duration_ms, i) => ({ week_start: `2026-10-0${i + 1}`, duration_ms }));
    expect(barHeights(w([0, 30, 60]))).toEqual([0, 50, 100]);
    expect(barHeights(w([0, 0]))).toEqual([0, 0]);
    expect(barHeights([])).toEqual([]);
  });
  it("月/日の表示", () => {
    expect(weekLabel("2026-10-05")).toBe("10/5");
    expect(weekLabel("2026-01-12")).toBe("1/12");
  });
});

const task = (id: string, title: string, status: string, keyResultId: string | null, keyResultTitle?: string) =>
  ({
    id,
    title,
    status,
    key_result_id: keyResultId,
    key_result: keyResultId ? { id: keyResultId, title: keyResultTitle ?? keyResultId } : null,
  }) as unknown as TaskView;

describe("linkCandidates / relinkCount", () => {
  const tasks = [
    task("t1", "本を読む", "todo", null),
    task("t2", "記事を読む", "doing", "k3", "写経を進める"),
    task("t3", "読了済み", "done", null),
    task("t4", "もう紐付いている", "todo", "k1", "週 3 回読書"),
    task("t5", "やめた", "dropped", null),
    task("t6", "要約を書く", "todo", null),
  ];
  it("未完了で、この成果の指標に紐付いていないものだけ。付け替えになるものは後ろ", () => {
    const c = linkCandidates(tasks, { id: "k1" }, "");
    expect(c.map((x) => x.task.id)).toEqual(["t1", "t6", "t2"]);
    expect(c.find((x) => x.task.id === "t2")?.fromKeyResult).toBe("写経を進める");
    expect(c[0].fromKeyResult).toBeNull();
  });
  it("タイトルで絞り込む", () => {
    expect(linkCandidates(tasks, { id: "k1" }, "読む").map((x) => x.task.id)).toEqual(["t1", "t2"]);
  });
  it("付け替えになる選択の件数", () => {
    const c = linkCandidates(tasks, { id: "k1" }, "");
    expect(relinkCount(c, new Set(["t1", "t2"]))).toBe(1);
    expect(relinkCount(c, new Set())).toBe(0);
  });
});
