// 目標: Objective と KR を追加 → 選択状態になる（URL に ID）→ 別の KR を選ぶとハッシュが変わり、戻ると前の選択に戻る
// → スキルのタブでスキルを選ぶと、そのスキルが付いたタスクがリンクで出る。
import { mockState, until } from "../lib/flow.mjs";

const hashOf = (page) => new URL(page.url()).hash;

export default {
  name: "okr",
  scenario: "default",
  screen: "okr",
  async run({ page }) {
    // Objective を追加（「＋ Objective」→ タイトル → Enter）。作ったものが選択される。
    await page.getByRole("button", { name: "＋ Objective", exact: true }).click();
    await page.getByRole("textbox", { name: "新しい Objective" }).fill("操作シナリオの Objective");
    await page.getByRole("textbox", { name: "新しい Objective" }).press("Enter");
    await until(() => mockState(page, (s) => s.objectives.some((o) => o.title === "操作シナリオの Objective")), "Objective が状態に入りません");
    await page.getByRole("heading", { name: "操作シナリオの Objective", level: 2 }).waitFor({ timeout: 5000 });
    const oid = await mockState(page, (s) => s.objectives.find((o) => o.title === "操作シナリオの Objective").id);
    await until(() => hashOf(page) === `#/okr/o/${oid}`, "Objective が URL に載りません");

    // KR を追加（詳細の「＋ KR」）。追加した KR の詳細へ移る。
    await page.getByRole("button", { name: "＋ KR", exact: true }).click();
    await page.getByRole("textbox", { name: "Key Result" }).fill("操作シナリオの KR");
    await page.getByRole("textbox", { name: "目標" }).fill("週 3 回");
    await page.getByRole("textbox", { name: "目標" }).press("Enter");
    await until(
      () =>
        mockState(page, (s) => {
          const o = s.objectives.find((x) => x.title === "操作シナリオの Objective");
          return s.krs.some((k) => k.title === "操作シナリオの KR" && k.objective_id === o.id && k.target === "週 3 回");
        }),
      "KR が状態に入りません",
    );
    await page.getByRole("heading", { name: "操作シナリオの KR", level: 2 }).waitFor({ timeout: 5000 });
    const kid = await mockState(page, (s) => s.krs.find((k) => k.title === "操作シナリオの KR").id);
    await until(() => hashOf(page) === `#/okr/kr/${kid}`, "KR が URL に載りません");

    // 別の KR を選ぶとハッシュが変わり、戻ると前の KR に戻る。
    await page.getByRole("button", { name: /Rust の学習に週 5 時間使う/ }).click();
    await page.getByRole("heading", { name: "Rust の学習に週 5 時間使う", level: 2 }).waitFor();
    await until(() => hashOf(page) !== `#/okr/kr/${kid}` && hashOf(page).startsWith("#/okr/kr/"), "選択が URL に載りません");
    await page.goBack();
    await page.getByRole("heading", { name: "操作シナリオの KR", level: 2 }).waitFor();
    await until(() => hashOf(page) === `#/okr/kr/${kid}`, "戻っても選択が戻りません");

    // スキルのタブ: 階層の一覧から選ぶと、そのスキルが付いたタスクへのリンクが出る。
    await page.getByRole("tab", { name: "スキル" }).click();
    await until(() => hashOf(page) === "#/okr?tab=skills", "スキルのタブが URL に載りません");
    await page.getByRole("button", { name: "Rust", exact: true }).click();
    await page.getByRole("heading", { name: "Rust", level: 2 }).waitFor();
    await page.getByRole("link", { name: "Rust 所有権の復習" }).waitFor();
  },
};
