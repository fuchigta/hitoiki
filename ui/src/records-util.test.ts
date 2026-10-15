import { describe, expect, it } from "vitest";
import type { Reframe, Condition, ErgDistribution, ErgLabel, Concern, Reflection } from "./api/types";
import { ergKey } from "./erg-util";
import {
  buildTimeline,
  conditionSummary,
  ergBadge,
  ergCountLine,
  filterTimeline,
  kindFilter,
  unconfirmedCount,
  recordsPeriod,
  recordsSelection,
  reflectionSummary,
} from "./records-util";

const condition = (p: Partial<Condition> = {}): Condition => ({
  id: "C1",
  at: 1000,
  energy: 4,
  mood: 4,
  stress: 2,
  erg_tag: null,
  note: null,
  nudge_id: null,
  ...p,
});
const concern = (p: Partial<Concern> = {}): Concern => ({
  id: "H1",
  at: 2000,
  text: "会議が\n長引いた",
  intensity: 3,
  segment_id: null,
  nudge_id: null,
  ...p,
});
const reframe = (p: Partial<Reframe> = {}): Reframe => ({
  id: "A1",
  at: 3000,
  event: "レビューで指摘された",
  belief: null,
  emotion: null,
  emotion_intensity: null,
  alternative: null,
  deepening_md: null,
  concern_id: null,
  ...p,
});
const reflection = (p: Partial<Reflection> = {}): Reflection => ({
  id: "R1",
  at: 4000,
  kind: "day_end",
  context_json: "{}",
  questions_md: "",
  answers_md: null,
  summary_md: null,
  structured_json: null,
  answers_json: null,
  ...p,
});
const label = (p: Partial<ErgLabel>): ErgLabel => ({
  record_kind: "concern",
  record_id: "H1",
  tag: "growth",
  source: "ai",
  confidence: 0.9,
  reason: null,
  at: 1,
  ...p,
});

describe("recordsSelection", () => {
  it("詳細と作成のパスを読む", () => {
    expect(recordsSelection(["condition", "C1"])).toEqual({ mode: "detail", kind: "condition", id: "C1" });
    expect(recordsSelection(["reflection", "R1"])).toEqual({ mode: "detail", kind: "reflection", id: "R1" });
    expect(recordsSelection(["new", "reframe"])).toEqual({ mode: "new", kind: "reframe" });
    expect(recordsSelection(["new", "day_start"])).toEqual({ mode: "new", kind: "day_start" });
  });
  it("形が合わなければ null", () => {
    expect(recordsSelection([])).toBeNull();
    expect(recordsSelection(["condition"])).toBeNull();
    expect(recordsSelection(["new"])).toBeNull();
    expect(recordsSelection(["new", "bogus"])).toBeNull();
    expect(recordsSelection(["task", "X"])).toBeNull();
  });
});

describe("kindFilter / recordsPeriod", () => {
  it("知らない値は既定に戻す", () => {
    expect(kindFilter("concern")).toBe("concern");
    expect(kindFilter("reflection")).toBe("reflection");
    expect(kindFilter("x")).toBe("all");
    expect(kindFilter(null)).toBe("all");
    expect(recordsPeriod("last-week")).toBe("last-week");
    expect(recordsPeriod("x")).toBe("this-week");
    expect(recordsPeriod(null)).toBe("this-week");
  });
});

describe("要約", () => {
  it("調子は入力のある値だけを並べる", () => {
    expect(conditionSummary(condition())).toBe("エネルギー 4・気分 4・ストレス 2");
    expect(conditionSummary(condition({ mood: null, stress: null }))).toBe("エネルギー 4");
    expect(conditionSummary(condition({ note: " 眠い " }))).toBe("エネルギー 4・気分 4・ストレス 2　眠い");
    expect(conditionSummary(condition({ energy: null, mood: null, stress: null, note: "メモだけ" }))).toBe("メモだけ");
  });
  it("振り返りは種類と回答の有無", () => {
    expect(reflectionSummary(reflection())).toBe("終業の振り返り・未回答");
    expect(reflectionSummary(reflection({ kind: "weekly", answers_json: "{}" }))).toBe("週次の振り返り・回答あり");
    expect(reflectionSummary(reflection({ kind: "day_start", answers_md: "はい" }))).toBe("始業の振り返り・回答あり");
  });
});

describe("buildTimeline", () => {
  const input = {
    conditions: [condition()],
    concerns: [concern()],
    reframes: [reframe()],
    reflections: [reflection(), reflection({ id: "R0", at: 500 })],
  };
  it("4 種類を時刻の降順に 1 本にする", () => {
    const t = buildTimeline(input, new Map());
    expect(t.map((e) => `${e.kind}:${e.id}`)).toEqual(["reflection:R1", "reframe:A1", "concern:H1", "condition:C1", "reflection:R0"]);
    expect(t[2].summary).toBe("会議が 長引いた");
  });
  it("同時刻は ID の降順で安定する", () => {
    const t = buildTimeline(
      { conditions: [condition({ id: "C1", at: 5 }), condition({ id: "C2", at: 5 })], concerns: [], reframes: [], reflections: [] },
      new Map(),
    );
    expect(t.map((e) => e.id)).toEqual(["C2", "C1"]);
  });
  it("振り返りだけ期間で絞る", () => {
    const t = buildTimeline(input, new Map(), { start: 900, end: 4001 });
    expect(t.map((e) => e.id)).toEqual(["R1", "A1", "H1", "C1"]);
    const t2 = buildTimeline(input, new Map(), { start: 0, end: 4000 });
    expect(t2.some((e) => e.id === "R1")).toBe(false);
  });
  it("ERG のラベルを引き当てる", () => {
    const l = label({ record_kind: "concern", record_id: "H1", source: "unconfirmed" });
    const t = buildTimeline(input, new Map([[ergKey("concern", "H1"), l]]));
    expect(t.find((e) => e.id === "H1")?.erg).toBe(l);
    expect(t.find((e) => e.id === "C1")?.erg).toBeUndefined();
  });
});

describe("filterTimeline", () => {
  const labels = new Map([
    [ergKey("concern", "H1"), label({ source: "unconfirmed" })],
    [ergKey("condition", "C1"), label({ record_kind: "condition", record_id: "C1", source: "ai" })],
  ]);
  const all = buildTimeline(
    { conditions: [condition()], concerns: [concern()], reframes: [reframe()], reflections: [reflection()] },
    labels,
  );
  it("種類で絞る", () => {
    expect(filterTimeline(all, "all", false)).toHaveLength(4);
    expect(filterTimeline(all, "reframe", false).map((e) => e.id)).toEqual(["A1"]);
    expect(filterTimeline(all, "reflection", false).map((e) => e.id)).toEqual(["R1"]);
  });
  it("ERG 未確定で絞る", () => {
    expect(filterTimeline(all, "all", true).map((e) => e.id)).toEqual(["H1"]);
    expect(filterTimeline(all, "condition", true)).toEqual([]);
    expect(unconfirmedCount(all)).toBe(1);
  });
});

describe("ERG の表示", () => {
  it("行の印: ユーザー確定は名前、AI は AI を添え、未確定は未確定", () => {
    expect(ergBadge(undefined)).toBeUndefined();
    expect(ergBadge(label({ source: "user", tag: "relatedness" }))).toBe("関係");
    expect(ergBadge(label({ source: "ai", tag: "growth" }))).toBe("成長 AI");
    expect(ergBadge(label({ source: "unconfirmed", tag: "existence" }))).toBe("未確定");
    expect(ergBadge(label({ source: "user", tag: "none" }))).toBe("該当なし");
  });
  const dist = (p: Partial<ErgDistribution> = {}): ErgDistribution => ({
    existence: 3,
    relatedness: 5,
    growth: 2,
    none: 1,
    unconfirmed: 0,
    unlabeled: 0,
    ai_decided: 0,
    total: 11,
    legend: "",
    ...p,
  });
  it("分布の 1 行（件数を並べるだけ）", () => {
    expect(ergCountLine(dist())).toBe("存在 3・関係 5・成長 2・該当なし 1");
    expect(ergCountLine(dist({ unlabeled: 2 }))).toBe("存在 3・関係 5・成長 2・該当なし 1（未判定 2）");
    expect(ergCountLine(dist({ unconfirmed: 1, unlabeled: 2 }))).toBe(
      "存在 3・関係 5・成長 2・該当なし 1（未確定 1、未判定 2）",
    );
    expect(ergCountLine(dist({ total: 0 }))).toBe("この期間に、文章のある記録はありません。");
  });
});
