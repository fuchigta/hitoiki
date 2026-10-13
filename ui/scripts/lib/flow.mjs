// 操作シナリオ（scripts/flows/*.mjs）の共通部品。
// 要素は getByRole と accessible name で選ぶ。結果はモックの状態（window.__mock.state）か画面で確かめる。

/** 条件が真になるまで待つ（最大 timeout ms）。ならなければ message で失敗する。 */
export async function until(fn, message, timeout = 5000) {
  const t0 = Date.now();
  let last;
  for (;;) {
    try {
      last = await fn();
      if (last) return last;
    } catch (e) {
      last = e;
    }
    if (Date.now() - t0 > timeout) throw new Error(`${message}（${timeout}ms 待ちました）`);
    await new Promise((r) => setTimeout(r, 50));
  }
}

/** モックの状態のうち、selector で取り出した部分を JSON として得る（毎回新しく読む）。 */
export function mockState(page, selector) {
  return page.evaluate((src) => {
    // eslint-disable-next-line no-new-func
    const pick = new Function("s", `return (${src})(s)`);
    return JSON.parse(JSON.stringify(pick(window.__mock.state)));
  }, selector.toString());
}

/** nav の「画面」リンクから画面を開く（ハッシュを直接触らず、利用者と同じ経路で移る）。 */
export async function gotoScreen(page, label) {
  await page.getByRole("navigation", { name: "画面" }).getByRole("link", { name: label, exact: true }).click();
}

/** 計画列の縦ドラッグ。 */
export function dragPlanLane(page, fromFrac, toFrac) {
  return dragLane(page, "計画", fromFrac, toFrac);
}

/** 1 日のタイムラインの列（予定・計画・実績）の縦ドラッグ。frac は列の高さに対する割合（0〜1）。 */
export async function dragLane(page, name, fromFrac, toFrac) {
  const lane = page.locator(".vtl-col").filter({ has: page.getByText(name, { exact: true }) }).locator(".vtl-lane");
  const b = await lane.boundingBox();
  if (!b) throw new Error(`${name}の列が見つかりません。`);
  const x = b.x + b.width / 2;
  await page.mouse.move(x, b.y + b.height * fromFrac);
  await page.mouse.down();
  await page.mouse.move(x, b.y + b.height * toFrac, { steps: 6 });
  await page.mouse.up();
}
