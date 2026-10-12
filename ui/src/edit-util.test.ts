import { describe, expect, it, vi } from "vitest";
import { createLeaveGuard, defaultBodyMode, isDirty, isSaveShortcut, type TaskDraft } from "./edit-util";

const base: TaskDraft = { title: "t", body: "b", urgent: false, important: true, est: "30", skills: ["rust", "ui"] };
const key = (o: Partial<Parameters<typeof isSaveShortcut>[0]>) => ({
  key: "s",
  ctrlKey: false,
  metaKey: false,
  altKey: false,
  shiftKey: false,
  ...o,
});

describe("isDirty", () => {
  it("同じ値なら未保存ではない", () => {
    expect(isDirty(base, { ...base })).toBe(false);
  });
  it("各欄の変更を検出する", () => {
    expect(isDirty(base, { ...base, title: "x" })).toBe(true);
    expect(isDirty(base, { ...base, body: "b2" })).toBe(true);
    expect(isDirty(base, { ...base, urgent: true })).toBe(true);
    expect(isDirty(base, { ...base, important: false })).toBe(true);
    expect(isDirty(base, { ...base, est: "45" })).toBe(true);
    expect(isDirty(base, { ...base, skills: ["rust"] })).toBe(true);
  });
  it("スキルの並び順と見積の前後空白は変更としない", () => {
    expect(isDirty(base, { ...base, skills: ["ui", "rust"], est: " 30 " })).toBe(false);
    expect(isDirty(base, { ...base, skills: ["rust", "ui", "go"] })).toBe(true);
  });
  it("見積を空にしたら変更", () => {
    expect(isDirty(base, { ...base, est: "" })).toBe(true);
  });
});

describe("isSaveShortcut", () => {
  it("Ctrl+S / Cmd+S", () => {
    expect(isSaveShortcut(key({ ctrlKey: true }))).toBe(true);
    expect(isSaveShortcut(key({ ctrlKey: true, key: "S" }))).toBe(true);
    expect(isSaveShortcut(key({ metaKey: true }))).toBe(true);
  });
  it("修飾なし・他のキー・Shift/Alt 付きは対象外", () => {
    expect(isSaveShortcut(key({}))).toBe(false);
    expect(isSaveShortcut(key({ ctrlKey: true, key: "a" }))).toBe(false);
    expect(isSaveShortcut(key({ ctrlKey: true, shiftKey: true }))).toBe(false);
    expect(isSaveShortcut(key({ ctrlKey: true, altKey: true }))).toBe(false);
  });
});

describe("defaultBodyMode", () => {
  it("本文があればプレビュー、空（空白のみ含む）なら編集", () => {
    expect(defaultBodyMode("# 題")).toBe("preview");
    expect(defaultBodyMode("")).toBe("edit");
    expect(defaultBodyMode("  \n")).toBe("edit");
  });
});

describe("createLeaveGuard", () => {
  it("未保存でなければ即移動する", () => {
    const g = createLeaveGuard();
    const go = vi.fn();
    g.requestLeave(go);
    expect(go).toHaveBeenCalledTimes(1);
    expect(g.hasPending()).toBe(false);
  });
  it("未保存なら保留し、破棄で移動・キャンセルで留まる", () => {
    const g = createLeaveGuard();
    const go = vi.fn();
    g.setDirty(true);
    g.requestLeave(go);
    expect(go).not.toHaveBeenCalled();
    expect(g.hasPending()).toBe(true);
    g.cancel();
    expect(go).not.toHaveBeenCalled();
    expect(g.hasPending()).toBe(false);
    expect(g.isDirty()).toBe(true);
    g.requestLeave(go);
    g.discard();
    expect(go).toHaveBeenCalledTimes(1);
    expect(g.isDirty()).toBe(false);
  });
  it("保留中に未保存でなくなったら保留を捨てる", () => {
    const g = createLeaveGuard();
    g.setDirty(true);
    g.requestLeave(() => {});
    g.setDirty(false);
    expect(g.hasPending()).toBe(false);
  });
  it("状態の変化を購読者へ通知する", () => {
    const g = createLeaveGuard();
    const l = vi.fn();
    const off = g.subscribe(l);
    g.setDirty(true);
    expect(l).toHaveBeenCalledTimes(1);
    off();
    g.setDirty(false);
    expect(l).toHaveBeenCalledTimes(1);
  });
});
