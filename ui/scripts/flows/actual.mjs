// 実績: 要確認の区間を選ぶ → TaskPicker で割り当て → 確定 → 次の区間が選ばれ、件数バッジが減る。
import { mockState, until } from "../lib/flow.mjs";

const pendingCount = (page) => mockState(page, (s) => s.segments.filter((x) => x.status === "needs_review").length);

export default {
  name: "actual",
  scenario: "default",
  screen: "actual",
  async run({ page }) {
    const rows = page.getByRole("list", { name: "区間" }).locator("li.object-row");
    const before = await rows.count();
    if (before < 2) throw new Error("要確認の区間が 2 件以上ありません。");
    const pendingBefore = await pendingCount(page);
    const badge = page.locator(".nav-badge");
    const badgeBefore = Number(await badge.innerText());

    const first = rows.first();
    const title = await first.locator(".object-title").innerText();
    await first.locator("button.object-main").click();

    // 右のパネルで、TaskPicker からタスクを選んで確定する。
    const picker = page.getByRole("combobox", { name: "タスク" });
    await picker.click();
    await picker.fill("Rust 所有権");
    await page.getByRole("option", { name: /Rust 所有権の復習/ }).click();
    await page.getByRole("button", { name: "確定", exact: true }).click();

    // 画面: 確定した区間が一覧から消え、次の要確認の区間が選ばれている。
    await until(async () => (await rows.count()) === before - 1, "確定した区間が一覧から消えません");
    await until(async () => (await rows.locator(".object-title", { hasText: title }).count()) === 0 || (await rows.count()) < before, `「${title}」が一覧に残っています`);
    await until(async () => (await rows.locator("button.object-main[aria-pressed=true]").count()) === 1, "次の要確認の区間が選ばれていません");
    // 件数バッジが 1 減る。
    await until(async () => Number(await badge.innerText()) === badgeBefore - 1, "ナビの件数バッジが減りません");
    // 状態: 要確認が 1 件減り、タスクがユーザー確定の区間に付いている。
    if ((await pendingCount(page)) !== pendingBefore - 1) throw new Error("モックの要確認の件数が 1 件減っていません。");
    const assigned = await mockState(page, (s) => {
      const t = s.tasks.find((x) => x.title === "Rust 所有権の復習");
      return s.segments.filter((x) => x.task_id === t.id && x.decided_by === "confirmation").length;
    });
    if (assigned < 1) throw new Error("確定した区間にタスクが付いていません。");
  },
};
