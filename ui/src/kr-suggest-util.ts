// KR への紐づけの提案（KR の詳細に出す候補）の純粋ロジック。画面に依存しない。
// 提案は自動では紐付けない。「付ける」「付けない」を選ぶまで何も変えない。
import type { KrCandidate, KrSuggestions, KrSummary } from "./api";

/** 採否のコマンドに渡す候補の指定（`task:<ID>` / `skill:<ID>`）。 */
export function candidateRef(c: Pick<KrCandidate, "kind" | "id">): string {
  return `${c.kind}:${c.id}`;
}

export function kindLabel(kind: KrCandidate["kind"]): string {
  return kind === "task" ? "タスク" : "スキル";
}

/** 候補の出どころの表示（手元の名前の近さ / AI / 両方）。 */
export function sourceLabel(source: KrCandidate["source"]): string {
  switch (source) {
    case "both":
      return "手元と AI";
    case "ai":
      return "AI";
    default:
      return "手元";
  }
}

/** 提案を出す KR か。スキルも紐付けたタスク（やめたものを除く）も無いときだけ。集計の読み込み前は false。 */
export function wantsSuggestion(s: KrSummary | null | undefined): boolean {
  if (!s) return false;
  return s.kr_skills.length === 0 && s.tasks.every((t) => t.status === "dropped");
}

/** 候補の下に添える一言（AI に聞いたか、聞けなかったか）。何も言うことが無ければ null。 */
export function aiNote(r: KrSuggestions, asked: boolean): string | null {
  if (r.ai.note) return r.ai.note;
  if (asked && !r.ai.used) return "AI が設定されていないため、手元の候補だけを表示しています";
  if (r.ai.used) return "AI の候補も合わせています";
  return null;
}

/** 採否のあとに候補を取り除く（再取得までの間の表示用）。 */
export function without(list: KrCandidate[], c: Pick<KrCandidate, "kind" | "id">): KrCandidate[] {
  return list.filter((x) => !(x.kind === c.kind && x.id === c.id));
}
