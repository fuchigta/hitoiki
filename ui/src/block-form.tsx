import { useState } from "preact/hooks";
import type { AppError, BlockInput, Millis, TaskView } from "./api";
import { ErrorBox, Field } from "./components";
import { fmtTime, toMillis } from "./lib";
import { describeConflicts, findOverlaps } from "./plan-util";

export interface BlockDraft {
  /** 既存のブロックを編集するときの ID。 */
  id?: string;
  start: Millis;
  end: Millis;
  task: string;
  label: string;
  focus: boolean;
}

export interface Other {
  id?: string;
  start_at: Millis;
  end_at: Millis;
  title: string;
}

interface Props {
  draft: BlockDraft;
  /** 日付（YYYY-MM-DD）。時刻入力をこの日の時刻として解釈する。 */
  date: string;
  dayEnd: Millis;
  tasks: TaskView[];
  /** 重なりを知らせる相手（ほかのブロックや busy の予定）。 */
  others?: Other[];
  busy?: boolean;
  error?: AppError | null;
  onSubmit: (input: BlockInput, id?: string) => void;
  onCancel?: () => void;
  onDelete?: () => void;
}

/** ブロックの作成・編集フォーム（計画画面・今日画面のポップオーバーで共通）。 */
export function BlockForm(p: Props) {
  const d = p.draft;
  const [start, setStart] = useState(fmtTime(d.start));
  // 24:00 は time 入力に入らないので 23:59 に寄せる。
  const [end, setEnd] = useState(d.end >= p.dayEnd ? "23:59" : fmtTime(d.end));
  const [task, setTask] = useState(d.task);
  const [label, setLabel] = useState(d.label);
  const [focus, setFocus] = useState(d.focus);

  const s = toMillis(p.date, start);
  const e = toMillis(p.date, end);
  const invalid = !start || !end || e <= s;
  const hits = invalid ? [] : findOverlaps({ start: s, end: e }, p.others ?? [], d.id).map((o) => o.title);

  const submit = (ev: Event) => {
    ev.preventDefault();
    if (invalid) return;
    p.onSubmit({ start: s, end: e, task: task || undefined, label: label.trim() || undefined, focus }, d.id);
  };

  return (
    <form class="stack" onSubmit={submit}>
      <div class="form-row">
        <Field label="開始">
          <input type="time" value={start} onInput={(ev) => setStart(ev.currentTarget.value)} />
        </Field>
        <Field label="終了">
          <input type="time" value={end} onInput={(ev) => setEnd(ev.currentTarget.value)} />
        </Field>
        <Field label="タスク">
          <select value={task} onChange={(ev) => setTask(ev.currentTarget.value)}>
            <option value="">（なし）</option>
            {p.tasks.map((t) => (
              <option key={t.id} value={t.id}>
                {t.title}
              </option>
            ))}
          </select>
        </Field>
        <Field label="ラベル">
          <input value={label} onInput={(ev) => setLabel(ev.currentTarget.value)} placeholder="任意" />
        </Field>
      </div>
      <label class="check">
        <input type="checkbox" checked={focus} onChange={(ev) => setFocus(ev.currentTarget.checked)} />
        集中の時間（この間は通知を控えます）
      </label>
      {invalid && <p class="muted">終了は開始より後にしてください。</p>}
      {hits.length > 0 && <p class="muted">重なり: {describeConflicts(hits)}（重ねても保存できます）</p>}
      <div class="row">
        {p.onDelete && (
          <button type="button" onClick={p.onDelete} disabled={p.busy}>
            削除
          </button>
        )}
        <span class="spacer" />
        {p.onCancel && (
          <button type="button" onClick={p.onCancel}>
            閉じる
          </button>
        )}
        <button class="primary" disabled={p.busy || invalid}>
          {d.id ? "保存" : "追加"}
        </button>
      </div>
      <ErrorBox error={p.error ?? null} />
    </form>
  );
}

/** ブロックの表示名。 */
export function blockTitle(b: { task_title?: string | null; label: string | null }): string {
  return b.task_title ?? b.label ?? "（ラベルなし）";
}
