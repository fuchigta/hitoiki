// タスク: 「＋ タスク」で作成 → 選択状態になる（URL に ID）→ 詳細を編集・保存 → タブ切り替え → 状態を「始める」
// → 別の行を選ぶとハッシュが変わり、戻ると前の選択に戻る → 「…」メニューから削除。
import { mockState, until } from "../lib/flow.mjs";

const hashOf = (page) => new URL(page.url()).hash;

export default {
  name: "tasks",
  scenario: "default",
  screen: "tasks",
  async run({ page }) {
    // 新規作成（「＋ タスク」→ タイトル → Enter）。作ったタスクが選択され、URL に ID が載る。
    await page.getByRole("button", { name: "＋ タスク", exact: true }).click();
    await page.getByRole("textbox", { name: "新しいタスクのタイトル" }).fill("操作シナリオ用のタスク");
    await page.getByRole("textbox", { name: "新しいタスクのタイトル" }).press("Enter");
    await page.getByRole("heading", { name: "操作シナリオ用のタスク", level: 2 }).waitFor({ timeout: 5000 });
    const created = await mockState(page, (s) => s.tasks.find((t) => t.title === "操作シナリオ用のタスク").id);
    await until(() => hashOf(page) === `#/tasks/${created}`, "作成したタスクが URL に載りません");

    // 概要タブで編集・保存
    await page.getByRole("textbox", { name: "タイトル" }).fill("操作シナリオ用のタスク（編集後）");
    await page.getByRole("textbox", { name: "見積（分）" }).fill("45");
    await page.getByRole("button", { name: "保存", exact: true }).click();
    await until(
      () => mockState(page, (s) => s.tasks.some((t) => t.title === "操作シナリオ用のタスク（編集後）" && t.estimate_min === 45)),
      "編集がモックの状態に反映されません",
    );
    await page.getByRole("button", { name: /操作シナリオ用のタスク（編集後）/ }).waitFor();

    // タブ（時間・ルール）。URL の ?tab= に載り、概要へ戻れる。
    await page.getByRole("tab", { name: "ルール" }).click();
    await until(() => hashOf(page) === `#/tasks/${created}?tab=rules`, "タブが URL に載りません");
    await page.getByRole("button", { name: "ルール追加" }).waitFor();
    await page.getByRole("tab", { name: "時間" }).click();
    await page.getByRole("heading", { name: "補正した時間帯", level: 3 }).waitFor();
    await page.getByRole("tab", { name: "概要" }).click();
    await until(() => hashOf(page) === `#/tasks/${created}`, "概要へ戻れません");

    // 状態を「始める」（primary）
    await page.getByRole("button", { name: "始める", exact: true }).click();
    await until(
      () => mockState(page, (s) => s.tasks.some((t) => t.title === "操作シナリオ用のタスク（編集後）" && t.status === "doing")),
      "タスクが進行中になりません",
    );
    await page.getByRole("button", { name: "完了にする", exact: true }).waitFor();

    // 別の行を選ぶとハッシュが変わり、ブラウザの「戻る」で前の選択に戻る。
    const other = page.getByRole("button", { name: /週次報告の資料作成/ });
    await other.click();
    await page.getByRole("heading", { name: "週次報告の資料作成", level: 2 }).waitFor();
    await until(() => hashOf(page) !== `#/tasks/${created}` && hashOf(page).startsWith("#/tasks/"), "選択が URL に載りません");
    await page.goBack();
    await page.getByRole("heading", { name: "操作シナリオ用のタスク（編集後）", level: 2 }).waitFor();
    await until(() => hashOf(page) === `#/tasks/${created}`, "戻っても選択が戻りません");

    // 「…」メニュー（キーボードで開く）から削除 → 確認ダイアログで「削除する」。
    const menuBtn = page.getByRole("button", { name: "タスクの操作" });
    await menuBtn.focus();
    await menuBtn.press("Enter");
    await page.getByRole("menuitem", { name: "削除…" }).waitFor();
    // 開いたら先頭の項目にフォーカスが移る。「やめる」が先頭なので、↓ で「削除…」へ動かしてから選ぶ。
    await page.waitForFunction(() => document.activeElement?.getAttribute("role") === "menuitem");
    await page.keyboard.press("ArrowDown");
    await page.keyboard.press("Enter");
    await page.getByRole("alertdialog").getByRole("button", { name: "削除する", exact: true }).click();
    await until(
      () => mockState(page, (s) => !s.tasks.some((t) => t.title === "操作シナリオ用のタスク（編集後）")),
      "タスクが削除されません",
    );
    await until(() => hashOf(page) === "#/tasks", "削除後に選択が外れません");
  },
};
