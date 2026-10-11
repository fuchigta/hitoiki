// ショートカット入力欄の補助。キーを押した組み合わせを、設定に保存する文字列（`Ctrl+Alt+Space`）にする。
// 押された組み合わせを 1 回読み取るだけで、記録や保存はしない（設定として確定したものだけが残る）。

export interface KeyLike {
  code: string;
  ctrlKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
  metaKey: boolean;
}

/** `KeyboardEvent.code` を主キーの名前にする（使えないキーは null）。 */
export function mainKey(code: string): string | null {
  let m = /^Key([A-Z])$/.exec(code) ?? /^Digit([0-9])$/.exec(code);
  if (m) return m[1];
  m = /^F([0-9]{1,2})$/.exec(code);
  if (m && Number(m[1]) >= 1 && Number(m[1]) <= 24) return `F${m[1]}`;
  const named: Record<string, string> = {
    Space: "Space",
    Enter: "Enter",
    Tab: "Tab",
    Backspace: "Backspace",
    Delete: "Delete",
    Insert: "Insert",
    Home: "Home",
    End: "End",
    PageUp: "PageUp",
    PageDown: "PageDown",
    ArrowUp: "Up",
    ArrowDown: "Down",
    ArrowLeft: "Left",
    ArrowRight: "Right",
  };
  return named[code] ?? null;
}

/** 押されたキーから設定の文字列を作る。修飾キーが無い・主キーが無い（修飾キーだけ）ときは null。 */
export function shortcutFromKey(e: KeyLike): string | null {
  const key = mainKey(e.code);
  if (!key) return null;
  const mods: string[] = [];
  if (e.ctrlKey) mods.push("Ctrl");
  if (e.altKey) mods.push("Alt");
  if (e.shiftKey) mods.push("Shift");
  if (e.metaKey) mods.push("Super");
  if (mods.length === 0) return null;
  return [...mods, key].join("+");
}
