import { useEffect, useRef, useState } from "preact/hooks";
import { api, type MatchField, type PatternType, type TaskStatus, type TaskView } from "../api";
import { Card, Empty, EmptyState, ErrorBox, Field, InlineNew, Section } from "../components";
import { useConfirmDelete } from "../confirm-delete";
import { ListDetail, ObjectList, ObjectRow, OverflowMenu, TabPanel, Tabs, type MenuItem } from "../ui/parts";
import { tabFromParam } from "../ui/tabs-util";
import { defaultBodyMode, isDirty, isSaveShortcut, keyResultPatch, leaveGuard, parseEstimate, type TaskDraft } from "../edit-util";
import { KeyResultPicker } from "../key-result-picker";
import { keyResultLabel, keyResultOptions } from "../key-result-util";
import { STATUS_LABEL, fmtDateTime, fmtDur, fmtTime, hrefOf, navigate, useAction, useAsync, useLiveTick, useRoute } from "../lib";
import { Markdown } from "../markdown";
import { SkillPicker } from "../skill-picker";
import { FIELDS, PTYPES, TaskRuleRow } from "../rule-row";
import { FIELD_LABEL, PTYPE_LABEL } from "../settings-util";

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
    keyResult: task.key_result_id ?? "",
  };
}

const FILTERS: { id: string; label: string; statuses: TaskStatus[] }[] = [
  { id: "open", label: "未完了", statuses: ["todo", "doing"] },
  { id: "done", label: "完了", statuses: ["done"] },
  { id: "all", label: "すべて", statuses: ["todo", "doing", "done", "dropped"] },
];


const TABS = [
  { id: "overview", label: "概要" },
  { id: "time", label: "時間" },
  { id: "rules", label: "ルール" },
];
const TAB_IDS = TABS.map((t) => t.id);

export function Tasks() {
  const route = useRoute();
  const selected = route.path[0] ?? null;
  const tab = tabFromParam(route.params.get("tab"), TAB_IDS, "overview");
  const [filter, setFilter] = useState("open");
  const tick = useLiveTick(["doing-changed"]);
  const list = useAsync(
    () => api.taskList(FILTERS.find((f) => f.id === filter)!.statuses),
    [filter, tick],
  );
  const tasks = list.data?.tasks ?? [];

  return (
    <ListDetail
      list={
        <div class="stack">
          <div class="list-head">
            {FILTERS.map((f) => (
              <button
                key={f.id}
                class={filter === f.id ? "chip on" : "chip"}
                aria-pressed={filter === f.id}
                onClick={() => setFilter(f.id)}
              >
                {f.label}
              </button>
            ))}
            <span class="spacer" />
            <InlineNew
              label="＋ タスク"
              inputLabel="新しいタスクのタイトル"
              onCreate={async (title) => {
                const t = await api.taskAdd({ title });
                list.reload();
                navigate(hrefOf("tasks", [t.id]));
                return t;
              }}
            />
          </div>
          <ErrorBox error={list.error} />
          {list.data && tasks.length === 0 && <Empty>タスクはありません。</Empty>}
          <ObjectList label="タスク">
            {tasks.map((t) => (
              <ObjectRow
                key={t.id}
                selected={selected === t.id}
                title={
                  <>
                    <span class={`status s-${t.status}`}>{STATUS_LABEL[t.status]}</span> {t.title}
                  </>
                }
                tags={rowMarks(t)}
                meta={t.estimate_min != null ? `${t.estimate_min}分` : undefined}
                // 未保存の編集があれば、navigate が確認してから別のタスクへ切り替える。
                onSelect={() => navigate(hrefOf("tasks", [t.id]))}
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
            tab={tab}
            onChanged={list.reload}
            onDeleted={() => {
              list.reload();
              navigate(hrefOf("tasks"), { replace: true });
            }}
          />
        ) : (
          <EmptyState>
            タスクを選ぶと詳細が出ます。
            {list.data && tasks.length > 0 && <> いまの一覧は {tasks.length} 件です。</>}
          </EmptyState>
        )
      }
    />
  );
}

/** 一覧の行に付ける小さな印（割り込み・緊急・重要）。 */
function rowMarks(t: TaskView): string[] | undefined {
  const marks: string[] = [];
  if (t.kind === "interrupt") marks.push("割り込み");
  if (t.urgent) marks.push("緊急");
  if (t.important) marks.push("重要");
  return marks.length > 0 ? marks : undefined;
}

function TaskDetail({
  id,
  tab,
  onChanged,
  onDeleted,
}: {
  id: string;
  tab: string;
  onChanged: () => void;
  onDeleted: () => void;
}) {
  const show = useAsync(() => api.taskShow(id), [id]);
  const skills = useAsync(() => api.skillList(), []);
  const act = useAction();
  if (show.error) return <ErrorBox error={show.error} />;
  if (!show.data) return <Empty>読み込み中…</Empty>;
  const { task, rules, tracked_ms, tracked_segments } = show.data;
  const after = async (fn: () => Promise<unknown>) => {
    const r = await act.run(fn);
    if (r !== undefined) {
      show.reload();
      onChanged();
    }
  };
  // タブは編集中のフォームを保ったまま切り替える（履歴は増やさない）。
  const setTab = (t: string) => location.replace(hrefOf("tasks", [id], t === "overview" ? {} : { tab: t }));
  const open = task.status === "todo" || task.status === "doing";
  const skillId = (name: string) => skills.data?.skills.find((s) => s.name === name)?.id;

  return (
    <Card
      title={task.title}
      aside={
        <>
          <span class={`status s-${task.status}`}>{STATUS_LABEL[task.status]}</span>
          {task.status === "todo" && (
            <button class="primary" disabled={act.busy} onClick={() => after(() => api.taskStart(task.id))}>
              始める
            </button>
          )}
          {task.status === "doing" && (
            <button class="primary" disabled={act.busy} onClick={() => after(() => api.taskDone(task.id))}>
              完了にする
            </button>
          )}
          <TaskMenu
            taskId={task.id}
            title={task.title}
            disabled={act.busy}
            extra={
              open
                ? [{ label: "やめる", onSelect: () => after(() => api.taskDrop(task.id)), disabled: act.busy }]
                : []
            }
            onDeleted={onDeleted}
          />
        </>
      }
    >
      {(task.key_result || task.skills.length > 0) && (
        <div class="relations">
          {task.key_result && (
            <a class="link-chip" href={hrefOf("goals", ["key-result", task.key_result.id])} title="この成果の指標の詳細を開く">
              {keyResultLabel({ title: task.key_result.title, goal_title: task.key_result.goal_title })}
            </a>
          )}
          {task.skills.map((name) => {
            const sid = skillId(name);
            return sid ? (
              <a key={name} class="link-chip" href={hrefOf("goals", ["skill", sid])} title="このスキルの詳細を開く">
                {name}
              </a>
            ) : (
              <span key={name} class="tag">
                {name}
              </span>
            );
          })}
        </div>
      )}
      <Tabs tabs={TABS} active={tab} onChange={setTab} idBase="task" label="タスクの詳細" />
      <TabPanel id="overview" idBase="task" active={tab}>
        <EditForm
          task={task}
          busy={act.busy}
          onSaved={() => {
            show.reload();
            onChanged();
          }}
        />
      </TabPanel>
      <TabPanel id="time" idBase="task" active={tab}>
        <Section level={3} title="これまでの時間">
          <p class="num">
            <strong>{fmtDur(tracked_ms)}</strong>
            <span class="meta"> （{tracked_segments} 区間）</span>
          </p>
          <p>
            <a href={hrefOf("log", [], { status: "all", task: task.id })}>このタスクの区間を実績で見る</a>
          </p>
        </Section>
        <Corrections
          taskId={task.id}
          onUndone={() => {
            show.reload();
            onChanged();
          }}
        />
      </TabPanel>
      <TabPanel id="rules" idBase="task" active={tab}>
        <Section
          level={3}
          title="タスクの割り当てルール"
          help="条件に当たる時間は、自動でこのタスクになります。変更すると今日の分が再計算されます。"
        >
          {rules.length === 0 && <Empty>ルールはありません。</Empty>}
          <ul class="plain">
            {rules.map((r) => (
              <TaskRuleRow key={r.id} rule={r} onChanged={show.reload} />
            ))}
          </ul>
          <RuleForm taskId={task.id} onAdded={show.reload} />
        </Section>
      </TabPanel>
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
  extra,
  onDeleted,
}: {
  taskId: string;
  title: string;
  disabled: boolean;
  /** 削除の前に並べる項目（やめる など）。 */
  extra: MenuItem[];
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
        items={[...extra, { label: "削除…", onSelect: del.start, disabled: disabled || del.busy }]}
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
    <Section
      level={3}
      title="補正した時間帯"
      help="今日画面の「実績」の列をドラッグすると、その時間帯をタスクへ付け替えられます。"
    >
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
    </Section>
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
  const goals = useAsync(() => api.goalList(), []);
  const keyResults = keyResultOptions(goals.data?.goals ?? [], draft.keyResult);
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
        // 成果の指標は選んでいれば紐付け、空にして保存したら外す。
        ...keyResultPatch(sent.keyResult, task.key_result_id),
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
        <span>成果の指標</span>
        <KeyResultPicker value={draft.keyResult} options={keyResults} onChange={(v) => set("keyResult", v)} ariaLabel="成果の指標を選ぶ" disabled={busy} />
      </div>
      <div class="row row-end">
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
          <option key={f} value={f}>{FIELD_LABEL[f]}</option>
        ))}
      </select>
      <select value={ptype} onChange={(e) => setPtype(e.currentTarget.value as PatternType)} aria-label="種類">
        {PTYPES.map((f) => (
          <option key={f} value={f}>{PTYPE_LABEL[f]}</option>
        ))}
      </select>
      <input class="grow" value={pattern} onInput={(e) => setPattern(e.currentTarget.value)} placeholder="パターン（例: *hitoiki*）" />
      <button disabled={act.busy || !pattern.trim()}>ルール追加</button>
      <ErrorBox error={act.error} />
    </form>
  );
}
