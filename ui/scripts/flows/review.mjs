// 仕分け: 要確認の区間にタスクを割り当てて確定 → 一覧から消える。
import { mockState, until } from "../lib/flow.mjs";

const pendingCount = (page) => mockState(page, (s) => s.segments.filter((x) => x.status === "needs_review").length);

export default {
  name: "review",
  scenario: "default",
  screen: "review",
  async run({ page }) {
    const cards = page.locator("section.card").filter({ has: page.getByRole("checkbox", { name: "この区間を選ぶ" }) });
    const before = await cards.count();
    if (before === 0) throw new Error("要確認の区間が 1 件もありません。");
    const pendingBefore = await pendingCount(page);

    const card = cards.first();
    const title = await card.locator(".review-title").first().innerText();
    await card.getByRole("combobox", { name: "タスク" }).selectOption({ label: "Rust 所有権の復習" });
    await card.getByRole("button", { name: "確定", exact: true }).click();

    // 画面: 確定した区間のカードが消える。
    await until(async () => (await cards.count()) === before - 1, "確定した区間が一覧から消えません");
    await until(async () => (await page.locator("section.card .review-title", { hasText: title }).count()) === 0, `「${title}」が一覧に残っています`);
    // 状態: 要確認が 1 件減り、「Rust 所有権の復習」がユーザー確定の区間に付いている。
    if ((await pendingCount(page)) !== pendingBefore - 1) throw new Error("モックの要確認の件数が 1 件減っていません。");
    const assigned = await mockState(page, (s) => {
      const t = s.tasks.find((x) => x.title === "Rust 所有権の復習");
      return s.segments.filter((x) => x.task_id === t.id && x.decided_by === "user").length;
    });
    if (assigned < 1) throw new Error("確定した区間にタスクが付いていません。");
  },
};
