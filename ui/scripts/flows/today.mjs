// 今日: 計画の列をドラッグ → 右のサイドパネルの BlockForm で保存 → ブロックが出る
// → ブロックをクリックして編集パネルを開き、「…」から削除する。
import { dragPlanLane, mockState, until } from "../lib/flow.mjs";

export default {
  name: "today",
  scenario: "default",
  screen: "today",
  async run({ page }) {
    const blocksBefore = await mockState(page, (s) => s.blocks.length);
    const panel = page.getByRole("complementary", { name: "詳細" });

    await dragPlanLane(page, 0.7, 0.76);
    await panel.getByRole("heading", { name: "ブロックを追加", level: 2 }).waitFor({ timeout: 5000 });
    await panel.getByRole("textbox", { name: "ラベル" }).fill("操作シナリオの作業");
    await panel.getByRole("button", { name: "追加", exact: true }).click();

    // 画面: ブロックが計画の列に出て、パネルは閉じる。
    await page.getByRole("button", { name: /操作シナリオの作業/ }).waitFor({ timeout: 5000 });
    await panel.waitFor({ state: "detached", timeout: 5000 });
    // 状態: ブロックが 1 件増え、ラベルが入っている。
    await until(
      async () => (await mockState(page, (s) => s.blocks.filter((b) => b.label === "操作シナリオの作業").length)) === 1,
      "ブロックがモックの状態に入りません",
    );
    if ((await mockState(page, (s) => s.blocks.length)) !== blocksBefore + 1) throw new Error("ブロックが 1 件だけ増えていません。");

    // クリックで編集パネル（選択状態）→ 「…」から削除。
    await page.getByRole("button", { name: /操作シナリオの作業/ }).click();
    await panel.getByRole("heading", { name: "ブロックを編集", level: 2 }).waitFor({ timeout: 5000 });
    await panel.getByRole("button", { name: "ブロックの操作" }).click();
    await page.getByRole("menuitem", { name: "削除" }).click();
    await until(async () => (await mockState(page, (s) => s.blocks.length)) === blocksBefore, "ブロックが削除されません");
    await panel.waitFor({ state: "detached", timeout: 5000 });
  },
};
