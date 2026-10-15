import { describe, expect, it } from "vitest";
import type { ActivityRule, TaskAssignRule } from "./api";
import { filterRules, mergeRules, settingsTab } from "./settings-util";

const tr = (id: string, task_id: string, pattern: string, priority = 0, enabled = true): TaskAssignRule => ({
  id,
  task_id,
  field: "exe",
  pattern,
  pattern_type: "glob",
  priority,
  enabled,
});
const ar = (id: string, pattern: string, label: string, priority = 0): ActivityRule => ({
  id,
  field: "exe",
  pattern,
  pattern_type: "glob",
  activity: label,
  priority,
  enabled: true,
});
const tasks = [
  { id: "t1", title: "企画書" },
  { id: "t2", title: "レビュー" },
];

describe("settingsTab", () => {
  it("知っているタブはそのまま", () => {
    expect(settingsTab("rules")).toBe("rules");
    expect(settingsTab("data")).toBe("data");
  });
  it("未指定・未知は general", () => {
    expect(settingsTab(undefined)).toBe("general");
    expect(settingsTab("")).toBe("general");
    expect(settingsTab("nope")).toBe("general");
  });
});

describe("mergeRules", () => {
  const items = mergeRules(
    [tr("a", "t1", "code*", 1), tr("b", "t2", "word*", 5), tr("c", "gone", "x*")],
    [ar("d", "slack*", "会議")],
    ["explorer.exe"],
    tasks,
  );
  it("種類 → 優先度の大きい順に並ぶ", () => {
    expect(items.map((i) => i.key)).toEqual(["task:b", "task:a", "task:c", "activity:d", "ignore:explorer.exe"]);
  });
  it("行き先にタスク名・ラベルが入り、消えたタスクは補う", () => {
    expect(items.find((i) => i.id === "a")?.target).toBe("企画書");
    expect(items.find((i) => i.id === "c")?.target).toBe("（不明なタスク）");
    expect(items.find((i) => i.id === "d")?.target).toBe("会議");
    expect(items.find((i) => i.kind === "ignore")?.target).toBe("");
  });
});

describe("filterRules", () => {
  const items = mergeRules([tr("a", "t1", "code*"), tr("b", "t2", "word*")], [ar("d", "slack*", "会議")], ["x.exe"], tasks);
  it("種類で絞る", () => {
    expect(filterRules(items, "all")).toHaveLength(4);
    expect(filterRules(items, "activity").map((i) => i.id)).toEqual(["d"]);
    expect(filterRules(items, "ignore").map((i) => i.id)).toEqual(["x.exe"]);
  });
  it("タスクで絞るとタスクの割り当てルールだけになる", () => {
    expect(filterRules(items, "all", "t2").map((i) => i.id)).toEqual(["b"]);
    expect(filterRules(items, "activity", "t1")).toEqual([]);
  });
});
