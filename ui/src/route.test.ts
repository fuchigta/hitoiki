import { beforeAll, describe, expect, it, vi } from "vitest";

// lib.ts は window のイベントを使うので、Node 環境用に最小限のスタブを置く。
beforeAll(() => {
  vi.stubGlobal("window", { dispatchEvent: () => true, addEventListener: () => {}, removeEventListener: () => {} });
});

const { hrefOf, routeFromHash } = await import("./lib");

describe("routeFromHash", () => {
  it("画面・パス・クエリに分ける", () => {
    const r = routeFromHash("#/tasks/01ABC?tab=time");
    expect(r.screen).toBe("tasks");
    expect(r.path).toEqual(["01ABC"]);
    expect(r.params.get("tab")).toBe("time");
  });

  it("深いパス・先頭の # や / の有無・末尾の /", () => {
    expect(routeFromHash("#/goals/key-result/K1").path).toEqual(["key-result", "K1"]);
    expect(routeFromHash("goals/skill/S1").path).toEqual(["skill", "S1"]);
    expect(routeFromHash("#/goals/").path).toEqual([]);
    expect(routeFromHash("#/goals?tab=skills").params.get("tab")).toBe("skills");
  });

  it("知らない画面・空は today", () => {
    expect(routeFromHash("")).toMatchObject({ screen: "today", path: [] });
    expect(routeFromHash("#/nope/x").screen).toBe("today");
  });

  it("通知の open_screen のような入口もそのまま画面を得る", () => {
    expect(routeFromHash("#/today/design")).toMatchObject({ screen: "today", path: ["design"] });
    expect(routeFromHash("#/actual/01ABC").screen).toBe("actual");
  });
});

describe("hrefOf", () => {
  it("組み立てと往復", () => {
    expect(hrefOf("tasks")).toBe("#/tasks");
    expect(hrefOf("tasks", ["01ABC"], { tab: "rules" })).toBe("#/tasks/01ABC?tab=rules");
    expect(hrefOf("goals", ["key-result", "K1"], { tab: undefined, x: "" })).toBe("#/goals/key-result/K1");
    const r = routeFromHash(hrefOf("goals", ["goal", "a b/c"], { tab: "skills" }));
    expect(r.path).toEqual(["goal", "a b/c"]);
    expect(r.params.get("tab")).toBe("skills");
    expect(hrefOf("tasks", [], new URLSearchParams("tab=time&e="))).toBe("#/tasks?tab=time");
  });
});

describe("旧ルートは受け付けない", () => {
  it("旧い画面名は today になる", () => {
    for (const old of ["plan", "reflect", "reflection/weekly", "log", "review", "okr", "onboarding", "checkin", "hassle"]) {
      expect(routeFromHash(`#/${old}`)).toMatchObject({ screen: "today", path: [] });
    }
  });
});

describe("画面の並び", () => {
  it("ナビは 今日 / 実績 / タスク / 目標 / 記録 / レポート / 設定（はじめにはナビ外）。旧名は含まない", async () => {
    const { SCREENS } = await import("./lib");
    expect(SCREENS.map((s) => s.id)).toEqual(["today", "actual", "tasks", "goals", "records", "report", "settings", "welcome"]);
  });
});
