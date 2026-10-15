import { describe, expect, it } from "vitest";
import {
  clickRange,
  currentBlock,
  describeConflicts,
  digestSegments,
  dragRange,
  findOverlaps,
  moveRange,
  nextFreeSlot,
  nowPercent,
  overlaps,
  placeTaskRange,
  resizeRange,
  shouldDesign,
  snapMillis,
  timeToPercent,
  visibleWindow,
  yToTime,
} from "./plan-util";

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

describe("clickRange", () => {
  it("クリック位置から 1 時間を取る", () => {
    expect(clickRange(360, HEIGHT, DAY_START, DAY_END)).toEqual({ start: DAY_START + 9 * H, end: DAY_START + 10 * H });
  });
  it("日の終わりを超えるときは手前にずらす", () => {
    const r = clickRange(955, HEIGHT, DAY_START, DAY_END);
    expect(r.end).toBe(DAY_END);
    expect(r.end - r.start).toBe(H);
  });
});

describe("moveRange", () => {
  const span = { start: DAY_START + 9 * H, end: DAY_START + 10 * H + 30 * MIN };
  it("長さを保ったまま刻みに合わせて動かす", () => {
    // 80px = 2 時間
    expect(moveRange(span, 80, HEIGHT, DAY_START, DAY_END)).toEqual({
      start: DAY_START + 11 * H,
      end: DAY_START + 12 * H + 30 * MIN,
    });
    // 4px(= 6 分) は動かない、8px(= 12 分) は 15 分動く
    expect(moveRange(span, 4, HEIGHT, DAY_START, DAY_END).start).toBe(span.start);
    expect(moveRange(span, 8, HEIGHT, DAY_START, DAY_END).start).toBe(span.start + 15 * MIN);
    expect(moveRange(span, -40, HEIGHT, DAY_START, DAY_END).start).toBe(DAY_START + 8 * H);
  });
  it("日の範囲からはみ出さない", () => {
    expect(moveRange(span, -9999, HEIGHT, DAY_START, DAY_END).start).toBe(DAY_START);
    const r = moveRange(span, 9999, HEIGHT, DAY_START, DAY_END);
    expect(r.end).toBe(DAY_END);
    expect(r.end - r.start).toBe(90 * MIN);
  });
});

describe("resizeRange", () => {
  const span = { start: DAY_START + 9 * H, end: DAY_START + 10 * H };
  it("下端・上端を動かす", () => {
    expect(resizeRange(span, "end", 440, HEIGHT, DAY_START, DAY_END)).toEqual({
      start: span.start,
      end: DAY_START + 11 * H,
    });
    expect(resizeRange(span, "start", 320, HEIGHT, DAY_START, DAY_END)).toEqual({
      start: DAY_START + 8 * H,
      end: span.end,
    });
  });
  it("反対側を越えても 1 刻み分は残す", () => {
    expect(resizeRange(span, "end", 100, HEIGHT, DAY_START, DAY_END).end).toBe(span.start + 15 * MIN);
    expect(resizeRange(span, "start", 800, HEIGHT, DAY_START, DAY_END).start).toBe(span.end - 15 * MIN);
  });
});

describe("overlaps / findOverlaps", () => {
  const a = { start: 10, end: 20 };
  it("接しているだけは重ならない", () => {
    expect(overlaps(a, { start: 20, end: 30 })).toBe(false);
    expect(overlaps(a, { start: 0, end: 10 })).toBe(false);
    expect(overlaps(a, { start: 19, end: 30 })).toBe(true);
    expect(overlaps(a, { start: 12, end: 14 })).toBe(true);
  });
  it("自分自身は除く", () => {
    const items = [
      { id: "x", start_at: 0, end_at: 15 },
      { id: "y", start_at: 15, end_at: 18 },
      { id: "z", start_at: 20, end_at: 30 },
    ];
    expect(findOverlaps(a, items).map((i) => i.id)).toEqual(["x", "y"]);
    expect(findOverlaps(a, items, "x").map((i) => i.id)).toEqual(["y"]);
  });
});

describe("placeTaskRange", () => {
  it("見積もりを刻みに切り上げた長さで置く（無ければ 1 時間）", () => {
    const r = placeTaskRange(360, HEIGHT, DAY_START, DAY_END, 50);
    expect(r).toEqual({ start: DAY_START + 9 * H, end: DAY_START + 9 * H + 60 * MIN });
    expect(placeTaskRange(360, HEIGHT, DAY_START, DAY_END, 20).end - (DAY_START + 9 * H)).toBe(30 * MIN);
    expect(placeTaskRange(360, HEIGHT, DAY_START, DAY_END, null).end - (DAY_START + 9 * H)).toBe(H);
  });
});

describe("nextFreeSlot", () => {
  const busy = [
    { start: DAY_START + 9 * H, end: DAY_START + 10 * H },
    { start: DAY_START + 10 * H, end: DAY_START + 11 * H + 20 * MIN },
  ];
  it("重ならない最初の空きを返す", () => {
    expect(nextFreeSlot(60, DAY_START + 9 * H, DAY_END, busy)).toEqual({
      start: DAY_START + 11 * H + 30 * MIN,
      end: DAY_START + 12 * H + 30 * MIN,
    });
    expect(nextFreeSlot(30, DAY_START + 8 * H + 5 * MIN, DAY_END, busy)?.start).toBe(DAY_START + 8 * H + 15 * MIN);
  });
  it("日の終わりまでに入らなければ null", () => {
    expect(nextFreeSlot(60, DAY_END - 30 * MIN, DAY_END, [])).toBeNull();
  });
});

describe("visibleWindow", () => {
  it("既定は 7〜22 時で、はみ出す分だけ広げる", () => {
    expect(visibleWindow(DAY_START, DAY_END, [])).toEqual({ start: DAY_START + 7 * H, end: DAY_START + 22 * H });
    const w = visibleWindow(DAY_START, DAY_END, [DAY_START + 5 * H + 20 * MIN, DAY_START + 23 * H + 10 * MIN]);
    expect(w).toEqual({ start: DAY_START + 5 * H, end: DAY_START + 24 * H });
  });
  it("日の範囲外の時刻は無視し、all で 1 日全体", () => {
    expect(visibleWindow(DAY_START, DAY_END, [DAY_END + 5 * H]).end).toBe(DAY_START + 22 * H);
    expect(visibleWindow(DAY_START, DAY_END, [], true)).toEqual({ start: DAY_START, end: DAY_END });
  });
});

describe("nowPercent / currentBlock", () => {
  it("現在時刻の位置は範囲内だけ返す", () => {
    expect(nowPercent(DAY_START + 12 * H, DAY_START, DAY_END)).toBe(50);
    expect(nowPercent(DAY_START - 1, DAY_START, DAY_END)).toBeNull();
    expect(nowPercent(DAY_END + 1, DAY_START, DAY_END)).toBeNull();
  });
  it("進行中のブロックを返す", () => {
    const bs = [
      { id: "a", start_at: DAY_START + 9 * H, end_at: DAY_START + 10 * H },
      { id: "b", start_at: DAY_START + 10 * H, end_at: DAY_START + 11 * H },
    ];
    expect(currentBlock(bs, DAY_START + 9 * H + 1)?.id).toBe("a");
    expect(currentBlock(bs, DAY_START + 10 * H)?.id).toBe("b");
    expect(currentBlock(bs, DAY_START + 11 * H)).toBeUndefined();
  });
});

describe("digestSegments", () => {
  it("要確認の件数・長さとエージェント対話の時間を数える", () => {
    const d = digestSegments([
      { start_at: 0, end_at: 10 * MIN, kind: "work", status: "needs_review" },
      { start_at: 0, end_at: 5 * MIN, kind: "agent_dialog", status: "auto" },
      { start_at: 0, end_at: 20 * MIN, kind: "agent_dialog", status: "needs_review" },
      { start_at: 0, end_at: 30 * MIN, kind: "agent_wait", status: "auto" },
    ]);
    expect(d).toEqual({ reviewCount: 2, reviewMs: 30 * MIN, agentMs: 25 * MIN });
    expect(digestSegments([])).toEqual({ reviewCount: 0, reviewMs: 0, agentMs: 0 });
  });
});

describe("shouldDesign / describeConflicts", () => {
  it("ブロック 0 件か明示の指定のときにデザインモード", () => {
    expect(shouldDesign(0, false, false)).toBe(true);
    expect(shouldDesign(0, false, true)).toBe(false);
    expect(shouldDesign(3, false, false)).toBe(false);
    expect(shouldDesign(3, true, true)).toBe(true);
  });
  it("重なりの名前を 3 件まで並べる", () => {
    expect(describeConflicts([])).toBe("");
    expect(describeConflicts(["a", "a", "b"])).toBe("a、b");
    expect(describeConflicts(["a", "b", "c", "d", "e"])).toBe("a、b、c ほか 2 件");
  });
});
