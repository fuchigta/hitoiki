// 1〜5 の選択（Scale）のロジック。

export const SCALE_VALUES = [1, 2, 3, 4, 5] as const;

/** 押した値の次の状態。同じ値をもう一度押すと選択を外す。 */
export function toggleScale(value: number | undefined, n: number): number | undefined {
  return value === n ? undefined : n;
}

/** ←→（↑↓）で動かしたときの次の値。未選択からは端の値になる。端では止まる。他のキーは `undefined`。 */
export function stepScale(value: number | undefined, key: string): number | undefined {
  const min = SCALE_VALUES[0];
  const max = SCALE_VALUES[SCALE_VALUES.length - 1];
  if (key === "ArrowRight" || key === "ArrowDown") return value === undefined ? min : Math.min(max, value + 1);
  if (key === "ArrowLeft" || key === "ArrowUp") return value === undefined ? max : Math.max(min, value - 1);
  return undefined;
}

/** フォームの文字列（"" = 未入力）と数値の相互変換。 */
export function scaleFromString(s: string): number | undefined {
  return s ? Number(s) : undefined;
}
export function scaleToString(n: number | undefined | null): string {
  return n == null ? "" : String(n);
}
