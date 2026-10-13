import type { ComponentChildren } from "preact";
import { useState } from "preact/hooks";
import { api, type AbcRecord, type Checkin, type DeleteOutcome, type Hassle } from "./api";
import { ErrorBox, Field, Section } from "./components";
import { useConfirmDelete } from "./confirm-delete";
import { fmtFull, useAction } from "./lib";
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

export function CheckinDetail({ c, erg, onChanged, onDeleted }: DetailProps & { c: Checkin }) {
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
      what="チェックイン"
      preview={() => named(api.checkinDelete(c.id, { dry_run: true }), fmtFull(c.at))}
      commit={() => api.checkinDelete(c.id, { force: true })}
      onDeleted={onDeleted}
    />
  );
  return (
    <Section title="チェックイン" aside={editing ? undefined : menu}>
      <span class="meta">{fmtFull(c.at)}</span>
      {editing ? (
        <form
          class="stack"
          onSubmit={async (e) => {
            e.preventDefault();
            const r = await act.run(() =>
              api.checkinEdit(c.id, {
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

export function HassleDetail({ h, erg, onChanged, onDeleted }: DetailProps & { h: Hassle }) {
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
      what="ハッスル"
      preview={() => api.hassleDelete(h.id, { dry_run: true })}
      commit={() => api.hassleDelete(h.id, { force: true })}
      onDeleted={onDeleted}
    />
  );
  return (
    <Section title="ハッスル" aside={editing ? undefined : menu}>
      <span class="meta">{fmtFull(h.at)}</span>
      {editing ? (
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

export function AbcDetail({
  a,
  erg,
  onChanged,
  onDeleted,
  children,
}: DetailProps & { a: AbcRecord; children?: ComponentChildren }) {
  const [editing, setEditing] = useState(false);
  const [ev, setEv] = useState(a.activating_event);
  const [belief, setBelief] = useState(a.belief ?? "");
  const [emotion, setEmotion] = useState(a.consequence_emotion ?? "");
  const [ci, setCi] = useState(str(a.consequence_intensity));
  const [dispute, setDispute] = useState(a.dispute ?? "");
  const act = useAction();
  const menu = (
    <RecordMenu
      onEdit={() => {
        setEv(a.activating_event);
        setBelief(a.belief ?? "");
        setEmotion(a.consequence_emotion ?? "");
        setCi(str(a.consequence_intensity));
        setDispute(a.dispute ?? "");
        setEditing(true);
      }}
      what="ABC の記録"
      preview={() => named(api.abcDelete(a.id, { dry_run: true }), a.activating_event)}
      commit={() => api.abcDelete(a.id, { force: true })}
      onDeleted={onDeleted}
    />
  );
  return (
    <Section title="ABC" aside={editing ? undefined : menu}>
      <span class="meta">{fmtFull(a.at)}</span>
      {editing ? (
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
          <Field label="反論・掘り下げのメモ">
            <textarea rows={2} value={dispute} onInput={(e) => setDispute(e.currentTarget.value)} />
          </Field>
          <SaveBar busy={act.busy} ok={!!ev.trim()} onCancel={() => setEditing(false)} />
          <ErrorBox error={act.error} />
        </form>
      ) : (
        <>
          <div class="record-text">A: {a.activating_event}</div>
          {a.belief && <div class="record-text">B: {a.belief}</div>}
          {a.consequence_emotion && (
            <div>
              C: {a.consequence_emotion}
              {a.consequence_intensity != null && `（${a.consequence_intensity}）`}
            </div>
          )}
          {a.dispute && <div class="hint">反論: {a.dispute}</div>}
          {erg}
          {children}
        </>
      )}
    </Section>
  );
}
