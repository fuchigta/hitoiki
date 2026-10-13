import { useEffect, useRef, useState } from "preact/hooks";
import { api, type MatchField, type PatternType, type TaskStatus, type TaskView } from "../api";
import { Card, Empty, ErrorBox, Field } from "../components";
import { useConfirmDelete } from "../confirm-delete";
import { Help, ListDetail, ObjectList, ObjectRow, OverflowMenu } from "../ui/parts";
import { defaultBodyMode, isDirty, isSaveShortcut, krPatch, leaveGuard, parseEstimate, type TaskDraft } from "../edit-util";
import { KrPicker } from "../kr-picker";
import { krOptions } from "../kr-util";
import { STATUS_LABEL, fmtDateTime, fmtDur, fmtTime, useAction, useAsync, useLiveTick } from "../lib";
import { Markdown } from "../markdown";
import { SkillPicker } from "../skill-picker";
import { TaskRuleRow } from "../rule-row";

/** 「保存しました」を出しておく時間。 */
const SAVED_NOTICE_MS = 4000;

function draftOf(task: TaskView): TaskDraft {
  return {
    title: task.title,
    body: task.body_md,
    urgent: !!task.urgent,
    important: !!task.important,
    est: task.estimate_min != null ? String(task.estimate_min) : "",
    skills: [...task.skills],
    kr: task.key_result_id ?? "",
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
    <ListDetail
      list={
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
          <ObjectList label="タスク">
            {list.data?.tasks.map((t) => (
              <ObjectRow
                key={t.id}
                selected={selected === t.id}
                title={
                  <>
                    <span class={`status s-${t.status}`}>{STATUS_LABEL[t.status]}</span> {t.title}
                  </>
                }
                tags={t.kind === "interrupt" ? ["割り込み"] : undefined}
                meta={t.estimate_min != null ? `${t.estimate_min}分` : undefined}
                onSelect={() => {
                  // 未保存の編集があれば、確認してから別のタスクへ切り替える。
                  if (t.id !== selected) leaveGuard.requestLeave(() => setSelected(t.id));
                }}
              />
            ))}
          </ObjectList>
        </div>
      }
      detail={
        selected ? (
          <TaskDetail
            key={selected}
            id={selected}
            onChanged={list.reload}
            onDeleted={() => {
              setSelected(null);
              list.reload();
            }}
          />
        ) : (
          <Empty>タスクを選ぶと詳細を編集できます。</Empty>
        )
      }
    />
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

function TaskDetail({ id, onChanged, onDeleted }: { id: string; onChanged: () => void; onDeleted: () => void }) {
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
    <Card
      title={task.title}
      aside={
        <>
          <span class={`status s-${task.status}`}>{STATUS_LABEL[task.status]}</span>
          <TaskMenu taskId={task.id} title={task.title} disabled={act.busy} onDeleted={onDeleted} />
        </>
      }
    >
      <p class="meta">これまでの時間: {fmtDur(tracked_ms)}</p>
      <div class="row">
        {task.status !== "doing" && task.status !== "done" && task.status !== "dropped" && (
          <button disabled={act.busy} onClick={() => after(() => api.taskStart(task.id))}>
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
          <TaskRuleRow key={r.id} rule={r} onChanged={show.reload} />
        ))}
      </ul>
      <RuleForm taskId={task.id} onAdded={show.reload} />
      <Corrections taskId={task.id} onUndone={() => {
        show.reload();
        onChanged();
      }} />
      <ErrorBox error={act.error} />
    </Card>
  );
}

/**
 * タスクの「…」メニュー（削除）。削除は影響（計上済みの時間・確定した割り当て・ルールなど）を見せる確認を出し、
 * 「削除する」で実行する。確認はアプリ内のダイアログ（`window.confirm` は使わない）。
 * 「やめる」は記録として残し、「削除」は誤入力を消す（計上した時間は未割り当てに戻る）。
 */
function TaskMenu({
  taskId,
  title,
  disabled,
  onDeleted,
}: {
  taskId: string;
  title: string;
  disabled: boolean;
  onDeleted: () => void;
}) {
  const del = useConfirmDelete({
    what: "タスク",
    preview: async () => {
      const p = await api.taskDeletePreview(taskId);
      return {
        dry_run: true,
        deleted: false,
        impact: {
          entity: "task",
          id: taskId,
          name: title,
          lines: [],
          warnings: p.summary ? [p.summary] : [],
          note: "削除は元に戻せません。記録として残したいときは「やめる」を使ってください（やめる=記録として残す／削除=誤入力を消す。計上した時間は未割り当てに戻ります）。",
        },
      };
    },
    commit: () => api.taskDelete(taskId),
    onDeleted,
  });
  return (
    <>
      <OverflowMenu
        label="タスクの操作"
        items={[{ label: "削除…", onSelect: del.start, disabled: disabled || del.busy }]}
      />
      {del.dialog}
    </>
  );
}

/** このタスクへ付け替えた時間帯（実績の補正）。取り消すと、その補正で作った確定を削除して再計算する。 */
function Corrections({ taskId, onUndone }: { taskId: string; onUndone: () => void }) {
  const tick = useLiveTick(["segments-updated"]);
  const list = useAsync(() => api.correctionList({ task: taskId }), [taskId, tick]);
  const act = useAction();
  const items = list.data?.corrections ?? [];
  const undo = async (id: string) => {
    const r = await act.run(() => api.correctionUndo(id));
    if (r) {
      list.reload();
      onUndone();
    }
  };
  return (
    <>
      <h3>
        補正した時間帯 <Help>今日画面の「実績」の列をドラッグすると、その時間帯をタスクへ付け替えられます。</Help>
      </h3>
      <ErrorBox error={list.error ?? act.error} />
      {items.length === 0 ? (
        <Empty>このタスクへ付け替えた時間帯はありません。</Empty>
      ) : (
        <ul class="plain corr-done">
          {items.map((c) => (
            <li key={c.id}>
              <span>
                {fmtDateTime(c.start_at)}–{fmtTime(c.end_at)}
              </span>{" "}
              <strong>{fmtDur(c.total_ms)}</strong>
              {c.offline_ms > 0 && <span class="tag">うち PC 外 {fmtDur(c.offline_ms)}</span>}
              {c.activity_label && <span class="meta"> {c.activity_label}</span>}
              <OverflowMenu
                label="補正の操作"
                items={[{ label: "取り消す", onSelect: () => undo(c.id), disabled: act.busy }]}
              />
            </li>
          ))}
        </ul>
      )}
    </>
  );
}

function EditForm({ task, busy, onSaved }: { task: TaskView; busy: boolean; onSaved: () => void }) {
  const [draft, setDraft] = useState<TaskDraft>(() => draftOf(task));
  // 保存済みの値。保存に成功するたびに、そのとき保存した値に更新する。
  const [saved, setSaved] = useState<TaskDraft>(draft);
  const [mode, setMode] = useState(() => defaultBodyMode(task.body_md));
  const [notice, setNotice] = useState(false);
  const act = useAction();
  // 補完に使う既知のスキル。取得できなくても他の欄の編集・保存は続けられる。
  const known = useAsync(() => api.skillList(), []);
  const okr = useAsync(() => api.okrList(), []);
  const krs = krOptions(okr.data?.objectives ?? [], draft.kr);
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
    const list = sent.skills;
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
        // KR は選んでいれば紐付け、空にして保存したら外す。
        ...krPatch(sent.kr, task.key_result_id),
      }),
    );
    if (r !== undefined) {
      setSaved(sent);
      setNotice(true);
      onSaved();
    }
  };

  const saveBar = (
    <div class={dirty ? "save-bar sticky" : "save-bar"}>
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
      <div class="field">
        <span>スキル</span>
        <SkillPicker
          value={draft.skills}
          skills={known.data?.skills ?? []}
          onChange={(v) => set("skills", v)}
          ariaLabel="スキルを追加"
          disabled={busy}
        />
      </div>
      <div class="field">
        <span>KR</span>
        <KrPicker value={draft.kr} options={krs} onChange={(v) => set("kr", v)} ariaLabel="KR を選ぶ" disabled={busy} />
      </div>
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
