import { describe, expect, it } from "vitest";
import type { CorrectionCandidate } from "./api";
import {
  boundaryTimes,
  describeTotals,
  effectiveSelection,
  fmtJpDuration,
  idleIncluded,
  matchesFilter,
  pickRange,
  setIdleIncluded,
  snapToBoundary,
  sumSelected,
  targetInput,
  targetLabel,
  toggleChecked,
  yToMinute,
} from "./correct-util";

const MIN = 60_000;
const T0 = 1_700_000_000_000 - (1_700_000_000_000 % (60 * MIN)); // 時の頭

function cand(id: string, s: number, e: number, over: Partial<CorrectionCandidate> = {}): CorrectionCandidate {
  return {
    id,
    gap: false,
    start_at: T0 + s * MIN,
    end_at: T0 + e * MIN,
    start: "",
    end: "",
    duration_ms: (e - s) * MIN,
    full_duration_ms: (e - s) * MIN,
    kind: "work",
    status: "needs_review",
    task_id: null,
    task_title: null,
    activity_label: null,
    exe_name: null,
    title: null,
    category: "work",
    offline: false,
    becomes_offline: false,
    checked: true,
    ...over,
  };
}

const idle = (id: string, s: number, e: number, checked = false) =>
  cand(id, s, e, { kind: "idle", category: "idle", becomes_offline: true, checked });
const gap = (s: number, e: number, checked = false) =>
  cand(`gap:${s}`, s, e, {
    gap: true,
    kind: null,
    status: null,
    category: "gap",
    becomes_offline: true,
    checked,
  });

describe("境目への吸着", () => {
  const b = boundaryTimes([
    { start_at: T0, end_at: T0 + 30 * MIN },
    { start_at: T0 + 30 * MIN, end_at: T0 + 60 * MIN },
  ]);

  it("境目は昇順・重複なし", () => {
    expect(b).toEqual([T0, T0 + 30 * MIN, T0 + 60 * MIN]);
  });

  it("吸着幅以内なら最も近い境目へ", () => {
    expect(snapToBoundary(T0 + 29 * MIN, b, 2 * MIN)).toBe(T0 + 30 * MIN);
    expect(snapToBoundary(T0 + 31 * MIN, b, 2 * MIN)).toBe(T0 + 30 * MIN);
    // ちょうど幅の距離も吸着する
    expect(snapToBoundary(T0 + 28 * MIN, b, 2 * MIN)).toBe(T0 + 30 * MIN);
  });

  it("幅の外なら動かさない", () => {
    expect(snapToBoundary(T0 + 27 * MIN, b, 2 * MIN)).toBe(T0 + 27 * MIN);
    expect(snapToBoundary(T0 + 90 * MIN, b, 2 * MIN)).toBe(T0 + 90 * MIN);
  });

  it("幅 0 以下・境目なしは吸着しない", () => {
    expect(snapToBoundary(T0 + 29 * MIN, b, 0)).toBe(T0 + 29 * MIN);
    expect(snapToBoundary(T0 + 29 * MIN, [], 2 * MIN)).toBe(T0 + 29 * MIN);
  });

  it("同じ距離の境目が 2 つあれば早い方", () => {
    expect(snapToBoundary(T0 + 15 * MIN, [T0 + 10 * MIN, T0 + 20 * MIN], 5 * MIN)).toBe(T0 + 10 * MIN);
  });
});

describe("範囲選択", () => {
  const view = { start: T0, end: T0 + 120 * MIN };
  const h = 1200; // 10px = 1 分

  it("縦位置を 1 分刻みの時刻にする", () => {
    expect(yToMinute(0, h, view.start, view.end)).toBe(T0);
    expect(yToMinute(604, h, view.start, view.end)).toBe(T0 + 60 * MIN);
    expect(yToMinute(h + 50, h, view.start, view.end)).toBe(T0 + 120 * MIN);
    expect(yToMinute(-5, h, view.start, view.end)).toBe(T0);
  });

  it("上向きのドラッグでも start < end", () => {
    const r = pickRange(600, 300, h, view.start, view.end, [], 0)!;
    expect(r).toEqual({ start: T0 + 30 * MIN, end: T0 + 60 * MIN });
  });

  it("両端が境目に吸着する", () => {
    const b = [T0 + 30 * MIN, T0 + 60 * MIN];
    // 29 分 → 30 分、61 分 → 60 分
    const r = pickRange(290, 610, h, view.start, view.end, b, 2 * MIN)!;
    expect(r).toEqual({ start: T0 + 30 * MIN, end: T0 + 60 * MIN });
    // 吸着しない設定ならそのまま
    expect(pickRange(290, 610, h, view.start, view.end, b, 0)).toEqual({
      start: T0 + 29 * MIN,
      end: T0 + 61 * MIN,
    });
  });

  it("1 分に満たない範囲は選ばない", () => {
    expect(pickRange(300, 304, h, view.start, view.end, [], 0)).toBeNull();
    // 吸着の結果、同じ境目に潰れても選ばない
    expect(pickRange(295, 305, h, view.start, view.end, [T0 + 30 * MIN], 2 * MIN)).toBeNull();
  });
});

describe("チェックリスト", () => {
  const cands = [
    cand("W1", 0, 20),
    cand("W2", 20, 30, { task_id: "T1", task_title: "A" }),
    cand("C1", 30, 40, { status: "confirmed", category: "confirmed", checked: false }),
    idle("I1", 40, 50),
    gap(50, 80),
  ];

  it("絞り込み: 未確定だけ・タスクなしだけ", () => {
    expect(cands.filter((c) => matchesFilter(c, "unconfirmed")).map((c) => c.id)).toEqual(["W1", "W2", "I1", "gap:50"]);
    expect(cands.filter((c) => matchesFilter(c, "no_task")).map((c) => c.id)).toEqual(["W1", "C1", "I1", "gap:50"]);
    expect(cands.filter((c) => matchesFilter(c, "all"))).toHaveLength(5);
  });

  it("チェックの切り替え", () => {
    const next = toggleChecked(cands, "C1");
    expect(next.find((c) => c.id === "C1")!.checked).toBe(true);
    expect(cands.find((c) => c.id === "C1")!.checked).toBe(false); // 元は変えない
  });

  it("無操作・記録なしのスイッチは該当するものだけを切り替える", () => {
    expect(idleIncluded(cands)).toBe(false);
    const on = setIdleIncluded(cands, true);
    expect(on.filter((c) => c.checked).map((c) => c.id)).toEqual(["W1", "W2", "I1", "gap:50"]);
    expect(idleIncluded(on)).toBe(true);
    const off = setIdleIncluded(on, false);
    expect(off.filter((c) => c.checked).map((c) => c.id)).toEqual(["W1", "W2"]);
    // 該当する候補が無ければ OFF のまま
    expect(idleIncluded([cand("W", 0, 10)])).toBe(false);
  });

  it("絞り込みに合わないものはチェックされていても対象外", () => {
    expect(effectiveSelection(cands, "all").map((c) => c.id)).toEqual(["W1", "W2"]);
    expect(effectiveSelection(cands, "no_task").map((c) => c.id)).toEqual(["W1"]);
  });
});

describe("合計", () => {
  it("対象の合計・PC 外・外した件数", () => {
    const cands = setIdleIncluded(
      [cand("W1", 0, 20), cand("C1", 20, 30, { checked: false, status: "confirmed" }), idle("I1", 30, 45), gap(45, 90)],
      true,
    );
    expect(sumSelected(cands, "all")).toEqual({
      selected: 3,
      total_ms: (20 + 15 + 45) * MIN,
      offline_ms: (15 + 45) * MIN,
      excluded: 1,
    });
    // 絞り込みで隠れた分も「外した」に数える
    expect(sumSelected(cands, "no_task")).toEqual({
      selected: 3,
      total_ms: (20 + 15 + 45) * MIN,
      offline_ms: (15 + 45) * MIN,
      excluded: 1,
    });
    expect(sumSelected(cands, "unconfirmed").excluded).toBe(1);
  });

  it("日本語の時間表示は core と同じ", () => {
    expect(fmtJpDuration(72 * MIN)).toBe("1時間12分");
    expect(fmtJpDuration(45 * MIN)).toBe("45分");
    expect(fmtJpDuration(120 * MIN)).toBe("2時間");
    expect(fmtJpDuration(30_000)).toBe("30秒");
    expect(fmtJpDuration(0)).toBe("0秒");
  });

  it("説明文", () => {
    expect(describeTotals("タスク X", { selected: 4, total_ms: 72 * MIN, offline_ms: 15 * MIN, excluded: 3 })).toBe(
      "タスク X に 1時間12分を割り当て（うち PC 外 15分）。外した区間 3 件",
    );
    expect(describeTotals("「休憩」", { selected: 1, total_ms: 10 * MIN, offline_ms: 0, excluded: 0 })).toBe(
      "「休憩」 に 10分を割り当て。",
    );
    expect(describeTotals("タスク X", { selected: 0, total_ms: 0, offline_ms: 0, excluded: 2 })).toBe(
      "タスク X に割り当てる区間がありません。",
    );
  });
});

describe("割り当て先", () => {
  it("API の入力に変換する", () => {
    expect(targetInput("none")).toEqual({ no_task: true });
    expect(targetInput("rest")).toEqual({ rest: true });
    expect(targetInput("01TASK")).toEqual({ task: "01TASK" });
  });

  it("表示名", () => {
    expect(targetLabel("none")).toBe("「タスクなし」");
    expect(targetLabel("rest")).toBe("「休憩」");
    expect(targetLabel("01TASK", "設計")).toBe("タスク 設計");
  });
});
