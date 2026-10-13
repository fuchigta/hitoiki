import { describe, expect, it } from "vitest";
import { HELP_CLOSED, helpEscape, helpOpen, helpToggle } from "./help-util";
import { firstEnabled, lastEnabled, menuKey, stepMenu } from "./menu-util";
import { PERIOD_LABELS, periodOptions } from "./period-util";
import { stepTab, tabFromParam } from "./tabs-util";
import { scaleFromString, scaleToString, stepScale, toggleScale } from "./scale-util";

const items = [{ disabled: true }, {}, { disabled: true }, {}];

describe("メニューのキー操作", () => {
  it("↓↑ は無効な項目を飛ばして回る", () => {
    expect(stepMenu(items, -1, 1)).toBe(1);
    expect(stepMenu(items, 1, 1)).toBe(3);
    expect(stepMenu(items, 3, 1)).toBe(1);
    expect(stepMenu(items, 1, -1)).toBe(3);
  });
  it("選べる項目がなければ -1", () => {
    expect(stepMenu([{ disabled: true }], 0, 1)).toBe(-1);
    expect(stepMenu([], 0, 1)).toBe(-1);
    expect(firstEnabled([{ disabled: true }])).toBe(-1);
  });
  it("Home / End", () => {
    expect(firstEnabled(items)).toBe(1);
    expect(lastEnabled(items)).toBe(3);
  });
  it("Enter は選択、Esc は閉じる、無効な項目は選べない", () => {
    expect(menuKey(items, 1, "Enter")).toEqual({ type: "select", at: 1 });
    expect(menuKey(items, 0, "Enter")).toBeNull();
    expect(menuKey(items, -1, "Enter")).toBeNull();
    expect(menuKey(items, 1, "Escape")).toEqual({ type: "close" });
    expect(menuKey(items, 1, "ArrowDown")).toEqual({ type: "move", to: 3 });
    expect(menuKey(items, 1, "a")).toBeNull();
  });
});

describe("Help の開閉", () => {
  it("hover / focus / 固定のどれかで開く", () => {
    expect(helpOpen(HELP_CLOSED)).toBe(false);
    expect(helpOpen({ ...HELP_CLOSED, hover: true })).toBe(true);
    expect(helpOpen({ ...HELP_CLOSED, focus: true })).toBe(true);
  });
  it("押すと固定して開き、もう一度押すと閉じる。Esc ですべて閉じる", () => {
    const s = helpToggle(HELP_CLOSED);
    expect(helpOpen(s)).toBe(true);
    expect(helpOpen(helpToggle(s))).toBe(false);
    expect(helpOpen(helpEscape())).toBe(false);
  });
  it("hover で開いているときに押すと閉じる", () => {
    expect(helpOpen(helpToggle({ ...HELP_CLOSED, hover: true }))).toBe(false);
  });
});

describe("Scale", () => {
  it("同じ値を押すと外れる", () => {
    expect(toggleScale(3, 3)).toBeUndefined();
    expect(toggleScale(3, 4)).toBe(4);
    expect(toggleScale(undefined, 1)).toBe(1);
  });
  it("矢印キーで動く（端で止まる）", () => {
    expect(stepScale(2, "ArrowRight")).toBe(3);
    expect(stepScale(5, "ArrowRight")).toBe(5);
    expect(stepScale(1, "ArrowLeft")).toBe(1);
    expect(stepScale(undefined, "ArrowRight")).toBe(1);
    expect(stepScale(undefined, "ArrowLeft")).toBe(5);
    expect(stepScale(2, "a")).toBeUndefined();
  });
  it("文字列との変換", () => {
    expect(scaleFromString("")).toBeUndefined();
    expect(scaleFromString("4")).toBe(4);
    expect(scaleToString(undefined)).toBe("");
    expect(scaleToString(null)).toBe("");
    expect(scaleToString(2)).toBe("2");
  });
});

describe("期間の選択肢", () => {
  it("既定は 4 つ、サブセットは指定順", () => {
    expect(periodOptions().map((p) => p.id)).toEqual(["today", "yesterday", "this-week", "last-week"]);
    expect(periodOptions(["today", "this-week", "last-week"]).map((p) => p.label)).toEqual(["今日", "今週", "先週"]);
    expect(PERIOD_LABELS["yesterday"]).toBe("昨日");
  });
});

describe("タブ", () => {
  const ids = ["overview", "time", "rules"];
  it("?tab= の値は使えるものだけ。それ以外は既定", () => {
    expect(tabFromParam("time", ids, "overview")).toBe("time");
    expect(tabFromParam("nope", ids, "overview")).toBe("overview");
    expect(tabFromParam(null, ids, "overview")).toBe("overview");
    expect(tabFromParam("", ids, "overview")).toBe("overview");
  });
  it("←→ Home End で動く（端で回り込む）", () => {
    expect(stepTab(ids, "overview", "ArrowRight")).toBe("time");
    expect(stepTab(ids, "rules", "ArrowRight")).toBe("overview");
    expect(stepTab(ids, "overview", "ArrowLeft")).toBe("rules");
    expect(stepTab(ids, "time", "Home")).toBe("overview");
    expect(stepTab(ids, "time", "End")).toBe("rules");
    expect(stepTab(ids, "time", "a")).toBeUndefined();
    expect(stepTab([], "x", "Home")).toBeUndefined();
  });
});
