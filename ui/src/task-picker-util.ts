// タスク選択（task-picker）の絞り込みと候補の組み立て。画面に依存しない純粋関数。
import type { TaskView } from "./api";

export type PickerItem =
  /** 既存の未完了タスク。 */
  | { kind: "task"; task: TaskView }
  /** 入力した文字列でタスクを新規作成する。 */
  | { kind: "create"; title: string; interrupt: boolean }
  /** 直前まで doing だったタスクに戻る。 */
  | { kind: "back"; task: TaskView };

/** 比較用に正規化する（全角半角・大文字小文字の差を無視）。 */
function norm(s: string): string {
  return s.normalize("NFKC").trim().toLowerCase();
}

/** 先頭の `!` / `！` を割り込みの指定として取り除く。 */
export function parseQuery(raw: string, interruptToggle = false): { text: string; interrupt: boolean } {
  const m = /^\s*[!！]\s*/.exec(raw);
  const text = (m ? raw.slice(m[0].length) : raw).trim();
  return { text, interrupt: interruptToggle || m !== null };
}

/** 未完了（todo / doing）のタスクを、タイトルの部分一致（大文字小文字無視）で絞り込む。空なら全て。 */
export function filterTasks(tasks: TaskView[], query: string): TaskView[] {
  const open = tasks.filter((t) => t.status === "todo" || t.status === "doing");
  const q = norm(query);
  if (!q) return open;
  return open.filter((t) => norm(t.title).includes(q));
}

export interface BuildOptions {
  tasks: TaskView[];
  /** 入力された文字列（`!` の接頭辞を含んでよい）。 */
  query: string;
  /** Tab などで切り替える割り込みのトグル。 */
  interrupt?: boolean;
  /** 直前の doing（戻る候補）。 */
  back?: TaskView | null;
  /** 一致が無いとき「新規作成」を出す（既定 true）。 */
  allowCreate?: boolean;
  /** 一致があっても（完全一致を除き）「新規作成」を出す。既定 true。 */
  alwaysOfferCreate?: boolean;
}

export interface Built {
  items: PickerItem[];
  /** 入力から `!` を除いた文字列。 */
  text: string;
  /** 割り込みとして作成するか（トグルまたは `!`）。 */
  interrupt: boolean;
}

/**
 * 候補を組み立てる。順序は「戻る」（入力が空か、戻り先のタイトルに一致するとき）→ 絞り込んだタスク → 「新規作成」。
 * 割り込みの指定があるときは「新規作成」を先頭にする（素早く作って切り替えるため）。
 * 「新規作成」は、同じタイトルの未完了タスクが既にあるときは出さない。
 */
export function buildItems(o: BuildOptions): Built {
  const { text, interrupt } = parseQuery(o.query, o.interrupt ?? false);
  const matches = filterTasks(o.tasks, text);
  const items: PickerItem[] = [];
  const q = norm(text);
  const back = o.back;
  const backMatches = back && (!q || norm(back.title).includes(q));
  if (back && backMatches) items.push({ kind: "back", task: back });
  // 戻る候補と同じタスクは重複して出さない。
  for (const t of matches) {
    if (back && backMatches && t.id === back.id) continue;
    items.push({ kind: "task", task: t });
  }
  const exact = filterTasks(o.tasks, "").some((t) => norm(t.title) === q);
  const offer = (o.allowCreate ?? true) && q !== "" && !exact && ((o.alwaysOfferCreate ?? true) || matches.length === 0);
  if (offer) {
    const create: PickerItem = { kind: "create", title: text, interrupt };
    if (interrupt) items.unshift(create);
    else items.push(create);
  }
  return { items, text, interrupt };
}

/** 候補の表示名。 */
export function itemLabel(it: PickerItem): string {
  switch (it.kind) {
    case "task":
      return it.task.title;
    case "back":
      return `戻る: ${it.task.title}`;
    case "create":
      return `新規作成: 『${it.title}』${it.interrupt ? "（割り込み）" : ""}`;
  }
}

/** 選択位置を上下に動かす（端で反対側に回る）。候補が無ければ -1。 */
export function moveIndex(cur: number, delta: number, len: number): number {
  if (len <= 0) return -1;
  const base = cur < 0 ? (delta > 0 ? -1 : 0) : cur;
  return (((base + delta) % len) + len) % len;
}
