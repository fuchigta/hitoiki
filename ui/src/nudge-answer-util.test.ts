import { describe, expect, it } from "vitest";
import type { NudgePayload } from "./api";
import { buildForm, dismissArgs, periodLabel, respondArgs } from "./nudge-answer-util";

const base: NudgePayload = {
  trigger: "day_start",
  title: "おはようございます",
  body: "b",
  actions: [1, 2, 3, 4, 5].map((n) => ({ id: "energy", label: String(n), value: n })),
  input: null,
  open_screen: "today/design",
};

describe("buildForm", () => {
  it("エネルギーは 1〜5 のボタンになり、開くも出る", () => {
    const f = buildForm(base);
    expect(f.buttons.map((b) => b.label)).toEqual(["1", "2", "3", "4", "5"]);
    expect(f.buttons[2]).toMatchObject({ action: "energy", value: 3 });
    expect(new Set(f.buttons.map((b) => b.key)).size).toBe(5);
    expect(f.canOpen).toBe(true);
  });

  it("入力欄だけの通知には送信ボタンが付く", () => {
    const f = buildForm({
      ...base,
      actions: [],
      input: { id: "hassle", placeholder: "ひとこと" },
      open_screen: null,
    });
    expect(f.buttons).toEqual([{ key: "text", label: "ひとこと残す", action: "text", value: null }]);
    expect(f.input?.placeholder).toBe("ひとこと");
    expect(f.canOpen).toBe(false);
  });

  it("無操作の 4 分類と、画面を開くボタン", () => {
    const idle = buildForm({
      ...base,
      trigger: "idle_return",
      actions: ["break", "meeting", "thinking", "away"].map((s) => ({ id: `idle_${s}`, label: s, value: null })),
      idle_range: [0, 60_000],
    });
    expect(idle.buttons.map((b) => b.action)).toEqual(["idle_break", "idle_meeting", "idle_thinking", "idle_away"]);
    const open = buildForm({
      ...base,
      actions: [{ id: "open", label: "振り返りを開く", value: null }],
      open_screen: "reflection/day_end",
    });
    expect(open.buttons).toHaveLength(1);
    expect(open.canOpen).toBe(false);
  });
});

describe("respondArgs", () => {
  it("ボタンと一言（空白除去）を渡す", () => {
    const f = buildForm({ ...base, input: { id: "hassle", placeholder: "x" } });
    expect(respondArgs("P1", base, f.buttons[3], "  会議が長い ")).toEqual({
      nudge_id: "P1",
      action: "energy",
      value: 4,
      input: "会議が長い",
      open_screen: null,
    });
    expect(respondArgs("P1", base, f.buttons[0], "   ").input).toBeUndefined();
  });

  it("開くは open と画面名を渡す", () => {
    expect(respondArgs("P1", base, null, "")).toMatchObject({ action: "open", open_screen: "today/design" });
    const a = { action: "open", value: null };
    expect(respondArgs("P1", base, a, "")).toMatchObject({ action: "open", open_screen: "today/design" });
  });

  it("見送りは dismiss", () => {
    expect(dismissArgs("P1")).toEqual({ nudge_id: "P1", action: "dismiss", value: null, open_screen: null });
  });
});

describe("periodLabel", () => {
  it("対象期間があれば範囲を返す", () => {
    const fmt = (ms: number) => `t${ms}`;
    expect(periodLabel({ ...base, idle_range: [1, 2] }, fmt)).toBe("t1〜t2");
    expect(periodLabel(base, fmt)).toBeNull();
    expect(periodLabel(null, fmt)).toBeNull();
  });
});
