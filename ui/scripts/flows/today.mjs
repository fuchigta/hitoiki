// 今日: 計画の列をドラッグしてブロックを作る → フォームで保存 → ブロックが出る。
import { dragPlanLane, mockState, until } from "../lib/flow.mjs";

export default {
  name: "today",
  scenario: "default",
  screen: "today",
  async run({ page }) {
    const blocksBefore = await mockState(page, (s) => s.blocks.length);

    await dragPlanLane(page, 0.7, 0.76);
    const dialog = page.getByRole("dialog", { name: "ブロック" });
    await dialog.waitFor({ timeout: 5000 });
    await dialog.getByRole("textbox", { name: "ラベル" }).fill("操作シナリオの作業");
    await dialog.getByRole("button", { name: "追加", exact: true }).click();

    // 画面: ブロックが計画の列に出る。
    await page.getByRole("button", { name: /操作シナリオの作業/ }).waitFor({ timeout: 5000 });
    // 状態: ブロックが 1 件増え、ラベルが入っている。
    await until(
      async () => (await mockState(page, (s) => s.blocks.filter((b) => b.label === "操作シナリオの作業").length)) === 1,
      "ブロックがモックの状態に入りません",
    );
    if ((await mockState(page, (s) => s.blocks.length)) !== blocksBefore + 1) throw new Error("ブロックが 1 件だけ増えていません。");
  },
};
