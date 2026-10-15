import src from "../api/index.ts?raw";
import { describe, expect, it, vi } from "vitest";
import { createMock } from "./handlers";
import { SCENARIOS } from "./state";

// `api/index.ts` が呼ぶ Tauri コマンドの一覧を、ソースから拾う。
// ソースを文字列として読む（Vite の ?raw。node の型定義に依存しない）。
const commands = [...new Set([...src.matchAll(/call<[^(]*?>\(\s*"([a-z_]+)"/g)].map((m) => m[1]))];

describe("モック IPC", () => {
  it("api の全コマンドを抽出できている", () => {
    expect(commands.length).toBeGreaterThan(100);
  });

  it("api の全コマンドに応答するハンドラがある（新しいコマンドを足したらモックにも足す）", () => {
    const mock = createMock("default");
    const missing = commands.filter((c) => !mock.commands.includes(c));
    expect(missing).toEqual([]);
  });

  it("未実装のコマンドは console.error を出して reject する", async () => {
    const err = vi.spyOn(console, "error").mockImplementation(() => {});
    await expect(createMock("default").call("no_such_command")).rejects.toMatchObject({ code: "internal" });
    expect(err).toHaveBeenCalledWith("[mock] unhandled command: no_such_command");
    err.mockRestore();
  });

  it.each(SCENARIOS)("%s: 状態を読む基本コマンドが応答する", async (sc) => {
    const m = createMock(sc);
    const st = (await m.call("status")) as { today: { total_ms: number } };
    expect(st.today).toBeDefined();
    await m.call("task_list", { filter: { statuses: [] } });
    await m.call("report", { by: "task", range: { date: "this-week" }, include_idle: false });
    await m.call("heatmap", { by: "time" });
    await m.call("goal_list");
  });

  it("タスクの追加と開始が状態に反映される", async () => {
    const m = createMock("default");
    const t = (await m.call("task_add", { input: { title: "新しいタスク" } })) as { id: string };
    await m.call("task_start", { id: t.id });
    const st = (await m.call("status")) as { doing: { task: { id: string } }[] };
    expect(st.doing.map((d) => d.task.id)).toEqual([t.id]);
  });

  it("empty はデータなし、default は今日の区間がある", async () => {
    const empty = (await createMock("empty").call("actual_list", { range: { date: "today" } })) as { segments: unknown[] };
    expect(empty.segments).toHaveLength(0);
    const def = (await createMock("default").call("actual_list", { range: { date: "today" } })) as { segments: unknown[] };
    expect(def.segments.length).toBeGreaterThan(3);
  });
});
