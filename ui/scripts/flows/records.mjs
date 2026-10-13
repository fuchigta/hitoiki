// 記録: チェックインを記録 → 一覧に出る。
import { mockState, until } from "../lib/flow.mjs";

export default {
  name: "records",
  scenario: "default",
  screen: "records",
  async run({ page }) {
    const card = page.locator("section.card").filter({ has: page.getByRole("heading", { name: "チェックイン", level: 2 }) }).first();
    await card.getByRole("group", { name: "エネルギー" }).getByRole("button", { name: "4", exact: true }).click();
    await card.getByRole("group", { name: "気分" }).getByRole("button", { name: "3", exact: true }).click();
    await card.getByRole("textbox", { name: "一言メモ" }).fill("操作シナリオのメモ");
    await card.getByRole("button", { name: "記録", exact: true }).click();

    await until(
      () => mockState(page, (s) => s.checkins.some((c) => c.note === "操作シナリオのメモ" && c.energy === 4 && c.mood === 3)),
      "チェックインがモックの状態に入りません",
    );
    // 画面: 一覧（チェックインの列）にメモが出る。
    await page.getByText("操作シナリオのメモ").waitFor({ timeout: 5000 });
  },
};
