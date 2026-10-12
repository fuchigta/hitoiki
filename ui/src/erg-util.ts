// ERG（存在・関係・成長）の表示用の純粋ロジック。判定は core（AI）が行い、ここは見せ方だけを持つ。
// 文言は事実を述べるだけにして、件数の偏りに良し悪しや「もっと〇〇すべき」といった評価は付けない。

import type { ErgChoice, ErgDistribution, ErgLabel, ErgRecordKind } from "./api/types";

export const ERG_CHOICES: ErgChoice[] = ["existence", "relatedness", "growth", "none"];

export const ERG_NAMES: Record<ErgChoice, string> = {
  existence: "存在",
  relatedness: "関係",
  growth: "成長",
  none: "どれにも当てはまらない",
};

/** 用語の短い説明（ERG 理論を知らなくても読めるように添える）。 */
export const ERG_HINTS: Record<ErgChoice, string> = {
  existence: "体や生活の安心",
  relatedness: "人とのつながり",
  growth: "手応えや学び",
  none: "どれにも関わらない",
};

export const ERG_LEGEND = "存在 = 体や生活の安心、関係 = 人とのつながり、成長 = 手応えや学び";

/** 記録 1 件を指すキー（ラベルの引き当て用）。 */
export function ergKey(kind: ErgRecordKind, id: string): string {
  return `${kind}:${id}`;
}

export function labelMap(labels: ErgLabel[]): Map<string, ErgLabel> {
  return new Map(labels.map((l) => [ergKey(l.record_kind, l.record_id), l]));
}

/** 画面での状態。unlabeled はラベル無し、pending は確認待ち（AI の提案で未確定）。 */
export type ErgState = "unlabeled" | "user" | "ai" | "pending";

export function ergState(l: ErgLabel | undefined | null): ErgState {
  if (!l) return "unlabeled";
  return l.source === "suggested" ? "pending" : l.source;
}

/** 確信度の表示（0.85 → "85%"）。無ければ空。 */
export function fmtConfidence(c: number | null | undefined): string {
  if (c == null || !Number.isFinite(c)) return "";
  return `${Math.round(Math.min(1, Math.max(0, c)) * 100)}%`;
}

/** 分布の表示行。件数が 0 の観点も出す（偏りは数字を見て判断してもらう）。 */
export function distributionRows(d: ErgDistribution): { key: ErgChoice; name: string; hint: string; count: number }[] {
  return ERG_CHOICES.map((key) => ({ key, name: ERG_NAMES[key], hint: ERG_HINTS[key], count: d[key] }));
}

/** 分布の一行要約（事実のみ）。 */
export function distributionSummary(d: ErgDistribution): string {
  if (d.total === 0) return "この期間に、文章のある記録はありません。";
  const parts = [`存在 ${d.existence}`, `関係 ${d.relatedness}`, `成長 ${d.growth}`, `どれにも当てはまらない ${d.none}`];
  let s = `記録 ${d.total} 件のうち、${parts.join(" ・ ")}`;
  const rest: string[] = [];
  if (d.pending > 0) rest.push(`確認待ち ${d.pending}`);
  if (d.unlabeled > 0) rest.push(`未判定 ${d.unlabeled}`);
  if (rest.length > 0) s += `（${rest.join("、")}）`;
  return s;
}

/** AI の判定を実行した結果のメッセージ。 */
export function classifyMessage(o: {
  skipped: string | null;
  targets: number;
  adopted: number;
  pending: number;
  unread: number;
}): string {
  if (o.skipped) return o.skipped;
  if (o.targets === 0) return "判定する記録はありません。";
  let s = `${o.targets} 件を判定しました。AI の判定を採用 ${o.adopted} 件、確認待ち ${o.pending} 件`;
  if (o.unread > 0) s += `、判定が得られなかったもの ${o.unread} 件`;
  return s + "。";
}

// ===== 週の範囲 =====

const pad = (n: number) => String(n).padStart(2, "0");

/** `YYYY-MM-DD` を含む月曜始まりの週の、開始日と終了日（日付のみ。core の `--to` は終了日の終わりまで含む）。 */
export function weekOf(date: string): { from: string; to: string } | null {
  const m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(date);
  if (!m) return null;
  const d = new Date(Date.UTC(Number(m[1]), Number(m[2]) - 1, Number(m[3])));
  if (Number.isNaN(d.getTime())) return null;
  const back = (d.getUTCDay() + 6) % 7; // 月曜 = 0
  const mon = new Date(d.getTime() - back * 86_400_000);
  const sun = new Date(mon.getTime() + 6 * 86_400_000);
  const f = (x: Date) => `${x.getUTCFullYear()}-${pad(x.getUTCMonth() + 1)}-${pad(x.getUTCDate())}`;
  return { from: f(mon), to: f(sun) };
}
