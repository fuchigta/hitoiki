// 戻る・進むのガード: タスクを編集中（未保存）にブラウザの「戻る」→ 元のハッシュに戻して確認が出る
// → キャンセルでその場に留まり（入力は残る）、「破棄して移動」で移る。
import { gotoScreen, until } from "../lib/flow.mjs";

const hashOf = (page) => new URL(page.url()).hash;

export default {
  name: "leave-guard",
  scenario: "default",
  screen: "today",
  async run({ page }) {
    await gotoScreen(page, "タスク");
    await page.getByRole("button", { name: /週次報告の資料作成/ }).click();
    await page.getByRole("heading", { name: "週次報告の資料作成", level: 2 }).waitFor();
    const detail = hashOf(page);
    if (!/^#\/tasks\/.+/.test(detail)) throw new Error("タスクの選択が URL に載りません。");

    const title = page.getByRole("textbox", { name: "タイトル" });
    await title.fill("週次報告の資料作成（未保存）");
    await page.waitForTimeout(300); // 未保存の判定（effect）が反映されるのを待つ

    // 戻る → 確認が出て、ハッシュは元のまま。
    await page.goBack();
    const dialog = page.getByRole("alertdialog");
    await dialog.waitFor({ timeout: 5000 });
    await until(() => hashOf(page) === detail, "戻ったのにハッシュが元に戻りません");

    // キャンセル → その場に留まり、入力は残る。
    await dialog.getByRole("button", { name: "キャンセル" }).click();
    await dialog.waitFor({ state: "detached" });
    if ((await title.inputValue()) !== "週次報告の資料作成（未保存）") throw new Error("キャンセルしたのに入力が消えました。");
    if (hashOf(page) !== detail) throw new Error("キャンセルしたのにハッシュが変わりました。");

    // もう一度戻る → 破棄して移動。
    await page.goBack();
    await dialog.waitFor({ timeout: 5000 });
    await dialog.getByRole("button", { name: "破棄して移動" }).click();
    await until(() => hashOf(page) !== detail, "破棄して移動してもハッシュが変わりません");
  },
};
