// タスク編集まわりの純粋なロジック（未保存判定・ショートカット判定・離脱確認の状態）。

/** 編集フォームの値（入力欄の文字列そのまま）。 */
export interface TaskDraft {
  title: string;
  body: string;
  urgent: boolean;
  important: boolean;
  est: string;
  skills: string;
}

/** スキル欄（カンマ区切り）を配列にする。 */
export function parseSkills(s: string): string[] {
  return s
    .split(/[,、]/)
    .map((x) => x.trim())
    .filter(Boolean);
}

/** 見積欄を分に。数として読めなければ undefined。 */
export function parseEstimate(s: string): number | undefined {
  const n = parseInt(s, 10);
  return Number.isFinite(n) ? n : undefined;
}

function normalize(d: TaskDraft) {
  return {
    title: d.title,
    body: d.body,
    urgent: d.urgent,
    important: d.important,
    est: parseEstimate(d.est) ?? null,
    skills: parseSkills(d.skills),
  };
}

/** 保存済みの値と異なれば true。スキルの区切りや見積の前後の空白だけの違いは同じ値として扱う。 */
export function isDirty(saved: TaskDraft, current: TaskDraft): boolean {
  return JSON.stringify(normalize(saved)) !== JSON.stringify(normalize(current));
}

/** Ctrl+S（macOS の Cmd+S も）か。Shift・Alt が付いたものは別のショートカットとして除く。 */
export function isSaveShortcut(e: { key: string; ctrlKey: boolean; metaKey: boolean; altKey: boolean; shiftKey: boolean }): boolean {
  return (e.ctrlKey || e.metaKey) && !e.altKey && !e.shiftKey && e.key.toLowerCase() === "s";
}

export type BodyMode = "edit" | "preview";

/** タスクを開いたときの本文の表示。本文があればプレビュー、空なら編集。 */
export function defaultBodyMode(body: string): BodyMode {
  return body.trim() === "" ? "edit" : "preview";
}

/**
 * 離脱確認の状態。未保存の変更があるとき、画面遷移・タスク切り替えを保留して確認 UI を出す。
 * `window.confirm` は使わず、アプリ内の確認 UI（`LeaveConfirm`）で応答する。
 */
export function createLeaveGuard() {
  let dirty = false;
  let pending: (() => void) | null = null;
  const listeners = new Set<() => void>();
  const emit = () => listeners.forEach((l) => l());
  return {
    isDirty: () => dirty,
    hasPending: () => pending !== null,
    setDirty(v: boolean) {
      if (dirty === v) return;
      dirty = v;
      if (!v) pending = null;
      emit();
    },
    /** 未保存がなければすぐ `proceed` を呼ぶ。あれば保留して確認を待つ。 */
    requestLeave(proceed: () => void) {
      if (!dirty) {
        proceed();
        return;
      }
      pending = proceed;
      emit();
    },
    /** 「破棄して移動」。 */
    discard() {
      const p = pending;
      pending = null;
      dirty = false;
      emit();
      p?.();
    },
    /** 「キャンセル」。その場に留まる。 */
    cancel() {
      pending = null;
      emit();
    },
    subscribe(l: () => void) {
      listeners.add(l);
      return () => {
        listeners.delete(l);
      };
    },
  };
}

/** アプリ全体で 1 つの離脱ガード。 */
export const leaveGuard = createLeaveGuard();
