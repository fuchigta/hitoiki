import { useEffect, useRef, useState } from "preact/hooks";
import { api, type MatchField, type PatternType, type TaskStatus, type TaskView } from "../api";
import { Card, Empty, ErrorBox, Field } from "../components";
import { defaultBodyMode, isDirty, isSaveShortcut, leaveGuard, parseEstimate, parseSkills, type TaskDraft } from "../edit-util";
import { STATUS_LABEL, fmtDur, useAction, useAsync, useLiveTick } from "../lib";
import { Markdown } from "../markdown";

/** 「保存しました」を出しておく時間。 */
const SAVED_NOTICE_MS = 4000;

function draftOf(task: TaskView): TaskDraft {
  return {
    title: task.title,
    body: task.body_md,
    urgent: !!task.urgent,
    important: !!task.important,
    est: task.estimate_min != null ? String(task.estimate_min) : "",
    skills: task.skills.join(", "),
  };
}

const FILTERS: { id: string; label: string; statuses: TaskStatus[] }[] = [
  { id: "open", label: "未完了", statuses: ["todo", "doing"] },
  { id: "done", label: "完了", statuses: ["done"] },
  { id: "all", label: "すべて", statuses: ["todo", "doing", "done", "dropped"] },
];

const FIELDS: MatchField[] = ["exe", "title", "cwd", "repo", "url", "git_branch"];
const PTYPES: PatternType[] = ["glob", "regex", "contains"];

export function Tasks() {
  const [filter, setFilter] = useState("open");
  const [selected, setSelected] = useState<string | null>(null);
  const tick = useLiveTick(["doing-changed"]);
  const list = useAsync(
    () => api.taskList(FILTERS.find((f) => f.id === filter)!.statuses),
    [filter, tick],
  );

  return (
    <div class="split">
      <div class="stack">
        <NewTask onAdded={list.reload} />
        <div class="row">
          {FILTERS.map((f) => (
            <button key={f.id} class={filter === f.id ? "chip on" : "chip"} onClick={() => setFilter(f.id)}>
              {f.label}
            </button>
          ))}
        </div>
        <ErrorBox error={list.error} />
        {list.data && list.data.tasks.length === 0 && <Empty>タスクはありません。</Empty>}
        <ul class="tasklist">
          {list.data?.tasks.map((t) => (
            <li key={t.id} class={selected === t.id ? "sel" : ""}>
              <button
                class="task-row"
                onClick={() => {
                  // 未保存の編集があれば、確認してから別のタスクへ切り替える。
                  if (t.id !== selected) leaveGuard.requestLeave(() => setSelected(t.id));
                }}
              >
                <span class={`status s-${t.status}`}>{STATUS_LABEL[t.status]}</span>
                <span class="title">{t.title}</span>
                {t.kind === "interrupt" && <span class="tag">割り込み</span>}
                {t.estimate_min != null && <span class="muted">{t.estimate_min}分</span>}
              </button>
            </li>
          ))}
        </ul>
      </div>
      <div>
        {selected ? (
          <TaskDetail key={selected} id={selected} onChanged={list.reload} />
        ) : (
          <Empty>タスクを選ぶと詳細を編集できます。</Empty>
        )}
      </div>
    </div>
  );
}

function NewTask({ onAdded }: { onAdded: () => void }) {
  const [title, setTitle] = useState("");
  const act = useAction();
  const submit = async (e: Event) => {
    e.preventDefault();
    if (!title.trim()) return;
    const r = await act.run(() => api.taskAdd({ title: title.trim() }));
    if (r) {
      setTitle("");
      onAdded();
    }
  };
  return (
    <form class="row" onSubmit={submit}>
      <input class="grow" value={title} onInput={(e) => setTitle(e.currentTarget.value)} placeholder="新しいタスク" />
      <button class="primary" disabled={act.busy || !title.trim()}>
        追加
      </button>
      <ErrorBox error={act.error} />
    </form>
  );
}

function TaskDetail({ id, onChanged }: { id: string; onChanged: () => void }) {
  const show = useAsync(() => api.taskShow(id), [id]);
  const act = useAction();
  if (show.error) return <ErrorBox error={show.error} />;
  if (!show.data) return <Empty>読み込み中…</Empty>;
  const { task, rules, tracked_ms } = show.data;
  const after = async (fn: () => Promise<unknown>) => {
    const r = await act.run(fn);
    if (r !== undefined) {
      show.reload();
      onChanged();
    }
  };
  return (
    <Card title={task.title} aside={<span class={`status s-${task.status}`}>{STATUS_LABEL[task.status]}</span>}>
      <p class="muted">これまでの時間: {fmtDur(tracked_ms)}</p>
      <div class="row">
        {task.status !== "doing" && task.status !== "done" && task.status !== "dropped" && (
          <button class="primary" disabled={act.busy} onClick={() => after(() => api.taskStart(task.id))}>
            始める
          </button>
        )}
        {task.status === "doing" && (
          <button disabled={act.busy} onClick={() => after(() => api.taskDone(task.id))}>
            完了
          </button>
        )}
        {(task.status === "todo" || task.status === "doing") && (
          <button class="ghost" disabled={act.busy} onClick={() => after(() => api.taskDrop(task.id))}>
            やめる
          </button>
        )}
      </div>
      <EditForm
        task={task}
        busy={act.busy}
        onSaved={() => {
          show.reload();
          onChanged();
        }}
      />
      <h3>帰属ルール</h3>
      {rules.length === 0 && <Empty>ルールはありません。</Empty>}
      <ul class="plain">
        {rules.map((r) => (
          <li key={r.id}>
            <code>
              {r.field} {r.pattern_type} {r.pattern}
            </code>
          </li>
        ))}
      </ul>
      <RuleForm taskId={task.id} onAdded={show.reload} />
      <ErrorBox error={act.error} />
    </Card>
  );
}

function EditForm({ task, busy, onSaved }: { task: TaskView; busy: boolean; onSaved: () => void }) {
  const [draft, setDraft] = useState<TaskDraft>(() => draftOf(task));
  // 保存済みの値。保存に成功するたびに、そのとき保存した値に更新する。
  const [saved, setSaved] = useState<TaskDraft>(draft);
  const [mode, setMode] = useState(() => defaultBodyMode(task.body_md));
  const [notice, setNotice] = useState(false);
  const act = useAction();
  const formRef = useRef<HTMLFormElement>(null);
  const set = <K extends keyof TaskDraft>(k: K, v: TaskDraft[K]) => setDraft((d) => ({ ...d, [k]: v }));
  const dirty = isDirty(saved, draft);

  // 未保存の状態を離脱ガードへ伝える。閉じる・再読み込みでの確認（beforeunload）も同じ条件。
  useEffect(() => {
    leaveGuard.setDirty(dirty);
  }, [dirty]);
  useEffect(() => () => leaveGuard.setDirty(false), []);
  useEffect(() => {
    if (!dirty) return;
    const h = (e: BeforeUnloadEvent) => {
      e.preventDefault();
      e.returnValue = "";
    };
    window.addEventListener("beforeunload", h);
    return () => window.removeEventListener("beforeunload", h);
  }, [dirty]);

  // 「保存しました」は数秒で消す。
  useEffect(() => {
    if (!notice) return;
    const t = setTimeout(() => setNotice(false), SAVED_NOTICE_MS);
    return () => clearTimeout(t);
  }, [notice]);

  const canSave = !busy && !act.busy && !!draft.title.trim();
  const save = async () => {
    if (!canSave) return;
    const sent = draft;
    const list = parseSkills(sent.skills);
    setNotice(false);
    const r = await act.run(() =>
      api.taskEdit(task.id, {
        title: sent.title,
        body: sent.body,
        urgent: sent.urgent,
        important: sent.important,
        estimate_min: parseEstimate(sent.est),
        skills: list,
        // 空にして保存したら、付いていたスキルを全て外す。
        clear_skills: list.length === 0 && task.skills.length > 0,
      }),
    );
    if (r !== undefined) {
      setSaved(sent);
      setNotice(true);
      onSaved();
    }
  };

  const saveBar = (
    <div class="save-bar">
      <button class={dirty ? "primary attn" : "primary"} disabled={!canSave}>
        保存
      </button>
      {dirty && (
        <span class="note dirty" role="status">
          未保存の変更があります
        </span>
      )}
      {!dirty && notice && (
        <span class="note saved" role="status">
          保存しました
        </span>
      )}
      <ErrorBox error={act.error} />
    </div>
  );

  return (
    <form
      ref={formRef}
      class="stack"
      onSubmit={(e) => {
        e.preventDefault();
        save();
      }}
      onKeyDown={(e) => {
        if (isSaveShortcut(e)) {
          e.preventDefault();
          formRef.current?.requestSubmit();
        }
      }}
    >
      <Field label="タイトル">
        <input value={draft.title} onInput={(e) => set("title", e.currentTarget.value)} />
      </Field>
      <div class="field">
        <div class="row">
          <span>本文（Markdown）</span>
          <span class="spacer" />
          <button type="button" class={mode === "edit" ? "chip on" : "chip"} onClick={() => setMode("edit")}>
            編集
          </button>
          <button type="button" class={mode === "preview" ? "chip on" : "chip"} onClick={() => setMode("preview")}>
            プレビュー
          </button>
        </div>
        {mode === "edit" ? (
          <textarea rows={8} value={draft.body} aria-label="本文（Markdown）" onInput={(e) => set("body", e.currentTarget.value)} />
        ) : draft.body.trim() ? (
          <Markdown text={draft.body} />
        ) : (
          <Empty>本文はありません。</Empty>
        )}
      </div>
      {saveBar}
      <Field label="スキル（カンマ区切り。空にすると全て外します）">
        <input value={draft.skills} onInput={(e) => set("skills", e.currentTarget.value)} />
      </Field>
      <div class="row">
        <label class="check">
          <input type="checkbox" checked={draft.urgent} onChange={(e) => set("urgent", e.currentTarget.checked)} />
          緊急
        </label>
        <label class="check">
          <input type="checkbox" checked={draft.important} onChange={(e) => set("important", e.currentTarget.checked)} />
          重要
        </label>
        <Field label="見積（分）">
          <input class="narrow" inputMode="numeric" value={draft.est} onInput={(e) => set("est", e.currentTarget.value)} />
        </Field>
      </div>
      {saveBar}
    </form>
  );
}

function RuleForm({ taskId, onAdded }: { taskId: string; onAdded: () => void }) {
  const [field, setField] = useState<MatchField>("exe");
  const [ptype, setPtype] = useState<PatternType>("glob");
  const [pattern, setPattern] = useState("");
  const act = useAction();
  return (
    <form
      class="row"
      onSubmit={async (e) => {
        e.preventDefault();
        const r = await act.run(() =>
          api.ruleAdd({ task: taskId, field, pattern: pattern.trim(), pattern_type: ptype }),
        );
        if (r) {
          setPattern("");
          onAdded();
        }
      }}
    >
      <select value={field} onChange={(e) => setField(e.currentTarget.value as MatchField)} aria-label="対象">
        {FIELDS.map((f) => (
          <option key={f}>{f}</option>
        ))}
      </select>
      <select value={ptype} onChange={(e) => setPtype(e.currentTarget.value as PatternType)} aria-label="種類">
        {PTYPES.map((f) => (
          <option key={f}>{f}</option>
        ))}
      </select>
      <input class="grow" value={pattern} onInput={(e) => setPattern(e.currentTarget.value)} placeholder="パターン（例: *hitoiki*）" />
      <button disabled={act.busy || !pattern.trim()}>ルール追加</button>
      <ErrorBox error={act.error} />
    </form>
  );
}
