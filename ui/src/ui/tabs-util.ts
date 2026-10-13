// タブ（role="tablist"）の純粋ロジック。表示に依存しない。

/** URL の `?tab=` の値を、使えるタブの ID に直す。使えない値・未指定は `fallback`。 */
export function tabFromParam(param: string | null | undefined, ids: readonly string[], fallback: string): string {
  return param && ids.includes(param) ? param : fallback;
}

/** ←→ Home End で動かした先のタブ ID。関係のないキーは undefined。端では回り込む。 */
export function stepTab(ids: readonly string[], current: string, key: string): string | undefined {
  if (ids.length === 0) return undefined;
  const i = Math.max(0, ids.indexOf(current));
  switch (key) {
    case "ArrowRight":
      return ids[(i + 1) % ids.length];
    case "ArrowLeft":
      return ids[(i - 1 + ids.length) % ids.length];
    case "Home":
      return ids[0];
    case "End":
      return ids[ids.length - 1];
    default:
      return undefined;
  }
}
