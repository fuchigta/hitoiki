// 記録（振り返り）: 「＋ 記録」→ 終業の振り返り → 問いを生成（モックの AI 応答）→ 選択された詳細で回答 → 保存
// → 回答で作られたハッスルが記録の一覧にも出る。旧い入口（#/reflect/day_end）も作成フォームに写る。
import { mockState, until } from "../lib/flow.mjs";

const hashOf = (page) => new URL(page.url()).hash;

export default {
  name: "records-reflect",
  scenario: "default",
  screen: "records",
  async run({ page }) {
    await page.getByRole("button", { name: "＋ 記録", exact: true }).click();
    await page.getByRole("menuitem", { name: "終業の振り返り", exact: true }).click();
    await until(() => hashOf(page) === "#/records/new/day_end", "振り返りの作成フォームのルートになりません");
    await page.getByRole("heading", { name: "終業の振り返りを始める", level: 2 }).waitFor();

    await page.getByRole("button", { name: "問いを生成", exact: true }).click();
    await until(() => mockState(page, (s) => s.reflections.some((r) => r.kind === "day_end" && r.answers_json == null && r.at === s.now)), "振り返りが作られません");
    const id = await mockState(page, (s) => s.reflections.find((r) => r.kind === "day_end" && r.at === s.now).id);
    await until(() => hashOf(page) === `#/records/reflection/${id}`, "作った振り返りが選択されません");
    await page.getByRole("heading", { name: "終業の振り返り", level: 2 }).waitFor();

    // 回答（ハッスルの問い）を保存する。
    const q = page.getByRole("group", { name: "問い 2" });
    await q.getByRole("textbox", { name: "気になったこと" }).fill("振り返りで書いたハッスル");
    await page.getByRole("button", { name: "回答を保存", exact: true }).click();
    await until(
      () => mockState(page, (s) => s.hassles.some((h) => h.text === "振り返りで書いたハッスル") && !!s.reflections[0].answers_json),
      "回答がモックの状態に入りません",
    );
    await page.getByText("回答を保存しました").waitFor();

    // 回答で作られたハッスルが、一覧にも出る。振り返りの行は「回答あり」になる。
    const list = page.getByRole("list", { name: "記録の時系列" });
    await list.getByRole("button", { name: /振り返りで書いたハッスル/ }).waitFor();
    await list.getByRole("button", { name: /終業の振り返り・回答あり/ }).first().waitFor();

    // 旧い入口は、作成フォームに写る（履歴は増やさず置き換える）。
    await page.evaluate(() => (location.hash = "#/reflect/weekly"));
    await until(() => hashOf(page) === "#/records/new/weekly", "旧い入口が作成フォームに写りません");
    await page.getByRole("heading", { name: "週次の振り返りを始める", level: 2 }).waitFor();
  },
};
