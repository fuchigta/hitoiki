import type { ComponentChildren } from "preact";
import { useEffect, useState } from "preact/hooks";
import { api, type Reflection } from "./api";
import { ErrorBox, Field, Section } from "./components";
import { ErgSummary } from "./erg-label";
import { ERG_LEGEND, weekOf } from "./erg-util";
import { dateInput, fmtFull, useAction, useAsync } from "./lib";
import { Markdown } from "./markdown";
import { StructuredAnswer } from "./reflect-form";
import { parseStructured } from "./reflect-util";
import { REFLECTION_KIND_LABEL } from "./records-util";

/** 振り返り 1 件の詳細。構造化された問いは種類ごとのフォームで、そうでないものは Markdown の回答欄で答える。 */
export function ReflectionDetail({
  r,
  erg,
  onChanged,
}: {
  r: Reflection;
  erg?: ComponentChildren;
  /** 回答を保存した後（一覧と、回答で作られた記録を読み直す）。 */
  onChanged: () => void;
}) {
  const structured = parseStructured(r);
  return (
    <Section title={`${REFLECTION_KIND_LABEL[r.kind]}の振り返り`}>
      <span class="meta">{fmtFull(r.at)}</span>
      {structured ? (
        <StructuredAnswer reflection={r} onSaved={onChanged} />
      ) : (
        <MarkdownAnswer r={r} onSaved={onChanged} />
      )}
      {erg}
      {r.kind === "weekly" && <WeekErg at={r.at} />}
    </Section>
  );
}

/** 週次の振り返りに、その週の ERG の分布（件数）を添える。 */
function WeekErg({ at }: { at: number }) {
  const week = weekOf(dateInput(at));
  const erg = useAsync(
    () => (week ? api.ergOverview(week) : Promise.resolve(null)),
    [week?.from],
  );
  if (!week) return null;
  return (
    <Section level={3} title="この週の記録の ERG（件数）"
      help={`ERG は「${ERG_LEGEND}」の 3 つの観点です。記録の内容から AI が判定し、確信が持てないものは確認待ちになります。`}
    >
      <ErrorBox error={erg.error} />
      {erg.data && (
        <ErgSummary
          distribution={erg.data.distribution}
          range={week}
          unconfirmedTotal={erg.data.unconfirmed_total}
          onChanged={erg.reload}
        />
      )}
    </Section>
  );
}

/** 構造化されていない（従来の）振り返り。問いは Markdown で、回答は Markdown の 1 つの欄。 */
function MarkdownAnswer({ r, onSaved }: { r: Reflection; onSaved: () => void }) {
  const [answer, setAnswer] = useState(r.answers_md ?? "");
  const [saved, setSaved] = useState("");
  const save = useAction();
  useEffect(() => {
    setAnswer(r.answers_md ?? "");
    setSaved("");
  }, [r.id]);
  return (
    <>
      <Markdown text={r.questions_md} />
      <form
        class="stack"
        onSubmit={async (e) => {
          e.preventDefault();
          const out = await save.run(() => api.reflectAnswer(r.id, answer));
          if (out) {
            setSaved("回答を保存しました。");
            onSaved();
          }
        }}
      >
        <Field label="いまの気持ちや考えたこと（Markdown）">
          <textarea
            rows={8}
            value={answer}
            onInput={(e) => {
              setAnswer(e.currentTarget.value);
              setSaved("");
            }}
          />
        </Field>
        <div class="row">
          <button class="primary" disabled={save.busy || !answer.trim()}>
            回答を保存
          </button>
          {saved && (
            <span class="meta" role="status">
              {saved}
            </span>
          )}
        </div>
        <ErrorBox error={save.error} />
      </form>
    </>
  );
}
