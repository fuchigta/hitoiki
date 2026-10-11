import { describe, expect, it } from "vitest";
import type { TaskView } from "./api";
import { buildItems, filterTasks, itemLabel, moveIndex, parseQuery } from "./task-picker-util";

function task(id: string, title: string, status: TaskView["status"] = "todo"): TaskView {
  return { id, title, status, skills: [], key_result: null } as unknown as TaskView;
}

const TASKS = [
  task("1", "設計レビュー"),
  task("2", "Review PR #12", "doing"),
  task("3", "メール返信"),
  task("4", "終わった仕事", "done"),
  task("5", "やめた仕事", "dropped"),
];

describe("parseQuery", () => {
  it("先頭の ! を割り込みの指定として取り除く", () => {
    expect(parseQuery("!障害対応")).toEqual({ text: "障害対応", interrupt: true });
    expect(parseQuery("！ 障害対応 ")).toEqual({ text: "障害対応", interrupt: true });
    expect(parseQuery("障害対応!")).toEqual({ text: "障害対応!", interrupt: false });
  });
  it("トグルが入っていれば ! が無くても割り込み", () => {
    expect(parseQuery("障害", true)).toEqual({ text: "障害", interrupt: true });
  });
});

describe("filterTasks", () => {
  it("空なら未完了のタスクだけを全て返す", () => {
    expect(filterTasks(TASKS, "").map((t) => t.id)).toEqual(["1", "2", "3"]);
    expect(filterTasks(TASKS, "  ").map((t) => t.id)).toEqual(["1", "2", "3"]);
  });
  it("部分一致で大文字小文字と全角半角を無視する", () => {
    expect(filterTasks(TASKS, "review").map((t) => t.id)).toEqual(["2"]);
    expect(filterTasks(TASKS, "ＲＥＶＩＥＷ").map((t) => t.id)).toEqual(["2"]);
    expect(filterTasks(TASKS, "レビュー").map((t) => t.id)).toEqual(["1"]);
  });
  it("完了・破棄のタスクは出さない", () => {
    expect(filterTasks(TASKS, "仕事")).toEqual([]);
  });
});

describe("buildItems", () => {
  it("一致が無ければ「新規作成」を出す", () => {
    const b = buildItems({ tasks: TASKS, query: "資料作成" });
    expect(b.items).toEqual([{ kind: "create", title: "資料作成", interrupt: false }]);
    expect(itemLabel(b.items[0])).toBe("新規作成: 『資料作成』");
  });
  it("一致があるときは絞り込んだタスクの後ろに「新規作成」を置く", () => {
    const b = buildItems({ tasks: TASKS, query: "レビュー" });
    expect(b.items.map((i) => i.kind)).toEqual(["task", "create"]);
  });
  it("同じタイトルの未完了タスクがあれば「新規作成」を出さない", () => {
    const b = buildItems({ tasks: TASKS, query: "メール返信" });
    expect(b.items.map((i) => i.kind)).toEqual(["task"]);
    // 完了済みの同名は妨げにならない。
    expect(buildItems({ tasks: TASKS, query: "終わった仕事" }).items.map((i) => i.kind)).toEqual(["create"]);
  });
  it("入力が空なら新規作成は出さず、タスクを全て並べる", () => {
    const b = buildItems({ tasks: TASKS, query: "" });
    expect(b.items.map((i) => i.kind)).toEqual(["task", "task", "task"]);
  });
  it("allowCreate=false なら出さない", () => {
    expect(buildItems({ tasks: TASKS, query: "資料作成", allowCreate: false }).items).toEqual([]);
  });
  it("戻る候補は先頭に出し、同じタスクを重ねて出さない", () => {
    const back = TASKS[0];
    const b = buildItems({ tasks: TASKS, query: "", back });
    expect(b.items.map((i) => i.kind)).toEqual(["back", "task", "task"]);
    expect(b.items.filter((i) => i.kind !== "create" && i.task.id === "1")).toHaveLength(1);
    expect(itemLabel(b.items[0])).toBe("戻る: 設計レビュー");
  });
  it("戻る候補は入力が戻り先に一致しないときは出さない", () => {
    const back = TASKS[0];
    expect(buildItems({ tasks: TASKS, query: "メール", back }).items.map((i) => i.kind)).toEqual(["task", "create"]);
    expect(buildItems({ tasks: TASKS, query: "設計", back }).items.map((i) => i.kind)).toEqual(["back", "create"]);
  });
  it("割り込み（! またはトグル）のときは「新規作成」を先頭にして割り込みの印を付ける", () => {
    const b = buildItems({ tasks: TASKS, query: "!レビュー" });
    expect(b.interrupt).toBe(true);
    expect(b.text).toBe("レビュー");
    expect(b.items[0]).toEqual({ kind: "create", title: "レビュー", interrupt: true });
    expect(itemLabel(b.items[0])).toBe("新規作成: 『レビュー』（割り込み）");
    const t = buildItems({ tasks: TASKS, query: "障害", interrupt: true });
    expect(t.items[0]).toEqual({ kind: "create", title: "障害", interrupt: true });
  });
  it("! だけで文字が無ければ新規作成は出ない", () => {
    expect(buildItems({ tasks: TASKS, query: "!" }).items.some((i) => i.kind === "create")).toBe(false);
  });
});

describe("moveIndex", () => {
  it("端で反対側に回る", () => {
    expect(moveIndex(0, 1, 3)).toBe(1);
    expect(moveIndex(2, 1, 3)).toBe(0);
    expect(moveIndex(0, -1, 3)).toBe(2);
  });
  it("未選択からは最初／最後に入る", () => {
    expect(moveIndex(-1, 1, 3)).toBe(0);
    expect(moveIndex(-1, -1, 3)).toBe(2);
  });
  it("候補が無ければ -1", () => {
    expect(moveIndex(0, 1, 0)).toBe(-1);
  });
});
