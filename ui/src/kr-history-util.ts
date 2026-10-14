// 成果の指標の現在値の履歴（値の推移と週ごとの時間を同じ時間軸で並べる）の純粋ロジック。画面に依存しない。
// 目標値との比較や評価はしない。並べるだけ。
import type { KrHistoryWeek } from "./api";

/** 週の行に出す現在値。記録が無い週は「-」。 */
export function weekValueText(w: Pick<KrHistoryWeek, "text">): string {
  return w.text ?? "-";
}

/**
 * 折れ線の y 座標（0 = 下、1 = 上）。数値のある週の最小〜最大を 0〜1 に伸ばす。
 * 数値が無い週は null、数値が 1 種類だけなら中央（0.5）。
 */
export function valueLevels(weeks: Pick<KrHistoryWeek, "value">[]): (number | null)[] {
  const vals = weeks.map((w) => w.value).filter((v): v is number => v !== null);
  if (vals.length === 0) return weeks.map(() => null);
  const min = Math.min(...vals);
  const max = Math.max(...vals);
  return weeks.map((w) => {
    if (w.value === null) return null;
    return max === min ? 0.5 : (w.value - min) / (max - min);
  });
}

export interface ChartPoint {
  x: number;
  y: number;
}

/**
 * `valueLevels` を SVG の座標に直し、値の無い週で線を切った区間の列にする。
 * 1 週の幅を `colW`、描画の高さを `height`（上下に `pad` の余白）とする。
 */
export function valueSegments(
  levels: (number | null)[],
  colW: number,
  height: number,
  pad = 4,
): ChartPoint[][] {
  const out: ChartPoint[][] = [];
  let cur: ChartPoint[] = [];
  levels.forEach((lv, i) => {
    if (lv === null) {
      if (cur.length > 0) out.push(cur);
      cur = [];
      return;
    }
    cur.push({ x: i * colW + colW / 2, y: pad + (1 - lv) * (height - pad * 2) });
  });
  if (cur.length > 0) out.push(cur);
  return out;
}

/** 履歴にも時間にも何も無いか（グラフを出さない判定）。 */
export function isEmptyHistory(weeks: KrHistoryWeek[]): boolean {
  return weeks.every((w) => w.duration_ms === 0 && w.text === null && w.changes === 0);
}
