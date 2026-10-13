// モック IPC（vite --mode mock）で UI を巡回し、スクリーンショットを撮る。
// AI エージェントが Tauri なしで画面を確認するための道具。使い方は docs/dev/development.md。
//
//   npm run shots                                  全シナリオ × 全画面 × 全ウインドウ
//   npm run shots -- --scenario default --screen today
//   npm run shots -- --window popup --dark         ダークテーマも撮る
//
// 出力: ui/shots/<scenario>/<window>-<screen>.png（popup / quick は <window>.png。ダークは末尾に -dark）。
// ブラウザはダウンロードせず、Windows に入っている Edge（なければ Chrome）を使う。
// console error・pageerror・"[mock] unhandled" が 1 件でもあれば exit code 1。

import { mkdirSync, readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright";
import { createServer } from "vite";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");

/** 固定の基準時刻: 2026-10-13（火）10:30 JST（src/dev/time.ts の BASE_NOW と同じ）。 */
const BASE_NOW = Date.UTC(2026, 9, 13, 1, 30);

/** 伸ばす高さの上限（長すぎる画面で画像が巨大になるのを防ぐ）。 */
const MAX_HEIGHT = 5000;

const SCENARIOS =["default", "empty", "busy"];
const WINDOWS = {
  // main は 1280x800。popup / quick は crates/hitoiki-app の WebviewWindowBuilder の inner_size に合わせる。
  main: { width: 1280, height: 800 },
  popup: { width: 360, height: 320 },
  quick: { width: 520, height: 380 },
};

// 画面の一覧は src/lib.ts の SCREENS から拾う（追加漏れを防ぐ）。
function readScreens() {
  const src = readFileSync(join(root, "src/lib.ts"), "utf-8");
  const block = /export const SCREENS = \[([\s\S]*?)\] as const;/.exec(src);
  if (!block) throw new Error("src/lib.ts の SCREENS を読めませんでした。");
  return [...block[1].matchAll(/id:\s*"([a-z]+)"/g)].map((m) => m[1]);
}

function parseArgs(argv) {
  const o = { scenario: null, screen: null, window: null, dark: false, out: join(root, "shots"), help: false };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    const next = () => argv[++i];
    if (a === "--scenario") o.scenario = next();
    else if (a === "--screen") o.screen = next();
    else if (a === "--window") o.window = next();
    else if (a === "--out") o.out = resolve(next());
    else if (a === "--dark") o.dark = true;
    else if (a === "--help" || a === "-h") o.help = true;
    else throw new Error(`知らない引数です: ${a}`);
  }
  return o;
}

function usage() {
  console.log(`使い方: npm run shots -- [--scenario default|empty|busy] [--window main|popup|quick] [--screen <画面ID>] [--dark] [--out <dir>]
  --scenario  撮るシナリオ（省略で全部）
  --window    撮るウインドウ種別（省略で全部）。popup / quick は画面の指定が無効
  --screen    main の画面 ID（${readScreens().join(" / ")}）。省略で全部
  --dark      ダークテーマも撮る（ファイル名の末尾に -dark）
  --out       出力先（既定は ui/shots）`);
}

async function launch() {
  for (const channel of ["msedge", "chrome"]) {
    try {
      return await chromium.launch({ channel });
    } catch (e) {
      console.warn(`[shots] ${channel} を起動できませんでした: ${String(e.message).split("\n")[0]}`);
    }
  }
  throw new Error("Edge も Chrome も起動できませんでした。どちらかをインストールしてください。");
}

async function main() {
  const opt = parseArgs(process.argv.slice(2));
  if (opt.help) {
    usage();
    return 0;
  }

  const screens = readScreens();
  const scenarios = opt.scenario ? [opt.scenario] : SCENARIOS;
  const windows = opt.window ? [opt.window] : Object.keys(WINDOWS);
  for (const s of scenarios) if (!SCENARIOS.includes(s)) throw new Error(`知らないシナリオです: ${s}`);
  for (const w of windows) if (!WINDOWS[w]) throw new Error(`知らないウインドウ種別です: ${w}`);
  if (opt.screen && !screens.includes(opt.screen)) throw new Error(`知らない画面です: ${opt.screen}（${screens.join(", ")}）`);
  const schemes = opt.dark ? ["light", "dark"] : ["light"];

  // 開発中の vite（5173）と衝突しないよう、別のポートで起動する。
  const server = await createServer({
    root,
    mode: "mock",
    logLevel: "error",
    server: { port: 5199, strictPort: false, open: false },
  });
  await server.listen();
  const base = server.resolvedUrls?.local?.[0]?.replace(/\/$/, "");
  if (!base) throw new Error("Vite の URL を取得できませんでした。");

  const browser = await launch();
  const problems = [];
  let count = 0;
  try {
    for (const scenario of scenarios) {
      mkdirSync(join(opt.out, scenario), { recursive: true });
      for (const win of windows) {
        const targets = win === "main" ? (opt.screen ? [opt.screen] : screens) : [null];
        for (const screen of targets) {
          for (const scheme of schemes) {
            const name = `${screen ? `${win}-${screen}` : win}${scheme === "dark" ? "-dark" : ""}`;
            const file = join(opt.out, scenario, `${name}.png`);
            const ctx = await browser.newContext({
              viewport: WINDOWS[win],
              colorScheme: scheme,
              locale: "ja-JP",
              timezoneId: "Asia/Tokyo",
              deviceScaleFactor: 1,
            });
            const page = await ctx.newPage();
            const where = `${scenario}/${name}`;
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
            const url = `${base}/?scenario=${scenario}&window=${win}${screen ? `#/${screen}` : ""}`;
            await page.goto(url, { waitUntil: "load" });
            // 描画とデータ取得の完了を待つ（読み込み中の表示が消えるまで）。
            await page
              .waitForSelector(win === "main" ? ".shell:not([aria-busy])" : "#app > *", { timeout: 10_000 })
              .catch(() => problems.push(`${where}: 画面が描画されませんでした`));
            await page.waitForFunction(() => !document.body.innerText.includes("読み込み中"), null, { timeout: 5_000 }).catch(() => {});
            await page.waitForTimeout(400);
            // 本文は .content の中でスクロールするので、fullPage だけでは切れる。ビューポートを本文の高さまで伸ばす。
            const full = await page.evaluate(() => document.querySelector(".content")?.scrollHeight ?? 0);
            if (full > WINDOWS[win].height) {
              await page.setViewportSize({ width: WINDOWS[win].width, height: Math.min(full, MAX_HEIGHT) });
              await page.waitForTimeout(300);
            }
            await page.screenshot({ path: file, fullPage: true });
            count += 1;
            console.log(`  ${where}.png`);
            await ctx.close();
          }
        }
      }
    }
  } finally {
    await browser.close();
    await server.close();
  }

  console.log(`\n${count} 枚を ${opt.out} に保存しました。`);
  if (problems.length > 0) {
    const uniq = [...new Set(problems)];
    console.error(`\n問題が ${uniq.length} 件ありました。`);
    for (const p of uniq) console.error(`  - ${p}`);
    return 1;
  }
  console.log("console error・pageerror・未実装コマンドはありませんでした。");
  return 0;
}

main().then(
  (code) => process.exit(code),
  (e) => {
    console.error(e.message ?? e);
    process.exit(1);
  },
);
