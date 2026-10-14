// 前回の撮影（shots-prev）との差分を調べる。画像は pixelmatch、aria スナップショットは行単位のテキスト差分。
// 開発専用。使い方は docs/dev/development.md の「UI の確認」。

import { cpSync, existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import pixelmatch from "pixelmatch";
import { PNG } from "pngjs";

/** 比較から外すサブフォルダ（差分画像と操作シナリオの失敗画像）。 */
const SKIP_DIRS = new Set(["diff", "flows", "audit"]);

/** 撮影の前に、前回の撮影結果を prevDir へ退避する（退避元が無ければ何もしない）。 */
export function rotateShots(outDir, prevDir) {
  if (!existsSync(outDir)) return;
  rmSync(prevDir, { recursive: true, force: true });
  rmSync(join(outDir, "diff"), { recursive: true, force: true });
  cpSync(outDir, prevDir, {
    recursive: true,
    filter: (src) => !SKIP_DIRS.has(src.replace(/\\/g, "/").split("/").pop() ?? ""),
  });
}

/** 行単位の差分（LCS）。"- 行" / "+ 行" の配列を返す。変化のない行は含めない。 */
export function lineDiff(a, b) {
  const x = a.split(/\r?\n/);
  const y = b.split(/\r?\n/);
  const dp = Array.from({ length: x.length + 1 }, () => new Int32Array(y.length + 1));
  for (let i = x.length - 1; i >= 0; i--) {
    for (let j = y.length - 1; j >= 0; j--) {
      dp[i][j] = x[i] === y[j] ? dp[i + 1][j + 1] + 1 : Math.max(dp[i + 1][j], dp[i][j + 1]);
    }
  }
  const out = [];
  let i = 0;
  let j = 0;
  while (i < x.length && j < y.length) {
    if (x[i] === y[j]) {
      i++;
      j++;
    } else if (dp[i + 1][j] >= dp[i][j + 1]) out.push(`- ${x[i++]}`);
    else out.push(`+ ${y[j++]}`);
  }
  while (i < x.length) out.push(`- ${x[i++]}`);
  while (j < y.length) out.push(`+ ${y[j++]}`);
  return out;
}

/**
 * files（outDir からの相対パス）を prevDir と比べる。
 * 戻り値: { changed: [{ file, detail }], added: [file], diffDir }
 */
export function compareShots(files, outDir, prevDir) {
  const diffDir = join(outDir, "diff");
  const changed = [];
  const added = [];
  for (const file of files) {
    const cur = join(outDir, file);
    const prev = join(prevDir, file);
    if (!existsSync(prev)) {
      added.push(file);
      continue;
    }
    if (file.endsWith(".png")) {
      const a = PNG.sync.read(readFileSync(prev));
      const b = PNG.sync.read(readFileSync(cur));
      if (a.width !== b.width || a.height !== b.height) {
        changed.push({ file, detail: `サイズが変わりました（${a.width}x${a.height} → ${b.width}x${b.height}）` });
        continue;
      }
      const out = new PNG({ width: a.width, height: a.height });
      const n = pixelmatch(a.data, b.data, out.data, a.width, a.height, { threshold: 0.1 });
      if (n > 0) {
        const dest = join(diffDir, file);
        mkdirSync(dirname(dest), { recursive: true });
        writeFileSync(dest, PNG.sync.write(out));
        const pct = ((n / (a.width * a.height)) * 100).toFixed(2);
        changed.push({ file, detail: `${n} px が変わりました（${pct}%）。差分画像: shots/diff/${file}` });
      }
    } else {
      const lines = lineDiff(readFileSync(prev, "utf-8"), readFileSync(cur, "utf-8"));
      if (lines.length > 0) changed.push({ file, detail: `構造が変わりました\n${lines.map((l) => `      ${l}`).join("\n")}` });
    }
  }
  return { changed, added, diffDir };
}

export function reportDiff({ changed, added }, total) {
  console.log(`\n前回との差分（${total} ファイルを比較）:`);
  if (changed.length === 0) console.log("  変化したファイルはありません。");
  for (const c of changed) console.log(`  変化: ${c.file}\n    ${c.detail}`);
  if (added.length > 0) console.log(`  前回に無かったファイル: ${added.length} 件`);
  for (const f of added) console.log(`    ${f}`);
}
