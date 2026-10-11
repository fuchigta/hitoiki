import { useEffect, useState } from "preact/hooks";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { api, on, type PushEvent } from "../api";
import { PushAnswer } from "../push-answer";

/**
 * トーストを出せないときのフォールバック（小さなポップアップ窓）。ウインドウラベル `popup` で描画される。
 * 回答 UI は今日画面の「未回答の問いかけ」と共有（`push-answer.tsx`）。応答は core が処理する。
 */
export function Popup() {
  const [ev, setEv] = useState<PushEvent | null>(null);

  useEffect(() => {
    api.pendingPush().then(setEv).catch(() => {});
    const off = on<PushEvent>("push", (e) => setEv(e)).catch(() => () => {});
    return () => {
      off.then((f) => f());
    };
  }, []);

  const close = () => getCurrentWindow().close().catch(() => {});
  const p = ev?.payload;

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
      <PushAnswer key={ev?.push_id ?? "test"} pushId={ev?.push_id ?? null} payload={p} onDone={close}>
        <button class="ghost" onClick={close}>
          あとで
        </button>
      </PushAnswer>
    </div>
  );
}
