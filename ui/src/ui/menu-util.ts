// OverflowMenu のキーボード操作のロジック（DOM に依存しない部分）。

export interface MenuItemLike {
  disabled?: boolean;
}

/** 次に選ぶ項目の位置。無効な項目は飛ばし、端では反対側へ回る。選べる項目がなければ -1。 */
export function stepMenu(items: MenuItemLike[], from: number, dir: 1 | -1): number {
  const n = items.length;
  if (n === 0) return -1;
  for (let i = 1; i <= n; i++) {
    const idx = (((from + dir * i) % n) + n) % n;
    if (!items[idx].disabled) return idx;
  }
  return -1;
}

/** 最初に選べる項目の位置。なければ -1。 */
export function firstEnabled(items: MenuItemLike[]): number {
  return items.findIndex((it) => !it.disabled);
}

/** 最後に選べる項目の位置。なければ -1。 */
export function lastEnabled(items: MenuItemLike[]): number {
  for (let i = items.length - 1; i >= 0; i--) if (!items[i].disabled) return i;
  return -1;
}

/** メニュー内のキー操作が何をするか。`null` は何もしない（既定の動作に任せる）。 */
export type MenuAction = { type: "move"; to: number } | { type: "close" } | { type: "select"; at: number } | null;

export function menuKey(items: MenuItemLike[], current: number, key: string): MenuAction {
  switch (key) {
    case "ArrowDown":
      return { type: "move", to: stepMenu(items, current, 1) };
    case "ArrowUp":
      return { type: "move", to: stepMenu(items, current < 0 ? 0 : current, -1) };
    case "Home":
      return { type: "move", to: firstEnabled(items) };
    case "End":
      return { type: "move", to: lastEnabled(items) };
    case "Escape":
      return { type: "close" };
    case "Enter":
    case " ":
      return current >= 0 && !items[current]?.disabled ? { type: "select", at: current } : null;
    default:
      return null;
  }
}
