// 成果の指標への紐づけの提案（成果の指標の詳細に出す候補）の純粋ロジック。画面に依存しない。
// 提案は自動では紐付けない。「付ける」「付けない」を選ぶまで何も変えない。
import type { KeyResultCandidate, KeyResultSuggestions, KeyResultSummary } from "./api";

/** 採否のコマンドに渡す候補の指定（`task:<ID>` / `skill:<ID>`）。 */
export function candidateRef(c: Pick<KeyResultCandidate, "kind" | "id">): string {
  return `${c.kind}:${c.id}`;
}

export function kindLabel(kind: KeyResultCandidate["kind"]): string {
  return kind === "task" ? "タスク" : "スキル";
}

/** 候補の出どころの表示（名前の近さ / AI / 両方）。 */
export function sourceLabel(source: KeyResultCandidate["source"]): string {
  switch (source) {
    case "both":
      return "名前の近さと AI";
    case "ai":
      return "AI";
    default:
      return "名前の近さ";
  }
}

/** 提案を出す成果の指標か。スキルも紐付けたタスク（やめたものを除く）も無いときだけ。集計の読み込み前は false。 */
export function wantsSuggestion(s: KeyResultSummary | null | undefined): boolean {
  if (!s) return false;
  return s.key_result_skills.length === 0 && s.tasks.every((t) => t.status === "dropped");
}

/** 候補の下に添える一言（AI に聞いたか、聞けなかったか）。何も言うことが無ければ null。 */
export function aiNote(r: KeyResultSuggestions, asked: boolean): string | null {
  if (r.ai.note) return r.ai.note;
  if (asked && !r.ai.used) return "AI が設定されていないため、名前の近い候補だけを表示しています";
  if (r.ai.used) return "AI の候補も合わせています";
  return null;
}

/** 採否のあとに候補を取り除く（再取得までの間の表示用）。 */
export function without(list: KeyResultCandidate[], c: Pick<KeyResultCandidate, "kind" | "id">): KeyResultCandidate[] {
  return list.filter((x) => !(x.kind === c.kind && x.id === c.id));
}
