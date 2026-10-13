// タスク: 新規作成 → 一覧に出る → 選んで詳細を編集・保存 → 状態を「始める」。
import { mockState, until } from "../lib/flow.mjs";

export default {
  name: "tasks",
  scenario: "default",
  screen: "tasks",
  async run({ page }) {
    // 新規作成
    await page.getByRole("textbox", { name: "新しいタスク" }).fill("操作シナリオ用のタスク");
    await page.getByRole("button", { name: "追加", exact: true }).click();
    const row = page.getByRole("button", { name: /操作シナリオ用のタスク/ });
    await row.waitFor({ timeout: 5000 });

    // 選んで詳細を編集・保存
    await row.click();
    await page.getByRole("heading", { name: "操作シナリオ用のタスク", level: 2 }).waitFor();
    await page.getByRole("textbox", { name: "タイトル" }).fill("操作シナリオ用のタスク（編集後）");
    await page.getByRole("textbox", { name: "見積（分）" }).fill("45");
    await page.getByRole("button", { name: "保存", exact: true }).click();
    await until(
      () => mockState(page, (s) => s.tasks.some((t) => t.title === "操作シナリオ用のタスク（編集後）" && t.estimate_min === 45)),
      "編集がモックの状態に反映されません",
    );
    await page.getByRole("button", { name: /操作シナリオ用のタスク（編集後）/ }).waitFor();

    // 状態を「始める」
    await page.getByRole("button", { name: "始める", exact: true }).click();
    await until(
      () => mockState(page, (s) => s.tasks.some((t) => t.title === "操作シナリオ用のタスク（編集後）" && t.status === "doing")),
      "タスクが進行中になりません",
    );
    await page.getByRole("button", { name: /進行中 操作シナリオ用のタスク（編集後）/ }).waitFor();
  },
};
