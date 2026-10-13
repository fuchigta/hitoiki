// 今日: 実績の列のクリックで区間をサイドパネルに開く → 実績の列のドラッグで付け替えのパネル → 付け替える → 取り消す。
import { dragLane, mockState, until } from "../lib/flow.mjs";

export default {
  name: "today-actual",
  scenario: "default",
  screen: "today",
  async run({ page }) {
    const panel = page.getByRole("complementary", { name: "詳細" });

    // 区間をクリック（いちばん高い区間）→ 区間のパネル（確定ボタン・実績で開くリンク）。
    const items = page.locator(".vtl-col").filter({ has: page.getByText("実績", { exact: true }) }).locator(".vtl-item.seg");
    await items.first().waitFor({ timeout: 5000 });
    let best = null;
    for (let i = 0; i < (await items.count()); i++) {
      const b = await items.nth(i).boundingBox();
      if (b && (!best || b.height > best.height)) best = b;
    }
    await page.mouse.click(best.x + best.width / 2, best.y + best.height / 2);
    await panel.getByRole("button", { name: /^確定/ }).waitFor({ timeout: 5000 });
    await panel.getByRole("link", { name: "実績で開く" }).waitFor();
    await page.locator(".vtl-item.seg.selected").first().waitFor();
    await panel.getByRole("button", { name: "閉じる" }).click();
    await panel.waitFor({ state: "detached", timeout: 5000 });

    // ドラッグ → 付け替えのパネル。
    const before = await mockState(page, (s) => s.corrections.length);
    await dragLane(page, "実績", 0.13, 0.2);
    await panel.getByRole("heading", { name: /^実績の付け替え/, level: 2 }).waitFor({ timeout: 5000 });
    await panel.getByRole("combobox", { name: /割り当て先/ }).selectOption({ index: 2 });
    await panel.getByRole("button", { name: "付け替える", exact: true }).click();
    await panel.getByText("付け替えました。").waitFor({ timeout: 5000 });
    await until(async () => (await mockState(page, (s) => s.corrections.length)) === before + 1, "補正がモックの状態に入りません");

    // 取り消す。
    await panel.getByRole("button", { name: "取り消す", exact: true }).click();
    await until(async () => (await mockState(page, (s) => s.corrections.length)) === before, "補正が取り消されません");
    await panel.getByRole("button", { name: "閉じる" }).first().click();
    await panel.waitFor({ state: "detached", timeout: 5000 });
  },
};
