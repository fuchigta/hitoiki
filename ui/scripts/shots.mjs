// モック IPC（vite --mode mock）で UI を巡回し、スクリーンショットを撮る。
// AI エージェントが Tauri なしで画面を確認するための道具。使い方は docs/dev/development.md。
//
//   npm run shots                                  全シナリオ × 全画面 × 全ウインドウ
//   npm run shots -- --scenario default --screen today
//   npm run shots -- --window popup --dark         ダークテーマも撮る
//   npm run shots -- --diff                        前回の撮影（ui/shots-prev/）との差分を出す
//
// 出力: ui/shots/<scenario>/<window>-<screen>.png と、同名の .aria.yml（ariaSnapshot）。
//       popup / quick は <window>.png。ダークは末尾に -dark（aria はライトのときだけ）。
// 撮影の開始時に、前回の ui/shots/ を ui/shots-prev/ へ退避する。
// ブラウザはダウンロードせず、Windows に入っている Edge（なければ Chrome）を使う。
// console error・pageerror・"[mock] unhandled" が 1 件でもあれば exit code 1。

import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { compareShots, reportDiff, rotateShots } from "./lib/diff.mjs";
import { launchBrowser, openPage, root, SCENARIOS, startServer, WINDOWS } from "./lib/harness.mjs";

/** 伸ばす高さの上限（長すぎる画面で画像が巨大になるのを防ぐ）。 */
const MAX_HEIGHT = 5000;

/**
 * 一覧で 1 つ選んだ状態（`#/tasks/<ID>` など）の撮影指定。ID はモックの状態から引く（フィクスチャに無ければ飛ばす）。
 * ファイル名は `main-<name>`。`--screen` を指定したときは、その画面のものだけ撮る。
 */
const SELECTED = [
  {
    name: "log-selected",
    screen: "log",
    hash: (s) => {
      // 一覧に出る（短い区間でない）要確認のうち、いちばん新しいもの。
      const x = s.segments.filter((y) => y.status === "needs_review" && y.end_at - y.start_at >= 120000).sort((a, b) => b.start_at - a.start_at)[0];
      return x && `#/log/${x.id}`;
    },
  },
  {
    name: "log-multi",
    screen: "log",
    hash: (s) => (s.segments.filter((y) => y.status === "needs_review").length >= 2 ? "#/log" : null),
    // 先頭の 2 件にチェックを付けた状態にする。
    act: async (page) => {
      const boxes = page.getByRole("checkbox", { name: "この区間を選ぶ" });
      await boxes.nth(0).check();
      await boxes.nth(1).check();
    },
  },
  {
    name: "tasks-selected",
    screen: "tasks",
    hash: (s) => {
      const t = s.tasks.find((x) => x.title.includes("Rust 所有権の復習"));
      return t && `#/tasks/${t.id}`;
    },
  },
  {
    name: "okr-kr-selected",
    screen: "okr",
    hash: (s) => {
      const k = s.krs.find((x) => x.title.includes("Rust の学習に週 5 時間"));
      return k && `#/okr/kr/${k.id}`;
    },
  },
  {
    name: "okr-skill-selected",
    screen: "okr",
    hash: (s) => {
      const k = s.skills.find((x) => x.name === "Rust");
      return k && `#/okr/skill/${k.id}?tab=skills`;
    },
  },
];

// 画面の一覧は src/lib.ts の SCREENS から拾う（追加漏れを防ぐ）。
function readScreens() {
  const src = readFileSync(join(root, "src/lib.ts"), "utf-8");
  const block = /export const SCREENS = \[([\s\S]*?)\] as const;/.exec(src);
  if (!block) throw new Error("src/lib.ts の SCREENS を読めませんでした。");
  return [...block[1].matchAll(/id:\s*"([a-z]+)"/g)].map((m) => m[1]);
}

function parseArgs(argv) {
  const o = { scenario: null, screen: null, window: null, dark: false, diff: false, out: join(root, "shots"), help: false };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    const next = () => argv[++i];
    if (a === "--scenario") o.scenario = next();
    else if (a === "--screen") o.screen = next();
    else if (a === "--window") o.window = next();
    else if (a === "--out") o.out = resolve(next());
    else if (a === "--dark") o.dark = true;
    else if (a === "--diff") o.diff = true;
    else if (a === "--help" || a === "-h") o.help = true;
    else throw new Error(`知らない引数です: ${a}`);
  }
  return o;
}

function usage() {
  console.log(`使い方: npm run shots -- [--scenario default|empty|busy] [--window main|popup|quick] [--screen <画面ID>] [--dark] [--diff] [--out <dir>]
  --scenario  撮るシナリオ（省略で全部）
  --window    撮るウインドウ種別（省略で全部）。popup / quick は画面の指定が無効
  --screen    main の画面 ID（${readScreens().join(" / ")}）。省略で全部
  --dark      ダークテーマも撮る（ファイル名の末尾に -dark）
  --diff      前回の撮影（<out>-prev）と画像・aria を比べ、変化したものを一覧する。差分画像は <out>/diff/
  --out       出力先（既定は ui/shots）`);
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

  const prevDir = `${opt.out}-prev`;
  rotateShots(opt.out, prevDir);

  const { server, base } = await startServer();
  const browser = await launchBrowser();
  const problems = [];
  const written = []; // opt.out からの相対パス（差分の比較対象）
  let count = 0;
  try {
    for (const scenario of scenarios) {
      mkdirSync(join(opt.out, scenario), { recursive: true });
      for (const win of windows) {
        const targets = win === "main" ? (opt.screen ? [opt.screen] : screens) : [null];
        for (const screen of targets) {
          for (const scheme of schemes) {
            const name = `${screen ? `${win}-${screen}` : win}${scheme === "dark" ? "-dark" : ""}`;
            const where = `${scenario}/${name}`;
            const { page, close } = await openPage(browser, { base, scenario, win, screen, scheme, problems, where });
            // 本文は .content の中でスクロールするので、fullPage だけでは切れる。ビューポートを本文の高さまで伸ばす。
            const full = await page.evaluate(() => document.querySelector(".content")?.scrollHeight ?? 0);
            if (full > WINDOWS[win].height) {
              await page.setViewportSize({ width: WINDOWS[win].width, height: Math.min(full, MAX_HEIGHT) });
              await page.waitForTimeout(300);
            }
            await page.screenshot({ path: join(opt.out, scenario, `${name}.png`), fullPage: true });
            written.push(`${scenario}/${name}.png`);
            if (scheme === "light") {
              // 構造（見出し・ボタンの名前など）。見た目が同じでも構造の崩れに気づける。
              const aria = await page.locator("body").ariaSnapshot();
              writeFileSync(join(opt.out, scenario, `${name}.aria.yml`), `${aria}\n`);
              written.push(`${scenario}/${name}.aria.yml`);
            }
            count += 1;
            console.log(`  ${where}.png`);
            await close();
          }
        }
      }
      // 一覧で 1 つ選んだ状態（main のみ）。
      for (const sp of SELECTED) {
        if (!windows.includes("main") || (opt.screen && opt.screen !== sp.screen) || scenario === "empty") continue;
        for (const scheme of schemes) {
          const name = `main-${sp.name}${scheme === "dark" ? "-dark" : ""}`;
          const where = `${scenario}/${name}`;
          const { page, close } = await openPage(browser, { base, scenario, win: "main", screen: sp.screen, scheme, problems, where });
          const hash = await page.evaluate((src) => {
            // eslint-disable-next-line no-new-func
            return new Function("s", `return (${src})(s)`)(window.__mock.state) ?? null;
          }, sp.hash.toString());
          if (!hash) {
            await close();
            continue;
          }
          await page.evaluate((h) => (location.hash = h), hash);
          await page.waitForFunction(() => !document.body.innerText.includes("読み込み中"), null, { timeout: 5_000 }).catch(() => {});
          await page.waitForTimeout(600);
          if (sp.act) {
            await sp.act(page);
            await page.waitForTimeout(300);
          }
          const full = await page.evaluate(() => document.querySelector(".content")?.scrollHeight ?? 0);
          if (full > WINDOWS.main.height) {
            await page.setViewportSize({ width: WINDOWS.main.width, height: Math.min(full, MAX_HEIGHT) });
            await page.waitForTimeout(300);
          }
          await page.screenshot({ path: join(opt.out, scenario, `${name}.png`), fullPage: true });
          written.push(`${scenario}/${name}.png`);
          if (scheme === "light") {
            const aria = await page.locator("body").ariaSnapshot();
            writeFileSync(join(opt.out, scenario, `${name}.aria.yml`), `${aria}\n`);
            written.push(`${scenario}/${name}.aria.yml`);
          }
          count += 1;
          console.log(`  ${where}.png`);
          await close();
        }
      }
    }
  } finally {
    await browser.close();
    await server.close();
  }

  console.log(`\n${count} 枚を ${opt.out} に保存しました。`);
  if (opt.diff) reportDiff(compareShots(written, opt.out, prevDir), written.length);
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
