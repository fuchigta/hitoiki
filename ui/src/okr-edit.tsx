import type { ComponentChildren } from "preact";
import { useState } from "preact/hooks";
import { api, type KeyResult, type ObjectiveView, type SkillView } from "./api";
import { ErrorBox, Field } from "./components";
import { ConfirmDelete } from "./confirm-delete";
import { useAction } from "./lib";
import { periodEdit, periodInputs } from "./manage-util";

const STATUS_LABEL: Record<string, string> = { active: "進行中", done: "達成", dropped: "やめた" };
const STATUSES = ["active", "done", "dropped"];

function StatusSelect({ value, onChange }: { value: string; onChange: (v: string) => void }) {
  return (
    <select value={value} onChange={(e) => onChange(e.currentTarget.value)} aria-label="状態">
      {STATUSES.map((s) => (
        <option key={s} value={s}>
          {STATUS_LABEL[s]}
        </option>
      ))}
    </select>
  );
}

/** Objective 1 件。表示・編集・削除。KR の一覧は子として渡す。 */
export function ObjectiveItem({
  o,
  onChanged,
  children,
}: {
  o: ObjectiveView;
  onChanged: () => void;
  children?: ComponentChildren;
}) {
  const [editing, setEditing] = useState(false);
  const init = periodInputs(o);
  const [title, setTitle] = useState(o.title);
  const [desc, setDesc] = useState(o.description_md ?? "");
  const [from, setFrom] = useState(init.from);
  const [to, setTo] = useState(init.to);
  const [status, setStatus] = useState(o.status);
  const act = useAction();

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
    <div class="obj">
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
          <Field label="Objective">
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
          <div class="row">
            <strong class="grow">{o.title}</strong>
            {o.status !== "active" && <span class="muted">{STATUS_LABEL[o.status] ?? o.status}</span>}
            <button type="button" class="ghost" onClick={open}>
              編集
            </button>
            <ConfirmDelete
              what="Objective"
              preview={() => api.okrDeleteObjective(o.id, { dry_run: true })}
              commit={() => api.okrDeleteObjective(o.id, { force: true })}
              onDeleted={onChanged}
            />
          </div>
          {(o.period_start != null || o.period_end != null) && (
            <p class="muted">{`${init.from || "…"} 〜 ${init.to || "…"}`}</p>
          )}
          {o.description_md && <p class="muted">{o.description_md}</p>}
        </>
      )}
      {children}
    </div>
  );
}

/** KR 1 件。表示・編集（現在値・状態・付け替え）・削除。 */
export function KrItem({
  k,
  objectives,
  onChanged,
}: {
  k: KeyResult;
  objectives: ObjectiveView[];
  onChanged: () => void;
}) {
  const [editing, setEditing] = useState(false);
  const [title, setTitle] = useState(k.title);
  const [target, setTarget] = useState(k.target ?? "");
  const [current, setCurrent] = useState(k.current ?? "");
  const [status, setStatus] = useState(k.status);
  const [objective, setObjective] = useState(k.objective_id);
  const act = useAction();

  if (!editing) {
    return (
      <li>
        <span class="row">
          <span class="grow">
            {k.title}
            {(k.target || k.current) && (
              <span class="muted">
                {" / "}
                {k.current ? `現在 ${k.current}` : ""}
                {k.current && k.target ? "、" : ""}
                {k.target ? `目標 ${k.target}` : ""}
              </span>
            )}
            {k.status !== "active" && <span class="muted"> （{STATUS_LABEL[k.status] ?? k.status}）</span>}
          </span>
          <button
            type="button"
            class="ghost"
            onClick={() => {
              setTitle(k.title);
              setTarget(k.target ?? "");
              setCurrent(k.current ?? "");
              setStatus(k.status);
              setObjective(k.objective_id);
              setEditing(true);
            }}
          >
            編集
          </button>
          <ConfirmDelete
            what="KR"
            preview={() => api.okrDeleteKr(k.id, { dry_run: true })}
            commit={() => api.okrDeleteKr(k.id, { force: true })}
            onDeleted={onChanged}
          />
        </span>
      </li>
    );
  }
  return (
    <li>
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
            }),
          );
          if (r) {
            setEditing(false);
            onChanged();
          }
        }}
      >
        <Field label="Key Result">
          <input value={title} onInput={(e) => setTitle(e.currentTarget.value)} />
        </Field>
        <div class="row">
          <Field label="目標">
            <input class="narrow" value={target} onInput={(e) => setTarget(e.currentTarget.value)} />
          </Field>
          <Field label="現在値">
            <input class="narrow" value={current} onInput={(e) => setCurrent(e.currentTarget.value)} />
          </Field>
          <Field label="状態">
            <StatusSelect value={status} onChange={setStatus} />
          </Field>
          <Field label="Objective">
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
    </li>
  );
}

/** スキル 1 件。名前・親・説明の編集と、子スキルの扱いを選べる削除。 */
export function SkillItem({ s, all, onChanged }: { s: SkillView; all: SkillView[]; onChanged: () => void }) {
  const [editing, setEditing] = useState(false);
  const [name, setName] = useState(s.name);
  const [parent, setParent] = useState(s.parent_id ?? "");
  const [desc, setDesc] = useState(s.description ?? "");
  const act = useAction();
  const hasChildren = all.some((x) => x.parent_id === s.id);

  if (!editing) {
    return (
      <li>
        <span class="row">
          <span class="grow">
            {s.parent_name && <span class="muted">{s.parent_name} › </span>}
            {s.name}
            {s.preset && <span class="muted"> （プリセット）</span>}
            {s.description && <span class="muted"> / {s.description}</span>}
          </span>
          <button
            type="button"
            class="ghost"
            onClick={() => {
              setName(s.name);
              setParent(s.parent_id ?? "");
              setDesc(s.description ?? "");
              setEditing(true);
            }}
          >
            編集
          </button>
          <ConfirmDelete
            what="スキル"
            preview={(c) => api.skillDelete(s.id, c === "delete" ? "delete" : "reparent", { dry_run: true })}
            commit={(c) => api.skillDelete(s.id, c === "delete" ? "delete" : "reparent", { force: true })}
            onDeleted={onChanged}
            choice={
              hasChildren
                ? {
                    label: "子スキルの扱い",
                    initial: "reparent",
                    options: [
                      { value: "reparent", label: "子スキルは残す（親を付け替える）" },
                      { value: "delete", label: "子スキルも一緒に削除する" },
                    ],
                  }
                : undefined
            }
          />
        </span>
      </li>
    );
  }
  return (
    <li>
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
          if (r) {
            setEditing(false);
            onChanged();
          }
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
          <button type="button" class="ghost" onClick={() => setEditing(false)}>
            キャンセル
          </button>
          <button class="primary" disabled={act.busy || !name.trim()}>
            保存
          </button>
        </div>
        <ErrorBox error={act.error} />
      </form>
    </li>
  );
}
