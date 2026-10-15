import type { ComponentChildren } from "preact";
import { useState } from "preact/hooks";
import { api, type Reframe, type Condition, type DeleteOutcome, type Concern } from "./api";
import { ErrorBox, Field, Section } from "./components";
import { useConfirmDelete } from "./confirm-delete";
import { fmtFull, useAction } from "./lib";
import { ABC_HELP } from "./records-util";
import { OverflowMenu, Scale } from "./ui/parts";
import { scaleFromString, scaleToString } from "./ui/scale-util";

// 記録 1 件の詳細（表示・編集・削除）。記録画面の右パネルに出す。編集は詳細の中でフォームに切り替える。

/** 削除の影響のプレビュー。確認ダイアログに出す名前だけ、記録の日時などに差し替える。 */
async function named(p: Promise<DeleteOutcome>, name: string): Promise<DeleteOutcome> {
  const o = await p;
  return { ...o, impact: { ...o.impact, name } };
}

/** フォームの文字列（"" = 未入力）で持つ 1〜5 の値を Scale で編集する。 */
function ScaleField({ label, value, onChange }: { label: string; value: string; onChange: (v: string) => void }) {
  return <Scale label={label} value={scaleFromString(value)} onChange={(n) => onChange(scaleToString(n))} />;
}

const num = (s: string): number | undefined => (s ? Number(s) : undefined);
const str = (n: number | null | undefined): string => (n == null ? "" : String(n));

/** 記録 1 件の「…」メニュー（編集・削除）。削除は影響を確認するダイアログを出す。 */
function RecordMenu({
  onEdit,
  what,
  preview,
  commit,
  onDeleted,
}: {
  onEdit: () => void;
  what: string;
  preview: () => Promise<DeleteOutcome>;
  commit: () => Promise<unknown>;
  onDeleted: () => void;
}) {
  const del = useConfirmDelete({ what, preview, commit, onDeleted });
  return (
    <span class="row">
      <OverflowMenu
        label={`${what}の操作`}
        items={[
          { label: "編集", onSelect: onEdit },
          { label: "削除…", onSelect: del.start, disabled: del.busy },
        ]}
      />
      {del.dialog}
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

interface DetailProps {
  /** ERG の行（`ErgLine`）。 */
  erg?: ComponentChildren;
  /** 編集した後（一覧と集計を読み直す）。 */
  onChanged: () => void;
  /** 削除した後（一覧を読み直して選択を外す）。 */
  onDeleted: () => void;
}

export function ConditionDetail({ c, erg, onChanged, onDeleted }: DetailProps & { c: Condition }) {
  const [editing, setEditing] = useState(false);
  const [energy, setEnergy] = useState(str(c.energy));
  const [mood, setMood] = useState(str(c.mood));
  const [stress, setStress] = useState(str(c.stress));
  const [note, setNote] = useState(c.note ?? "");
  const act = useAction();
  const ok = !!(energy || mood || stress || note.trim());
  const menu = (
    <RecordMenu
      onEdit={() => {
        setEnergy(str(c.energy));
        setMood(str(c.mood));
        setStress(str(c.stress));
        setNote(c.note ?? "");
        setEditing(true);
      }}
      what="調子"
      preview={() => named(api.conditionDelete(c.id, { dry_run: true }), fmtFull(c.at))}
      commit={() => api.conditionDelete(c.id, { force: true })}
      onDeleted={onDeleted}
    />
  );
  return (
    <Section title="調子" aside={editing ? undefined : menu}>
      <span class="meta">{fmtFull(c.at)}</span>
      {editing ? (
        <form
          class="stack"
          onSubmit={async (e) => {
            e.preventDefault();
            const r = await act.run(() =>
              api.conditionEdit(c.id, {
                energy: num(energy),
                mood: num(mood),
                stress: num(stress),
                note: note.trim() || undefined,
              }),
            );
            if (r) {
              setEditing(false);
              onChanged();
            }
          }}
        >
          <div class="row">
            <ScaleField label="エネルギー" value={energy} onChange={setEnergy} />
            <ScaleField label="気分" value={mood} onChange={setMood} />
            <ScaleField label="ストレス" value={stress} onChange={setStress} />
          </div>
          <Field label="一言メモ">
            <input value={note} onInput={(e) => setNote(e.currentTarget.value)} />
          </Field>
          <SaveBar busy={act.busy} ok={ok} onCancel={() => setEditing(false)} />
          <ErrorBox error={act.error} />
        </form>
      ) : (
        <>
          <div>
            エネルギー {c.energy ?? "-"}
            {c.mood != null && ` ・ 気分 ${c.mood}`}
            {c.stress != null && ` ・ ストレス ${c.stress}`}
          </div>
          {c.note && <div>{c.note}</div>}
          {erg}
        </>
      )}
    </Section>
  );
}

export function ConcernDetail({ h, erg, onChanged, onDeleted }: DetailProps & { h: Concern }) {
  const [editing, setEditing] = useState(false);
  const [text, setText] = useState(h.text);
  const [intensity, setIntensity] = useState(String(h.intensity));
  const act = useAction();
  const menu = (
    <RecordMenu
      onEdit={() => {
        setText(h.text);
        setIntensity(String(h.intensity));
        setEditing(true);
      }}
      what="気になったこと"
      preview={() => api.concernDelete(h.id, { dry_run: true })}
      commit={() => api.concernDelete(h.id, { force: true })}
      onDeleted={onDeleted}
    />
  );
  return (
    <Section title="気になったこと" aside={editing ? undefined : menu}>
      <span class="meta">{fmtFull(h.at)}</span>
      {editing ? (
        <form
          class="stack"
          onSubmit={async (e) => {
            e.preventDefault();
            const r = await act.run(() => api.concernEdit(h.id, text.trim(), num(intensity)));
            if (r) {
              setEditing(false);
              onChanged();
            }
          }}
        >
          <Field label="気になったこと">
            <textarea rows={3} value={text} onInput={(e) => setText(e.currentTarget.value)} />
          </Field>
          <ScaleField label="強さ" value={intensity} onChange={setIntensity} />
          <SaveBar busy={act.busy} ok={!!text.trim()} onCancel={() => setEditing(false)} />
          <ErrorBox error={act.error} />
        </form>
      ) : (
        <>
          <div class="record-text">{h.text}</div>
          <span class="meta">強さ {h.intensity}</span>
          {erg}
        </>
      )}
    </Section>
  );
}

export function ReframeDetail({
  a,
  erg,
  onChanged,
  onDeleted,
  children,
}: DetailProps & { a: Reframe; children?: ComponentChildren }) {
  const [editing, setEditing] = useState(false);
  const [ev, setEv] = useState(a.event);
  const [belief, setBelief] = useState(a.belief ?? "");
  const [emotion, setEmotion] = useState(a.emotion ?? "");
  const [ci, setCi] = useState(str(a.emotion_intensity));
  const [alternative, setAlternative] = useState(a.alternative ?? "");
  const act = useAction();
  const menu = (
    <RecordMenu
      onEdit={() => {
        setEv(a.event);
        setBelief(a.belief ?? "");
        setEmotion(a.emotion ?? "");
        setCi(str(a.emotion_intensity));
        setAlternative(a.alternative ?? "");
        setEditing(true);
      }}
      what="捉え直し"
      preview={() => named(api.reframeDelete(a.id, { dry_run: true }), a.event)}
      commit={() => api.reframeDelete(a.id, { force: true })}
      onDeleted={onDeleted}
    />
  );
  return (
    <Section title="捉え直し（ABC）" help={ABC_HELP} aside={editing ? undefined : menu}>
      <span class="meta">{fmtFull(a.at)}</span>
      {editing ? (
        <form
          class="stack"
          onSubmit={async (e) => {
            e.preventDefault();
            const r = await act.run(() =>
              api.reframeEdit(a.id, {
                event: ev.trim(),
                belief: belief.trim() || undefined,
                emotion: emotion.trim() || undefined,
                emotion_intensity: num(ci),
                alternative: alternative.trim() || undefined,
              }),
            );
            if (r) {
              setEditing(false);
              onChanged();
            }
          }}
        >
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
            <ScaleField label="強さ" value={ci} onChange={setCi} />
          </div>
          <Field label="別の捉え方のメモ">
            <textarea rows={2} value={alternative} onInput={(e) => setAlternative(e.currentTarget.value)} />
          </Field>
          <SaveBar busy={act.busy} ok={!!ev.trim()} onCancel={() => setEditing(false)} />
          <ErrorBox error={act.error} />
        </form>
      ) : (
        <>
          <div class="record-text">A: {a.event}</div>
          {a.belief && <div class="record-text">B: {a.belief}</div>}
          {a.emotion && (
            <div>
              C: {a.emotion}
              {a.emotion_intensity != null && `（${a.emotion_intensity}）`}
            </div>
          )}
          {a.alternative && <div class="hint">別の捉え方: {a.alternative}</div>}
          {erg}
          {children}
        </>
      )}
    </Section>
  );
}
