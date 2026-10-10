import { useState } from "preact/hooks";
import { api, type AbcRecord } from "../api";
import { Card, Empty, ErrorBox, Field } from "../components";
import { Markdown } from "../markdown";
import { fmtDateTime, useAction, useAsync } from "../lib";

const PERIODS = [
  { id: "today", label: "今日" },
  { id: "this-week", label: "今週" },
  { id: "last-week", label: "先週" },
];

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
            onClick={() => onChange(value === n ? undefined : n)}
          >
            {n}
          </button>
        ))}
      </div>
    </div>
  );
}

export function Records() {
  const [period, setPeriod] = useState("today");
  const range = { date: period };
  const checkins = useAsync(() => api.checkinList(range), [period]);
  const hassles = useAsync(() => api.hassleList(range), [period]);
  const abcs = useAsync(() => api.abcList(range), [period]);

  return (
    <div class="stack">
      <div class="row">
        <select value={period} onChange={(e) => setPeriod(e.currentTarget.value)} aria-label="期間">
          {PERIODS.map((p) => (
            <option key={p.id} value={p.id}>
              {p.label}
            </option>
          ))}
        </select>
        <span class="muted">一覧の期間</span>
      </div>
      <div class="grid2">
        <CheckinCard onAdded={checkins.reload} />
        <HassleCard onAdded={hassles.reload} />
      </div>
      <div class="grid2">
        <Card title="チェックイン">
          <ErrorBox error={checkins.error} />
          {checkins.data?.items.length === 0 && <Empty>記録はありません。</Empty>}
          <ul class="plain">
            {checkins.data?.items.map((c) => (
              <li key={c.id}>
                <span class="muted">{fmtDateTime(c.at)}</span> エネルギー {c.energy ?? "-"}
                {c.mood != null && ` ・ 気分 ${c.mood}`}
                {c.stress != null && ` ・ ストレス ${c.stress}`}
                {c.note && <div>{c.note}</div>}
              </li>
            ))}
          </ul>
        </Card>
        <Card title="ハッスル">
          <ErrorBox error={hassles.error} />
          {hassles.data?.items.length === 0 && <Empty>記録はありません。</Empty>}
          <ul class="plain">
            {hassles.data?.items.map((h) => (
              <li key={h.id}>
                <span class="muted">{fmtDateTime(h.at)}</span> {h.text}
                <span class="muted"> （強さ {h.intensity}）</span>
              </li>
            ))}
          </ul>
        </Card>
      </div>
      <AbcCard onAdded={abcs.reload} />
      <Card title="ABC の記録">
        <ErrorBox error={abcs.error} />
        {abcs.data?.items.length === 0 && <Empty>記録はありません。</Empty>}
        <ul class="plain">
          {abcs.data?.items.map((a) => (
            <li key={a.id}>
              <span class="muted">{fmtDateTime(a.at)}</span>
              <div>A: {a.activating_event}</div>
              {a.belief && <div>B: {a.belief}</div>}
              {a.consequence_emotion && (
                <div>
                  C: {a.consequence_emotion}
                  {a.consequence_intensity != null && `（${a.consequence_intensity}）`}
                </div>
              )}
              <AbcDialog abc={a} onChanged={abcs.reload} />
            </li>
          ))}
        </ul>
      </Card>
    </div>
  );
}

/** B（受け止め方）を掘り下げる AI の問い。生成には時間がかかるので、実行中は表示を変える。 */
function AbcDialog({ abc, onChanged }: { abc: AbcRecord; onChanged: () => void }) {
  const act = useAction();
  return (
    <div class="stack">
      {abc.ai_dialog_md && <Markdown text={abc.ai_dialog_md} />}
      <div class="row">
        <button
          class="ghost"
          disabled={act.busy}
          onClick={async () => {
            if (await act.run(() => api.abcDispute(abc.id))) onChanged();
          }}
        >
          {act.busy ? "AI が考えています…" : abc.ai_dialog_md ? "問いを追加する" : "AI に問いを聞く"}
        </button>
      </div>
      <ErrorBox error={act.error} />
    </div>
  );
}

function CheckinCard({ onAdded }: { onAdded: () => void }) {
  const [energy, setEnergy] = useState<number>();
  const [mood, setMood] = useState<number>();
  const [stress, setStress] = useState<number>();
  const [note, setNote] = useState("");
  const act = useAction();
  return (
    <Card title="チェックイン">
      <Scale label="エネルギー" value={energy} onChange={setEnergy} />
      <Scale label="気分" value={mood} onChange={setMood} />
      <Scale label="ストレス" value={stress} onChange={setStress} />
      <Field label="一言メモ">
        <input value={note} onInput={(e) => setNote(e.currentTarget.value)} />
      </Field>
      <div class="row">
        <span class="spacer" />
        <button
          class="primary"
          disabled={act.busy || (energy == null && mood == null && stress == null && !note.trim())}
          onClick={async () => {
            const r = await act.run(() => api.checkinAdd({ energy, mood, stress, note: note.trim() || undefined }));
            if (r) {
              setEnergy(undefined);
              setMood(undefined);
              setStress(undefined);
              setNote("");
              onAdded();
            }
          }}
        >
          記録
        </button>
      </div>
      <ErrorBox error={act.error} />
    </Card>
  );
}

function HassleCard({ onAdded }: { onAdded: () => void }) {
  const [text, setText] = useState("");
  const [intensity, setIntensity] = useState<number | undefined>(3);
  const act = useAction();
  return (
    <Card title="ハッスルを記録">
      <Field label="気になったこと">
        <textarea rows={3} value={text} onInput={(e) => setText(e.currentTarget.value)} />
      </Field>
      <Scale label="強さ" value={intensity} onChange={setIntensity} />
      <div class="row">
        <span class="spacer" />
        <button
          class="primary"
          disabled={act.busy || !text.trim()}
          onClick={async () => {
            const r = await act.run(() => api.hassleAdd(text.trim(), intensity));
            if (r) {
              setText("");
              onAdded();
            }
          }}
        >
          記録
        </button>
      </div>
      <ErrorBox error={act.error} />
    </Card>
  );
}

function AbcCard({ onAdded }: { onAdded: () => void }) {
  const [a, setA] = useState("");
  const [b, setB] = useState("");
  const [c, setC] = useState("");
  const [ci, setCi] = useState<number>();
  const act = useAction();
  return (
    <Card title="ABC を書く">
      <div class="grid3">
        <Field label="A: 出来事">
          <textarea rows={3} value={a} onInput={(e) => setA(e.currentTarget.value)} />
        </Field>
        <Field label="B: 受け止め方">
          <textarea rows={3} value={b} onInput={(e) => setB(e.currentTarget.value)} />
        </Field>
        <Field label="C: 感情">
          <input value={c} onInput={(e) => setC(e.currentTarget.value)} />
          <Scale label="強さ" value={ci} onChange={setCi} />
        </Field>
      </div>
      <div class="row">
        <span class="spacer" />
        <button
          class="primary"
          disabled={act.busy || !a.trim()}
          onClick={async () => {
            const r = await act.run(() =>
              api.abcAdd({
                activating_event: a.trim(),
                belief: b.trim() || undefined,
                consequence_emotion: c.trim() || undefined,
                consequence_intensity: ci,
              }),
            );
            if (r) {
              setA("");
              setB("");
              setC("");
              setCi(undefined);
              onAdded();
            }
          }}
        >
          記録
        </button>
      </div>
      <ErrorBox error={act.error} />
    </Card>
  );
}
