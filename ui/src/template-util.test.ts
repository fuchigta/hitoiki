import { describe, expect, it } from "vitest";
import type { PreviewBlock, TemplateView } from "./api";
import {
  emptyItem,
  parseHm,
  sortItems,
  summarizePreview,
  suggestionText,
  toggleWeekday,
  toInput,
  validateTemplate,
  weekdayOf,
  weekdaysLabel,
} from "./template-util";

const pb = (n: number): PreviewBlock => ({
  start_at: 0,
  end_at: 1,
  label: "x",
  focus: false,
  task_id: null,
  task_title: null,
  conflicts: Array.from({ length: n }, () => ({ kind: "block" as const, title: "t", start_at: 0, end_at: 1 })),
});

describe("weekdays", () => {
  it("曜日を月〜日の順に並べて表示する", () => {
    expect(weekdaysLabel([5, 3])).toBe("水・金");
    expect(weekdaysLabel([])).toBe("");
  });
  it("選択を切り替える", () => {
    expect(toggleWeekday([3], 1)).toEqual([1, 3]);
    expect(toggleWeekday([1, 3], 3)).toEqual([1]);
  });
  it("日付の曜日を求める（1=月 … 7=日）", () => {
    expect(weekdayOf("2026-10-14")).toBe(3); // 水
    expect(weekdayOf("2026-10-11")).toBe(7); // 日
    expect(weekdayOf("2026-10-12")).toBe(1); // 月
  });
});

describe("parseHm", () => {
  it("HH:MM を分にする", () => {
    expect(parseHm("09:30")).toBe(570);
    expect(parseHm("9:05")).toBe(545);
    expect(parseHm("24:00")).toBeNull();
    expect(parseHm("24:00", true)).toBe(1440);
    expect(parseHm("12:60")).toBeNull();
    expect(parseHm("abc")).toBeNull();
  });
});

describe("validateTemplate", () => {
  const ok = { name: "水曜", weekdays: [3], items: [{ ...emptyItem(), label: "定例" }] };
  it("正しい入力は通る", () => {
    expect(validateTemplate(ok)).toBeNull();
    expect(validateTemplate({ ...ok, items: [] })).toBeNull();
  });
  it("名前・時刻・ラベルの不足を知らせる", () => {
    expect(validateTemplate({ ...ok, name: " " })).toMatch("名前");
    expect(validateTemplate({ ...ok, items: [{ ...emptyItem(), label: "a", end: "08:00" }] })).toMatch("終了");
    expect(validateTemplate({ ...ok, items: [{ ...emptyItem(), label: "a", start: "x" }] })).toMatch("時刻");
    expect(validateTemplate({ ...ok, items: [{ ...emptyItem(), label: " " }] })).toMatch("ラベル");
    expect(validateTemplate({ ...ok, items: [{ ...emptyItem(), label: "", task: "T1" }] })).toBeNull();
  });
});

describe("sortItems / toInput", () => {
  it("開始時刻順に並べる", () => {
    const a = { ...emptyItem(), start: "14:00", end: "15:00" };
    const b = { ...emptyItem(), start: "09:00", end: "10:00" };
    expect(sortItems([a, b]).map((i) => i.start)).toEqual(["09:00", "14:00"]);
  });
  it("保存済みのテンプレートを入力に戻す", () => {
    const t: TemplateView = {
      id: "i",
      name: "n",
      description: null,
      weekdays: [1],
      created_at: 0,
      updated_at: 0,
      blocks: [
        { id: "b", start: "09:00", end: "10:00", start_min: 540, end_min: 600, label: null, focus: true, task_id: "T", task_title: "t" },
      ],
    };
    expect(toInput(t)).toEqual({
      name: "n",
      description: "",
      weekdays: [1],
      items: [{ start: "09:00", end: "10:00", label: "", focus: true, task: "T" }],
    });
  });
});

describe("summarizePreview", () => {
  const blocks = [pb(0), pb(1), pb(2)];
  it("skip では重なる項目を作らない", () => {
    expect(summarizePreview(blocks, "skip")).toEqual({ create: 1, skip: 2, conflicting: 2 });
  });
  it("overlay では全部作る", () => {
    expect(summarizePreview(blocks, "overlay")).toEqual({ create: 3, skip: 0, conflicting: 2 });
  });
});

describe("suggestionText", () => {
  it("曜日のパターンを適用するか尋ねる", () => {
    expect(suggestionText(["水曜のパターン"], 3)).toBe("水曜日のパターン「水曜のパターン」を適用しますか？");
    expect(suggestionText([], 3)).toBe("");
  });
});
