import type { ComponentChildren } from "preact";
import { useState } from "preact/hooks";
import { api, type AbcRecord, type Checkin, type DeleteOutcome, type Hassle } from "./api";
import { ErrorBox, Field } from "./components";
import { ConfirmDelete } from "./confirm-delete";
import { fmtDateTime, useAction } from "./lib";

/** 依存先のない記録の削除確認。 */
function plain(id: string, name: string): DeleteOutcome {
  return { dry_run: true, deleted: false, impact: { entity: "record", id, name, lines: [], note: null } };
}

function ScaleSelect({ label, value, onChange }: { label: string; value: string; onChange: (v: string) => void }) {
  return (
    <Field label={label}>
      <select value={value} onChange={(e) => onChange(e.currentTarget.value)}>
        <option value="">-</option>
        {[1, 2, 3, 4, 5].map((n) => (
          <option key={n} value={String(n)}>
            {n}
          </option>
        ))}
      </select>
    </Field>
  );
}

const num = (s: string): number | undefined => (s ? Number(s) : undefined);
const str = (n: number | null | undefined): string => (n == null ? "" : String(n));

function Actions({ onEdit, children }: { onEdit: () => void; children: ComponentChildren }) {
  return (
    <span class="row">
      <button type="button" class="ghost" onClick={onEdit}>
        編集
      </button>
      {children}
    </span>
  );
}

function SaveBar({ busy, ok, onCancel }: { busy: boolean; ok: boolean; onCancel: () => void }) {
  return (
    <div class="row">
      <span class="spacer" />
      <button type="button" class="ghost" onClick={onCancel}>
        キャンセル
      </button>
      <button class="primary" disabled={busy || !ok}>
        保存
      </button>
    </div>
  );
}

export function CheckinItem({ c, onChanged }: { c: Checkin; onChanged: () => void }) {
  const [editing, setEditing] = useState(false);
  const [energy, setEnergy] = useState(str(c.energy));
  const [mood, setMood] = useState(str(c.mood));
  const [stress, setStress] = useState(str(c.stress));
  const [note, setNote] = useState(c.note ?? "");
  const act = useAction();
  const ok = !!(energy || mood || stress || note.trim() || c.erg_tag);
  if (editing) {
    return (
      <li>
        <form
          class="stack"
          onSubmit={async (e) => {
            e.preventDefault();
            const r = await act.run(() =>
              api.checkinEdit(c.id, {
                energy: num(energy),
                mood: num(mood),
                stress: num(stress),
                erg: c.erg_tag ?? undefined,
                note: note.trim() || undefined,
              }),
            );
            if (r) {
              setEditing(false);
              onChanged();
            }
          }}
        >
          <span class="muted">{fmtDateTime(c.at)}</span>
          <div class="row">
            <ScaleSelect label="エネルギー" value={energy} onChange={setEnergy} />
            <ScaleSelect label="気分" value={mood} onChange={setMood} />
            <ScaleSelect label="ストレス" value={stress} onChange={setStress} />
          </div>
          <Field label="一言メモ">
            <input value={note} onInput={(e) => setNote(e.currentTarget.value)} />
          </Field>
          <SaveBar busy={act.busy} ok={ok} onCancel={() => setEditing(false)} />
          <ErrorBox error={act.error} />
        </form>
      </li>
    );
  }
  return (
    <li>
      <span class="muted">{fmtDateTime(c.at)}</span> エネルギー {c.energy ?? "-"}
      {c.mood != null && ` ・ 気分 ${c.mood}`}
      {c.stress != null && ` ・ ストレス ${c.stress}`}
      {c.note && <div>{c.note}</div>}
      <Actions
        onEdit={() => {
          setEnergy(str(c.energy));
          setMood(str(c.mood));
          setStress(str(c.stress));
          setNote(c.note ?? "");
          setEditing(true);
        }}
      >
        <ConfirmDelete
          what="チェックイン"
          preview={async () => plain(c.id, fmtDateTime(c.at))}
          commit={() => api.checkinDelete(c.id)}
          onDeleted={onChanged}
        />
      </Actions>
    </li>
  );
}

export function HassleItem({ h, onChanged }: { h: Hassle; onChanged: () => void }) {
  const [editing, setEditing] = useState(false);
  const [text, setText] = useState(h.text);
  const [intensity, setIntensity] = useState(String(h.intensity));
  const act = useAction();
  if (editing) {
    return (
      <li>
        <form
          class="stack"
          onSubmit={async (e) => {
            e.preventDefault();
            const r = await act.run(() => api.hassleEdit(h.id, text.trim(), num(intensity)));
            if (r) {
              setEditing(false);
              onChanged();
            }
          }}
        >
          <span class="muted">{fmtDateTime(h.at)}</span>
          <Field label="気になったこと">
            <textarea rows={3} value={text} onInput={(e) => setText(e.currentTarget.value)} />
          </Field>
          <ScaleSelect label="強さ" value={intensity} onChange={setIntensity} />
          <SaveBar busy={act.busy} ok={!!text.trim()} onCancel={() => setEditing(false)} />
          <ErrorBox error={act.error} />
        </form>
      </li>
    );
  }
  return (
    <li>
      <span class="muted">{fmtDateTime(h.at)}</span> {h.text}
      <span class="muted"> （強さ {h.intensity}）</span>
      <Actions
        onEdit={() => {
          setText(h.text);
          setIntensity(String(h.intensity));
          setEditing(true);
        }}
      >
        <ConfirmDelete
          what="ハッスル"
          preview={() => api.hassleDelete(h.id, { dry_run: true })}
          commit={() => api.hassleDelete(h.id, { force: true })}
          onDeleted={onChanged}
        />
      </Actions>
    </li>
  );
}

export function AbcItem({ a, onChanged, children }: { a: AbcRecord; onChanged: () => void; children?: ComponentChildren }) {
  const [editing, setEditing] = useState(false);
  const [ev, setEv] = useState(a.activating_event);
  const [belief, setBelief] = useState(a.belief ?? "");
  const [emotion, setEmotion] = useState(a.consequence_emotion ?? "");
  const [ci, setCi] = useState(str(a.consequence_intensity));
  const [dispute, setDispute] = useState(a.dispute ?? "");
  const act = useAction();
  if (editing) {
    return (
      <li>
        <form
          class="stack"
          onSubmit={async (e) => {
            e.preventDefault();
            const r = await act.run(() =>
              api.abcEdit(a.id, {
                activating_event: ev.trim(),
                belief: belief.trim() || undefined,
                consequence_emotion: emotion.trim() || undefined,
                consequence_intensity: num(ci),
                dispute: dispute.trim() || undefined,
              }),
            );
            if (r) {
              setEditing(false);
              onChanged();
            }
          }}
        >
          <span class="muted">{fmtDateTime(a.at)}</span>
          <Field label="A: 出来事">
            <textarea rows={2} value={ev} onInput={(e) => setEv(e.currentTarget.value)} />
          </Field>
          <Field label="B: 受け止め方">
            <textarea rows={2} value={belief} onInput={(e) => setBelief(e.currentTarget.value)} />
          </Field>
          <div class="row">
            <Field label="C: 感情">
              <input value={emotion} onInput={(e) => setEmotion(e.currentTarget.value)} />
            </Field>
            <ScaleSelect label="強さ" value={ci} onChange={setCi} />
          </div>
          <Field label="反論・掘り下げのメモ">
            <textarea rows={2} value={dispute} onInput={(e) => setDispute(e.currentTarget.value)} />
          </Field>
          <SaveBar busy={act.busy} ok={!!ev.trim()} onCancel={() => setEditing(false)} />
          <ErrorBox error={act.error} />
        </form>
      </li>
    );
  }
  return (
    <li>
      <span class="muted">{fmtDateTime(a.at)}</span>
      <div>A: {a.activating_event}</div>
      {a.belief && <div>B: {a.belief}</div>}
      {a.consequence_emotion && (
        <div>
          C: {a.consequence_emotion}
          {a.consequence_intensity != null && `（${a.consequence_intensity}）`}
        </div>
      )}
      {a.dispute && <div class="muted">反論: {a.dispute}</div>}
      <Actions
        onEdit={() => {
          setEv(a.activating_event);
          setBelief(a.belief ?? "");
          setEmotion(a.consequence_emotion ?? "");
          setCi(str(a.consequence_intensity));
          setDispute(a.dispute ?? "");
          setEditing(true);
        }}
      >
        <ConfirmDelete
          what="ABC の記録"
          preview={async () => plain(a.id, a.activating_event)}
          commit={() => api.abcDelete(a.id)}
          onDeleted={onChanged}
        />
      </Actions>
      {children}
    </li>
  );
}
