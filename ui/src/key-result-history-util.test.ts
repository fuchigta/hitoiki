import { describe, expect, it } from "vitest";
import type { KeyResultHistoryWeek } from "./api";
import { isEmptyHistory, valueLevels, valueSegments, weekValueText } from "./key-result-history-util";

const week = (over: Partial<KeyResultHistoryWeek> = {}): KeyResultHistoryWeek => ({
  week_start: "2026-10-05",
  duration_ms: 0,
  value: null,
  text: null,
  changes: 0,
  ...over,
});

describe("weekValueText", () => {
  it("記録が無い週は -", () => {
    expect(weekValueText(week())).toBe("-");
    expect(weekValueText(week({ text: "5 回" }))).toBe("5 回");
  });
});

describe("valueLevels", () => {
  it("最小〜最大を 0〜1 に伸ばし、数値の無い週は null", () => {
    expect(valueLevels([week({ value: 2 }), week(), week({ value: 6 }), week({ value: 4 })])).toEqual([
      0,
      null,
      1,
      0.5,
    ]);
  });
  it("数値が 1 種類だけなら中央、1 つも無ければ全て null", () => {
    expect(valueLevels([week({ value: 3 }), week({ value: 3 })])).toEqual([0.5, 0.5]);
    expect(valueLevels([week(), week()])).toEqual([null, null]);
  });
});

describe("valueSegments", () => {
  it("値の無い週で線を切る", () => {
    const segs = valueSegments([1, null, 0, 0.5], 20, 60, 4);
    expect(segs).toHaveLength(2);
    expect(segs[0]).toEqual([{ x: 10, y: 4 }]);
    expect(segs[1]).toEqual([
      { x: 50, y: 56 },
      { x: 70, y: 30 },
    ]);
  });
  it("空なら区間なし", () => {
    expect(valueSegments([null, null], 20, 60)).toEqual([]);
  });
});

describe("isEmptyHistory", () => {
  it("時間も値も無ければ空", () => {
    expect(isEmptyHistory([week(), week()])).toBe(true);
    expect(isEmptyHistory([week(), week({ duration_ms: 1 })])).toBe(false);
    expect(isEmptyHistory([week({ text: "1" })])).toBe(false);
  });
});
