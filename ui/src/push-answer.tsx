import type { ComponentChildren } from "preact";
import { useState } from "preact/hooks";
import { api } from "./api";
import type { PushPayload, PushView } from "./api";
import { ErrorBox } from "./components";
import { fmtTime, useAction } from "./lib";
import { buildForm, dismissArgs, periodLabel, respondArgs } from "./push-answer-util";

/**
 * 通知への回答 UI（選択肢のボタン・一言入力・開く・答えない）。今日画面のサイドパネルと
 * ポップアップ窓で共有する。応答は core が処理する（`push_respond`）。
 * `pushId` が無い（テスト通知）ときは「答えない」を出さない。`children` は下段の行に並べる。
 */
export function PushAnswer({
  pushId,
  payload,
  onDone,
  children,
}: {
  pushId: string | null;
  payload: PushPayload;
  onDone: () => void;
  children?: ComponentChildren;
}) {
  const [note, setNote] = useState("");
  const act = useAction();
  const form = buildForm(payload);

  const send = (args: ReturnType<typeof respondArgs>) =>
    act.run(() => api.pushRespond(args).then(() => true)).then((ok) => ok && onDone());

  return (
    <div class="push-answer">
      {form.buttons.length > 0 && (
        <div class="actions">
          {form.buttons.map((b) => (
            <button
              key={b.key}
              class="chip"
              disabled={act.busy}
              onClick={() => send(respondArgs(pushId, payload, b, note))}
            >
              {b.label}
            </button>
          ))}
        </div>
      )}
      {form.input && (
        <input
          value={note}
          onInput={(e) => setNote(e.currentTarget.value)}
          placeholder={form.input.placeholder}
          aria-label={form.input.placeholder}
        />
      )}
      <ErrorBox error={act.error} />
      <div class="row">
        {form.canOpen && (
          <button class="ghost" disabled={act.busy} onClick={() => send(respondArgs(pushId, payload, null, note))}>
            開く
          </button>
        )}
        <span class="spacer" />
        {children}
        {pushId && (
          <button class="ghost" disabled={act.busy} onClick={() => send(dismissArgs(pushId))}>
            答えない
          </button>
        )}
      </div>
    </div>
  );
}

/** 未回答の問いかけの一覧（今日の画面のサイドパネル）。 */
export function PushList({ pushes, onDone }: { pushes: PushView[]; onDone: () => void }) {
  return (
    <ul class="plain push-list">
      {pushes.map((p) => (
        <li key={p.id}>
          <div>
            <span class="meta">
              {fmtTime(p.at)} {p.trigger_label}
            </span>{" "}
            <strong>{p.title}</strong>
          </div>
          {p.body && <p class="hint">{p.body}</p>}
          {periodLabel(p.payload, fmtTime) && <p class="meta">対象の時間帯: {periodLabel(p.payload, fmtTime)}</p>}
          {p.payload && <PushAnswer pushId={p.id} payload={p.payload} onDone={onDone} />}
        </li>
      ))}
    </ul>
  );
}
