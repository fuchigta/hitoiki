import { useState } from "preact/hooks";
import { api } from "./api";
import { ErrorBox, Field, Section } from "./components";
import { dateInput, useAction } from "./lib";
import { ABC_HELP, REFLECTION_KIND_LABEL, isReflectionKind, type NewKind, type RecordSource } from "./records-util";
import { Scale } from "./ui/parts";

// 記録の作成フォーム（`#/records/new/<種類>`）。保存したら `onCreated` で作った記録を渡し、画面がその記録を選択する。

export function NewRecord({ kind, onCreated }: { kind: NewKind; onCreated: (s: RecordSource) => void }) {
  if (kind === "condition") return <NewCondition onCreated={onCreated} />;
  if (kind === "concern") return <NewConcern onCreated={onCreated} />;
  if (kind === "reframe") return <NewReframe onCreated={onCreated} />;
  if (isReflectionKind(kind)) return <NewReflection kind={kind} onCreated={onCreated} />;
  return null;
}

function SaveRow({ busy, ok, label = "記録" }: { busy: boolean; ok: boolean; label?: string }) {
  return (
    <div class="row">
      <span class="spacer" />
      <button class="primary" disabled={busy || !ok}>
        {label}
      </button>
    </div>
  );
}

function NewCondition({ onCreated }: { onCreated: (s: RecordSource) => void }) {
  const [energy, setEnergy] = useState<number>();
  const [mood, setMood] = useState<number>();
  const [stress, setStress] = useState<number>();
  const [note, setNote] = useState("");
  const act = useAction();
  return (
    <Section title="調子を記録">
      <form
        class="stack"
        onSubmit={async (e) => {
          e.preventDefault();
          const r = await act.run(() => api.conditionAdd({ energy, mood, stress, note: note.trim() || undefined }));
          if (r) onCreated({ kind: "condition", rec: r });
        }}
      >
        <Scale label="エネルギー" value={energy} onChange={setEnergy} />
        <Scale label="気分" value={mood} onChange={setMood} />
        <Scale label="ストレス" value={stress} onChange={setStress} />
        <Field label="一言メモ">
          <input value={note} onInput={(e) => setNote(e.currentTarget.value)} />
        </Field>
        <SaveRow busy={act.busy} ok={energy != null || mood != null || stress != null || !!note.trim()} />
        <ErrorBox error={act.error} />
      </form>
    </Section>
  );
}

function NewConcern({ onCreated }: { onCreated: (s: RecordSource) => void }) {
  const [text, setText] = useState("");
  const [intensity, setIntensity] = useState<number | undefined>(3);
  const act = useAction();
  return (
    <Section title="気になったことを記録">
      <form
        class="stack"
        onSubmit={async (e) => {
          e.preventDefault();
          const r = await act.run(() => api.concernAdd(text.trim(), intensity));
          if (r) onCreated({ kind: "concern", rec: r });
        }}
      >
        <Field label="気になったこと">
          <textarea rows={3} value={text} onInput={(e) => setText(e.currentTarget.value)} />
        </Field>
        <Scale label="強さ" value={intensity} onChange={setIntensity} />
        <SaveRow busy={act.busy} ok={!!text.trim()} />
        <ErrorBox error={act.error} />
      </form>
    </Section>
  );
}

function NewReframe({ onCreated }: { onCreated: (s: RecordSource) => void }) {
  const [a, setA] = useState("");
  const [b, setB] = useState("");
  const [c, setC] = useState("");
  const [ci, setCi] = useState<number>();
  const act = useAction();
  return (
    <Section title="捉え直し（ABC）を書く" help={ABC_HELP}>
      <form
        class="stack"
        onSubmit={async (e) => {
          e.preventDefault();
          const r = await act.run(() =>
            api.reframeAdd({
              event: a.trim(),
              belief: b.trim() || undefined,
              emotion: c.trim() || undefined,
              emotion_intensity: ci,
            }),
          );
          if (r) onCreated({ kind: "reframe", rec: r });
        }}
      >
        <Field label="A: 出来事">
          <textarea rows={3} value={a} onInput={(e) => setA(e.currentTarget.value)} />
        </Field>
        <Field label="B: 受け止め方">
          <textarea rows={3} value={b} onInput={(e) => setB(e.currentTarget.value)} />
        </Field>
        <Field label="C: 感情">
          <input value={c} onInput={(e) => setC(e.currentTarget.value)} />
        </Field>
        <Scale label="強さ" value={ci} onChange={setCi} />
        <SaveRow busy={act.busy} ok={!!a.trim()} />
        <ErrorBox error={act.error} />
      </form>
    </Section>
  );
}

const REFLECT_HINTS = {
  day_start: "昨日までを材料に、今日のはじまりを考えます。",
  day_end: "今日を一緒にふりかえります。",
  weekly: "今週の時間の使い方をふりかえります。",
} as const;

/** 振り返りの作成。基準日を選んで問いを生成すると、その振り返りが選択され、詳細で答えられる。 */
function NewReflection({ kind, onCreated }: { kind: keyof typeof REFLECT_HINTS; onCreated: (s: RecordSource) => void }) {
  const [date, setDate] = useState(dateInput());
  const gen = useAction();
  return (
    <Section title={`${REFLECTION_KIND_LABEL[kind]}の振り返りを始める`}>
      <p class="hint">
        {REFLECT_HINTS[kind]}
        {kind === "day_start" && (
          <>
            {" "}
            <a href="#/today/design">今日のデザイン</a>でブロックを並べることもできます。
          </>
        )}
      </p>
      <form
        class="stack"
        onSubmit={async (e) => {
          e.preventDefault();
          const r = await gen.run(() => api.reflectGenerate(kind, date));
          if (r) onCreated({ kind: "reflection", rec: r });
        }}
      >
        <div class="row">
          <Field label={kind === "weekly" ? "基準日（その週）" : "基準日"}>
            <input type="date" value={date} onInput={(e) => setDate(e.currentTarget.value)} />
          </Field>
          <button class="primary" disabled={gen.busy || !date}>
            {gen.busy ? "生成中…" : "問いを生成"}
          </button>
        </div>
        {gen.busy && (
          <p class="hint" role="status">
            AI が問いを考えています。数十秒かかることがあります。他の画面を使っていても大丈夫です。
          </p>
        )}
        <ErrorBox error={gen.error} />
      </form>
    </Section>
  );
}
