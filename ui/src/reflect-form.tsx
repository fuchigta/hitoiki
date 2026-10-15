import { useEffect, useMemo, useState } from "preact/hooks";
import { api, type AnswerDetail, type Question, type Reflection } from "./api";
import { ErrorBox, Field } from "./components";
import { useAction } from "./lib";
import { Help, Scale } from "./ui/parts";
import { Markdown } from "./markdown";
import {
  askedScales,
  buildAnswers,
  detailLines,
  hasAnswers,
  initialForms,
  parseAnswered,
  parseStructured,
  SCALE_LABELS,
  type QuestionForm,
} from "./reflect-util";

const KIND_LABELS: Record<Question["kind"], string> = {
  condition: "調子",
  concern: "気になったこと",
  reframe: "捉え直し",
  free: "自由記述",
};

/** 保存した捉え直しについて、B を掘り下げる AI の問い（既存の `reframe deepen`）を続けて聞く。 */
function ReframeFollowUp({ recordId }: { recordId: string }) {
  const act = useAction();
  const [dialog, setDialog] = useState("");
  return (
    <div class="stack">
      <div class="row">
        <button
          type="button"
          disabled={act.busy}
          onClick={async () => {
            const r = await act.run(() => api.reframeDeepen(recordId));
            if (r) setDialog(r.deepening_md ?? "");
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
  // 回答済みの問いに、保存した調子・気になったこと・捉え直しの中身を並べて見せる。
  const [details, setDetails] = useState<AnswerDetail[] | null>(null);
  const detailOf = (i: number) => details?.find((d) => d.index === i);
  useEffect(() => {
    let live = true;
    setDetails(null);
    if (answered.items.length > 0) {
      api
        .reflectAnswerDetails(reflection.id)
        .then((d) => live && setDetails(d))
        .catch(() => {});
    }
    return () => {
      live = false;
    };
  }, [reflection.id, reflection.answers_json]);

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
      setSaved("回答を保存しました。調子・気になったこと・捉え直しは「記録」の一覧にも出ます。");
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
            <legend class="meta">
              {i + 1}. {KIND_LABELS[q.kind]}
              {done && " ・回答済み"}
            </legend>
            <p>{q.prompt}</p>
            {done ? (
              <>
                {done.text && <Markdown text={done.text} />}
                {detailOf(i) && (
                  <ul class="plain" aria-label="保存した回答">
                    {detailLines(detailOf(i)!).map((l) => (
                      <li key={l}>{l}</li>
                    ))}
                  </ul>
                )}
                {detailOf(i)?.missing && <p class="hint">この回答の記録は削除されています。</p>}
                {q.kind === "reframe" && done.record_id && !detailOf(i)?.missing && <ReframeFollowUp recordId={done.record_id} />}
              </>
            ) : (
              f && (
                <>
                  {q.kind === "condition" && (
                    <>
                      {askedScales(q).map((s) => (
                        <Scale key={s} label={SCALE_LABELS[s]} value={f[s]} onChange={(n) => patch(i, { [s]: n })} />
                      ))}
                      <Field label="メモ（任意）">
                        <input value={f.note} onInput={(e) => patch(i, { note: e.currentTarget.value })} />
                      </Field>
                    </>
                  )}
                  {q.kind === "concern" && (
                    <>
                      {(q.examples?.length ?? 0) > 0 && <p class="hint">例: {q.examples!.join(" / ")}</p>}
                      <Field label="気になったこと">
                        <textarea rows={3} value={f.text} onInput={(e) => patch(i, { text: e.currentTarget.value })} />
                      </Field>
                      <Scale label="強さ" value={f.intensity} onChange={(n) => patch(i, { intensity: n })} />
                    </>
                  )}
                  {q.kind === "reframe" && (
                    <>
                      {(q.examples?.length ?? 0) > 0 && <p class="hint">例: {q.examples!.join(" / ")}</p>}
                      <Field label="A: 出来事（AI の候補です。自由に書き換えられます）">
                        <textarea
                          rows={2}
                          value={f.event}
                          onInput={(e) => patch(i, { event: e.currentTarget.value })}
                        />
                      </Field>
                      <Field label={`B: 受け止め方${q.belief_prompt ? `（${q.belief_prompt}）` : ""}`}>
                        <textarea rows={2} value={f.belief} onInput={(e) => patch(i, { belief: e.currentTarget.value })} />
                      </Field>
                      <Field label="C: 感情">
                        <input value={f.emotion} onInput={(e) => patch(i, { emotion: e.currentTarget.value })} />
                      </Field>
                      <Scale label="感情の強さ" value={f.emotion_intensity} onChange={(n) => patch(i, { emotion_intensity: n })} />
                      <Field label="別の捉え方（任意）">
                        <textarea rows={2} value={f.alternative} onInput={(e) => patch(i, { alternative: e.currentTarget.value })} />
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
          <Help>答えた問いだけが保存されます。残りは後から答えられます。</Help>
        </div>
      )}
      {saved && (
        <span class="meta" role="status">
          {saved}
        </span>
      )}
      <ErrorBox error={save.error} />
    </form>
  );
}
