import { describe, expect, it } from "vitest";
import type { HeatmapDay } from "./api";
import { buildBy, buildWeeks, cellTitle, fmtHeatValue, monthLabels, weekdayOf } from "./heatmap-util";

function span(from: string, n: number): HeatmapDay[] {
  const [y, m, d] = from.split("-").map(Number);
  return Array.from({ length: n }, (_, i) => {
    const t = new Date(Date.UTC(y, m - 1, d + i));
    return { date: t.toISOString().slice(0, 10), value: 0, level: 0 };
  });
}

describe("buildWeeks", () => {
  it("月曜始まりで 7 マスずつに並べ、前後を null で埋める", () => {
    // 2026-10-07 は水曜（月曜 = 0 なので 2）。
    const days = span("2026-10-07", 10);
    const weeks = buildWeeks(days, weekdayOf("2026-10-07"));
    expect(weeks).toHaveLength(2);
    expect(weeks[0].slice(0, 2)).toEqual([null, null]);
    expect(weeks[0][2]?.date).toBe("2026-10-07");
    expect(weeks[0][6]?.date).toBe("2026-10-11");
    expect(weeks[1][0]?.date).toBe("2026-10-12");
    expect(weeks[1][4]?.date).toBe("2026-10-16");
    expect(weeks[1].slice(5)).toEqual([null, null]);
  });

  it("月曜開始なら先頭は埋めない", () => {
    const weeks = buildWeeks(span("2026-10-05", 14), 0);
    expect(weeks).toHaveLength(2);
    expect(weeks[0][0]?.date).toBe("2026-10-05");
    expect(weeks[1][6]?.date).toBe("2026-10-18");
  });
});

describe("monthLabels", () => {
  it("月が替わる週に出し、近すぎるラベルは省く", () => {
    const weeks = buildWeeks(span("2026-09-28", 7 * 8), 0);
    // 週 0 = 9/28（9 月）、週 1 = 10/5（近いので省く）、週 5 = 11/2（11 月）。
    expect(monthLabels(weeks)).toEqual([
      { week: 0, label: "9月" },
      { week: 5, label: "11月" },
    ]);
    expect(monthLabels(weeks, 1).map((l) => l.label)).toEqual(["9月", "10月", "11月"]);
  });

  it("minGap より近い月ラベルは省く", () => {
    // 先頭の週が 9 月、次の週が 10 月: 間隔 1 列なので 10 月は出さない。
    const weeks = buildWeeks(span("2026-09-28", 14), 0);
    expect(monthLabels(weeks).map((l) => l.label)).toEqual(["9月"]);
  });
});

describe("表示", () => {
  it("値の書式", () => {
    expect(fmtHeatValue("ms", 90 * 60000)).toBe("1時間30分");
    expect(fmtHeatValue("ms", 30 * 60000)).toBe("30分");
    expect(fmtHeatValue("count", 3)).toBe("3件");
  });
  it("ツールチップ", () => {
    expect(cellTitle({ date: "2026-10-12", value: 120 * 60000, level: 4 }, "ms")).toBe("2026-10-12（月） 2時間");
    expect(cellTitle({ date: "2026-10-11", value: 0, level: 0 }, "ms")).toBe("2026-10-11（日） なし");
  });
  it("by の組み立て", () => {
    expect(buildBy("time", "")).toBe("time");
    expect(buildBy("kind", "work")).toBe("kind:work");
    expect(buildBy("task", "")).toBeNull();
    expect(buildBy("key_result", "")).toBeNull();
    expect(buildBy("key_result", "01ABC")).toBe("key_result:01ABC");
    expect(buildBy("records", "")).toBe("records");
    expect(buildBy("records", "reframe")).toBe("records:reframe");
  });
});
