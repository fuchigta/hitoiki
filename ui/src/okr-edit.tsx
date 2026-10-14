import { useState } from "preact/hooks";
import { api, type KeyResult, type KrMeasure, type ObjectiveView, type SkillView } from "./api";
import { Card, Empty, ErrorBox, Field, Section } from "./components";
import { useConfirmDelete } from "./confirm-delete";
import { KrDetail } from "./kr-detail";
import { MEASURE_OPTION, MEASURES, parseDayMin } from "./kr-util";
import { hrefOf, navigate, useAction, useAsync } from "./lib";
import { periodEdit, periodInputs } from "./manage-util";
import { tasksWithSkill } from "./okr-util";
import { SkillPicker } from "./skill-picker";
import { Help, OverflowMenu } from "./ui/parts";

export const OKR_STATUS_LABEL: Record<string, string> = { active: "進行中", done: "達成", dropped: "やめた" };
const STATUSES = ["active", "done", "dropped"];

function StatusSelect({ value, onChange }: { value: string; onChange: (v: string) => void }) {
  return (
    <select value={value} onChange={(e) => onChange(e.currentTarget.value)} aria-label="状態">
      {STATUSES.map((s) => (
        <option key={s} value={s}>
          {OKR_STATUS_LABEL[s]}
        </option>
      ))}
    </select>
  );
}

/** 目標の削除（確認ダイアログつき）。一覧の行のメニューと詳細のメニューで共有する。 */
export function useObjectiveDelete(id: string, onDeleted: () => void) {
  return useConfirmDelete({
    what: "目標",
    preview: () => api.okrDeleteObjective(id, { dry_run: true }),
    commit: () => api.okrDeleteObjective(id, { force: true }),
    onDeleted,
  });
}

// ===== 目標 =====

/** 目標の詳細。タイトル・説明・期間・状態の編集と、配下の成果の指標の一覧（リンク）。 */
export function ObjectiveDetail({
  o,
  onChanged,
  onDeleted,
  initialEditing = false,
  initialAdding = false,
}: {
  o: ObjectiveView;
  onChanged: () => void;
  onDeleted: () => void;
  initialEditing?: boolean;
  initialAdding?: boolean;
}) {
  const init = periodInputs(o);
  const [editing, setEditing] = useState(initialEditing);
  const [adding, setAdding] = useState(initialAdding);
  const [title, setTitle] = useState(o.title);
  const [desc, setDesc] = useState(o.description_md ?? "");
  const [from, setFrom] = useState(init.from);
  const [to, setTo] = useState(init.to);
  const [status, setStatus] = useState(o.status);
  const act = useAction();
  const del = useObjectiveDelete(o.id, onDeleted);

  const open = () => {
    const p = periodInputs(o);
    setTitle(o.title);
    setDesc(o.description_md ?? "");
    setFrom(p.from);
    setTo(p.to);
    setStatus(o.status);
    setEditing(true);
  };

  return (
    <Card
      title={o.title}
      aside={
        <>
          {o.status !== "active" && <span class="tag">{OKR_STATUS_LABEL[o.status] ?? o.status}</span>}
          <OverflowMenu
            label="目標の操作"
            items={[
              { label: "編集", onSelect: open },
              { label: "成果の指標を追加", onSelect: () => setAdding(true) },
              { label: "削除…", onSelect: del.start, disabled: del.busy },
            ]}
          />
          {del.dialog}
        </>
      }
    >
      {editing ? (
        <form
          class="stack"
          onSubmit={async (e) => {
            e.preventDefault();
            const r = await act.run(() =>
              api.okrEditObjective(o.id, {
                title: title.trim(),
                description: desc.trim(),
                status,
                ...periodEdit(from, to, o),
              }),
            );
            if (r) {
              setEditing(false);
              onChanged();
            }
          }}
        >
          <Field label="目標">
            <input value={title} onInput={(e) => setTitle(e.currentTarget.value)} />
          </Field>
          <Field label="説明">
            <textarea rows={2} value={desc} onInput={(e) => setDesc(e.currentTarget.value)} />
          </Field>
          <div class="row">
            <Field label="期間の開始">
              <input type="date" value={from} onInput={(e) => setFrom(e.currentTarget.value)} />
            </Field>
            <Field label="終了（この日を含む）">
              <input type="date" value={to} onInput={(e) => setTo(e.currentTarget.value)} />
            </Field>
            <Field label="状態">
              <StatusSelect value={status} onChange={setStatus} />
            </Field>
          </div>
          <div class="row">
            <span class="spacer" />
            <button type="button" class="ghost" onClick={() => setEditing(false)}>
              キャンセル
            </button>
            <button class="primary" disabled={act.busy || !title.trim()}>
              保存
            </button>
          </div>
          <ErrorBox error={act.error} />
        </form>
      ) : (
        <>
          {(o.period_start != null || o.period_end != null) && (
            <p class="meta">{`期間 ${init.from || "…"} 〜 ${init.to || "…"}`}</p>
          )}
          {o.description_md && <p class="hint">{o.description_md}</p>}
        </>
      )}
      <Section
        level={3}
        title="成果の指標"
        aside={
          !adding && (
            <button type="button" class="ghost" onClick={() => setAdding(true)}>
              ＋ 成果の指標
            </button>
          )
        }
      >
        {o.key_results.length === 0 && <Empty>成果の指標はまだありません。</Empty>}
        <ul class="link-list">
          {o.key_results.map((k) => (
            <li key={k.id}>
              <a href={hrefOf("okr", ["kr", k.id])}>{k.title}</a>
              {k.target && <span class="meta">目標値 {k.target}</span>}
              {k.status !== "active" && <span class="tag">{OKR_STATUS_LABEL[k.status] ?? k.status}</span>}
            </li>
          ))}
        </ul>
        {adding && (
          <KrForm
            objective={o.id}
            onCancel={() => setAdding(false)}
            onAdded={(k) => {
              setAdding(false);
              onChanged();
              navigate(hrefOf("okr", ["kr", k.id]));
            }}
          />
        )}
      </Section>
    </Card>
  );
}

/** 成果の指標を追加するフォーム（タイトル・目標値・スキル）。 */
function KrForm({
  objective,
  onAdded,
  onCancel,
}: {
  objective: string;
  onAdded: (k: KeyResult) => void;
  onCancel: () => void;
}) {
  const [title, setTitle] = useState("");
  const [target, setTarget] = useState("");
  const [skillNames, setSkillNames] = useState<string[]>([]);
  const known = useAsync(() => api.skillList(), []);
  const act = useAction();
  return (
    <form
      class="stack"
      onSubmit={async (e) => {
        e.preventDefault();
        const r = await act.run(() => api.okrAddKr(objective, title.trim(), target.trim() || undefined, skillNames));
        if (r) onAdded(r);
      }}
    >
      <div class="row">
        <input
          class="grow"
          value={title}
          onInput={(e) => setTitle(e.currentTarget.value)}
          placeholder="成果の指標"
          aria-label="成果の指標"
        />
        <input
          class="narrow"
          value={target}
          onInput={(e) => setTarget(e.currentTarget.value)}
          placeholder="目標値"
          aria-label="目標値"
        />
        <button class="primary" disabled={act.busy || !title.trim()}>
          追加
        </button>
        <button type="button" class="ghost" onClick={onCancel}>
          キャンセル
        </button>
      </div>
      <div class="field">
        <span>
          スキル{" "}
          <Help>このスキルが付いたタスク・活動の時間をこの成果の指標の時間として数えます。タスクの紐付けは作成後に補助として足せます。</Help>
        </span>
        <SkillPicker
          value={skillNames}
          skills={known.data?.skills ?? []}
          onChange={setSkillNames}
          ariaLabel="成果の指標のスキルを追加"
          disabled={act.busy}
        />
      </div>
      <ErrorBox error={act.error ?? known.error} />
    </form>
  );
}

// ===== 成果の指標 =====

/** 成果の指標の詳細。ヘッダーの「編集」で、タイトル・目標値・現在値・軸・状態・目標の編集に切り替える。 */
export function KrPanel({
  k,
  objectives,
  onChanged,
  onDeleted,
}: {
  k: KeyResult;
  objectives: ObjectiveView[];
  onChanged: () => void;
  onDeleted: () => void;
}) {
  const [editing, setEditing] = useState(false);
  const [title, setTitle] = useState(k.title);
  const [target, setTarget] = useState(k.target ?? "");
  const [current, setCurrent] = useState(k.current ?? "");
  const [status, setStatus] = useState(k.status);
  const [objective, setObjective] = useState(k.objective_id);
  const [measure, setMeasure] = useState<KrMeasure>(k.measure);
  const [dayMin, setDayMin] = useState(String(k.day_min));
  const act = useAction();
  const del = useConfirmDelete({
    what: "成果の指標",
    preview: () => api.okrDeleteKr(k.id, { dry_run: true }),
    commit: () => api.okrDeleteKr(k.id, { force: true }),
    onDeleted,
  });
  const parent = objectives.find((o) => o.id === k.objective_id);

  const open = () => {
    setTitle(k.title);
    setTarget(k.target ?? "");
    setCurrent(k.current ?? "");
    setStatus(k.status);
    setObjective(k.objective_id);
    setMeasure(k.measure);
    setDayMin(String(k.day_min));
    setEditing(true);
  };

  return (
    <Card
      title={k.title}
      aside={
        <>
          {k.status !== "active" && <span class="tag">{OKR_STATUS_LABEL[k.status] ?? k.status}</span>}
          {!editing && (
            <button type="button" onClick={open}>
              編集
            </button>
          )}
          <OverflowMenu label="成果の指標の操作" items={[{ label: "削除…", onSelect: del.start, disabled: del.busy }]} />
          {del.dialog}
        </>
      }
    >
      {editing ? (
        <form
          class="stack"
          onSubmit={async (e) => {
            e.preventDefault();
            const r = await act.run(() =>
              api.okrEditKr(k.id, {
                title: title.trim(),
                target: target.trim(),
                current: current.trim(),
                status,
                objective: objective !== k.objective_id ? objective : undefined,
                measure,
                day_min: measure === "days" ? parseDayMin(dayMin, k.day_min) : undefined,
              }),
            );
            if (r) {
              setEditing(false);
              onChanged();
            }
          }}
        >
          <Field label="成果の指標">
            <input value={title} onInput={(e) => setTitle(e.currentTarget.value)} />
          </Field>
          <div class="row">
            <Field label="目標値">
              <input class="narrow" value={target} onInput={(e) => setTarget(e.currentTarget.value)} />
            </Field>
            <Field label="現在値">
              <input class="narrow" value={current} onInput={(e) => setCurrent(e.currentTarget.value)} />
            </Field>
            <Field label="状態">
              <StatusSelect value={status} onChange={setStatus} />
            </Field>
            <Field label="目標">
              <select value={objective} onChange={(e) => setObjective(e.currentTarget.value)}>
                {objectives.map((o) => (
                  <option key={o.id} value={o.id}>
                    {o.title}
                  </option>
                ))}
              </select>
            </Field>
          </div>
          <div class="row">
            <Field label="何を軸に見るか">
              <select value={measure} onChange={(e) => setMeasure(e.currentTarget.value as KrMeasure)}>
                {MEASURES.map((m) => (
                  <option key={m} value={m}>
                    {MEASURE_OPTION[m]}
                  </option>
                ))}
              </select>
            </Field>
            {measure === "days" && (
              <Field label="取り組んだ日とみなす 1 日の最小分数">
                <input
                  class="narrow"
                  inputMode="numeric"
                  value={dayMin}
                  onInput={(e) => setDayMin(e.currentTarget.value)}
                />
              </Field>
            )}
          </div>
          <div class="row">
            <span class="spacer" />
            <button type="button" class="ghost" onClick={() => setEditing(false)}>
              キャンセル
            </button>
            <button class="primary" disabled={act.busy || !title.trim()}>
              保存
            </button>
          </div>
          <ErrorBox error={act.error} />
        </form>
      ) : (
        <>
          <div class="relations">
            {parent && (
              <a class="link-chip" href={hrefOf("okr", ["o", parent.id])} title="この目標の詳細を開く">
                {parent.title}
              </a>
            )}
            {(k.target || k.current) && (
              <span class="meta">
                {k.current ? `現在 ${k.current}` : ""}
                {k.current && k.target ? "、" : ""}
                {k.target ? `目標値 ${k.target}` : ""}
              </span>
            )}
          </div>
          <KrDetail k={k} onChanged={onChanged} />
        </>
      )}
    </Card>
  );
}

// ===== スキル =====

/** スキルの詳細。名前・親・説明の編集と、このスキルが付いたタスク・成果の指標（リンク。参照のみ）。削除は「…」から。 */
export function SkillDetail({
  s,
  all,
  onChanged,
  onDeleted,
}: {
  s: SkillView;
  all: SkillView[];
  onChanged: () => void;
  onDeleted: () => void;
}) {
  const [name, setName] = useState(s.name);
  const [parent, setParent] = useState(s.parent_id ?? "");
  const [desc, setDesc] = useState(s.description ?? "");
  const act = useAction();
  const hasChildren = all.some((x) => x.parent_id === s.id);
  const del = useConfirmDelete({
    what: "スキル",
    preview: (c) => api.skillDelete(s.id, c === "delete" ? "delete" : "reparent", { dry_run: true }),
    commit: (c) => api.skillDelete(s.id, c === "delete" ? "delete" : "reparent", { force: true }),
    onDeleted,
    choice: hasChildren
      ? {
          label: "子スキルの扱い",
          initial: "reparent",
          options: [
            { value: "reparent", label: "子スキルは残す（親を付け替える）" },
            { value: "delete", label: "子スキルも一緒に削除する" },
          ],
        }
      : undefined,
  });

  return (
    <Card
      title={s.name}
      aside={
        <>
          {s.preset && <span class="tag">プリセット</span>}
          <OverflowMenu label="スキルの操作" items={[{ label: "削除…", onSelect: del.start, disabled: del.busy }]} />
          {del.dialog}
        </>
      }
    >
      <form
        class="stack"
        onSubmit={async (e) => {
          e.preventDefault();
          const r = await act.run(() =>
            api.skillEdit(s.id, {
              name: name.trim(),
              parent: parent || undefined,
              clear_parent: parent === "",
              description: desc.trim(),
            }),
          );
          if (r) onChanged();
        }}
      >
        <div class="row">
          <Field label="名前">
            <input value={name} onInput={(e) => setName(e.currentTarget.value)} />
          </Field>
          <Field label="親">
            <select value={parent} onChange={(e) => setParent(e.currentTarget.value)}>
              <option value="">（なし）</option>
              {all
                .filter((x) => x.id !== s.id)
                .map((x) => (
                  <option key={x.id} value={x.id}>
                    {x.name}
                  </option>
                ))}
            </select>
          </Field>
        </div>
        <Field label="説明">
          <input value={desc} onInput={(e) => setDesc(e.currentTarget.value)} />
        </Field>
        <div class="row">
          <span class="spacer" />
          <button class="primary" disabled={act.busy || !name.trim()}>
            保存
          </button>
        </div>
        <ErrorBox error={act.error} />
      </form>
      <SkillUsers name={s.name} />
    </Card>
  );
}

/** このスキルが付いたタスクと成果の指標。リンクで相手の詳細へ移る（付け外しは相手の側で行う）。 */
function SkillUsers({ name }: { name: string }) {
  const tasks = useAsync(() => api.taskList(["todo", "doing", "done", "dropped"]), []);
  const krs = useAsync(async () => {
    const { objectives } = await api.okrList();
    const all = objectives.flatMap((o) => o.key_results.map((k) => ({ k, o })));
    const sums = await Promise.all(all.map((x) => api.okrShow(x.k.id).catch(() => null)));
    return all.filter((_, i) => sums[i]?.kr_skills.includes(name));
  }, [name]);
  const ts = tasksWithSkill(tasks.data?.tasks ?? [], name);
  return (
    <>
      <Section level={3} title="このスキルが付いたタスク" help="付け外しは、タスクの詳細の「概要」で行います。">
        <ErrorBox error={tasks.error} />
        {tasks.data && ts.length === 0 && <Empty>付いているタスクはありません。</Empty>}
        <ul class="link-list">
          {ts.map((t) => (
            <li key={t.id}>
              <a href={hrefOf("tasks", [t.id])}>{t.title}</a>
            </li>
          ))}
        </ul>
      </Section>
      <Section level={3} title="このスキルが付いた成果の指標" help="付け外しは、成果の指標の詳細の「スキル」で行います。">
        <ErrorBox error={krs.error} />
        {krs.data && krs.data.length === 0 && <Empty>付いている成果の指標はありません。</Empty>}
        <ul class="link-list">
          {krs.data?.map(({ k, o }) => (
            <li key={k.id}>
              <a href={hrefOf("okr", ["kr", k.id])}>{k.title}</a>
              <span class="meta">{o.title}</span>
            </li>
          ))}
        </ul>
      </Section>
    </>
  );
}
