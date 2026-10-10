import { useState } from "preact/hooks";
import { api, type MatchField, type PatternType, type TaskStatus, type TaskView } from "../api";
import { Card, Empty, ErrorBox, Field } from "../components";
import { STATUS_LABEL, fmtDur, useAction, useAsync, useLiveTick } from "../lib";

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
              <button class="task-row" onClick={() => setSelected(t.id)}>
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
      <EditForm task={task} busy={act.busy} onSave={(input) => after(() => api.taskEdit(task.id, input))} />
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

function EditForm({
  task,
  busy,
  onSave,
}: {
  task: TaskView;
  busy: boolean;
  onSave: (input: {
    title: string;
    body: string;
    urgent: boolean;
    important: boolean;
    estimate_min?: number;
    skills: string[];
    clear_skills: boolean;
  }) => void;
}) {
  const [title, setTitle] = useState(task.title);
  const [body, setBody] = useState(task.body_md);
  const [urgent, setUrgent] = useState(!!task.urgent);
  const [important, setImportant] = useState(!!task.important);
  const [est, setEst] = useState(task.estimate_min != null ? String(task.estimate_min) : "");
  const [skills, setSkills] = useState(task.skills.join(", "));
  return (
    <form
      class="stack"
      onSubmit={(e) => {
        e.preventDefault();
        const n = parseInt(est, 10);
        const list = skills
          .split(/[,、]/)
          .map((x) => x.trim())
          .filter(Boolean);
        onSave({
          title,
          body,
          urgent,
          important,
          estimate_min: Number.isFinite(n) ? n : undefined,
          skills: list,
          // 空にして保存したら、付いていたスキルを全て外す。
          clear_skills: list.length === 0 && task.skills.length > 0,
        });
      }}
    >
      <Field label="タイトル">
        <input value={title} onInput={(e) => setTitle(e.currentTarget.value)} />
      </Field>
      <Field label="本文（Markdown）">
        <textarea rows={8} value={body} onInput={(e) => setBody(e.currentTarget.value)} />
      </Field>
      <Field label="スキル（カンマ区切り。空にすると全て外します）">
        <input value={skills} onInput={(e) => setSkills(e.currentTarget.value)} />
      </Field>
      <div class="row">
        <label class="check">
          <input type="checkbox" checked={urgent} onChange={(e) => setUrgent(e.currentTarget.checked)} />
          緊急
        </label>
        <label class="check">
          <input type="checkbox" checked={important} onChange={(e) => setImportant(e.currentTarget.checked)} />
          重要
        </label>
        <Field label="見積（分）">
          <input class="narrow" inputMode="numeric" value={est} onInput={(e) => setEst(e.currentTarget.value)} />
        </Field>
        <span class="spacer" />
        <button class="primary" disabled={busy || !title.trim()}>
          保存
        </button>
      </div>
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
