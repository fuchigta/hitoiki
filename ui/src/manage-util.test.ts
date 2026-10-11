import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import type { Impact } from "./api";

// manage-util は lib.ts（window のイベントを使う）に依存するので、Node 環境用に最小限のスタブを置く。
beforeAll(() => {
  vi.stubGlobal("window", { dispatchEvent: () => true, addEventListener: () => {}, removeEventListener: () => {} });
});

const { setTimeZone, toMillis } = await import("./lib");
const { blankToUndef, hasImpact, impactLines, parseScaleInput, periodEdit, periodInputs } = await import("./manage-util");

afterEach(() => setTimeZone(undefined));

const impact = (lines: Impact["lines"]): Impact => ({ entity: "key_result", id: "K", name: "成果", lines, note: null });

describe("impactLines / hasImpact", () => {
  it("件数つきの文にする", () => {
    const i = impact([{ label: "紐付いたタスク", count: 3 }]);
    expect(impactLines(i)).toEqual(["紐付いたタスク 3 件"]);
    expect(hasImpact(i)).toBe(true);
  });
  it("影響がなければ空", () => {
    expect(impactLines(impact([]))).toEqual([]);
    expect(hasImpact(impact([]))).toBe(false);
  });
});

describe("期間", () => {
  beforeEach(() => setTimeZone("Asia/Tokyo"));
  it("終了日はその日の終わり（翌日 0 時）として保存し、表示は元の日付に戻る", () => {
    const e = periodEdit("2026-10-01", "2026-12-31", { period_start: null, period_end: null });
    expect(e.from).toBe(toMillis("2026-10-01", "00:00"));
    expect(e.to).toBe(toMillis("2027-01-01", "00:00"));
    expect(e.clear_period).toBe(true);
    expect(periodInputs({ period_start: e.from!, period_end: e.to! })).toEqual({
      from: "2026-10-01",
      to: "2026-12-31",
    });
  });
  it("両方空なら期間を消す。元から無ければ何もしない", () => {
    expect(periodEdit("", "", { period_start: 1, period_end: 2 })).toEqual({ clear_period: true });
    expect(periodEdit("", "", { period_start: null, period_end: null })).toEqual({});
  });
  it("片方だけでも、もう片方は消える", () => {
    const e = periodEdit("2026-10-01", "", { period_start: 1, period_end: 2 });
    expect(e.to).toBeUndefined();
    expect(e.clear_period).toBe(true);
  });
  it("期間なしは空欄", () => {
    expect(periodInputs({ period_start: null, period_end: null })).toEqual({ from: "", to: "" });
  });
});

describe("入力の解釈", () => {
  it("空白は未入力", () => {
    expect(blankToUndef("  ")).toBeUndefined();
    expect(blankToUndef(" a ")).toBe("a");
  });
  it("1〜5 の数値だけ受け付ける", () => {
    expect(parseScaleInput("")).toBeUndefined();
    expect(parseScaleInput("3")).toBe(3);
    expect(parseScaleInput("0")).toBeNull();
    expect(parseScaleInput("6")).toBeNull();
    expect(parseScaleInput("x")).toBeNull();
    expect(parseScaleInput("2.5")).toBeNull();
  });
});
