import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";

// lib.ts は window のイベントを使うので、Node 環境用に最小限のスタブを置く。
beforeAll(() => {
  vi.stubGlobal("window", { dispatchEvent: () => true, addEventListener: () => {}, removeEventListener: () => {} });
});

const { dateInput, fmtDateTime, fmtDur, fmtFull, fmtTime, screenFromHash, setTimeZone, toMillis, wallClock } =
  await import("./lib");

afterEach(() => setTimeZone(undefined));

// 2026-03-05 01:30 UTC = 2026-03-05 10:30 JST
const MS = Date.UTC(2026, 2, 5, 1, 30);

describe("タイムゾーン付きの時刻整形", () => {
  it("UTC", () => {
    setTimeZone("UTC");
    expect(fmtTime(MS)).toBe("01:30");
    expect(dateInput(MS)).toBe("2026-03-05");
    expect(fmtDateTime(MS)).toBe("3/5 01:30");
    expect(fmtFull(MS)).toBe("2026-03-05 01:30");
  });

  it("Asia/Tokyo は同じ ms を +9 時間で表示する", () => {
    setTimeZone("Asia/Tokyo");
    expect(fmtTime(MS)).toBe("10:30");
    expect(dateInput(MS)).toBe("2026-03-05");
    expect(wallClock(MS)).toEqual({ y: 2026, mo: 3, d: 5, h: 10, mi: 30 });
  });

  it("日付をまたぐ", () => {
    const late = Date.UTC(2026, 2, 5, 20, 0); // JST では翌日 5:00
    setTimeZone("UTC");
    expect(dateInput(late)).toBe("2026-03-05");
    setTimeZone("Asia/Tokyo");
    expect(dateInput(late)).toBe("2026-03-06");
    expect(fmtTime(late)).toBe("05:00");
  });

  it("0 時は 24:00 ではなく 00:00", () => {
    setTimeZone("Asia/Tokyo");
    expect(fmtTime(Date.UTC(2026, 2, 4, 15, 0))).toBe("00:00");
  });

  it("不正なタイムゾーン名は無視される", () => {
    setTimeZone("Not/AZone");
    expect(() => fmtTime(MS)).not.toThrow();
  });
});

describe("toMillis", () => {
  it("UTC / Tokyo で壁時計から UTC ミリ秒へ", () => {
    setTimeZone("UTC");
    expect(toMillis("2026-03-05", "01:30")).toBe(MS);
    setTimeZone("Asia/Tokyo");
    expect(toMillis("2026-03-05", "10:30")).toBe(MS);
  });

  it("表示との往復が一致する", () => {
    for (const tz of ["UTC", "Asia/Tokyo", "America/New_York"]) {
      setTimeZone(tz);
      for (const ms of [MS, Date.UTC(2026, 6, 1, 12, 45), Date.UTC(2026, 11, 31, 23, 15)]) {
        expect(toMillis(dateInput(ms), fmtTime(ms))).toBe(ms);
      }
    }
  });
});

describe("その他の整形", () => {
  it("fmtDur", () => {
    expect(fmtDur(30 * 60_000)).toBe("30分");
    expect(fmtDur(60 * 60_000)).toBe("1時間");
    expect(fmtDur(90 * 60_000)).toBe("1時間30分");
    expect(fmtDur(-5)).toBe("0分");
  });

  it("screenFromHash", () => {
    expect(screenFromHash("#/plan")).toBe("today");
    expect(screenFromHash("#/tasks?x=1")).toBe("tasks");
    expect(screenFromHash("#/unknown")).toBe("today");
    expect(screenFromHash("")).toBe("today");
  });
});
