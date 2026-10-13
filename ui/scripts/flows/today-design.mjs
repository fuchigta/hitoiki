// 今日: 「デザイン」でサイドパネルを開く → テンプレートを適用 → 取り消す → 完了。
// `#/today/design`（始業の通知の本体クリックと同じ入口）でも開く。
import { mockState, until } from "../lib/flow.mjs";

const hashOf = (page) => new URL(page.url()).hash;

export default {
  name: "today-design",
  scenario: "default",
  screen: "today",
  async run({ page }) {
    const panel = page.getByRole("complementary", { name: "詳細" });
    const blocksBefore = await mockState(page, (s) => s.blocks.length);

    await page.getByRole("button", { name: "デザイン", exact: true }).click();
    await panel.getByRole("heading", { name: "今日のデザイン", level: 2 }).waitFor({ timeout: 5000 });
    await panel.getByRole("link", { name: "テンプレートを管理" }).waitFor();
    await panel.getByRole("button", { name: "この日の配置をテンプレートとして保存" }).waitFor();

    // テンプレートの適用（重ねて作る）→ ブロックが増える → 取り消す。
    await panel.getByRole("combobox", { name: "重なるとき" }).selectOption("overlay");
    await panel.getByRole("button", { name: /件を並べる$/ }).click();
    await until(async () => (await mockState(page, (s) => s.blocks.length)) > blocksBefore, "テンプレートのブロックが増えません");
    await panel.getByRole("button", { name: "直前の適用を取り消す" }).click();
    await until(async () => (await mockState(page, (s) => s.blocks.length)) === blocksBefore, "適用が取り消されません");

    // ブロックを開いて閉じると、デザインのパネルに戻る。
    await page.getByRole("button", { name: /認証まわりの設計書ドラフト/ }).click();
    await panel.getByRole("heading", { name: "ブロックを編集", level: 2 }).waitFor({ timeout: 5000 });
    await panel.getByRole("button", { name: "閉じる", exact: true }).click();
    await panel.getByRole("heading", { name: "今日のデザイン", level: 2 }).waitFor({ timeout: 5000 });

    // 完了で閉じる。
    await panel.getByRole("button", { name: "完了", exact: true }).click();
    await panel.waitFor({ state: "detached", timeout: 5000 });

    // `#/today/design` で開く → 完了で `#/today` に戻る。
    await page.evaluate(() => (location.hash = "#/today/design"));
    await panel.getByRole("heading", { name: "今日のデザイン", level: 2 }).waitFor({ timeout: 5000 });
    await panel.getByRole("button", { name: "完了", exact: true }).click();
    await until(() => hashOf(page) === "#/today", "完了しても #/today に戻りません");
    await panel.waitFor({ state: "detached", timeout: 5000 });
  },
};
