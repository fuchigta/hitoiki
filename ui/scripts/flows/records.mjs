// 記録: 「＋ 記録」→ チェックイン → 保存 → 一覧の先頭に出て選択される（URL に ID）→ 種類で絞り込む → ハッスルを「…」から編集・削除。
import { mockState, until } from "../lib/flow.mjs";

const hashOf = (page) => new URL(page.url()).hash;

export default {
  name: "records",
  scenario: "default",
  screen: "records",
  async run({ page }) {
    const list = page.getByRole("list", { name: "記録の時系列" });

    // 「＋ 記録」のメニューから種類を選ぶと、右に作成フォームが出る。
    await page.getByRole("button", { name: "＋ 記録", exact: true }).click();
    await page.getByRole("menuitem", { name: "チェックイン", exact: true }).click();
    await until(() => hashOf(page) === "#/records/new/checkin", "作成フォームのルートになりません");
    await page.getByRole("heading", { name: "チェックインを記録", level: 2 }).waitFor();

    const form = page.locator("section.card").filter({ has: page.getByRole("heading", { name: "チェックインを記録", level: 2 }) });
    await form.getByRole("radiogroup", { name: "エネルギー" }).getByRole("radio", { name: "4", exact: true }).click();
    await form.getByRole("radiogroup", { name: "気分" }).getByRole("radio", { name: "3", exact: true }).click();
    await form.getByRole("textbox", { name: "一言メモ" }).fill("操作シナリオのメモ");
    await form.getByRole("button", { name: "記録", exact: true }).click();

    await until(
      () => mockState(page, (s) => s.checkins.some((c) => c.note === "操作シナリオのメモ" && c.energy === 4 && c.mood === 3)),
      "チェックインがモックの状態に入りません",
    );
    const id = await mockState(page, (s) => s.checkins.find((c) => c.note === "操作シナリオのメモ").id);

    // 作った記録が選択され（URL に ID）、一覧の先頭に出て、右に詳細が出る。
    await until(() => hashOf(page) === `#/records/checkin/${id}`, "作った記録が選択されません");
    await page.getByRole("heading", { name: "チェックイン", level: 2 }).waitFor();
    await page.locator(".ld-detail").getByText("操作シナリオのメモ").waitFor();
    const first = list.getByRole("listitem").first();
    await first.getByRole("button", { name: /エネルギー 4・気分 3/ }).waitFor();
    await until(async () => (await first.getByRole("button").first().getAttribute("aria-pressed")) === "true", "先頭の行が選択状態になりません");

    // 種類で絞り込む（URL の ?kind=）。ハッスルだけにするとチェックインの行は消える。
    await page.getByRole("group", { name: "種類" }).getByRole("button", { name: "ハッスル", exact: true }).click();
    await until(() => hashOf(page).includes("kind=hassle"), "絞り込みが URL に載りません");
    await until(async () => (await list.getByRole("button", { name: /エネルギー 4・気分 3/ }).count()) === 0, "チェックインの行が残っています");
    const row = list.getByRole("listitem").first();
    await row.getByRole("button").first().click();
    await page.getByRole("heading", { name: "ハッスル", level: 2 }).waitFor();

    // 詳細の「…」から編集（その場でフォームに切り替わる）→ 保存。
    await page.getByRole("button", { name: "ハッスルの操作" }).click();
    await page.getByRole("menuitem", { name: "編集", exact: true }).click();
    await page.getByRole("textbox", { name: "気になったこと" }).fill("編集後のハッスル");
    await page.getByRole("button", { name: "保存", exact: true }).click();
    await until(() => mockState(page, (s) => s.hassles.some((h) => h.text === "編集後のハッスル")), "編集がモックの状態に反映されません");
    await page.locator(".ld-detail").getByText("編集後のハッスル").waitFor();

    // 削除（確認ダイアログ）。一覧から消え、選択が外れる。
    await page.getByRole("button", { name: "ハッスルの操作" }).click();
    await page.getByRole("menuitem", { name: "削除…", exact: true }).click();
    await page.getByRole("button", { name: "削除する", exact: true }).click();
    await until(() => mockState(page, (s) => !s.hassles.some((h) => h.text === "編集後のハッスル")), "ハッスルが削除されません");
    await until(() => !hashOf(page).includes("/hassle/"), "選択が外れません");
  },
};
