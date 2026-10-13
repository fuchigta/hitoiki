// 設定画面の「自分で作った一覧」（活動ルール・カレンダーソース・活動ラベル）。
import { useState } from "preact/hooks";
import { api, type MatchField, type PatternType } from "./api";
import { Card, Empty, ErrorBox, Field } from "./components";
import { useConfirmDelete } from "./confirm-delete";
import { useAction, useAsync } from "./lib";
import { ActivityRuleRow, FIELDS, PTYPES } from "./rule-row";
import { OverflowMenu } from "./ui/parts";

export function DataSettings() {
  return (
    <>
      <ActivityRulesCard />
      <ActivityLabelsCard />
      <CalendarSourcesCard />
    </>
  );
}

function ActivityRulesCard() {
  const rules = useAsync(() => api.activityRuleList(), []);
  const [field, setField] = useState<MatchField>("exe");
  const [ptype, setPtype] = useState<PatternType>("glob");
  const [pattern, setPattern] = useState("");
  const [label, setLabel] = useState("");
  const act = useAction();
  return (
    <Card
      title="活動ルール"
      help="アプリ名やタイトルなどのパターンに当てはまる時間へ、活動のラベルを付けます。無効にするとルールを残したまま判定に使いません。"
    >
      <ErrorBox error={rules.error} />
      {rules.data?.rules.length === 0 && <Empty>活動ルールはありません。</Empty>}
      <ul class="plain">
        {rules.data?.rules.map((r) => (
          <ActivityRuleRow key={r.id} rule={r} onChanged={rules.reload} />
        ))}
      </ul>
      <form
        class="row"
        onSubmit={async (e) => {
          e.preventDefault();
          const r = await act.run(() =>
            api.activityRuleAdd({ field, pattern: pattern.trim(), pattern_type: ptype, activity_label: label.trim() }),
          );
          if (r) {
            setPattern("");
            rules.reload();
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
        <input class="grow" value={pattern} onInput={(e) => setPattern(e.currentTarget.value)} placeholder="パターン（例: slack*）" />
        <input class="narrow" value={label} onInput={(e) => setLabel(e.currentTarget.value)} placeholder="活動" />
        <button disabled={act.busy || !pattern.trim() || !label.trim()}>追加</button>
      </form>
      <ErrorBox error={act.error} />
    </Card>
  );
}

function ActivityLabelsCard() {
  const labels = useAsync(() => api.activityLabelList(), []);
  const [add, setAdd] = useState("");
  const [renaming, setRenaming] = useState<string | null>(null);
  const [to, setTo] = useState("");
  const act = useAction();
  const after = async <R,>(fn: () => Promise<R>) => {
    if ((await act.run(fn)) !== undefined) labels.reload();
  };
  return (
    <Card title="活動ラベル" help="判定器に渡す活動の候補です。外しても、過去の時間や活動ルールに付いたラベルは残ります。">
      <ErrorBox error={labels.error} />
      <ul class="plain">
        {labels.data?.labels.map((l) => (
          <li key={l}>
            {renaming === l ? (
              <form
                class="row"
                onSubmit={async (e) => {
                  e.preventDefault();
                  await after(() => api.activityLabelRename(l, to.trim()));
                  setRenaming(null);
                }}
              >
                <input class="grow" value={to} onInput={(e) => setTo(e.currentTarget.value)} aria-label="新しい名前" />
                <button class="primary" disabled={act.busy || !to.trim()}>
                  変更
                </button>
                <button type="button" class="ghost" onClick={() => setRenaming(null)}>
                  キャンセル
                </button>
              </form>
            ) : (
              <span class="row">
                <span class="grow">{l}</span>
                <OverflowMenu
                  label={`ラベル「${l}」の操作`}
                  items={[
                    {
                      label: "名前を変える",
                      onSelect: () => {
                        setRenaming(l);
                        setTo(l);
                      },
                    },
                    { label: "外す", onSelect: () => after(() => api.activityLabelRemove(l)), disabled: act.busy },
                  ]}
                />
              </span>
            )}
          </li>
        ))}
      </ul>
      <form
        class="row"
        onSubmit={async (e) => {
          e.preventDefault();
          await after(() => api.activityLabelAdd(add.trim()));
          setAdd("");
        }}
      >
        <input class="grow" value={add} onInput={(e) => setAdd(e.currentTarget.value)} placeholder="新しいラベル" />
        <button disabled={act.busy || !add.trim()}>追加</button>
      </form>
      <ErrorBox error={act.error} />
    </Card>
  );
}

function CalendarSourcesCard() {
  const sources = useAsync(() => api.calendarSourceList(), []);
  const [name, setName] = useState("");
  const [url, setUrl] = useState("");
  const [editing, setEditing] = useState<string | null>(null);
  const [newName, setNewName] = useState("");
  const [newUrl, setNewUrl] = useState("");
  const act = useAction();
  return (
    <Card
      title="カレンダー（ICS）"
      help="ICS の URL は秘密情報なので、ここにはホスト名だけを表示します。URL を変えるときは新しい URL を入れてください。"
    >
      <ErrorBox error={sources.error} />
      {sources.data?.sources.length === 0 && <Empty>カレンダーのソースはありません。</Empty>}
      <ul class="plain">
        {sources.data?.sources.map((s) => (
          <li key={s.name}>
            {editing === s.name ? (
              <form
                class="stack"
                onSubmit={async (e) => {
                  e.preventDefault();
                  const r = await act.run(() =>
                    api.calendarSourceEdit(s.name, newName.trim() !== s.name ? newName.trim() : undefined, newUrl.trim() || undefined),
                  );
                  if (r) {
                    setEditing(null);
                    sources.reload();
                  }
                }}
              >
                <Field label="名前">
                  <input value={newName} onInput={(e) => setNewName(e.currentTarget.value)} />
                </Field>
                <Field label="新しい URL（空なら変えない）">
                  <input value={newUrl} onInput={(e) => setNewUrl(e.currentTarget.value)} placeholder="https://…" />
                </Field>
                <div class="row">
                  <span class="spacer" />
                  <button type="button" class="ghost" onClick={() => setEditing(null)}>
                    キャンセル
                  </button>
                  <button class="primary" disabled={act.busy || !newName.trim()}>
                    保存
                  </button>
                </div>
              </form>
            ) : (
              <SourceRow
                name={s.name}
                host={s.host}
                onEdit={() => {
                  setEditing(s.name);
                  setNewName(s.name);
                  setNewUrl("");
                }}
                onDeleted={sources.reload}
              />
            )}
          </li>
        ))}
      </ul>
      <form
        class="row"
        onSubmit={async (e) => {
          e.preventDefault();
          const r = await act.run(() => api.calendarSourceAdd(name.trim(), url.trim()));
          if (r) {
            setName("");
            setUrl("");
            sources.reload();
          }
        }}
      >
        <input class="narrow" value={name} onInput={(e) => setName(e.currentTarget.value)} placeholder="名前" />
        <input class="grow" value={url} onInput={(e) => setUrl(e.currentTarget.value)} placeholder="ICS の URL（https:// か webcal://）" />
        <button disabled={act.busy || !name.trim() || !url.trim()}>追加</button>
      </form>
      <ErrorBox error={act.error} />
    </Card>
  );
}

/** カレンダーのソース 1 行。編集・削除は「…」メニュー。 */
function SourceRow({ name, host, onEdit, onDeleted }: { name: string; host: string; onEdit: () => void; onDeleted: () => void }) {
  const del = useConfirmDelete({
    what: "カレンダーのソース",
    preview: () => api.calendarSourceDelete(name, { dry_run: true }),
    commit: () => api.calendarSourceDelete(name, { force: true }),
    onDeleted,
  });
  return (
    <span class="row">
      <span class="grow">
        {name} <span class="meta">（{host}）</span>
      </span>
      <OverflowMenu
        label={`「${name}」の操作`}
        items={[
          { label: "編集", onSelect: onEdit },
          { label: "削除…", onSelect: del.start, disabled: del.busy },
        ]}
      />
      {del.dialog}
    </span>
  );
}
