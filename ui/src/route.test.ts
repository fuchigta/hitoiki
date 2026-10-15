import { beforeAll, describe, expect, it, vi } from "vitest";

// lib.ts は window のイベントを使うので、Node 環境用に最小限のスタブを置く。
beforeAll(() => {
  vi.stubGlobal("window", { dispatchEvent: () => true, addEventListener: () => {}, removeEventListener: () => {} });
});

const { hrefOf, legacyRoute, routeFromHash } = await import("./lib");

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

describe("legacyRoute", () => {
  it("他の画面は恒等", () => {
    const t = routeFromHash("#/tasks/01ABC?tab=time");
    expect(legacyRoute(t)).toEqual(t);
  });
});

describe("legacyRoute: 計画（P4）", () => {
  const map = (hash: string) => {
    const r = legacyRoute(routeFromHash(hash));
    return hrefOf(r.screen, r.path, r.params);
  };
  it("plan は today に写す（date は引き継ぐ）", () => {
    expect(map("#/plan")).toBe("#/today");
    expect(map("#/plan?date=2026-10-13")).toBe("#/today?date=2026-10-13");
  });
  it("today とそのサブルートはそのまま", () => {
    expect(map("#/today")).toBe("#/today");
    expect(map("#/today/design")).toBe("#/today/design");
    expect(map("#/today?date=2026-10-13")).toBe("#/today?date=2026-10-13");
  });
});

describe("画面の並び", () => {
  it("ナビは 今日 / 実績 / タスク / 目標 / 記録 / レポート / 設定（はじめにはナビ外）。旧名は含まない", async () => {
    const { SCREENS } = await import("./lib");
    expect(SCREENS.map((s) => s.id)).toEqual(["today", "actual", "tasks", "goals", "records", "report", "settings", "onboarding"]);
  });
  it("旧名（plan・reflect）も受け付けて、画面としては今日・記録に写す", () => {
    const screens = ["#/plan", "#/reflect", "#/reflection/weekly"].map((h) => legacyRoute(routeFromHash(h)).screen);
    expect(screens).toEqual(["today", "records", "records"]);
  });
});

describe("legacyRoute: 記録・振り返り（P5）", () => {
  const map = (hash: string) => {
    const r = legacyRoute(routeFromHash(hash));
    return hrefOf(r.screen, r.path, r.params);
  };
  it("reflect は記録の振り返りの絞り込みに写す", () => {
    expect(map("#/reflect")).toBe("#/records?kind=reflection");
    expect(map("#/reflect/bogus")).toBe("#/records?kind=reflection");
  });
  it("reflect/<kind> と reflection/<kind> は作成フォームに写す", () => {
    expect(map("#/reflect/day_start")).toBe("#/records/new/day_start");
    expect(map("#/reflect/day_end")).toBe("#/records/new/day_end");
    expect(map("#/reflect/weekly")).toBe("#/records/new/weekly");
    expect(map("#/reflection/day_end")).toBe("#/records/new/day_end");
    expect(map("#/reflection/weekly")).toBe("#/records/new/weekly");
  });
  it("reflection だけ（種類なし）は記録の振り返りの絞り込み", () => {
    expect(map("#/reflection")).toBe("#/records?kind=reflection");
  });
  it("checkin と hassle は作成フォームに写す", () => {
    expect(map("#/checkin")).toBe("#/records/new/checkin");
    expect(map("#/hassle")).toBe("#/records/new/hassle");
  });
  it("記録の新しいルートはそのまま", () => {
    expect(map("#/records")).toBe("#/records");
    expect(map("#/records/checkin/C1?period=today")).toBe("#/records/checkin/C1?period=today");
    expect(map("#/records/new/abc")).toBe("#/records/new/abc");
  });
});
