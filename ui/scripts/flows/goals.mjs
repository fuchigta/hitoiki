// 目標: 目標と成果の指標を追加 → 選択状態になる（URL に ID）→ 別の成果の指標を選ぶとハッシュが変わり、戻ると前の選択に戻る
// → スキルのタブでスキルを選ぶと、そのスキルが付いたタスクがリンクで出る。
import { mockState, until } from "../lib/flow.mjs";

const hashOf = (page) => new URL(page.url()).hash;

export default {
  name: "goals",
  scenario: "default",
  screen: "goals",
  async run({ page }) {
    // 目標を追加（「＋ 目標」→ タイトル → Enter）。作ったものが選択される。
    await page.getByRole("button", { name: "＋ 目標", exact: true }).click();
    await page.getByRole("textbox", { name: "新しい目標" }).fill("操作シナリオの目標");
    await page.getByRole("textbox", { name: "新しい目標" }).press("Enter");
    await until(() => mockState(page, (s) => s.goals.some((o) => o.title === "操作シナリオの目標")), "目標が状態に入りません");
    await page.getByRole("heading", { name: "操作シナリオの目標", level: 2 }).waitFor({ timeout: 5000 });
    const oid = await mockState(page, (s) => s.goals.find((o) => o.title === "操作シナリオの目標").id);
    await until(() => hashOf(page) === `#/goals/goal/${oid}`, "目標が URL に載りません");

    // 成果の指標を追加（詳細の「＋成果の指標」）。追加した成果の指標の詳細へ移る。
    await page.getByRole("button", { name: "＋ 成果の指標", exact: true }).click();
    await page.getByRole("textbox", { name: "成果の指標" }).fill("操作シナリオの成果の指標");
    await page.getByRole("textbox", { name: "目標値" }).fill("週 3 回");
    await page.getByRole("textbox", { name: "目標値" }).press("Enter");
    await until(
      () =>
        mockState(page, (s) => {
          const o = s.goals.find((x) => x.title === "操作シナリオの目標");
          return s.keyResults.some((k) => k.title === "操作シナリオの成果の指標" && k.goal_id === o.id && k.target === "週 3 回");
        }),
      "成果の指標が状態に入りません",
    );
    await page.getByRole("heading", { name: "操作シナリオの成果の指標", level: 2 }).waitFor({ timeout: 5000 });
    const kid = await mockState(page, (s) => s.keyResults.find((k) => k.title === "操作シナリオの成果の指標").id);
    await until(() => hashOf(page) === `#/goals/key-result/${kid}`, "成果の指標が URL に載りません");

    // 別の成果の指標を選ぶとハッシュが変わり、戻ると前の成果の指標に戻る。
    await page.getByRole("button", { name: /Rust の学習に週 5 時間使う/ }).click();
    await page.getByRole("heading", { name: "Rust の学習に週 5 時間使う", level: 2 }).waitFor();
    await until(() => hashOf(page) !== `#/goals/key-result/${kid}` && hashOf(page).startsWith("#/goals/key-result/"), "選択が URL に載りません");
    await page.goBack();
    await page.getByRole("heading", { name: "操作シナリオの成果の指標", level: 2 }).waitFor();
    await until(() => hashOf(page) === `#/goals/key-result/${kid}`, "戻っても選択が戻りません");

    // スキルのタブ: 階層の一覧から選ぶと、そのスキルが付いたタスクへのリンクが出る。
    await page.getByRole("tab", { name: "スキル" }).click();
    await until(() => hashOf(page) === "#/goals?tab=skills", "スキルのタブが URL に載りません");
    await page.getByRole("button", { name: "Rust", exact: true }).click();
    await page.getByRole("heading", { name: "Rust", level: 2 }).waitFor();
    await page.getByRole("link", { name: "Rust 所有権の復習" }).waitFor();
  },
};
