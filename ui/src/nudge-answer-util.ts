// 問いかけへの回答 UI の純粋なロジック。今日画面の「未回答の問いかけ」とポップアップ窓で共有する。
import type { Millis, NudgeAction, NudgeInput, NudgePayload } from "./api";

/** 入力欄だけの問いかけ用の送信ボタンの `action`（トーストの `text` と同じ）。 */
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
  input: NudgeInput | null;
  /** ボタンとは別に「開く」を出すか（画面を開く `open` ボタンがすでにあれば出さない）。 */
  canOpen: boolean;
}

/** 問いかけの `actions` / `input` からフォームの定義を作る。入力欄だけの問いかけには送信ボタンを足す。 */
export function buildForm(p: NudgePayload): FormDef {
  const actions: NudgeAction[] = p.actions;
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

/** `nudge_respond` コマンドの引数。 */
export interface RespondArgs {
  nudge_id: string | null;
  action: string | null;
  value: number | null;
  input?: string;
  open_screen: string | null;
}

/** ボタン（`null` は本体＝「開く」）が押されたときの引数。一言は空白を除いて、あれば付ける。 */
export function respondArgs(
  nudgeId: string | null,
  p: NudgePayload,
  button: { action: string; value: number | null } | null,
  note: string,
): RespondArgs {
  const text = note.trim();
  const action = button?.action ?? "open";
  return {
    nudge_id: nudgeId,
    action,
    value: button?.value ?? null,
    input: text ? text : undefined,
    open_screen: action === "open" ? p.open_screen : null,
  };
}

/** 「答えない」（見送り）の引数。 */
export function dismissArgs(nudgeId: string): RespondArgs {
  return { nudge_id: nudgeId, action: "dismiss", value: null, open_screen: null };
}

/** idle_return など対象期間のある問いかけの「HH:MM〜HH:MM」。無ければ null。 */
export function periodLabel(p: NudgePayload | null | undefined, fmt: (ms: Millis) => string): string | null {
  const r = p?.idle_range;
  return r ? `${fmt(r[0])}〜${fmt(r[1])}` : null;
}
