// Help（「?」の吹き出し）の開閉の状態。押して固定、hover / focus で一時的に開く。

export interface HelpState {
  hover: boolean;
  focus: boolean;
  pinned: boolean;
}

export const HELP_CLOSED: HelpState = { hover: false, focus: false, pinned: false };

export function helpOpen(s: HelpState): boolean {
  return s.hover || s.focus || s.pinned;
}

/** Esc ですべて閉じる。 */
export function helpEscape(): HelpState {
  return HELP_CLOSED;
}

/** ボタンを押したときの切り替え。開いているなら閉じ、閉じているなら固定して開く。 */
export function helpToggle(s: HelpState): HelpState {
  return helpOpen(s) ? HELP_CLOSED : { ...s, pinned: true };
}
