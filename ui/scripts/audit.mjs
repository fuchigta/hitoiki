// UI の細かい不統一を機械的に検出する（モック IPC の画面を Playwright で巡回）。使い方は docs/dev/development.md。
//
//   npm run audit                              全シナリオ × 全画面 × 通常幅・狭い幅
//   npm run audit -- --scenario default --screen report
//   npm run audit -- --only shift              位置ずれの検査だけ（style で値の集計だけ）
//
// 検査:
//   shift  切り替え系の部品（タブ・chip・ラジオ・チェックボックス・select の各 option）を順に操作し、
//          押した部品の上側・同じ高さにある部品、見出し、本文の上端が 1px 以上動いたら報告する。
//   style  部品の種類ごとの高さ・文字サイズ・余白・角丸の少数派、トークン外の値、横並びの高さと上下位置の不揃い。
// 出力: ui/shots/audit/report.md と report.json。許可リスト（audit-allow.json）に載せたものを除いて 1 件でもあれば exit 1。

import { mkdirSync, readFileSync, writeFileSync, existsSync } from "node:fs";
import { join, resolve } from "node:path";
import { collectStyles, listToggles, markPressed, measureAgain, tagAll } from "./lib/audit-page.mjs";
import { launchBrowser, openPage, root, SCENARIOS, startServer } from "./lib/harness.mjs";

const WIDTHS = [1280, 800];
const WIDE = WIDTHS[0];
const CONCURRENCY = 4;
/** 位置・大きさがこの値（px）以上変わったら報告する。 */
const EPS = 0.99;
/** 1 つの目標あたり、操作する候補の上限（暴走防止）。 */
const MAX_TOGGLES = 60;

// ---------- 対象 ----------

function readScreens() {
  const src = readFileSync(join(root, "src/lib.ts"), "utf-8");
  const block = /export const SCREENS = \[([\s\S]*?)\] as const;/.exec(src);
  if (!block) throw new Error("src/lib.ts の SCREENS を読めませんでした。");
  return [...block[1].matchAll(/id:\s*"([a-z]+)"/g)].map((m) => m[1]);
}

function settingsTabs() {
  const src = readFileSync(join(root, "src/settings-util.ts"), "utf-8");
  const block = /export const SETTINGS_TABS = \[([\s\S]*?)\] as const;/.exec(src);
  if (!block) throw new Error("src/settings-util.ts の SETTINGS_TABS を読めませんでした。");
  return [...block[1].matchAll(/id:\s*"([a-z]+)"/g)].map((m) => m[1]);
}

/** 主要な選択状態。hash はモックの状態から引く（無ければ飛ばす）。 */
const SELECTED = [
  { name: "today-design", screen: "today", fixed: "#/today/design" },
  { name: "tasks-selected", screen: "tasks", hash: (s) => s.tasks[0] && `#/tasks/${s.tasks[0].id}` },
  { name: "okr-kr-selected", screen: "okr", hash: (s) => s.krs[0] && `#/okr/kr/${s.krs[0].id}` },
  { name: "okr-skill-selected", screen: "okr", hash: (s) => s.skills[0] && `#/okr/skill/${s.skills[0].id}?tab=skills` },
  { name: "records-checkin-selected", screen: "records", hash: (s) => s.checkins[0] && `#/records/checkin/${s.checkins[0].id}` },
  { name: "records-reflection-selected", screen: "records", hash: (s) => s.reflections[0] && `#/records/reflection/${s.reflections[0].id}` },
  { name: "records-new-abc", screen: "records", fixed: "#/records/new/abc" },
  {
    name: "log-selected",
    screen: "log",
    hash: (s) => {
      const x = s.segments.filter((y) => y.status === "needs_review" && y.end_at - y.start_at >= 120000).sort((a, b) => b.start_at - a.start_at)[0];
      return x && `#/log/${x.id}`;
    },
  },
];

function buildTargets(opt) {
  const screens = readScreens();
  const targets = screens.map((s) => ({ name: s, screen: s, hash: null }));
  for (const t of settingsTabs()) if (t !== "general") targets.push({ name: `settings-${t}`, screen: "settings", fixed: `#/settings/${t}` });
  for (const s of SELECTED) targets.push(s);
  return targets.filter((t) => !opt.screen || t.screen === opt.screen || t.name === opt.screen);
}

// ---------- 引数 ----------

function parseArgs(argv) {
  const o = { scenario: null, screen: null, only: null, out: join(root, "shots/audit"), help: false };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    const next = () => argv[++i];
    if (a === "--scenario") o.scenario = next();
    else if (a === "--screen") o.screen = next();
    else if (a === "--only") o.only = next();
    else if (a === "--out") o.out = resolve(next());
    else if (a === "--help" || a === "-h") o.help = true;
    else throw new Error(`知らない引数です: ${a}`);
  }
  return o;
}

// ---------- ページ操作 ----------

async function settle(page) {
  await page.waitForSelector(".shell:not([aria-busy])", { timeout: 10_000 }).catch(() => {});
  await page.waitForFunction(() => !document.body.innerText.includes("読み込み中"), null, { timeout: 5_000 }).catch(() => {});
  await page.waitForTimeout(250);
}

async function gotoTarget(page, target) {
  const hash =
    target.fixed ??
    (target.hash
      ? await page.evaluate((src) => new Function("s", `return (${src})(s)`)(window.__mock.state) ?? null, target.hash.toString())
      : null);
  if (target.hash || target.fixed) {
    if (!hash) return false;
    await page.evaluate((h) => (location.hash = h), hash);
    await settle(page);
    return hash;
  }
  return true;
}

async function reload(page) {
  await page.reload({ waitUntil: "load" });
  await settle(page);
}

// ---------- 位置ずれ ----------

async function runShift({ browser, base, scenario, width, target, problems, issues, stats }) {
  const where = `${scenario}@${width}/${target.name}`;
  const { page, close } = await openPage(browser, { base, scenario, screen: target.screen, width, problems, where });
  page.setDefaultTimeout(3000);
  try {
    const hash = await gotoTarget(page, target);
    if (!hash) return;
    const { SEL, list } = await page.evaluate(listToggles);
    stats.toggles += list.length;
    for (const cand of list.slice(0, MAX_TOGGLES)) {
      await reload(page);
      const before = await page.evaluate(tagAll);
      if (!(await page.evaluate(markPressed, { SEL, index: cand.index }))) continue;
      try {
        const loc = page.locator(SEL).nth(cand.index);
        if (cand.option !== null) await loc.selectOption({ index: cand.option });
        else await loc.click();
      } catch {
        continue; // 操作できなかった（隠れている等）
      }
      await page.mouse.move(0, 0);
      await page.waitForTimeout(250);
      const after = await page.evaluate(measureAgain);
      stats.operated += 1;
      const pressed = after.els.find((e) => e.pressed);
      const pb = pressed && before.els.find((e) => e.n === pressed.n);
      if (!pb) continue;
      const hOk = before.cw === after.cw; // スクロールバーの出入りで横幅が変わったときは横方向を見ない
      const byN = new Map(after.els.map((e) => [e.n, e]));
      const hits = [];
      for (const b of before.els) {
        const a = byN.get(b.n);
        if (!a || !a.vis) continue;
        if (b.y >= pb.y + pb.h - 0.5) continue; // 押した部品より下は変わってよい
        const dx = hOk ? a.x - b.x : 0;
        const dy = a.y - b.y;
        const dw = hOk ? a.w - b.w : 0;
        const dh = a.h - b.h;
        const moved = Math.abs(dx) >= EPS || Math.abs(dy) >= EPS;
        // 大きさは操作部品だけ見る（見出しや入れ物は中身の増減で変わってよい）。祖先は位置だけ。
        const resized = b.ctl && !a.anc && (Math.abs(dw) >= EPS || Math.abs(dh) >= EPS);
        if (!moved && !resized) continue;
        if (a.anc && !moved) continue;
        const who = `${b.tag}${b.cls ? `.${b.cls}` : ""}${b.text ? `「${b.text}」` : ""}`;
        const what = [moved && `移動 (${r(dx)}, ${r(dy)})`, resized && `サイズ (${r(dw)}, ${r(dh)})`].filter(Boolean).join(" ");
        hits.push({ who, what, id: `${b.tag}${b.cls ? `.${b.cls}` : ""}` });
      }
      if (hits.length > 0) {
        const pk = cand.name.replace(/ → .*/, "");
        const shown = hits.slice(0, 8).map((h) => `${h.who} ${h.what}`);
        if (hits.length > 8) shown.push(`ほか ${hits.length - 8} 件`);
        addIssue(issues, {
          check: "shift",
          key: `shift|${target.name}|${pk}`,
          title: `${target.name}: 「${cand.name}」を操作すると ${hits.length} 個の部品が動く`,
          detail: shown.join("\n"),
          where,
        });
      }
    }
  } finally {
    await close();
  }
}

const r = (n) => (Math.round(n * 10) / 10).toString();

// ---------- 値の集計 ----------

async function runStyle({ browser, base, scenario, win, target, problems, records }) {
  const where = `${scenario}/${win === "main" ? target.name : win}`;
  const { page, close } = await openPage(browser, { base, scenario, win, screen: win === "main" ? target.screen : null, width: win === "main" ? WIDE : null, problems, where });
  try {
    if (win === "main") {
      const hash = await gotoTarget(page, target);
      if (!hash) return;
    }
    const res = await page.evaluate(collectStyles);
    records.push({ where, ...res });
  } finally {
    await close();
  }
}

/** 少数派の判定: 同じ種類が 8 個以上あり、その値が全体の 15% 未満。 */
function minorityIssues(records, issues) {
  const groups = new Map(); // kind -> metric -> value -> {count, samples}
  for (const rec of records)
    for (const k of rec.kinds) {
      const metrics = /^h\d$/.test(k.kind)
        ? { 文字サイズ: k.fs, 太さ: k.fw, 余白: k.mb }
        : { 高さ: `${k.h}px`, 文字サイズ: k.fs, 内側の余白: k.pad, 角丸: k.radius };
      for (const [m, v] of Object.entries(metrics)) {
        if (v === "" || v == null) continue;
        const g = groups.get(k.kind) ?? new Map();
        const mm = g.get(m) ?? new Map();
        const e = mm.get(v) ?? { count: 0, samples: [] };
        e.count += 1;
        if (e.samples.length < 3 && !e.samples.some((s) => s.startsWith(k.desc))) e.samples.push(`${k.desc} @${rec.where}`);
        mm.set(v, e);
        g.set(m, mm);
        groups.set(k.kind, g);
      }
    }
  for (const [kind, g] of groups)
    for (const [metric, mm] of g) {
      const total = [...mm.values()].reduce((s, e) => s + e.count, 0);
      if (total < 8 || mm.size < 2) continue;
      const dist = [...mm.entries()].sort((a, b) => b[1].count - a[1].count).map(([v, e]) => `${v}×${e.count}`).join(", ");
      for (const [v, e] of mm)
        if (e.count / total < 0.15)
          addIssue(issues, {
            check: "style",
            key: `style|${kind}|${metric}=${v}`,
            title: `${kind} の${metric}が少数派: ${v}（${e.count}/${total}）`,
            detail: `分布: ${dist}\n例: ${e.samples.join(" / ")}`,
            where: e.samples[0]?.split("@")[1] ?? "",
          });
    }
}

function offTokenIssues(records, issues) {
  const merged = new Map();
  for (const rec of records)
    for (const o of rec.off) {
      const e = merged.get(o.key) ?? { count: 0, sample: o.sample, where: rec.where };
      e.count += o.count;
      merged.set(o.key, e);
    }
  for (const [key, e] of merged)
    addIssue(issues, {
      check: "token",
      key: `token|${key}`,
      title: `トークン外の値: ${key.replace("|", " = ")}（${e.count} 要素）`,
      detail: `例: ${e.sample} @${e.where}`,
      where: e.where,
    });
}

function alignIssues(records, issues) {
  for (const rec of records)
    for (const x of rec.rows) {
      const norm = (s) => s.replace(/「.*」/, "");
      const [a, b] = [norm(x.a), norm(x.b)].sort();
      addIssue(issues, {
        check: "align",
        key: `align|${a}|${b}`,
        title: `横並びの不揃い: ${x.a} (高さ ${x.ha}) と ${x.b} (高さ ${x.hb})`,
        detail: `垂直中心の差 ${x.dc}px`,
        where: rec.where,
      });
    }
}

// ---------- 集約・出力 ----------

function addIssue(issues, i) {
  const e = issues.get(i.key);
  if (e) {
    if (e.wheres.length < 6 && !e.wheres.includes(i.where)) e.wheres.push(i.where);
    e.count += 1;
  } else issues.set(i.key, { check: i.check, key: i.key, title: i.title, detail: i.detail, wheres: [i.where], count: 1 });
}

function loadAllow() {
  const f = join(root, "scripts/audit-allow.json");
  if (!existsSync(f)) return [];
  return JSON.parse(readFileSync(f, "utf-8"));
}

function writeReport(out, issues, allowed, unused, summary) {
  mkdirSync(out, { recursive: true });
  const byCheck = (list) => {
    const m = {};
    for (const i of list) m[i.check] = (m[i.check] ?? 0) + 1;
    return m;
  };
  writeFileSync(join(out, "report.json"), JSON.stringify({ summary, issues, allowed, unusedAllow: unused }, null, 2));
  const NAMES = { shift: "位置ずれ", style: "値のばらつき（少数派）", token: "トークン外の値", align: "横並びの高さ・上下位置の不揃い" };
  const lines = [`# UI 監査レポート`, ``, `- 操作した切り替え部品: ${summary.operated} 回（候補 ${summary.toggles}）`, `- 問題: ${issues.length} 件（${Object.entries(byCheck(issues)).map(([k, v]) => `${NAMES[k]} ${v}`).join("、") || "なし"}）`, `- 許可リストで除外: ${allowed.length} 件`, ``];
  for (const check of Object.keys(NAMES)) {
    const list = issues.filter((i) => i.check === check);
    if (list.length === 0) continue;
    lines.push(`## ${NAMES[check]}（${list.length} 件）`, ``);
    for (const i of list) {
      lines.push(`- ${i.title}`, `  - key: \`${i.key}\``, `  - 場所: ${i.wheres.join(", ")}${i.count > i.wheres.length ? ` ほか（${i.count} 回）` : ""}`);
      for (const d of i.detail.split("\n")) lines.push(`  - ${d}`);
    }
    lines.push(``);
  }
  if (allowed.length > 0) {
    lines.push(`## 許可リストで除外したもの`, ``);
    for (const a of allowed) lines.push(`- \`${a.key}\` : ${a.reason}`);
    lines.push(``);
  }
  if (unused.length > 0) {
    lines.push(`## 使われなかった許可リストの項目`, ``);
    for (const a of unused) lines.push(`- \`${a.pattern}\` : ${a.reason}`);
    lines.push(``);
  }
  writeFileSync(join(out, "report.md"), lines.join("\n"));
}

async function pool(items, n, fn) {
  const q = [...items];
  const workers = Array.from({ length: n }, async () => {
    while (q.length > 0) await fn(q.shift());
  });
  await Promise.all(workers);
}

async function main() {
  const opt = parseArgs(process.argv.slice(2));
  if (opt.help) {
    console.log("使い方: npm run audit -- [--scenario default|empty|busy] [--screen <画面ID>] [--only shift|style] [--out <dir>]");
    return 0;
  }
  const scenarios = opt.scenario ? [opt.scenario] : SCENARIOS;
  for (const s of scenarios) if (!SCENARIOS.includes(s)) throw new Error(`知らないシナリオです: ${s}`);
  const targets = buildTargets(opt);
  const { server, base } = await startServer();
  const browser = await launchBrowser();
  const problems = [];
  const issues = new Map();
  const records = [];
  const stats = { toggles: 0, operated: 0 };
  try {
    if (opt.only !== "shift") {
      const jobs = [];
      for (const scenario of scenarios) {
        for (const target of targets) jobs.push({ scenario, win: "main", target });
        if (scenario === "default" && !opt.screen) for (const win of ["popup", "quick"]) jobs.push({ scenario, win, target: { name: win } });
      }
      await pool(jobs, CONCURRENCY, async (j) => {
        await runStyle({ browser, base, ...j, problems, records });
        console.log(`  style ${j.scenario}/${j.win === "main" ? j.target.name : j.win}`);
      });
      minorityIssues(records, issues);
      offTokenIssues(records, issues);
      alignIssues(records, issues);
    }
    if (opt.only !== "style") {
      const jobs = [];
      for (const scenario of scenarios) for (const width of WIDTHS) for (const target of targets) jobs.push({ scenario, width, target });
      await pool(jobs, CONCURRENCY, async (j) => {
        await runShift({ browser, base, ...j, problems, issues, stats });
        console.log(`  shift ${j.scenario}@${j.width}/${j.target.name}`);
      });
    }
  } finally {
    await browser.close();
    await server.close();
  }

  // 許可リスト: { "pattern": "<key に対する正規表現>", "reason": "..." }
  const allow = loadAllow();
  const used = new Set();
  const open = [];
  const allowed = [];
  for (const i of [...issues.values()].sort((a, b) => a.key.localeCompare(b.key))) {
    const hit = allow.find((a) => new RegExp(a.pattern).test(i.key));
    if (hit) {
      used.add(hit);
      allowed.push({ key: i.key, reason: hit.reason });
    } else open.push(i);
  }
  const unused = opt.scenario || opt.screen || opt.only ? [] : allow.filter((a) => !used.has(a));
  writeReport(opt.out, open, allowed, unused, stats);
  console.log(`\n問題 ${open.length} 件（許可リストで除外 ${allowed.length} 件）。レポート: ${join(opt.out, "report.md")}`);
  const probs = [...new Set(problems)];
  if (probs.length > 0) {
    console.error(`\nページのエラーが ${probs.length} 件ありました。`);
    for (const p of probs) console.error(`  - ${p}`);
  }
  if (unused.length > 0) console.warn(`使われなかった許可リストの項目が ${unused.length} 件あります（report.md を参照）。`);
  return open.length > 0 || probs.length > 0 ? 1 : 0;
}

main().then(
  (code) => process.exit(code),
  (e) => {
    console.error(e.stack ?? e);
    process.exit(1);
  },
);
