import { describe, expect, it } from "vitest";
import { dragRange, snapMillis, timeToPercent, yToTime } from "./plan-util";

const MIN = 60_000;
const H = 60 * MIN;
const DAY_START = Date.UTC(2026, 0, 10, 0, 0); // 任意の基準（UTC 0 時）
const DAY_END = DAY_START + 24 * H;
const HEIGHT = 960; // 1 時間 = 40px

describe("snapMillis", () => {
  it("15 分刻みに四捨五入する", () => {
    expect(snapMillis(DAY_START + 7 * MIN, DAY_START)).toBe(DAY_START);
    expect(snapMillis(DAY_START + 8 * MIN, DAY_START)).toBe(DAY_START + 15 * MIN);
    expect(snapMillis(DAY_START + 22 * MIN, DAY_START)).toBe(DAY_START + 15 * MIN);
    expect(snapMillis(DAY_START + 23 * MIN, DAY_START)).toBe(DAY_START + 30 * MIN);
  });
});

describe("yToTime", () => {
  it("縦位置を時刻にしてスナップする", () => {
    expect(yToTime(0, HEIGHT, DAY_START, DAY_END)).toBe(DAY_START);
    expect(yToTime(360, HEIGHT, DAY_START, DAY_END)).toBe(DAY_START + 9 * H);
    // 9:00 + 4px(= 6 分) は 9:00 に、+ 8px(= 12 分) は 9:15 に丸まる
    expect(yToTime(364, HEIGHT, DAY_START, DAY_END)).toBe(DAY_START + 9 * H);
    expect(yToTime(368, HEIGHT, DAY_START, DAY_END)).toBe(DAY_START + 9 * H + 15 * MIN);
  });

  it("範囲外は 0 時〜24 時に収める", () => {
    expect(yToTime(-50, HEIGHT, DAY_START, DAY_END)).toBe(DAY_START);
    expect(yToTime(5000, HEIGHT, DAY_START, DAY_END)).toBe(DAY_END);
    expect(yToTime(10, 0, DAY_START, DAY_END)).toBe(DAY_START);
  });
});

describe("dragRange", () => {
  it("上向きのドラッグでも start < end", () => {
    const r = dragRange(440, 360, HEIGHT, DAY_START, DAY_END);
    expect(r).toEqual({ start: DAY_START + 9 * H, end: DAY_START + 11 * H });
  });

  it("幅がないときは 1 刻み分を確保する", () => {
    const r = dragRange(360, 361, HEIGHT, DAY_START, DAY_END);
    expect(r).toEqual({ start: DAY_START + 9 * H, end: DAY_START + 9 * H + 15 * MIN });
  });

  it("日の終わりでも範囲を保つ", () => {
    const r = dragRange(HEIGHT, HEIGHT + 20, HEIGHT, DAY_START, DAY_END);
    expect(r.end).toBe(DAY_END);
    expect(r.end - r.start).toBe(15 * MIN);
  });
});

describe("timeToPercent", () => {
  it("割合に変換し、範囲外は端に寄せる", () => {
    expect(timeToPercent(DAY_START + 12 * H, DAY_START, DAY_END)).toBe(50);
    expect(timeToPercent(DAY_START - H, DAY_START, DAY_END)).toBe(0);
    expect(timeToPercent(DAY_END + H, DAY_START, DAY_END)).toBe(100);
  });
});
