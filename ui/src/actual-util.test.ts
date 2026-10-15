import { describe, expect, it } from "vitest";
import type { SegmentKind, SegmentView } from "./api";
import { filterSegments, isUntasked, nextToReview, statusFromParam, toggleId, totalMs } from "./actual-util";

const seg = (id: string, kind: SegmentKind, task_id: string | null = null): SegmentView =>
  ({ id, kind, task_id, duration_ms: 60000 }) as SegmentView;

describe("statusFromParam", () => {
  it("不正・未指定は要確認", () => {
    expect(statusFromParam(null)).toBe("review");
    expect(statusFromParam("x")).toBe("review");
    expect(statusFromParam("all")).toBe("all");
    expect(statusFromParam("untasked")).toBe("untasked");
  });
});

describe("filterSegments", () => {
  const segs = [seg("a", "work"), seg("b", "work", "T1"), seg("c", "agent_dialog"), seg("d", "meeting"), seg("e", "ignored"), seg("f", "idle")];
  it("すべては無視を出さない", () => {
    expect(filterSegments(segs, "all").map((s) => s.id)).toEqual(["a", "b", "c", "d", "f"]);
  });
  it("タスクなしは作業・エージェントでタスクが無いものだけ", () => {
    expect(filterSegments(segs, "untasked").map((s) => s.id)).toEqual(["a", "c"]);
    expect(isUntasked(seg("x", "meeting"))).toBe(false);
  });
  it("タスクの指定", () => {
    expect(filterSegments(segs, "all", "T1").map((s) => s.id)).toEqual(["b"]);
  });
});

describe("nextToReview", () => {
  const ids = ["a", "b", "c", "d"];
  it("確定した区間より後ろの未確定を選ぶ", () => {
    expect(nextToReview(ids, ["b"])).toBe("c");
    expect(nextToReview(ids, ["a", "b"])).toBe("c");
  });
  it("後ろが無ければ先頭側", () => {
    expect(nextToReview(ids, ["d"])).toBe("a");
    expect(nextToReview(ids, ["c", "d"])).toBe("a");
  });
  it("残りが無ければ null", () => {
    expect(nextToReview(["a"], ["a"])).toBeNull();
    expect(nextToReview([], [])).toBeNull();
  });
  it("一覧にない ID なら先頭", () => {
    expect(nextToReview(ids, ["z"])).toBe("a");
  });
});

describe("選択", () => {
  it("toggleId と totalMs", () => {
    expect(toggleId(["a"], "b")).toEqual(["a", "b"]);
    expect(toggleId(["a", "b"], "a")).toEqual(["b"]);
    expect(totalMs([seg("a", "work"), seg("b", "work")])).toBe(120000);
  });
});
