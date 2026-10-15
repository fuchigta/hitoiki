// 実績: 複数の区間にチェックを付ける → パネルが「N 件を選択中」になる → まとめて確定 → 一覧から消える。
import { mockState, until } from "../lib/flow.mjs";

const pendingCount = (page) => mockState(page, (s) => s.segments.filter((x) => x.status === "needs_review").length);

export default {
  name: "actual-many",
  scenario: "default",
  screen: "actual",
  async run({ page }) {
    const rows = page.getByRole("list", { name: "区間" }).locator("li.object-row");
    const before = await rows.count();
    if (before < 3) throw new Error("要確認の区間が 3 件以上ありません。");
    const pendingBefore = await pendingCount(page);

    await rows.nth(0).getByRole("checkbox", { name: "この区間を選ぶ" }).check();
    await rows.nth(1).getByRole("checkbox", { name: "この区間を選ぶ" }).check();
    await page.getByRole("heading", { name: "2 件を選択中" }).waitFor({ timeout: 5000 });

    const picker = page.getByRole("combobox", { name: "タスク" });
    await picker.click();
    await picker.fill("Rust 所有権");
    await page.getByRole("option", { name: /Rust 所有権の復習/ }).click();
    await page.getByRole("button", { name: "2 件を確定" }).click();

    await until(async () => (await rows.count()) === before - 2, "まとめて確定した区間が一覧から消えません");
    if ((await pendingCount(page)) !== pendingBefore - 2) throw new Error("モックの要確認の件数が 2 件減っていません。");
    const assigned = await mockState(page, (s) => {
      const t = s.tasks.find((x) => x.title === "Rust 所有権の復習");
      return s.segments.filter((x) => x.task_id === t.id && x.decided_by === "confirmation").length;
    });
    if (assigned < 2) throw new Error("確定した 2 件にタスクが付いていません。");
  },
};
