// 通知（プッシュ）への回答 UI の純粋なロジック。今日画面の「未回答の問いかけ」とポップアップ窓で共有する。
import type { Millis, PushAction, PushInput, PushPayload } from "./api";

/** 入力欄だけの通知用の送信ボタンの `action`（トーストの `text` と同じ）。 */
export const TEXT_ACTION = "text";

export interface FormButton {
  /** 描画用のキー。 */
  key: string;
  label: string;
  action: string;
  value: number | null;
}

export interface FormDef {
  /** 通知と同じ選択肢（エネルギー 1〜5、無操作の分類、画面を開くボタンなど）。 */
  buttons: FormButton[];
  /** 一言入力欄（あれば）。 */
  input: PushInput | null;
  /** ボタンとは別に「開く」を出すか（画面を開く `open` ボタンがすでにあれば出さない）。 */
  canOpen: boolean;
}

/** 通知の `actions` / `input` からフォームの定義を作る。入力欄だけの通知には送信ボタンを足す。 */
export function buildForm(p: PushPayload): FormDef {
  const actions: PushAction[] = p.actions;
  const buttons: FormButton[] = actions.map((a, i) => ({
    key: `${a.id}:${a.value ?? i}`,
    label: a.label,
    action: a.id,
    value: a.value,
  }));
  if (buttons.length === 0 && p.input) {
    buttons.push({ key: TEXT_ACTION, label: "ひとこと残す", action: TEXT_ACTION, value: null });
  }
  return {
    buttons,
    input: p.input,
    canOpen: !!p.open_screen && !actions.some((a) => a.id === "open"),
  };
}

/** `push_respond` コマンドの引数。 */
export interface RespondArgs {
  push_id: string | null;
  action: string | null;
  value: number | null;
  input?: string;
  open_screen: string | null;
}

/** ボタン（`null` は本体＝「開く」）が押されたときの引数。一言は空白を除いて、あれば付ける。 */
export function respondArgs(
  pushId: string | null,
  p: PushPayload,
  button: { action: string; value: number | null } | null,
  note: string,
): RespondArgs {
  const text = note.trim();
  const action = button?.action ?? "open";
  return {
    push_id: pushId,
    action,
    value: button?.value ?? null,
    input: text ? text : undefined,
    open_screen: action === "open" ? p.open_screen : null,
  };
}

/** 「答えない」（見送り）の引数。 */
export function dismissArgs(pushId: string): RespondArgs {
  return { push_id: pushId, action: "dismiss", value: null, open_screen: null };
}

/** idle_return など対象期間のある通知の「HH:MM〜HH:MM」。無ければ null。 */
export function periodLabel(p: PushPayload | null | undefined, fmt: (ms: Millis) => string): string | null {
  const r = p?.idle_range;
  return r ? `${fmt(r[0])}〜${fmt(r[1])}` : null;
}
