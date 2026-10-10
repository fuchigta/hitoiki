import { useEffect, useState } from "preact/hooks";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { api, on, type PushPayload } from "../api";
import { ErrorBox } from "../components";
import { useAction } from "../lib";

/** トーストを出せないときのフォールバック（小さなポップアップ窓）。ウインドウラベル `popup` で描画される。 */
export function Popup() {
  const [p, setP] = useState<PushPayload | null>(null);
  const [note, setNote] = useState("");
  const act = useAction();

  useEffect(() => {
    api.pendingPush().then(setP).catch(() => {});
    const off = on<PushPayload>("push", setP).catch(() => () => {});
    return () => {
      off.then((f) => f());
    };
  }, []);

  const close = () => getCurrentWindow().close().catch(() => {});
  // 既定: エネルギー 1〜5 のチェックイン。
  const actions = p?.actions?.length
    ? p.actions
    : [1, 2, 3, 4, 5].map((n) => ({ id: `energy:${n}`, label: String(n) }));

  return (
    <div class="popup">
      <h1>{p?.title || "ひといき"}</h1>
      <p>{p?.body || "いまのエネルギーは？"}</p>
      <div class="scale">
        {actions.map((a) => (
          <button
            key={a.id}
            class="chip"
            disabled={act.busy}
            onClick={async () => {
              await act.run(() => api.pushRespond(a.id, note.trim() || undefined));
            }}
          >
            {a.label}
          </button>
        ))}
      </div>
      <input value={note} onInput={(e) => setNote(e.currentTarget.value)} placeholder={p?.input?.placeholder || "一言メモ（任意）"} />
      <ErrorBox error={act.error} />
      <div class="row">
        <span class="spacer" />
        <button class="ghost" onClick={close}>
          あとで
        </button>
      </div>
    </div>
  );
}
