import { describe, expect, it } from "vitest";
import type { SkillView, TaskView } from "./api";
import { krMetaText, okrSelection, okrTab, skillTree, tasksWithSkill } from "./okr-util";

const sk = (id: string, name: string, parent_id: string | null = null): SkillView => ({
  id,
  name,
  parent_id,
  parent_name: null,
  description: null,
  preset: false,
});

describe("okrSelection / okrTab", () => {
  it("パスから種類と ID を得る", () => {
    expect(okrSelection(["kr", "K1"])).toEqual({ kind: "kr", id: "K1" });
    expect(okrSelection(["o", "O1"])).toEqual({ kind: "o", id: "O1" });
    expect(okrSelection(["skill", "S1"])).toEqual({ kind: "skill", id: "S1" });
    expect(okrSelection([])).toBeNull();
    expect(okrSelection(["kr"])).toBeNull();
    expect(okrSelection(["x", "1"])).toBeNull();
  });
  it("タブは ?tab=skills か、スキルを選んでいるときにスキル", () => {
    expect(okrTab(null, null)).toBe("okr");
    expect(okrTab("skills", null)).toBe("skills");
    expect(okrTab(null, { kind: "skill", id: "S1" })).toBe("skills");
    expect(okrTab("okr", { kind: "skill", id: "S1" })).toBe("okr");
    expect(okrTab("nope", { kind: "kr", id: "K1" })).toBe("okr");
  });
});

describe("krMetaText", () => {
  it("値と目標値を並べるだけ", () => {
    expect(krMetaText("19時間23分", "週5時間")).toBe("19時間23分 / 目標値 週5時間");
    expect(krMetaText("3 日", null)).toBe("3 日");
    expect(krMetaText("3 日", "  ")).toBe("3 日");
  });
});

describe("skillTree", () => {
  it("親の下に子を並べ、深さを付ける", () => {
    const rows = skillTree([sk("a", "A"), sk("c", "C", "b"), sk("b", "B", "a"), sk("d", "D")]);
    expect(rows.map((r) => [r.skill.id, r.depth])).toEqual([
      ["a", 0],
      ["b", 1],
      ["c", 2],
      ["d", 0],
    ]);
  });
  it("親が無い・循環していても止まって全件出す（循環の分は根の扱いにならず落ちない範囲で）", () => {
    const orphan = skillTree([sk("x", "X", "missing")]);
    expect(orphan.map((r) => r.depth)).toEqual([0]);
    const self = skillTree([sk("y", "Y", "y")]);
    expect(self).toHaveLength(1);
  });
});

describe("tasksWithSkill", () => {
  it("スキル名が付いたタスクだけ", () => {
    const t = (id: string, skills: string[]) => ({ id, skills }) as unknown as TaskView;
    expect(tasksWithSkill([t("1", ["Rust"]), t("2", ["設計", "Rust"]), t("3", [])], "Rust").map((x) => x.id)).toEqual([
      "1",
      "2",
    ]);
  });
});
