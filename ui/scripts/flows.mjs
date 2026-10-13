// モック IPC の UI で、主要な操作の流れを Playwright で実行して確かめる（L3）。使い方は docs/dev/development.md。
//
//   npm run flows                  scripts/flows/*.mjs を全部実行
//   npm run flows -- --flow tasks  1 本だけ実行（--flow は繰り返し可）
//
// 1 本ごとに新しいブラウザコンテキスト（= モックの状態が初期化された状態）で実行する。
// 失敗すると ui/shots/flows/<名前>.png に画面を保存し、最後に exit code 1。
// console error・pageerror・"[mock] unhandled" も失敗として扱う。

import { mkdirSync, readdirSync } from "node:fs";
import { join } from "node:path";
import { pathToFileURL } from "node:url";
import { launchBrowser, openPage, root, startServer } from "./lib/harness.mjs";

const FLOWS_DIR = join(root, "scripts/flows");
const OUT_DIR = join(root, "shots/flows");

async function loadFlows() {
  const files = readdirSync(FLOWS_DIR).filter((f) => f.endsWith(".mjs")).sort();
  const flows = [];
  for (const f of files) {
    const mod = await import(pathToFileURL(join(FLOWS_DIR, f)).href);
    flows.push(mod.default);
  }
  return flows;
}

async function main() {
  const only = [];
  const argv = process.argv.slice(2);
  for (let i = 0; i < argv.length; i++) {
    if (argv[i] === "--flow") only.push(argv[++i]);
    else throw new Error(`知らない引数です: ${argv[i]}`);
  }
  let flows = await loadFlows();
  if (only.length > 0) {
    for (const n of only) if (!flows.some((f) => f.name === n)) throw new Error(`知らない操作シナリオです: ${n}（${flows.map((f) => f.name).join(", ")}）`);
    flows = flows.filter((f) => only.includes(f.name));
  }

  const { server, base } = await startServer();
  const browser = await launchBrowser();
  const failed = [];
  try {
    for (const flow of flows) {
      const problems = [];
      const { page, close } = await openPage(browser, { base, scenario: flow.scenario, screen: flow.screen, problems, where: flow.name });
      page.setDefaultTimeout(5000);
      let error = null;
      try {
        await flow.run({ page });
        await page.waitForTimeout(200); // 直後に出る console error を拾う
        if (problems.length > 0) error = new Error(`ページのエラー:\n  ${[...new Set(problems)].join("\n  ")}`);
      } catch (e) {
        error = e;
      }
      if (error) {
        mkdirSync(OUT_DIR, { recursive: true });
        const shot = join(OUT_DIR, `${flow.name}.png`);
        await page.screenshot({ path: shot, fullPage: true }).catch(() => {});
        failed.push(flow.name);
        if (process.env.FLOW_STACK) console.error(error.stack);
        console.error(`  NG ${flow.name}: ${String(error.message ?? error).split("\n")[0]}`);
        if (problems.length > 0) for (const p of new Set(problems)) console.error(`       ${p}`);
        console.error(`     画面: ${shot}`);
      } else {
        console.log(`  OK ${flow.name}`);
      }
      await close();
    }
  } finally {
    await browser.close();
    await server.close();
  }

  console.log(`\n${flows.length - failed.length}/${flows.length} 本が成功しました。`);
  return failed.length > 0 ? 1 : 0;
}

main().then(
  (code) => process.exit(code),
  (e) => {
    console.error(e.message ?? e);
    process.exit(1);
  },
);
