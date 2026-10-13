import { useState } from "preact/hooks";
import { api, type ActivityRule, type DeleteOutcome, type MatchField, type PatternType, type TaskMatchRule } from "./api";
import { ErrorBox } from "./components";
import { useConfirmDelete } from "./confirm-delete";
import { useAction } from "./lib";
import { OverflowMenu } from "./ui/parts";

export const FIELDS: MatchField[] = ["exe", "title", "cwd", "repo", "url", "git_branch"];
export const PTYPES: PatternType[] = ["glob", "regex", "contains"];

const RULE_NOTE =
  "このルールで決まっていた区間は、再計算で変わります（確定済みの区間は変わりません）。過去の日へ反映するには再計算を実行してください。";

/** ルールの削除には依存先がないので、確認ダイアログ用に影響なし（補足つき）の結果を作る。 */
function noImpact(id: string, name: string): DeleteOutcome {
  return {
    dry_run: true,
    deleted: false,
    impact: { entity: "rule", id, name, lines: [], note: RULE_NOTE },
  };
}

interface Draft {
  field: MatchField;
  type: PatternType;
  pattern: string;
  priority: string;
  label: string;
}

function EditFields({ d, set, withLabel }: { d: Draft; set: (d: Draft) => void; withLabel: boolean }) {
  return (
    <>
      <select value={d.field} onChange={(e) => set({ ...d, field: e.currentTarget.value as MatchField })} aria-label="対象">
        {FIELDS.map((f) => (
          <option key={f}>{f}</option>
        ))}
      </select>
      <select value={d.type} onChange={(e) => set({ ...d, type: e.currentTarget.value as PatternType })} aria-label="種類">
        {PTYPES.map((f) => (
          <option key={f}>{f}</option>
        ))}
      </select>
      <input class="grow" value={d.pattern} onInput={(e) => set({ ...d, pattern: e.currentTarget.value })} aria-label="パターン" />
      {withLabel && (
        <input class="narrow" value={d.label} onInput={(e) => set({ ...d, label: e.currentTarget.value })} placeholder="活動" aria-label="活動ラベル" />
      )}
      <input
        class="narrow"
        inputMode="numeric"
        value={d.priority}
        onInput={(e) => set({ ...d, priority: e.currentTarget.value })}
        aria-label="優先度"
        title="優先度（大きいほど先に評価）"
      />
    </>
  );
}

function priorityOf(s: string): number | undefined {
  const n = Number(s.trim());
  return s.trim() !== "" && Number.isInteger(n) ? n : undefined;
}

interface RowProps<R> {
  rule: R;
  onChanged: () => void;
}

/** タスクの帰属ルール 1 行。有効/無効の切り替え、編集、削除。 */
export function TaskRuleRow({ rule: r, onChanged }: RowProps<TaskMatchRule>) {
  const mk = (): Draft => ({ field: r.field, type: r.pattern_type, pattern: r.pattern, priority: String(r.priority), label: "" });
  const [editing, setEditing] = useState(false);
  const [d, setD] = useState<Draft>(mk);
  const act = useAction();
  const del = useConfirmDelete({
    what: "帰属ルール",
    preview: async () => noImpact(r.id, `${r.field} ${r.pattern}`),
    commit: () => api.ruleDelete(r.id),
    onDeleted: onChanged,
  });
  const save = async () => {
    const x = await act.run(() =>
      api.ruleEdit(r.id, { field: d.field, pattern_type: d.type, pattern: d.pattern.trim(), priority: priorityOf(d.priority) }),
    );
    if (x) {
      setEditing(false);
      onChanged();
    }
  };
  return (
    <li>
      {editing ? (
        <form
          class="row"
          onSubmit={(e) => {
            e.preventDefault();
            void save();
          }}
        >
          <EditFields d={d} set={setD} withLabel={false} />
          <button class="primary" disabled={act.busy || !d.pattern.trim()}>
            保存
          </button>
          <button type="button" class="ghost" onClick={() => setEditing(false)}>
            キャンセル
          </button>
        </form>
      ) : (
        <span class="row">
          <label class="check" title="無効にするとルールは残したまま判定に使いません">
            <input
              type="checkbox"
              checked={r.enabled}
              disabled={act.busy}
              onChange={async (e) => {
                if (await act.run(() => api.ruleEdit(r.id, { enabled: e.currentTarget.checked }))) onChanged();
              }}
              aria-label="有効"
            />
          </label>
          <code class={r.enabled ? "grow" : "grow meta"}>
            {r.field} {r.pattern_type} {r.pattern}
            {r.priority !== 0 && ` （優先度 ${r.priority}）`}
          </code>
          <OverflowMenu
            label="ルールの操作"
            items={[
              {
                label: "編集",
                onSelect: () => {
                  setD(mk());
                  setEditing(true);
                },
              },
              { label: "削除…", onSelect: del.start, disabled: act.busy || del.busy },
            ]}
          />
          {del.dialog}
        </span>
      )}
      <ErrorBox error={act.error} />
    </li>
  );
}

/** 活動ルール 1 行（アプリ・タイトル等 → 活動ラベル）。 */
export function ActivityRuleRow({ rule: r, onChanged }: RowProps<ActivityRule>) {
  const mk = (): Draft => ({
    field: r.field,
    type: r.pattern_type,
    pattern: r.pattern,
    priority: String(r.priority),
    label: r.activity_label,
  });
  const [editing, setEditing] = useState(false);
  const [d, setD] = useState<Draft>(mk);
  const act = useAction();
  const del = useConfirmDelete({
    what: "活動ルール",
    preview: async () => noImpact(r.id, `${r.field} ${r.pattern}`),
    commit: () => api.activityRuleDelete(r.id),
    onDeleted: onChanged,
  });
  const save = async () => {
    const x = await act.run(() =>
      api.activityRuleEdit(r.id, {
        field: d.field,
        pattern_type: d.type,
        pattern: d.pattern.trim(),
        activity_label: d.label.trim(),
        priority: priorityOf(d.priority),
      }),
    );
    if (x) {
      setEditing(false);
      onChanged();
    }
  };
  return (
    <li>
      {editing ? (
        <form
          class="row"
          onSubmit={(e) => {
            e.preventDefault();
            void save();
          }}
        >
          <EditFields d={d} set={setD} withLabel />
          <button class="primary" disabled={act.busy || !d.pattern.trim() || !d.label.trim()}>
            保存
          </button>
          <button type="button" class="ghost" onClick={() => setEditing(false)}>
            キャンセル
          </button>
        </form>
      ) : (
        <span class="row">
          <label class="check">
            <input
              type="checkbox"
              checked={r.enabled}
              disabled={act.busy}
              onChange={async (e) => {
                if (await act.run(() => api.activityRuleEdit(r.id, { enabled: e.currentTarget.checked }))) onChanged();
              }}
              aria-label="有効"
            />
          </label>
          <code class={r.enabled ? "grow" : "grow meta"}>
            {r.field} {r.pattern_type} {r.pattern} → {r.activity_label}
            {r.priority !== 0 && ` （優先度 ${r.priority}）`}
          </code>
          <OverflowMenu
            label="ルールの操作"
            items={[
              {
                label: "編集",
                onSelect: () => {
                  setD(mk());
                  setEditing(true);
                },
              },
              { label: "削除…", onSelect: del.start, disabled: act.busy || del.busy },
            ]}
          />
          {del.dialog}
        </span>
      )}
      <ErrorBox error={act.error} />
    </li>
  );
}
