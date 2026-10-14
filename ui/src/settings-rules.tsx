// 設定の「ルール」タブ。タスクの割り当てルール（全タスク分）・活動ルール・無視するアプリを 1 つの一覧にまとめる。
import { useState } from "preact/hooks";
import { api } from "./api";
import { EmptyState, ErrorBox, Field, Section } from "./components";
import { useConfirmDelete } from "./confirm-delete";
import { hasExe, normalizeExe } from "./ignore-util";
import { useAction, useAsync } from "./lib";
import { RuleFields, noImpact, priorityOf, type Draft } from "./rule-row";
import {
  FIELD_LABEL,
  PTYPE_LABEL,
  RULE_FILTERS,
  RULE_KIND_LABEL,
  filterRules,
  mergeRules,
  type RuleFilter,
  type RuleItem,
  type RuleKind,
} from "./settings-util";
import { Help, ListDetail, ObjectList, ObjectRow, type MenuItem } from "./ui/parts";

const IGNORE_HELP =
  "ここのアプリが前面の時間は、短いものは前後の作業に含め、続いたものは集計・実績・レポートに数えません。記録は消えず、外せば元に戻ります。追加・削除すると直近 7 日を再計算します。";

export function RulesTab() {
  const tasks = useAsync(() => api.taskList(["todo", "doing", "done", "dropped"]), []);
  const taskRules = useAsync(() => api.ruleList(), []);
  const actRules = useAsync(() => api.activityRuleList(), []);
  const ignore = useAsync(() => api.ignoreList(), []);
  const labels = useAsync(() => api.activityLabelList(), []);
  const [kind, setKind] = useState<RuleFilter>("all");
  const [taskId, setTaskId] = useState("");
  const [sel, setSel] = useState<string | null>(null); // 行の key か "new"

  const taskList = tasks.data?.tasks ?? [];
  const items = mergeRules(taskRules.data?.rules ?? [], actRules.data?.rules ?? [], ignore.data?.exes ?? [], taskList);
  const shown = filterRules(items, kind, taskId);
  const current = sel && sel !== "new" ? items.find((i) => i.key === sel) : undefined;
  const reload = () => {
    taskRules.reload();
    actRules.reload();
    ignore.reload();
  };
  const loaded = !!taskRules.data && !!actRules.data && !!ignore.data;

  return (
    <Section
      title="ルール"
      help="時間の行き先を決めるルールです。「タスク」はウインドウなどのパターンからタスクへ、「活動」は活動ラベルへ、「無視」は特定のアプリを集計から外します。無効にするとルールを残したまま判定に使いません。"
    >
      <ErrorBox error={taskRules.error ?? actRules.error ?? ignore.error ?? tasks.error} />
      <ListDetail
        list={
          <div class="stack">
            <div class="list-head" role="group" aria-label="ルールの種類">
              {RULE_FILTERS.map((f) => (
                <button
                  key={f.id}
                  class={kind === f.id ? "chip on" : "chip"}
                  aria-pressed={kind === f.id}
                  onClick={() => setKind(f.id)}
                >
                  {f.label}
                </button>
              ))}
              <span class="spacer" />
              <button type="button" onClick={() => setSel("new")}>
                ＋ ルール
              </button>
            </div>
            {(kind === "all" || kind === "task") && taskList.length > 0 && (
              <select value={taskId} onChange={(e) => setTaskId(e.currentTarget.value)} aria-label="タスクで絞り込み">
                <option value="">すべてのタスク</option>
                {taskList.map((t) => (
                  <option key={t.id} value={t.id}>
                    {t.title}
                  </option>
                ))}
              </select>
            )}
            {loaded && shown.length === 0 ? (
              <EmptyState>
                {items.length === 0 ? "ルールはまだありません。「＋ ルール」で追加できます。" : "この条件のルールはありません。"}
              </EmptyState>
            ) : (
              <ObjectList label="ルール">
                {shown.map((i) => (
                  <RuleListRow key={i.key} item={i} selected={sel === i.key} onSelect={() => setSel(i.key)} onChanged={reload} />
                ))}
              </ObjectList>
            )}
          </div>
        }
        detail={
          sel === "new" ? (
            <RuleDetail
              key="new"
              item={null}
              tasks={taskList}
              labels={labels.data?.labels ?? []}
              exes={ignore.data?.exes ?? []}
              initialKind={kind === "all" ? "activity" : kind}
              onChanged={reload}
              onDone={(key) => setSel(key)}
            />
          ) : current ? (
            <RuleDetail
              key={current.key}
              item={current}
              tasks={taskList}
              labels={labels.data?.labels ?? []}
              exes={ignore.data?.exes ?? []}
              initialKind={current.kind}
              onChanged={reload}
              onDone={(key) => setSel(key)}
            />
          ) : (
            <EmptyState>ルールを選ぶと、ここで編集できます。</EmptyState>
          )
        }
      />
    </Section>
  );
}

function RuleListRow({
  item: i,
  selected,
  onSelect,
  onChanged,
}: {
  item: RuleItem;
  selected: boolean;
  onSelect: () => void;
  onChanged: () => void;
}) {
  const act = useAction();
  const del = useConfirmDelete({
    what: i.kind === "ignore" ? "無視するアプリ" : `${RULE_KIND_LABEL[i.kind]}ルール`,
    preview: async () => noImpact(i.id, i.pattern),
    commit: () => removeRule(i),
    onDeleted: onChanged,
  });
  const menu: MenuItem[] = [];
  if (i.kind !== "ignore") {
    menu.push({
      label: i.enabled ? "無効にする" : "有効にする",
      disabled: act.busy,
      onSelect: async () => {
        if (await act.run(() => patchEnabled(i, !i.enabled))) onChanged();
      },
    });
  }
  menu.push({ label: i.kind === "ignore" ? "外す…" : "削除…", onSelect: del.start, disabled: act.busy || del.busy });
  const meta = [i.target && `→ ${i.target}`, i.priority !== 0 && `優先度 ${i.priority}`].filter(Boolean).join("　");
  return (
    <>
      <ObjectRow
        trailing={del.dialog}
        title={
          <>
            <code class={i.enabled ? undefined : "meta"}>
              {i.kind === "ignore" ? i.pattern : `${FIELD_LABEL[i.field!]} ${PTYPE_LABEL[i.type!]} ${i.pattern}`}
            </code>
            {meta && (
              <>
                <br />
                <span class="meta">{meta}</span>
              </>
            )}
          </>
        }
        tags={[RULE_KIND_LABEL[i.kind], ...(i.enabled ? [] : ["無効"])]}
        selected={selected}
        onSelect={onSelect}
        menu={menu}
        menuLabel={`ルール「${i.pattern}」の操作`}
      />
      {act.error && (
        <li>
          <ErrorBox error={act.error} />
        </li>
      )}
    </>
  );
}

function removeRule(i: RuleItem) {
  if (i.kind === "task") return api.ruleDelete(i.id);
  if (i.kind === "activity") return api.activityRuleDelete(i.id);
  return api.ignoreRemove(i.id);
}

function patchEnabled(i: RuleItem, enabled: boolean): Promise<unknown> {
  return i.kind === "task" ? api.ruleEdit(i.id, { enabled }) : api.activityRuleEdit(i.id, { enabled });
}

/** 右側の編集フォーム。`item` が null なら新規（種類を選べる）。 */
function RuleDetail({
  item,
  tasks,
  labels,
  exes,
  initialKind,
  onChanged,
  onDone,
}: {
  item: RuleItem | null;
  tasks: { id: string; title: string }[];
  labels: string[];
  exes: string[];
  initialKind: RuleKind;
  onChanged: () => void;
  onDone: (key: string | null) => void;
}) {
  const [kind, setKind] = useState<RuleKind>(initialKind);
  const [d, setD] = useState<Draft>({
    field: item?.field ?? "exe",
    type: item?.type ?? "glob",
    pattern: item && item.kind !== "ignore" ? item.pattern : "",
    priority: String(item?.priority ?? 0),
    label: item?.kind === "activity" ? item.target : "",
  });
  const [task, setTask] = useState(item?.taskId ?? "");
  const [enabled, setEnabled] = useState(item?.enabled ?? true);
  const [exe, setExe] = useState("");
  const act = useAction();
  const isNew = item === null;
  const exeName = normalizeExe(exe);

  const valid =
    kind === "ignore"
      ? isNew && !!exeName && !hasExe(exes, exeName)
      : !!d.pattern.trim() && (kind === "task" ? !!task : !!d.label.trim());

  const save = async () => {
    const pattern = d.pattern.trim();
    const priority = priorityOf(d.priority);
    if (kind === "ignore") {
      if (await act.run(() => api.ignoreAdd(exeName))) {
        onChanged();
        onDone(`ignore:${exeName}`);
      }
      return;
    }
    if (kind === "task") {
      const r = isNew
        ? await act.run(() => api.ruleAdd({ task, field: d.field, pattern, pattern_type: d.type, priority }))
        : await act.run(() => api.ruleEdit(item.id, { field: d.field, pattern, pattern_type: d.type, priority, enabled }));
      if (r) {
        onChanged();
        onDone(`task:${r.id}`);
      }
      return;
    }
    const label = d.label.trim();
    const r = isNew
      ? await act.run(() => api.activityRuleAdd({ field: d.field, pattern, pattern_type: d.type, activity_label: label, priority }))
      : await act.run(() =>
          api.activityRuleEdit(item.id, { field: d.field, pattern, pattern_type: d.type, activity_label: label, priority, enabled }),
        );
    if (r) {
      onChanged();
      onDone(`activity:${r.id}`);
    }
  };

  return (
    <form
      class="stack"
      aria-label={isNew ? "新しいルール" : "ルールの編集"}
      onSubmit={(e) => {
        e.preventDefault();
        void save();
      }}
    >
      <h3>{isNew ? "新しいルール" : "ルールの編集"}</h3>
      {isNew ? (
        <Field label="ルールの種類">
          <select value={kind} onChange={(e) => setKind(e.currentTarget.value as RuleKind)}>
            {(Object.keys(RULE_KIND_LABEL) as RuleKind[]).map((k) => (
              <option key={k} value={k}>
                {RULE_KIND_LABEL[k]}
              </option>
            ))}
          </select>
        </Field>
      ) : (
        <p class="meta">{RULE_KIND_LABEL[kind]}ルール</p>
      )}
      {kind === "ignore" ? (
        isNew ? (
          <>
            <Field label="exe 名">
              <input value={exe} onInput={(e) => setExe(e.currentTarget.value)} placeholder="explorer.exe" />
            </Field>
            <p class="hint">
              集計から外すアプリです。 <Help>{IGNORE_HELP}</Help>
            </p>
          </>
        ) : (
          <p>
            <code>{item.pattern}</code> が前面の時間は集計に数えません。一覧の「…」から外せます。 <Help>{IGNORE_HELP}</Help>
          </p>
        )
      ) : (
        <>
          {kind === "task" ? (
            isNew ? (
              <Field label="タスク">
                <select value={task} onChange={(e) => setTask(e.currentTarget.value)}>
                  <option value="">選んでください</option>
                  {tasks.map((t) => (
                    <option key={t.id} value={t.id}>
                      {t.title}
                    </option>
                  ))}
                </select>
              </Field>
            ) : (
              <p>
                タスク: <strong>{item.target}</strong>
              </p>
            )
          ) : (
            <Field label="活動ラベル">
              <input
                list="rule-labels"
                value={d.label}
                onInput={(e) => setD({ ...d, label: e.currentTarget.value })}
                placeholder="例: 会議"
              />
              <datalist id="rule-labels">
                {labels.map((l) => (
                  <option key={l} value={l} />
                ))}
              </datalist>
            </Field>
          )}
          <RuleFields d={d} set={setD} />
          {!isNew && (
            <label class="check">
              <input type="checkbox" checked={enabled} onChange={(e) => setEnabled(e.currentTarget.checked)} />
              有効（無効にするとルールを残したまま判定に使いません）
            </label>
          )}
        </>
      )}
      <ErrorBox error={act.error} />
      <div class="row">
        <span class="spacer" />
        <button type="button" class="ghost" onClick={() => onDone(null)}>
          {isNew ? "キャンセル" : "閉じる"}
        </button>
        {(isNew || kind !== "ignore") && (
          <button class="primary" disabled={act.busy || !valid}>
            {isNew ? "追加" : "保存"}
          </button>
        )}
      </div>
    </form>
  );
}
