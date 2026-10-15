import { useEffect, useState } from "preact/hooks";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { api, on, type NudgeEvent } from "../api";
import { NudgeAnswer } from "../nudge-answer";

/**
 * トーストを出せないときのフォールバック（小さなポップアップ窓）。ウインドウラベル `popup` で描画される。
 * 回答 UI は今日画面の「未回答の問いかけ」と共有（`nudge-answer.tsx`）。応答は core が処理する。
 */
export function Popup() {
  const [ev, setEv] = useState<NudgeEvent | null>(null);

  useEffect(() => {
    api.pendingNudge().then(setEv).catch(() => {});
    const off = on<NudgeEvent>("nudge", (e) => setEv(e)).catch(() => () => {});
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
      <NudgeAnswer key={ev?.nudge_id ?? "test"} nudgeId={ev?.nudge_id ?? null} payload={p} onDone={close}>
        <button class="ghost" onClick={close}>
          あとで
        </button>
      </NudgeAnswer>
    </div>
  );
}
