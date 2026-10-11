import { useState } from "preact/hooks";
import { api } from "./api";
import { Card, ErrorBox } from "./components";
import { useAction, useAsync } from "./lib";
import { shortcutFromKey } from "./shortcut-util";

/** 設定画面の「クイック入力」カード: ショートカットの確認・変更（再起動なしで登録し直す）。 */
export function QuickShortcutCard() {
  const st = useAsync(() => api.quickShortcutState(), []);
  const [text, setText] = useState<string | null>(null);
  const [recording, setRecording] = useState(false);
  const [msg, setMsg] = useState("");
  const act = useAction();
  const value = text ?? st.data?.shortcut ?? "";

  const save = async (v: string) => {
    setMsg("");
    const r = await act.run(() => api.quickShortcutSet(v.trim()));
    if (r) {
      setText(null);
      setMsg(r.shortcut ? `${r.shortcut} を登録しました。` : "ショートカットを無効にしました。");
      st.reload();
    }
  };

  const onKeyDown = (e: KeyboardEvent) => {
    if (!recording) return;
    if (e.key === "Escape") {
      e.preventDefault();
      setRecording(false);
      return;
    }
    // 押された組み合わせを 1 回読み取るだけ（保存するのは確定した文字列だけ）。
    e.preventDefault();
    const s = shortcutFromKey(e);
    if (s) {
      setText(s);
      setRecording(false);
    }
  };

  return (
    <Card title="クイック入力">
      <div class="stack">
        <p class="muted">
          割り込みなどのとき、どの画面からでもショートカットで小さな入力窓を開き、タスクを選ぶか新しく作ってすぐ開始できます。
          トレイのメニューからも開けます。空にすると無効になります。
        </p>
        <div class="row">
          <input
            aria-label="クイック入力のショートカット"
            value={recording ? "キーを押してください（Esc で取り消し）" : value}
            readOnly={recording}
            placeholder="Ctrl+Alt+Space"
            onInput={(e) => setText(e.currentTarget.value)}
            onKeyDown={onKeyDown}
            onBlur={() => setRecording(false)}
          />
          <button type="button" onClick={() => setRecording(true)} disabled={act.busy}>
            キーを押して記録
          </button>
          <button type="button" class="primary" onClick={() => save(value)} disabled={act.busy || text === null}>
            保存
          </button>
        </div>
        <p class="muted">
          いまの登録: {st.data?.registered ?? "なし"}。修飾キー（Ctrl / Alt / Shift / Super）を 1 つ以上含めてください。
        </p>
        {st.data?.error && !act.error && <div class="error" role="alert">{st.data.error}</div>}
        <ErrorBox error={act.error ?? st.error} />
        {msg && <p class="muted">{msg}</p>}
      </div>
    </Card>
  );
}
