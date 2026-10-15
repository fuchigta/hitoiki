// 設定: タブの切り替え（URL に載る）→ ルールタブで活動ルールを追加 → 一覧に出る → 無効にする → 状態が変わる
// → テンプレートタブでテンプレートを作る。
import { gotoScreen, mockState, until } from "../lib/flow.mjs";

const hashOf = (page) => new URL(page.url()).hash;

export default {
  name: "settings",
  scenario: "default",
  screen: "settings",
  async run({ page }) {
    // 既定は「全般」。タブを選ぶと URL に載る。
    await page.getByRole("tab", { name: "全般", selected: true }).waitFor();
    await page.getByRole("tab", { name: "ルール", exact: true }).click();
    await until(() => hashOf(page) === "#/settings/rules", "タブが URL に載りません");

    // 活動ルールを追加（「＋ ルール」→ 種類は活動が既定）。
    await page.getByRole("button", { name: "＋ ルール", exact: true }).click();
    await page.getByRole("combobox", { name: "活動", exact: true }).fill("操作確認");
    await page.getByRole("textbox", { name: "パターン" }).fill("flowtest*");
    await page.getByRole("button", { name: "追加", exact: true }).click();
    await until(
      () => mockState(page, (s) => s.activityRules.some((r) => r.pattern === "flowtest*" && r.activity === "操作確認" && r.enabled)),
      "活動ルールがモックの状態に増えません",
    );
    // 追加すると一覧に出て、選択される。
    await page.getByRole("button", { name: /^アプリ（実行ファイル名） ワイルドカード（\* と \?） flowtest\*/ }).waitFor();
    await page.getByRole("heading", { name: "ルールの編集", level: 3 }).waitFor();

    // 種類で絞り込む（活動だけ → タスクは消える）。
    await page.getByRole("button", { name: "活動", exact: true }).click();
    await page.getByRole("button", { name: /^アプリ（実行ファイル名） 含む Teams\.exe/ }).waitFor();
    await page.getByRole("button", { name: "タスク", exact: true }).click();
    await page.getByRole("button", { name: /^アプリ（実行ファイル名） ワイルドカード（\* と \?） flowtest\*/ }).waitFor({ state: "detached" });
    await page.getByRole("button", { name: "すべて", exact: true }).click();

    // 行のメニューから無効にする。状態が変わり、行に「無効」が付く。
    await page.getByRole("button", { name: "ルール「flowtest*」の操作" }).click();
    await page.getByRole("menuitem", { name: "無効にする" }).click();
    await until(
      () => mockState(page, (s) => s.activityRules.some((r) => r.pattern === "flowtest*" && r.enabled === false)),
      "ルールが無効になりません",
    );
    await page.getByRole("button", { name: /^アプリ（実行ファイル名） ワイルドカード（\* と \?） flowtest\*.*無効/ }).waitFor();

    // テンプレートタブで作成。
    await page.getByRole("tab", { name: "テンプレート", exact: true }).click();
    await until(() => hashOf(page) === "#/settings/templates", "テンプレートのタブが URL に載りません");
    await page.getByRole("button", { name: "テンプレートを作る" }).click();
    await page.getByRole("textbox", { name: "名前" }).fill("操作確認の日");
    await page.getByLabel("開始").first().fill("09:00");
    await page.getByLabel("終了").first().fill("10:00");
    await page.getByRole("textbox", { name: "ラベル" }).fill("定例");
    await page.getByRole("button", { name: "保存" }).click();
    await until(
      () => mockState(page, (s) => s.templates.some((t) => t.name === "操作確認の日")),
      "テンプレートがモックの状態に増えません",
    );
    await page.getByText("操作確認の日").first().waitFor();

    // 旧来の入口: ナビの「設定」からも開ける。
    await gotoScreen(page, "設定");
    await page.getByRole("tab", { name: "テンプレート", exact: true }).waitFor();
  },
};
