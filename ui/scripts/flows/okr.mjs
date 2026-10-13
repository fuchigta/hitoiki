// OKR: Objective と KR を追加 → 表示される。
import { mockState, until } from "../lib/flow.mjs";

export default {
  name: "okr",
  scenario: "default",
  screen: "okr",
  async run({ page }) {
    // Objective を追加
    await page.getByRole("textbox", { name: "新しい Objective" }).fill("操作シナリオの Objective");
    await page.getByRole("textbox", { name: "新しい Objective" }).press("Enter");
    await until(() => mockState(page, (s) => s.objectives.some((o) => o.title === "操作シナリオの Objective")), "Objective が状態に入りません");
    await page.getByText("操作シナリオの Objective").first().waitFor({ timeout: 5000 });

    // KR を追加（新しい Objective は一覧の最後なので、最後の KR フォームを使う）
    await page.getByRole("textbox", { name: "Key Result" }).last().fill("操作シナリオの KR");
    await page.getByRole("textbox", { name: "目標" }).last().fill("週 3 回");
    await page.getByRole("textbox", { name: "目標" }).last().press("Enter");
    await until(
      () =>
        mockState(page, (s) => {
          const o = s.objectives.find((x) => x.title === "操作シナリオの Objective");
          return s.krs.some((k) => k.title === "操作シナリオの KR" && k.objective_id === o.id && k.target === "週 3 回");
        }),
      "KR が状態に入りません",
    );
    await page.getByText("操作シナリオの KR").first().waitFor({ timeout: 5000 });
  },
};
