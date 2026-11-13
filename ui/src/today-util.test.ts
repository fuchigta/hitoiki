import { describe, expect, it } from "vitest";
import { hashStep } from "./hash-guard";
import {
  SIDE_INITIAL,
  blockIsTask,
  breakdownOf,
  dateFromParam,
  dateParams,
  daySum,
  legendKinds,
  mergedDoingIds,
  actualPeriodFor,
  shiftDate,
  shownPanel,
  startGuideFor,
  timelineSelection,
  unplacedTasks,
  viewAnchors,
  weekdayLabel,
  sideReducer,
  visiblePanel,
  type SideState,
} from "./today-util";

describe("日付の移動", () => {
  it("前後に動かす（月・年またぎ、うるう日）", () => {
    expect(shiftDate("2026-10-14", -1)).toBe("2026-10-13");
    expect(shiftDate("2026-10-31", 1)).toBe("2026-11-01");
    expect(shiftDate("2026-01-01", -1)).toBe("2025-12-31");
    expect(shiftDate("2028-02-28", 1)).toBe("2028-02-29");
    expect(shiftDate("2026-03-01", -1)).toBe("2026-02-28");
  });
  it("?date= は YYYY-MM-DD だけ受け付ける（不正は今日）", () => {
    expect(dateFromParam("2026-10-13", "2026-10-14")).toBe("2026-10-13");
    expect(dateFromParam(null, "2026-10-14")).toBe("2026-10-14");
    expect(dateFromParam("yesterday", "2026-10-14")).toBe("2026-10-14");
    expect(dateFromParam("2026-02-30", "2026-10-14")).toBe("2026-10-14");
  });
  it("今日のパラメータは省略する・曜日・実績画面の期間", () => {
    expect(dateParams("2026-10-14", "2026-10-14")).toEqual({ date: undefined });
    expect(dateParams("2026-10-13", "2026-10-14")).toEqual({ date: "2026-10-13" });
    expect(weekdayLabel("2026-10-14")).toBe("水曜日");
    expect(actualPeriodFor("2026-10-14", "2026-10-14")).toBe("today");
    expect(actualPeriodFor("2026-10-13", "2026-10-14")).toBe("yesterday");
    expect(actualPeriodFor("2026-10-01", "2026-10-14")).toBeUndefined();
  });
});

describe("サイドパネルの状態遷移", () => {
  const draft = { start: 1, end: 2, task: "", label: "", focus: false };
  const run = (s: SideState, ...a: Parameters<typeof sideReducer>[1][]) => a.reduce(sideReducer, s);

  it("最初は何も出さない", () => {
    expect(visiblePanel(SIDE_INITIAL)).toBeNull();
  });
  it("別のパネルを開くと入れ替わる（同時に 1 つ）", () => {
    const s = run(SIDE_INITIAL, { type: "open", panel: { kind: "segment", id: "a" } }, { type: "open", panel: { kind: "block", draft } });
    expect(visiblePanel(s)?.kind).toBe("block");
    expect(run(s, { type: "close" })).toEqual(SIDE_INITIAL);
  });
  it("デザイン中に別のパネルを閉じると、デザインに戻る", () => {
    let s = run(SIDE_INITIAL, { type: "open", panel: { kind: "design" } });
    expect(visiblePanel(s)?.kind).toBe("design");
    s = run(s, { type: "open", panel: { kind: "block", draft } });
    expect(visiblePanel(s)?.kind).toBe("block");
    s = run(s, { type: "close" });
    expect(visiblePanel(s)?.kind).toBe("design");
  });
  it("デザインの開閉と完了", () => {
    let s = run(SIDE_INITIAL, { type: "toggleDesign" });
    expect(visiblePanel(s)?.kind).toBe("design");
    s = run(s, { type: "toggleDesign" });
    expect(visiblePanel(s)).toBeNull();
    // 別のパネルが出ているときの切り替えはデザインに戻す。
    s = run(SIDE_INITIAL, { type: "toggleDesign" }, { type: "open", panel: { kind: "nudge" } }, { type: "toggleDesign" });
    expect(visiblePanel(s)?.kind).toBe("design");
    expect(visiblePanel(run(s, { type: "finishDesign" }))).toBeNull();
  });
  it("日付が変わると、ブロック・補正・区間は閉じる（問いかけとデザインは残す）", () => {
    expect(visiblePanel(run(SIDE_INITIAL, { type: "open", panel: { kind: "correct", span: { start: 1, end: 2 } } }, { type: "dateChanged" }))).toBeNull();
    expect(visiblePanel(run(SIDE_INITIAL, { type: "open", panel: { kind: "nudge" } }, { type: "dateChanged" }))?.kind).toBe("nudge");
    const s = run(SIDE_INITIAL, { type: "open", panel: { kind: "design" } }, { type: "open", panel: { kind: "segment", id: "x" } }, { type: "dateChanged" });
    expect(visiblePanel(s)?.kind).toBe("design");
  });
});

describe("サイドパネルの表示とタイムラインの強調", () => {
  const draft = { start: 1, end: 2, task: "", label: "", focus: false };
  it("ブロックの編集は日の範囲が届くまで出さない（ほかは範囲に関係なく出す）", () => {
    expect(shownPanel(null, true)).toBeNull();
    expect(shownPanel({ kind: "block", draft }, false)).toBeNull();
    expect(shownPanel({ kind: "block", draft }, true)?.kind).toBe("block");
    expect(shownPanel({ kind: "segment", id: "x" }, false)?.kind).toBe("segment");
    expect(shownPanel({ kind: "design" }, false)?.kind).toBe("design");
  });
  it("パネルの種類に応じて、タイムラインで強調するものが 1 つだけ決まる", () => {
    const none = { draft: null, actualSpan: null, segmentId: null };
    expect(timelineSelection(null)).toEqual(none);
    expect(timelineSelection({ kind: "design" })).toEqual(none);
    expect(timelineSelection({ kind: "nudge" })).toEqual(none);
    expect(timelineSelection({ kind: "block", draft })).toEqual({ ...none, draft });
    expect(timelineSelection({ kind: "correct", span: { start: 3, end: 4 } })).toEqual({ ...none, actualSpan: { start: 3, end: 4 } });
    expect(timelineSelection({ kind: "segment", id: "S1" })).toEqual({ ...none, segmentId: "S1" });
  });
});

describe("タイムライン", () => {
  it("表示範囲に収める時刻は、現在時刻（今日だけ）・ブロック・区間・終日でない予定の両端", () => {
    const blocks = [{ start_at: 10, end_at: 20 }];
    const segs = [{ start_at: 30, end_at: 40 }];
    const events = [
      { start_at: 50, end_at: 60, all_day: false },
      { start_at: 0, end_at: 100, all_day: true },
    ];
    expect(viewAnchors(5, blocks, segs, events)).toEqual([5, 10, 20, 30, 40, 50, 60]);
    expect(viewAnchors(undefined, blocks, segs, events)).toEqual([10, 20, 30, 40, 50, 60]);
    expect(viewAnchors(undefined, [], [], [])).toEqual([]);
  });
  it("凡例の種別は出現順で重複なし", () => {
    expect(legendKinds([{ kind: "work" }, { kind: "idle" }, { kind: "work" }, { kind: "meeting" }])).toEqual(["work", "idle", "meeting"]);
    expect(legendKinds([])).toEqual([]);
  });
  it("ブロックに置いたタスクは候補から外す", () => {
    const tasks = [{ id: "T1" }, { id: "T2" }, { id: "T3" }];
    expect(unplacedTasks(tasks, [{ task_id: "T2" }, { task_id: null }])).toEqual([{ id: "T1" }, { id: "T3" }]);
    expect(unplacedTasks(tasks, [])).toEqual(tasks);
  });
});

describe("内訳と案内", () => {
  const row = (label: string) => ({ key: null, label, duration_ms: 1, ratio: 1, segments: 1 });
  const status = { today: { by_kind: [row("作業")], top_tasks: [row("A")], total_ms: 100 } };
  const rows9 = Array.from({ length: 9 }, (_, i) => row(`T${i}`));
  it("今日は status の集計（届くまで null）", () => {
    expect(breakdownOf(true, undefined, undefined, undefined)).toBeNull();
    expect(breakdownOf(true, status, null, null)).toEqual({ kindRows: status.today.by_kind, taskRows: status.today.top_tasks, total: 100 });
  });
  it("今日以外はレポートの集計（タスクは上位 8 件。両方届くまで null）", () => {
    const kinds = { rows: [row("会議")], total_ms: 50 };
    expect(breakdownOf(false, status, kinds, undefined)).toBeNull();
    expect(breakdownOf(false, undefined, undefined, { rows: rows9, total_ms: 1 })).toBeNull();
    const b = breakdownOf(false, undefined, kinds, { rows: rows9, total_ms: 1 });
    expect(b?.kindRows).toEqual(kinds.rows);
    expect(b?.taskRows).toHaveLength(8);
    expect(b?.total).toBe(50);
  });
  it("「次にやること」は今日・データが届いた後・デザイン中でない・使い始めのときだけ", () => {
    const fresh = { doing: [] as unknown[] };
    expect(startGuideFor(true, fresh, [], false)).toEqual({ doing: 0, segments: 0 });
    expect(startGuideFor(true, { doing: [1] }, [], false)).toEqual({ doing: 1, segments: 0 });
    expect(startGuideFor(true, { doing: [1] }, [1], false)).toBeNull();
    expect(startGuideFor(false, fresh, [], false)).toBeNull();
    expect(startGuideFor(true, undefined, [], false)).toBeNull();
    expect(startGuideFor(true, fresh, undefined, false)).toBeNull();
    expect(startGuideFor(true, fresh, [], true)).toBeNull();
  });
});

describe("いま", () => {
  const doing = [{ task: { id: "T1", title: "設計レビュー" } }, { task: { id: "T2", title: "別の作業" } }];
  it("ブロックと進行中のタスクが同じなら 1 行にまとめる", () => {
    expect(blockIsTask({ task_id: "T1", task_title: "設計レビュー", label: null }, doing[0])).toBe(true);
    expect(blockIsTask({ task_id: null, task_title: null, label: "設計レビュー" }, doing[0])).toBe(true);
    expect(blockIsTask({ task_id: "T9", task_title: "ほか", label: null }, doing[0])).toBe(false);
    expect([...mergedDoingIds({ task_id: "T1", task_title: "設計レビュー", label: null }, doing)]).toEqual(["T1"]);
    expect(mergedDoingIds(undefined, doing).size).toBe(0);
  });
  it("ラベルが空のブロックは、空のタイトルと一致扱いにしない", () => {
    expect(blockIsTask({ task_id: null, task_title: null, label: null }, { task: { id: "T", title: "" } })).toBe(false);
  });
});

describe("daySum", () => {
  it("無操作・休憩・無視は含めない", () => {
    const m = 60_000;
    expect(
      daySum([
        { start_at: 0, end_at: 30 * m, kind: "work" },
        { start_at: 0, end_at: 10 * m, kind: "idle" },
        { start_at: 0, end_at: 5 * m, kind: "break" },
        { start_at: 0, end_at: 5 * m, kind: "ignored" },
        { start_at: 0, end_at: 20 * m, kind: "meeting" },
      ]),
    ).toBe(50 * m);
  });
});

describe("戻る・進むのガード", () => {
  it("同じハッシュは何もしない", () => {
    expect(hashStep("#/tasks", "#/tasks", true)).toBe("skip");
  });
  it("未保存がなければ受け入れる", () => {
    expect(hashStep("#/tasks", "#/today", false)).toBe("accept");
  });
  it("未保存があれば元に戻して確認する", () => {
    expect(hashStep("#/tasks/T1", "#/today", true)).toBe("restore");
    // 戻したあとに届く（受け入れ済みと同じ）イベントは何もしない。
    expect(hashStep("#/tasks/T1", "#/tasks/T1", true)).toBe("skip");
  });
});
