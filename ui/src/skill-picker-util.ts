// スキル選択（skill-picker）の絞り込み。画面に依存しない純粋関数。
import type { SkillView } from "./api";

/** 比較用に正規化する（全角半角・大文字小文字の差を無視）。 */
export function normSkill(s: string): string {
  return s.normalize("NFKC").trim().toLowerCase();
}

/**
 * 既知のスキルを名前の部分一致で絞り込む。付与済み（`selected`）は除く。
 * 前方一致を先に、続けて部分一致（それぞれ元の順）。入力が空なら全て（付与済みを除く）。
 */
export function filterSkills(all: SkillView[], query: string, selected: string[]): SkillView[] {
  const taken = new Set(selected.map(normSkill));
  const q = normSkill(query);
  const rest = all.filter((s) => !taken.has(normSkill(s.name)));
  if (!q) return rest;
  const prefix: SkillView[] = [];
  const partial: SkillView[] = [];
  for (const s of rest) {
    const n = normSkill(s.name);
    if (n.startsWith(q)) prefix.push(s);
    else if (n.includes(q)) partial.push(s);
  }
  return [...prefix, ...partial];
}
