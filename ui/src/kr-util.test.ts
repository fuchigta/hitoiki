import { describe, expect, it } from "vitest";
import type { KrSummary, ObjectiveView, TaskView } from "./api";
import {
  barHeights,
  filterKrs,
  krLabel,
  krOptions,
  linkCandidates,
  measureValues,
  parseDayMin,
  relinkCount,
  splitMeasures,
  weekLabel,
} from "./kr-util";

const kr = (id: string, title: string, status = "active") => ({ id, title, status }) as never;
const objectives = [
  { id: "o1", title: "読書習慣", key_results: [kr("k1", "週 3 回読書"), kr("k2", "技術書を 5 冊", "dropped")] },
  { id: "o2", title: "ＲＵＳＴ習得", key_results: [kr("k3", "写経を進める")] },
] as unknown as ObjectiveView[];

describe("krOptions / filterKrs", () => {
  it("やめた KR は除くが、選択中なら残す", () => {
    expect(krOptions(objectives).map((k) => k.id)).toEqual(["k1", "k3"]);
    expect(krOptions(objectives, "k2").map((k) => k.id)).toEqual(["k1", "k2", "k3"]);
  });
  it("空入力なら全て、Objective 名も対象、全角半角を吸収する", () => {
    const all = krOptions(objectives);
    expect(filterKrs(all, "")).toHaveLength(2);
    expect(filterKrs(all, "rust").map((k) => k.id)).toEqual(["k3"]);
    expect(filterKrs(all, "読書").map((k) => k.id)).toEqual(["k1"]);
    expect(filterKrs(all, "zzz")).toEqual([]);
  });
  it("KR タイトルの前方一致を先に出す", () => {
    const all = krOptions(objectives);
    // 「写」は k3 のタイトル前方一致。「読」は k1 のタイトル中・Objective 名にも含まれる。
    expect(filterKrs(all, "写").map((k) => k.id)).toEqual(["k3"]);
    expect(krLabel(all[0])).toBe("読書習慣 / 週 3 回読書");
  });
});

const summary = (over: Partial<KrSummary> = {}): KrSummary =>
  ({
    measure: "time",
    time_ms: 90 * 60_000,
    active_days: 4,
    day_min: 15,
    done_count: 2,
    task_count: 5,
    ...over,
  }) as KrSummary;

describe("measureValues / splitMeasures", () => {
  it("3 つの軸の値を作る", () => {
    const v = measureValues(summary());
    expect(v.time.text).toBe("1時間30分");
    expect(v.days.text).toBe("4 日");
    expect(v.days.label).toContain("15 分");
    expect(v.done.text).toBe("2 / 5 件");
  });
  it("選んだ軸が main、残りが補助", () => {
    expect(splitMeasures(summary()).main.measure).toBe("time");
    expect(splitMeasures(summary()).aux.map((a) => a.measure)).toEqual(["days", "done"]);
    const d = splitMeasures(summary({ measure: "days" }));
    expect([d.main.measure, ...d.aux.map((a) => a.measure)]).toEqual(["days", "time", "done"]);
    const n = splitMeasures(summary({ measure: "done" }));
    expect([n.main.measure, ...n.aux.map((a) => a.measure)]).toEqual(["done", "time", "days"]);
  });
});

describe("parseDayMin", () => {
  it("1 以上の整数だけ受け付け、それ以外は現在の値のまま", () => {
    expect(parseDayMin("20", 15)).toBe(20);
    expect(parseDayMin(" 1 ", 15)).toBe(1);
    expect(parseDayMin("0", 15)).toBe(15);
    expect(parseDayMin("-5", 15)).toBe(15);
    expect(parseDayMin("1.5", 15)).toBe(15);
    expect(parseDayMin("", 15)).toBe(15);
    expect(parseDayMin("abc", 15)).toBe(15);
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

const task = (id: string, title: string, status: string, krId: string | null, krTitle?: string) =>
  ({
    id,
    title,
    status,
    key_result_id: krId,
    key_result: krId ? { id: krId, title: krTitle ?? krId } : null,
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
  it("未完了で、この KR に紐付いていないものだけ。付け替えになるものは後ろ", () => {
    const c = linkCandidates(tasks, { id: "k1" }, "");
    expect(c.map((x) => x.task.id)).toEqual(["t1", "t6", "t2"]);
    expect(c.find((x) => x.task.id === "t2")?.fromKr).toBe("写経を進める");
    expect(c[0].fromKr).toBeNull();
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
