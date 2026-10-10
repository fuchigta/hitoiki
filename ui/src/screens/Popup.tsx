import { useEffect, useState } from "preact/hooks";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { api, on, type PushAction, type PushEvent } from "../api";
import { ErrorBox } from "../components";
import { useAction } from "../lib";

/** 入力欄だけの通知用の送信ボタン（トーストの `text` と同じ）。 */
const SUBMIT_TEXT: PushAction = { id: "text", label: "ひとこと残す", value: null };

/**
 * トーストを出せないときのフォールバック（小さなポップアップ窓）。ウインドウラベル `popup` で描画される。
 * 通知の `actions` / `input` / `open_screen` をそのまま描画する汎用の表示で、応答は core が処理する。
 */
export function Popup() {
  const [ev, setEv] = useState<PushEvent | null>(null);
  const [note, setNote] = useState("");
  const act = useAction();

  useEffect(() => {
    api.pendingPush().then(setEv).catch(() => {});
    const off = on<PushEvent>("push", (e) => {
      setEv(e);
      setNote("");
    }).catch(() => () => {});
    return () => {
      off.then((f) => f());
    };
  }, []);

  const close = () => getCurrentWindow().close().catch(() => {});
  const p = ev?.payload;
  const actions = p ? (p.actions.length === 0 && p.input ? [SUBMIT_TEXT] : p.actions) : [];

  const respond = (a: PushAction | null) =>
    act.run(() =>
      api.pushRespond({
        push_id: ev?.push_id ?? null,
        action: a?.id ?? null,
        value: a?.value ?? null,
        input: note.trim() || undefined,
        open_screen: p?.open_screen ?? null,
      }),
    );

  if (!p) {
    return (
      <div class="popup">
        <p>いまのところ通知はありません。</p>
        <div class="row">
          <span class="spacer" />
          <button class="ghost" onClick={close}>
            閉じる
          </button>
        </div>
      </div>
    );
  }

  return (
    <div class="popup">
      <h1>{p.title}</h1>
      <p>{p.body}</p>
      {actions.length > 0 && (
        <div class="actions">
          {actions.map((a, i) => (
            <button key={`${a.id}:${a.value ?? i}`} class="chip" disabled={act.busy} onClick={() => respond(a)}>
              {a.label}
            </button>
          ))}
        </div>
      )}
      {p.input && (
        <input
          value={note}
          onInput={(e) => setNote(e.currentTarget.value)}
          placeholder={p.input.placeholder}
          aria-label={p.input.placeholder}
        />
      )}
      <ErrorBox error={act.error} />
      <div class="row">
        {p.open_screen && (
          <button class="ghost" disabled={act.busy} onClick={() => respond({ id: "open", label: "", value: null })}>
            開く
          </button>
        )}
        <span class="spacer" />
        <button class="ghost" onClick={close}>
          あとで
        </button>
      </div>
    </div>
  );
}
