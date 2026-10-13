// shots.mjs と flows.mjs が共有する部品（Vite の起動、ブラウザの起動、ページの用意）。
// 開発専用。docs/dev/development.md の「UI の確認」を参照。

import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright";
import { createServer } from "vite";

export const root = resolve(dirname(fileURLToPath(import.meta.url)), "../..");

/** 固定の基準時刻: 2026-10-13（火）10:30 JST（src/dev/time.ts の BASE_NOW と同じ）。 */
export const BASE_NOW = Date.UTC(2026, 9, 13, 1, 30);

export const SCENARIOS = ["default", "empty", "busy"];

export const WINDOWS = {
  // main は 1280x800。popup / quick は crates/hitoiki-app の WebviewWindowBuilder の inner_size に合わせる。
  main: { width: 1280, height: 800 },
  popup: { width: 360, height: 320 },
  quick: { width: 520, height: 380 },
};

/** モックモードの Vite を起動する。開発中の vite（5173）と衝突しないよう、別のポートを使う。 */
export async function startServer() {
  const server = await createServer({
    root,
    mode: "mock",
    logLevel: "error",
    server: { port: 5199, strictPort: false, open: false },
  });
  await server.listen();
  const base = server.resolvedUrls?.local?.[0]?.replace(/\/$/, "");
  if (!base) throw new Error("Vite の URL を取得できませんでした。");
  return { server, base };
}

/** インストール済みの Edge（なければ Chrome）を起動する。ブラウザのダウンロードはしない。 */
export async function launchBrowser() {
  for (const channel of ["msedge", "chrome"]) {
    try {
      return await chromium.launch({ channel });
    } catch (e) {
      console.warn(`[harness] ${channel} を起動できませんでした: ${String(e.message).split("\n")[0]}`);
    }
  }
  throw new Error("Edge も Chrome も起動できませんでした。どちらかをインストールしてください。");
}

/**
 * ページを開いて描画を待つ。console error・pageerror・"[mock] unhandled" は problems に積む。
 * 戻り値の close() でコンテキストを閉じる。
 */
export async function openPage(browser, { base, scenario, win = "main", screen = null, scheme = "light", problems, where }) {
  const ctx = await browser.newContext({
    viewport: WINDOWS[win],
    colorScheme: scheme,
    locale: "ja-JP",
    timezoneId: "Asia/Tokyo",
    deviceScaleFactor: 1,
  });
  const page = await ctx.newPage();
  page.on("console", (m) => {
    if (m.type() !== "error") return;
    const t = m.text();
    // index.html に favicon が無く、ブラウザが取りに行って 404 になるだけなので無視する。
    if (/favicon/.test(t) || /favicon/.test(m.location().url ?? "")) return;
    problems.push(`${where}: console.error: ${t}`);
  });
  page.on("pageerror", (e) => problems.push(`${where}: pageerror: ${e.message}`));
  // 現在時刻を固定する（タイマーは進むので画面は動く）。
  await page.clock.setFixedTime(BASE_NOW);
  await page.goto(`${base}/?scenario=${scenario}&window=${win}${screen ? `#/${screen}` : ""}`, { waitUntil: "load" });
  // 描画とデータ取得の完了を待つ（読み込み中の表示が消えるまで）。
  await page
    .waitForSelector(win === "main" ? ".shell:not([aria-busy])" : "#app > *", { timeout: 10_000 })
    .catch(() => problems.push(`${where}: 画面が描画されませんでした`));
  await page.waitForFunction(() => !document.body.innerText.includes("読み込み中"), null, { timeout: 5_000 }).catch(() => {});
  await page.waitForTimeout(300);
  return { page, close: () => ctx.close() };
}
