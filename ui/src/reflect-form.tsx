import { useEffect, useMemo, useState } from "preact/hooks";
import { api, type ErgTag, type Question, type Reflection } from "./api";
import { ErrorBox, Field } from "./components";
import { useAction } from "./lib";
import { Markdown } from "./markdown";
import {
  askedScales,
  buildAnswers,
  ERG_LABELS,
  hasAnswers,
  initialForms,
  parseAnswered,
  parseStructured,
  SCALE_LABELS,
  type QuestionForm,
} from "./reflect-util";

const KIND_LABELS: Record<Question["kind"], string> = {
  checkin: "チェックイン",
  hassle: "ハッスル",
  abc: "ABC",
  free: "自由記述",
};

/** 1〜5 のボタン。もう一度押すと選択を外す。 */
function Scale({ value, onChange, label }: { value?: number; onChange: (n: number | undefined) => void; label: string }) {
  return (
    <div class="field">
      <span>{label}</span>
      <div class="scale" role="group" aria-label={label}>
        {[1, 2, 3, 4, 5].map((n) => (
          <button
            type="button"
            key={n}
            class={value === n ? "chip on" : "chip"}
            aria-pressed={value === n}
            onClick={() => onChange(value === n ? undefined : n)}
          >
            {n}
          </button>
        ))}
      </div>
    </div>
  );
}

/** 保存した ABC 記録について、B を掘り下げる AI の問い（既存の `abc dispute`）を続けて聞く。 */
function AbcFollowUp({ recordId }: { recordId: string }) {
  const act = useAction();
  const [dialog, setDialog] = useState("");
  return (
    <div class="stack">
      <div class="row">
        <button
          type="button"
          class="ghost"
          disabled={act.busy}
          onClick={async () => {
            const r = await act.run(() => api.abcDispute(recordId));
            if (r) setDialog(r.ai_dialog_md ?? "");
          }}
        >
          {act.busy ? "AI が考えています…" : dialog ? "問いを追加する" : "受け止め方を掘り下げる問いを AI に聞く"}
        </button>
      </div>
      {dialog && <Markdown text={dialog} />}
      <ErrorBox error={act.error} />
    </div>
  );
}

/** 構造化された振り返りの問いを、種類ごとのフォームで描画して答えを保存する。 */
export function StructuredAnswer({
  reflection,
  onSaved,
}: {
  reflection: Reflection;
  onSaved: (r: Reflection) => void;
}) {
  const out = useMemo(() => parseStructured(reflection), [reflection.structured_json]);
  const answered = useMemo(() => parseAnswered(reflection), [reflection.answers_json]);
  const answeredIdx = useMemo(() => new Set(answered.items.map((i) => i.index)), [answered]);
  const [forms, setForms] = useState<QuestionForm[]>(() => (out ? initialForms(out) : []));
  const [saved, setSaved] = useState("");
  const save = useAction();

  useEffect(() => {
    setForms(out ? initialForms(out) : []);
    setSaved("");
  }, [reflection.id]);

  if (!out) return null;
  const patch = (i: number, p: Partial<QuestionForm>) => {
    setForms((fs) => fs.map((f, j) => (j === i ? { ...f, ...p } : f)));
    setSaved("");
  };
  const submit = async (e: Event) => {
    e.preventDefault();
    const r = await save.run(() => api.reflectAnswerStructured(reflection.id, buildAnswers(out.questions, forms, answeredIdx)));
    if (r) {
      setSaved("回答を保存しました。チェックイン・ハッスル・ABC は「記録」画面にも出ます。");
      onSaved(r);
    }
  };
  const remaining = out.questions.some((_, i) => !answeredIdx.has(i));

  return (
    <form class="stack" onSubmit={submit}>
      {out.summary && <Markdown text={out.summary} />}
      {out.questions.map((q, i) => {
        const f = forms[i];
        const done = answered.items.find((a) => a.index === i);
        return (
          <fieldset key={i} class="stack reflect-q" aria-label={`問い ${i + 1}`}>
            <legend class="muted">
              {i + 1}. {KIND_LABELS[q.kind]}
              {done && " ・回答済み"}
            </legend>
            <p>{q.prompt}</p>
            {done ? (
              <>
                {done.text && <Markdown text={done.text} />}
                {q.kind === "abc" && done.record_id && <AbcFollowUp recordId={done.record_id} />}
              </>
            ) : (
              f && (
                <>
                  {q.kind === "checkin" && (
                    <>
                      {askedScales(q).map((s) => (
                        <Scale key={s} label={SCALE_LABELS[s]} value={f[s]} onChange={(n) => patch(i, { [s]: n })} />
                      ))}
                      {(q.erg_candidates?.length ?? 0) > 0 && (
                        <div class="field">
                          <span>ERG タグ（任意）</span>
                          <div class="row" role="group" aria-label="ERG タグ">
                            {q.erg_candidates!.map((t: ErgTag) => (
                              <button
                                type="button"
                                key={t}
                                class={f.erg === t ? "chip on" : "chip"}
                                aria-pressed={f.erg === t}
                                onClick={() => patch(i, { erg: f.erg === t ? undefined : t })}
                              >
                                {ERG_LABELS[t]}
                              </button>
                            ))}
                          </div>
                        </div>
                      )}
                      <Field label="メモ（任意）">
                        <input value={f.note} onInput={(e) => patch(i, { note: e.currentTarget.value })} />
                      </Field>
                    </>
                  )}
                  {q.kind === "hassle" && (
                    <>
                      {(q.examples?.length ?? 0) > 0 && <p class="muted">例: {q.examples!.join(" / ")}</p>}
                      <Field label="気になったこと">
                        <textarea rows={3} value={f.text} onInput={(e) => patch(i, { text: e.currentTarget.value })} />
                      </Field>
                      <Scale label="強さ" value={f.intensity} onChange={(n) => patch(i, { intensity: n })} />
                    </>
                  )}
                  {q.kind === "abc" && (
                    <>
                      {(q.examples?.length ?? 0) > 0 && <p class="muted">例: {q.examples!.join(" / ")}</p>}
                      <Field label="A: 出来事（AI の候補です。自由に書き換えられます）">
                        <textarea
                          rows={2}
                          value={f.activating_event}
                          onInput={(e) => patch(i, { activating_event: e.currentTarget.value })}
                        />
                      </Field>
                      <Field label={`B: 受け止め方${q.belief_prompt ? `（${q.belief_prompt}）` : ""}`}>
                        <textarea rows={2} value={f.belief} onInput={(e) => patch(i, { belief: e.currentTarget.value })} />
                      </Field>
                      <Field label="C: 感情">
                        <input value={f.emotion} onInput={(e) => patch(i, { emotion: e.currentTarget.value })} />
                      </Field>
                      <Scale label="感情の強さ" value={f.emotion_intensity} onChange={(n) => patch(i, { emotion_intensity: n })} />
                      <Field label="別の捉え方・反論（任意）">
                        <textarea rows={2} value={f.dispute} onInput={(e) => patch(i, { dispute: e.currentTarget.value })} />
                      </Field>
                    </>
                  )}
                  {q.kind === "free" && (
                    <Field label="いまの気持ちや考えたこと（Markdown）">
                      <textarea rows={4} value={f.text} onInput={(e) => patch(i, { text: e.currentTarget.value })} />
                    </Field>
                  )}
                </>
              )
            )}
          </fieldset>
        );
      })}
      {remaining && (
        <div class="row">
          <button class="primary" disabled={save.busy || !hasAnswers(out.questions, forms, answeredIdx)}>
            回答を保存
          </button>
          <span class="muted">答えた問いだけが保存されます。残りは後から答えられます。</span>
        </div>
      )}
      {saved && (
        <span class="muted" role="status">
          {saved}
        </span>
      )}
      <ErrorBox error={save.error} />
    </form>
  );
}
