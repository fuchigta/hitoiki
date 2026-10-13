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
    expect(routeFromHash("#/okr/kr/K1").path).toEqual(["kr", "K1"]);
    expect(routeFromHash("okr/skill/S1").path).toEqual(["skill", "S1"]);
    expect(routeFromHash("#/okr/").path).toEqual([]);
    expect(routeFromHash("#/okr?tab=skills").params.get("tab")).toBe("skills");
  });

  it("知らない画面・空は today", () => {
    expect(routeFromHash("")).toMatchObject({ screen: "today", path: [] });
    expect(routeFromHash("#/nope/x").screen).toBe("today");
  });

  it("通知の open_screen のような入口もそのまま画面を得る", () => {
    expect(routeFromHash("#/today/design")).toMatchObject({ screen: "today", path: ["design"] });
    expect(routeFromHash("#/review").screen).toBe("review");
  });
});

describe("hrefOf", () => {
  it("組み立てと往復", () => {
    expect(hrefOf("tasks")).toBe("#/tasks");
    expect(hrefOf("tasks", ["01ABC"], { tab: "rules" })).toBe("#/tasks/01ABC?tab=rules");
    expect(hrefOf("okr", ["kr", "K1"], { tab: undefined, x: "" })).toBe("#/okr/kr/K1");
    const r = routeFromHash(hrefOf("okr", ["o", "a b/c"], { tab: "skills" }));
    expect(r.path).toEqual(["o", "a b/c"]);
    expect(r.params.get("tab")).toBe("skills");
    expect(hrefOf("tasks", [], new URLSearchParams("tab=time&e="))).toBe("#/tasks?tab=time");
  });
});

describe("legacyRoute", () => {
  it("P2 では恒等（旧名の写像は統合する段階で足す）", () => {
    const r = routeFromHash("#/review?x=1");
    expect(legacyRoute(r)).toEqual(r);
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
