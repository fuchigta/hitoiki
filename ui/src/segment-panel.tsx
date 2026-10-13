import { useState } from "preact/hooks";
import { api, type IdlePreset, type SegmentKind, type SegmentView, type TaskView } from "./api";
import { ErrorBox, Section } from "./components";
import { totalMs } from "./log-util";
import { KIND_LABEL, KIND_ORDER, fmtDateTime, fmtDur, fmtTime, useAction } from "./lib";
import { TaskPicker } from "./task-picker";
import { OverflowMenu, type MenuItem } from "./ui/parts";

/** 無操作区間のワンタップ分類（core の `IdlePreset` と同じ並び）。 */
const IDLE_PRESETS: { id: IdlePreset; label: string }[] = [
  { id: "break", label: "休憩" },
  { id: "meeting", label: "会議" },
  { id: "thinking", label: "考えごと" },
  { id: "away", label: "離席" },
];

interface Props {
  /** 選んだ区間（1 件なら単一、2 件以上ならまとめて確定）。 */
  segs: SegmentView[];
  /** 割り当て先の候補（未完了のタスク）。 */
  tasks: TaskView[];
  /** 確定・無視などが済んだ（対象の区間 ID）。呼び出し側が一覧を取り直して次の区間を選ぶ。 */
  onDone: (ids: string[]) => void;
  /** タスクを新規作成した（候補を取り直す）。 */
  onTasksChanged?: () => void;
}

/**
 * 実績画面の右パネル。選んだ区間のタスク・種別・活動を指定して確定する。
 * 複数選択のときは同じフォームで全部まとめて確定する。確定済みの区間も付け替えられる。
 */
export function SegmentPanel({ segs, tasks, onDone, onTasksChanged }: Props) {
  const many = segs.length > 1;
  const one = segs[0];
  const ids = segs.map((s) => s.id);
  const [task, setTask] = useState(many ? "" : (one.task_id ?? ""));
  const [created, setCreated] = useState<TaskView[]>([]);
  const [kind, setKind] = useState<SegmentKind | "">("");
  const [activity, setActivity] = useState(many ? "" : (one.activity_label ?? ""));
  const [rule, setRule] = useState(false);
  const act = useAction();

  const candidates = [...tasks, ...created.filter((c) => !tasks.some((t) => t.id === c.id))];

  const submit = async () => {
    const r = await act.run(async () => {
      await (many ? api.reviewSetMany({ segments: ids, task: task || undefined, kind: kind || undefined, activity: activity.trim() || undefined, rule }) : api.reviewSet({ segment: one.id, task: task || undefined, kind: kind || undefined, activity: activity.trim() || undefined, rule }));
      return true;
    });
    if (r) onDone(ids);
  };

  const asIdle = async (preset: IdlePreset) => {
    if (await act.run(() => api.reviewIdle(ids, preset))) onDone(ids);
  };

  const ignoreThese = async () => {
    if (await act.run(() => api.reviewIgnore(ids))) onDone(ids);
  };

  const ignoreApp = async () => {
    if (one.exe_name && (await act.run(() => api.ignoreAdd(one.exe_name!)))) onDone(ids);
  };

  // 割り込みを新しいタスクにして割り当てる。
  const asInterrupt = async () => {
    const title = (one.title || one.exe_name || "割り込み").slice(0, 60);
    const r = await act.run(async () => {
      const t = await api.taskAdd({ title, interrupt: true });
      return api.reviewSet({ segment: one.id, task: t.id, rule });
    });
    if (r) {
      onTasksChanged?.();
      onDone(ids);
    }
  };

  const menu: MenuItem[] = many
    ? [{ label: `${segs.length} 件を無視`, onSelect: ignoreThese, disabled: act.busy }]
    : [
        { label: "この区間を無視", onSelect: ignoreThese, disabled: act.busy },
        ...(one.exe_name ? [{ label: `${one.exe_name} を今後も無視`, onSelect: ignoreApp, disabled: act.busy }] : []),
        { label: "割り込みとしてタスク化", onSelect: asInterrupt, disabled: act.busy },
      ];

  const idleOnly = segs.every((s) => s.kind === "idle");
  const title = many ? `${segs.length} 件を選択中` : one.title || one.exe_name || "（タイトルなし）";

  return (
    <Section title={title} aside={<OverflowMenu label="区間の操作" items={menu} />}>
      {many ? (
        <p class="meta">合計 {fmtDur(totalMs(segs))}</p>
      ) : (
        <p class="meta seg-meta">
          {one.exe_name ? `${one.exe_name} ・ ` : ""}
          {fmtDateTime(one.start_at)} – {fmtTime(one.end_at)} ・ {fmtDur(one.duration_ms)} ・ {KIND_LABEL[one.kind]}
          {one.confidence != null ? ` ・ 確信度 ${Math.round(one.confidence * 100)}%` : ""}
          {one.status === "needs_review" && <span class="tag">要確認</span>}
          {one.offline && <span class="tag">PC 外</span>}
        </p>
      )}
      {idleOnly && (
        <div class="row" role="group" aria-label="この無操作の時間は">
          <span class="meta">この時間は</span>
          {IDLE_PRESETS.map((p) => (
            <button key={p.id} class="chip" disabled={act.busy} onClick={() => asIdle(p.id)}>
              {p.label}
            </button>
          ))}
        </div>
      )}
      <div class="stack">
        <div class="field">
          <span>タスク</span>
          <TaskPicker
            tasks={candidates}
            selectedId={task || undefined}
            ariaLabel="タスク"
            placeholder="タスクを選ぶ・新しく作る"
            disabled={act.busy}
            onClear={() => setTask("")}
            onPick={async (it) => {
              if (it.kind === "create") {
                const t = await act.run(() => api.taskAdd({ title: it.title, interrupt: it.interrupt }));
                if (t) {
                  setCreated((c) => [...c, t]);
                  setTask(t.id);
                  onTasksChanged?.();
                }
              } else {
                setTask(it.task.id);
              }
            }}
          />
        </div>
        <div class="form-row">
          <label class="field">
            <span>種別</span>
            <select value={kind} onChange={(e) => setKind(e.currentTarget.value as SegmentKind | "")}>
              <option value="">（そのまま）</option>
              {KIND_ORDER.map((k) => (
                <option key={k} value={k}>
                  {KIND_LABEL[k]}
                </option>
              ))}
            </select>
          </label>
          <label class="field">
            <span>活動</span>
            <input value={activity} onInput={(e) => setActivity(e.currentTarget.value)} placeholder="例: コードレビュー" />
          </label>
        </div>
        <div class="row">
          <label class="check">
            <input type="checkbox" checked={rule} onChange={(e) => setRule(e.currentTarget.checked)} />
            この条件を常にこのタスクに
          </label>
          <span class="spacer" />
          <button class="primary" disabled={act.busy || (!task && !kind && !activity.trim())} onClick={submit}>
            {many ? `${segs.length} 件を確定` : "確定"}
          </button>
        </div>
      </div>
      <ErrorBox error={act.error} />
    </Section>
  );
}
