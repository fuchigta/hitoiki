// 今日: 日付の帯で前の日へ移る（URL に ?date=）→ その日の合計が出る → ブラウザの戻る・進む → 「今日」で戻る。
import { until } from "../lib/flow.mjs";

const hashOf = (page) => new URL(page.url()).hash;

export default {
  name: "today-date",
  scenario: "default",
  screen: "today",
  async run({ page }) {
    const today = page.getByRole("button", { name: "今日", exact: true });
    if (!(await today.isDisabled())) throw new Error("今日を見ているのに「今日」のボタンが有効です。");

    await page.getByRole("button", { name: "前の日" }).click();
    await until(() => /^#\/today\?date=\d{4}-\d{2}-\d{2}$/.test(hashOf(page)), "前日の日付が URL に載りません");
    const prev = hashOf(page);
    await page.getByText(/ の合計$/).waitFor({ timeout: 5000 });
    if (await today.isDisabled()) throw new Error("前日を見ているのに「今日」のボタンが無効です。");
    // 前日は実績が出る（モックの前日には記録がある）。
    await page.locator(".vtl-item.seg").first().waitFor({ timeout: 5000 });

    // ブラウザの戻る → 今日、進む → 前日。
    await page.goBack();
    await until(() => hashOf(page) === "#/today" || hashOf(page) === "", "戻っても今日になりません");
    await page.getByText("今日の合計", { exact: true }).waitFor({ timeout: 5000 });
    await page.goForward();
    await until(() => hashOf(page) === prev, "進んでも前日になりません");

    // 「今日」で戻る。
    await today.click();
    await until(() => hashOf(page) === "#/today", "「今日」を押しても今日に戻りません");
    await page.getByText("今日の合計", { exact: true }).waitFor({ timeout: 5000 });
  },
};
