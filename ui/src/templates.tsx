import { useState } from "preact/hooks";
import { api } from "./api";
import type { AppError, Overlap, TaskView, TemplateInput, TemplateView } from "./api";
import { Empty, ErrorBox, Field } from "./components";
import { fmtTime, useAction, useAsync } from "./lib";
import {
  WEEKDAY_LABELS,
  emptyItem,
  sortItems,
  summarizePreview,
  toInput,
  toggleWeekday,
  validateTemplate,
  weekdaysLabel,
} from "./template-util";

/** テンプレートが 0 件のときの案内。 */
export const TEMPLATE_EMPTY_TEXT = "よく使うパターンを保存しておくと、毎朝ワンタップで並べられます。";

// ===== 編集フォーム =====

interface EditorProps {
  initial: TemplateInput;
  tasks: TaskView[];
  busy: boolean;
  error: AppError | null;
  onSave: (input: TemplateInput) => void;
  onCancel: () => void;
}

export function TemplateEditor(p: EditorProps) {
  const [t, setT] = useState<TemplateInput>(p.initial);
  const [msg, setMsg] = useState<string | null>(null);

  const setItem = (i: number, patch: Partial<TemplateInput["items"][number]>) =>
    setT({ ...t, items: t.items.map((it, j) => (j === i ? { ...it, ...patch } : it)) });

  const submit = (e: Event) => {
    e.preventDefault();
    const input = { ...t, items: sortItems(t.items) };
    const err = validateTemplate(input);
    setMsg(err);
    if (!err) p.onSave(input);
  };

  return (
    <form class="stack" onSubmit={submit}>
      <div class="form-row">
        <Field label="名前">
          <input value={t.name} onInput={(e) => setT({ ...t, name: e.currentTarget.value })} placeholder="例: 水曜（定例の日）" />
        </Field>
        <Field label="説明">
          <input
            value={t.description ?? ""}
            onInput={(e) => setT({ ...t, description: e.currentTarget.value })}
            placeholder="任意"
          />
        </Field>
      </div>
      <fieldset class="weekdays">
        <legend>この曜日の既定にする（任意・複数可）</legend>
        {WEEKDAY_LABELS.map((w, i) => (
          <label key={w} class="check">
            <input
              type="checkbox"
              checked={t.weekdays.includes(i + 1)}
              onChange={() => setT({ ...t, weekdays: toggleWeekday(t.weekdays, i + 1) })}
            />
            {w}
          </label>
        ))}
      </fieldset>
      <div class="stack">
        {t.items.length === 0 && <p class="muted">ブロックはまだありません。</p>}
        {t.items.map((it, i) => (
          <div key={i} class="form-row tpl-item">
            <Field label="開始">
              <input type="time" value={it.start} onInput={(e) => setItem(i, { start: e.currentTarget.value })} />
            </Field>
            <Field label="終了">
              <input type="time" value={it.end} onInput={(e) => setItem(i, { end: e.currentTarget.value })} />
            </Field>
            <Field label="ラベル">
              <input value={it.label ?? ""} onInput={(e) => setItem(i, { label: e.currentTarget.value })} placeholder="例: 定例" />
            </Field>
            <Field label="タスク">
              <select value={it.task ?? ""} onChange={(e) => setItem(i, { task: e.currentTarget.value || null })}>
                <option value="">（なし）</option>
                {p.tasks.map((x) => (
                  <option key={x.id} value={x.id}>
                    {x.title}
                  </option>
                ))}
              </select>
            </Field>
            <label class="check">
              <input type="checkbox" checked={!!it.focus} onChange={(e) => setItem(i, { focus: e.currentTarget.checked })} />
              集中
            </label>
            <button type="button" aria-label="このブロックを外す" onClick={() => setT({ ...t, items: t.items.filter((_, j) => j !== i) })}>
              外す
            </button>
          </div>
        ))}
        <div class="row">
          <button type="button" onClick={() => setT({ ...t, items: [...t.items, emptyItem()] })}>
            ブロックを足す
          </button>
        </div>
      </div>
      {msg && (
        <div class="error" role="alert">
          {msg}
        </div>
      )}
      <ErrorBox error={p.error} />
      <div class="row">
        <span class="spacer" />
        <button type="button" onClick={p.onCancel}>
          キャンセル
        </button>
        <button class="primary" disabled={p.busy}>
          保存
        </button>
      </div>
    </form>
  );
}

function TemplateSummary({ t }: { t: TemplateView }) {
  return (
    <div>
      <strong>{t.name}</strong>
      {t.weekdays.length > 0 && <span class="tag">{weekdaysLabel(t.weekdays)}</span>}
      {t.description && <div class="muted">{t.description}</div>}
      <ul class="plain muted">
        {t.blocks.map((b) => (
          <li key={b.id}>
            {b.start}–{b.end} {b.task_title ?? b.label ?? ""}
            {b.focus && <span class="tag">集中</span>}
          </li>
        ))}
        {t.blocks.length === 0 && <li>ブロックなし</li>}
      </ul>
    </div>
  );
}

// ===== 管理（計画画面） =====

/** テンプレートの一覧・作成・編集・削除と、指定日の配置からの保存。 */
export function TemplateManager({ date, tasks }: { date: string; tasks: TaskView[] }) {
  const list = useAsync(() => api.templateList(), []);
  const [editing, setEditing] = useState<{ id?: string; input: TemplateInput } | null>(null);
  const act = useAction();

  const save = async (input: TemplateInput) => {
    const e = editing;
    if (!e) return;
    const r = await act.run(() => (e.id ? api.templateEdit(e.id, input) : api.templateAdd(input)));
    if (r) {
      setEditing(null);
      list.reload();
    }
  };
  const remove = async (t: TemplateView) => {
    const r = await act.run(() => api.templateDelete(t.id));
    if (r) list.reload();
  };

  return (
    <div class="stack">
      <ErrorBox error={list.error} />
      {editing ? (
        <TemplateEditor
          initial={editing.input}
          tasks={tasks}
          busy={act.busy}
          error={act.error}
          onSave={save}
          onCancel={() => setEditing(null)}
        />
      ) : (
        <>
          {list.data && list.data.templates.length === 0 && <Empty>{TEMPLATE_EMPTY_TEXT}</Empty>}
          <ul class="plain stack">
            {list.data?.templates.map((t) => (
              <li key={t.id} class="row tpl-row">
                <TemplateSummary t={t} />
                <span class="spacer" />
                <button onClick={() => setEditing({ id: t.id, input: toInput(t) })}>編集</button>
                <button onClick={() => remove(t)} disabled={act.busy}>
                  削除
                </button>
              </li>
            ))}
          </ul>
          <ErrorBox error={act.error} />
          <div class="row">
            <button onClick={() => setEditing({ input: { name: "", description: "", weekdays: [], items: [emptyItem()] } })}>
              テンプレートを作る
            </button>
            <SaveAsTemplate date={date} onSaved={list.reload} />
          </div>
        </>
      )}
    </div>
  );
}

// ===== 今日の配置から保存 =====

/** 指定日のブロックをテンプレートとして保存する（名前と曜日を入れる小さなフォーム）。 */
export function SaveAsTemplate({ date, onSaved }: { date: string; onSaved?: (t: TemplateView) => void }) {
  const [open, setOpen] = useState(false);
  const [name, setName] = useState("");
  const [weekdays, setWeekdays] = useState<number[]>([]);
  const [done, setDone] = useState<string | null>(null);
  const act = useAction();

  const submit = async (e: Event) => {
    e.preventDefault();
    if (!name.trim()) return;
    const r = await act.run(() => api.templateSaveFrom({ name: name.trim(), date, weekdays }));
    if (r) {
      setOpen(false);
      setName("");
      setWeekdays([]);
      setDone(`「${r.name}」として保存しました。`);
      onSaved?.(r);
    }
  };

  if (!open) {
    return (
      <>
        <button
          onClick={() => {
            setDone(null);
            act.clear();
            setOpen(true);
          }}
        >
          この日の配置をテンプレートとして保存
        </button>
        {done && <span class="muted" role="status">{done}</span>}
      </>
    );
  }
  return (
    <form class="stack" onSubmit={submit}>
      <div class="form-row">
        <Field label="テンプレートの名前">
          <input value={name} onInput={(e) => setName(e.currentTarget.value)} placeholder="例: 金曜（内省の日）" />
        </Field>
      </div>
      <fieldset class="weekdays">
        <legend>この曜日の既定にする（任意）</legend>
        {WEEKDAY_LABELS.map((w, i) => (
          <label key={w} class="check">
            <input
              type="checkbox"
              checked={weekdays.includes(i + 1)}
              onChange={() => setWeekdays(toggleWeekday(weekdays, i + 1))}
            />
            {w}
          </label>
        ))}
      </fieldset>
      <ErrorBox error={act.error} />
      <div class="row">
        <button class="primary" disabled={act.busy || !name.trim()}>
          保存
        </button>
        <button type="button" onClick={() => setOpen(false)}>
          キャンセル
        </button>
      </div>
    </form>
  );
}

// ===== 適用（今日画面） =====

interface ApplyProps {
  /** 適用する日（YYYY-MM-DD）。 */
  date: string;
  /** 曜日の既定として提案するテンプレート（名前）。 */
  suggested?: string[];
  /** ブロックが変わった（作った・取り消した）。 */
  onChanged: () => void;
}

/** テンプレートを選び、重なりをプレビューして適用する。直前の適用は取り消せる。 */
export function ApplyTemplate({ date, suggested = [], onChanged }: ApplyProps) {
  const list = useAsync(() => api.templateList(), []);
  const [chosen, setChosen] = useState<string>("");
  const [overlap, setOverlap] = useState<Overlap>("skip");
  const [result, setResult] = useState<string | null>(null);
  const [undoable, setUndoable] = useState(false);
  const act = useAction();

  const templates = list.data?.templates ?? [];
  const current = chosen || templates.find((t) => suggested.includes(t.name))?.id || templates[0]?.id || "";
  const preview = useAsync(
    () => (current ? api.templatePreview({ template: current, date, overlap }) : Promise.resolve(null)),
    [current, date, overlap, list.data],
  );

  const apply = async () => {
    const r = await act.run(() => api.templateApply({ template: current, date, overlap }));
    if (r) {
      setUndoable(r.created.length > 0);
      setResult(
        `「${r.template}」を適用しました（${r.created.length} 件を作成${r.skipped.length > 0 ? `、重なりのため ${r.skipped.length} 件はスキップ` : ""}）。`,
      );
      onChanged();
      preview.reload();
    }
  };
  const undo = async () => {
    const r = await act.run(() => api.templateUndo(date));
    if (r) {
      setUndoable(false);
      setResult(`「${r.template}」の適用を取り消しました（${r.removed} 件を削除）。`);
      onChanged();
      preview.reload();
    }
  };

  if (list.data && templates.length === 0) {
    return (
      <Empty>
        {TEMPLATE_EMPTY_TEXT} 計画画面で作るか、配置してから「この日の配置をテンプレートとして保存」を押してください。
      </Empty>
    );
  }

  const p = preview.data;
  const sum = p ? summarizePreview(p.blocks, overlap) : null;
  return (
    <div class="stack">
      <ErrorBox error={list.error ?? preview.error} />
      <div class="form-row">
        <Field label="テンプレート">
          <select value={current} onChange={(e) => setChosen(e.currentTarget.value)}>
            {templates.map((t) => (
              <option key={t.id} value={t.id}>
                {t.name}
                {t.weekdays.length > 0 ? `（${weekdaysLabel(t.weekdays)}）` : ""}
              </option>
            ))}
          </select>
        </Field>
        <Field label="重なるとき">
          <select value={overlap} onChange={(e) => setOverlap(e.currentTarget.value as Overlap)}>
            <option value="skip">その枠は作らない</option>
            <option value="overlay">重ねて作る</option>
          </select>
        </Field>
      </div>
      {p && (
        <ul class="plain tpl-preview">
          {p.blocks.map((b, i) => (
            <li key={i} class={b.conflicts.length > 0 ? "conflict" : ""}>
              {fmtTime(b.start_at)}–{fmtTime(b.end_at)} {b.task_title ?? b.label ?? ""}
              {b.focus && <span class="tag">集中</span>}
              {b.conflicts.length > 0 && (
                <span class="muted">
                  {" "}
                  重なり: {b.conflicts.map((c) => `${c.kind === "calendar" ? "予定" : "ブロック"}「${c.title || "（無題）"}」`).join("、")}
                  {overlap === "skip" && "（作りません）"}
                </span>
              )}
            </li>
          ))}
          {p.blocks.length === 0 && <li class="muted">作られるブロックはありません。</li>}
        </ul>
      )}
      <ErrorBox error={act.error} />
      <div class="row">
        <button class="primary" onClick={apply} disabled={act.busy || !current || !sum || sum.create === 0}>
          {sum ? `${sum.create} 件を並べる` : "並べる"}
        </button>
        {undoable && (
          <button onClick={undo} disabled={act.busy}>
            直前の適用を取り消す
          </button>
        )}
        {result && (
          <span class="muted" role="status">
            {result}
          </span>
        )}
      </div>
    </div>
  );
}
