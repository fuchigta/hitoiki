// 実績画面の絞り込みと選択の純粋関数。画面に依存しない。
import type { SegmentKind, SegmentView } from "./api";

/** 状態の絞り込み。`review` は要確認、`untasked` はタスクなし、`all` はすべて。 */
export type LogStatus = "review" | "untasked" | "all";

export const LOG_STATUSES: LogStatus[] = ["review", "untasked", "all"];

/** `?status=` の値。不正・未指定は要確認。 */
export function statusFromParam(v: string | null | undefined): LogStatus {
  return LOG_STATUSES.find((s) => s === v) ?? "review";
}

const WORK_KINDS: SegmentKind[] = ["work", "agent_dialog", "agent_watch"];

/** 「タスクなし」の対象: タスクが付いていない作業・エージェントの区間。 */
export function isUntasked(s: Pick<SegmentView, "task_id" | "kind">): boolean {
  return s.task_id == null && WORK_KINDS.includes(s.kind);
}

/**
 * `api.log` の区間を、状態とタスクで絞り込む（要確認は別の API なのでここでは触らない）。
 * 無視（ignored）は「すべて」でも出さない。
 */
export function filterSegments(segs: SegmentView[], status: LogStatus, taskId?: string | null): SegmentView[] {
  return segs.filter((s) => {
    if (s.kind === "ignored") return false;
    if (taskId && s.task_id !== taskId) return false;
    if (status === "untasked") return isUntasked(s);
    return true;
  });
}

/**
 * 確定した区間（`done`）の次に選ぶ要確認の区間の ID。
 * 一覧（時刻順）で、確定した区間の最初の位置より後ろの未確定を優先し、なければ先頭側の未確定。なければ null。
 */
export function nextToReview(ids: string[], done: string[]): string | null {
  const gone = new Set(done);
  const at = ids.findIndex((id) => gone.has(id));
  const rest = ids.filter((id) => !gone.has(id));
  if (rest.length === 0) return null;
  if (at < 0) return rest[0];
  const after = ids.slice(at + 1).find((id) => !gone.has(id));
  return after ?? rest[0];
}

/** 選んだ区間の合計の長さ（ミリ秒）。 */
export function totalMs(segs: Pick<SegmentView, "duration_ms">[]): number {
  return segs.reduce((a, s) => a + s.duration_ms, 0);
}

/** 選んだ ID の並びに、チェックを付け外しする。 */
export function toggleId(cur: string[], id: string): string[] {
  return cur.includes(id) ? cur.filter((x) => x !== id) : [...cur, id];
}
