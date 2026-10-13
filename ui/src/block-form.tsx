import { useState } from "preact/hooks";
import { api, type AppError, type BlockInput, type Millis, type TaskView } from "./api";
import { ErrorBox, Field } from "./components";
import { fmtTime, toMillis, useAction } from "./lib";
import { describeConflicts, findOverlaps } from "./plan-util";
import { TaskPicker } from "./task-picker";
import type { PickerItem } from "./task-picker-util";

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

/** ブロックの作成・編集フォーム（今日の画面のサイドパネルで使う）。 */
export function BlockForm(p: Props) {
  const d = p.draft;
  const [start, setStart] = useState(fmtTime(d.start));
  // 24:00 は time 入力に入らないので 23:59 に寄せる。
  const [end, setEnd] = useState(d.end >= p.dayEnd ? "23:59" : fmtTime(d.end));
  const [task, setTask] = useState(d.task);
  const [label, setLabel] = useState(d.label);
  const [focus, setFocus] = useState(d.focus);
  // この場で新規作成したタスク（親の一覧にはまだ載っていない）。
  const [created, setCreated] = useState<TaskView[]>([]);
  const creating = useAction();
  const tasks = [...p.tasks, ...created.filter((c) => !p.tasks.some((t) => t.id === c.id))];

  const pickTask = async (it: PickerItem) => {
    if (it.kind === "create") {
      const v = await creating.run(() => api.taskAdd({ title: it.title }));
      if (v) {
        setCreated((l) => [...l, v]);
        setTask(v.id);
      }
    } else {
      setTask(it.task.id);
    }
  };

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
        <div class="field">
          <span>タスク</span>
          <TaskPicker
            tasks={tasks}
            selectedId={task || undefined}
            ariaLabel="タスク"
            placeholder="（なし）入力で絞り込み・新規作成"
            disabled={creating.busy}
            onPick={pickTask}
            onClear={() => setTask("")}
          />
        </div>
        <Field label="ラベル">
          <input value={label} onInput={(ev) => setLabel(ev.currentTarget.value)} placeholder="任意" />
        </Field>
      </div>
      <label class="check">
        <input type="checkbox" checked={focus} onChange={(ev) => setFocus(ev.currentTarget.checked)} />
        集中の時間（この間は通知を控えます）
      </label>
      <ErrorBox error={creating.error} />
      {invalid && <p class="hint" role="alert">終了は開始より後にしてください。</p>}
      {hits.length > 0 && <p class="hint">重なり: {describeConflicts(hits)}（重ねても保存できます）</p>}
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
