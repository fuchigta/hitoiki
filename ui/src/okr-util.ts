// 目標画面（目標 / 成果の指標 / スキルの一覧＋詳細）の純粋ロジック。画面に依存しない。
// 達成率の計算や評価はしない。値と目標値を並べるだけ。
import type { SkillView, TaskView } from "./api";

export type OkrKind = "o" | "kr" | "skill";

export interface OkrSelection {
  kind: OkrKind;
  id: string;
}

/** `#/okr/<kind>/<ID>` のパスから選択を得る。形が合わなければ null。 */
export function okrSelection(path: string[]): OkrSelection | null {
  const [kind, id] = path;
  if ((kind === "o" || kind === "kr" || kind === "skill") && id) return { kind, id };
  return null;
}

export type OkrTab = "okr" | "skills";

/** 一覧のタブ。`?tab=skills` か、スキルを選んでいるときはスキル。それ以外は目標。 */
export function okrTab(param: string | null | undefined, sel: OkrSelection | null): OkrTab {
  if (param === "skills") return "skills";
  if (param === "okr") return "okr";
  return sel?.kind === "skill" ? "skills" : "okr";
}

/** 成果の指標の行に出す値。主軸の値に、書いてある目標値を並べるだけ（達成率は出さない）。 */
export function krMetaText(mainText: string, target: string | null | undefined): string {
  const t = target?.trim();
  return t ? `${mainText} / 目標値 ${t}` : mainText;
}

export interface SkillRow {
  skill: SkillView;
  /** 階層の深さ（根が 0）。 */
  depth: number;
}

/** スキルを親の下に子が並ぶ順にして、深さを付ける。親が見つからないものは根として扱う。循環があっても止まる。 */
export function skillTree(skills: SkillView[]): SkillRow[] {
  const ids = new Set(skills.map((s) => s.id));
  const children = new Map<string | null, SkillView[]>();
  for (const s of skills) {
    const key = s.parent_id && ids.has(s.parent_id) && s.parent_id !== s.id ? s.parent_id : null;
    const list = children.get(key) ?? [];
    list.push(s);
    children.set(key, list);
  }
  const out: SkillRow[] = [];
  const seen = new Set<string>();
  const walk = (parent: string | null, depth: number) => {
    for (const s of children.get(parent) ?? []) {
      if (seen.has(s.id)) continue;
      seen.add(s.id);
      out.push({ skill: s, depth });
      walk(s.id, depth + 1);
    }
  };
  walk(null, 0);
  return out;
}

/** そのスキルが付いたタスク（名前の一致。参照のみ）。 */
export function tasksWithSkill(tasks: TaskView[], name: string): TaskView[] {
  return tasks.filter((t) => t.skills.includes(name));
}
