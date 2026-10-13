import { useState } from "preact/hooks";
import { api, type IdlePreset, type IgnoreSuggestion, type RuleSuggestion, type SegmentKind, type SegmentView } from "../api";
import { Card, Empty, ErrorBox } from "../components";
import { OverflowMenu, PeriodSelect } from "../ui/parts";
import { suggestionLine } from "../ignore-util";
import { KIND_LABEL, KIND_ORDER, fmtDateTime, fmtDur, useAction, useAsync, useLiveTick } from "../lib";

/** 短い区間のしきい値の表記（「1分」「90秒」）。 */
function fmtShortLimit(sec: number): string {
  return sec >= 60 && sec % 60 === 0 ? `${sec / 60}分` : `${sec}秒`;
}

export function Review() {
  const [period, setPeriod] = useState("this-week");
  const tick = useLiveTick();
  const [showShort, setShowShort] = useState(false);
  const list = useAsync(() => api.reviewList({ date: period }, showShort), [period, showShort, tick]);
  const tasks = useAsync(() => api.taskList(["todo", "doing"]), [tick]);
  const suggestions = useAsync(() => api.reviewSuggestions(), [tick]);
  const ignoreSuggestions = useAsync(() => api.ignoreSuggestions(), [tick]);
  const [selected, setSelected] = useState<string[]>([]);
  const toggle = (id: string) =>
    setSelected((cur) => (cur.includes(id) ? cur.filter((x) => x !== id) : [...cur, id]));
  const done = () => {
    setSelected([]);
    list.reload();
    suggestions.reload();
    ignoreSuggestions.reload();
  };
  // 一覧から消えた区間は選択から外す。
  const visible = list.data?.segments ?? [];
  const chosen = selected.filter((id) => visible.some((s) => s.id === id));

  return (
    <div class="stack">
      <div class="row">
        <PeriodSelect value={period} onChange={setPeriod} />
        <span class="meta">{list.data ? `${list.data.count}件` : ""}</span>
        {list.data && (list.data.hidden_short > 0 || showShort) && (
          <label class="row hint">
            <input
              type="checkbox"
              checked={showShort}
              onChange={(e) => setShowShort(e.currentTarget.checked)}
            />
            {showShort
              ? `短い区間（${fmtShortLimit(list.data.min_sec)}未満）も表示中`
              : `短い区間 ${list.data.hidden_short} 件を表示（${fmtShortLimit(list.data.min_sec)}未満）`}
          </label>
        )}
      </div>
      <ErrorBox error={list.error ?? tasks.error ?? suggestions.error ?? ignoreSuggestions.error} />
      {(ignoreSuggestions.data ?? []).slice(0, 3).map((sg) => (
        <IgnoreSuggestionCard key={sg.exe} sg={sg} onDone={done} />
      ))}
      {(suggestions.data ?? []).map((sg) => (
        <SuggestionCard key={`${sg.task_id}|${sg.field}|${sg.pattern}`} sg={sg} onDone={done} />
      ))}
      {visible.length > 1 && (
        <BatchBar
          ids={chosen}
          all={visible.map((s) => s.id)}
          tasks={tasks.data?.tasks ?? []}
          onSelect={setSelected}
          onDone={done}
        />
      )}
      {list.data && list.data.segments.length === 0 && <Empty>
          {list.data.hidden_short > 0
            ? "確認が必要な区間はありません（短い区間のみ隠れています）。"
            : "確認が必要な区間はありません。"}
        </Empty>}
      {list.data?.segments.map((s) => (
        <ReviewItem
          key={s.id}
          seg={s}
          tasks={tasks.data?.tasks ?? []}
          onDone={done}
          selected={chosen.includes(s.id)}
          onToggle={() => toggle(s.id)}
        />
      ))}
    </div>
  );
}

/** 無操作区間のワンタップ分類（core の `IdlePreset` と同じ並び）。 */
const IDLE_PRESETS: { id: IdlePreset; label: string }[] = [
  { id: "break", label: "休憩" },
  { id: "meeting", label: "会議" },
  { id: "thinking", label: "考えごと" },
  { id: "away", label: "離席" },
];

/** 「Orca.exe はいつもこのタスクにしますか？」というルール化の提案。 */
function SuggestionCard({ sg, onDone }: { sg: RuleSuggestion; onDone: () => void }) {
  const act = useAction();
  const ref = { task_id: sg.task_id, field: sg.field, pattern: sg.pattern, pattern_type: sg.pattern_type };
  const accept = async () => {
    if (await act.run(() => api.reviewSuggestAccept(ref))) onDone();
  };
  const dismiss = async () => {
    // 戻り値は void なので、エラーが無かったかで判定する。
    const r = await act.run(async () => {
      await api.reviewSuggestDismiss(ref);
      return true;
    });
    if (r) onDone();
  };
  return (
    <Card>
      <div class="row">
        <span>
          <strong>{sg.target}</strong> はいつも「{sg.task_title ?? "このタスク"}」にしますか？
          <span class="meta"> （最近 {sg.count} 回、同じ割り当てを確定しました）</span>
        </span>
        <span class="spacer" />
        <button class="ghost" disabled={act.busy} onClick={dismiss}>
          今はしない
        </button>
        <button disabled={act.busy} onClick={accept}>
          ルールにする
        </button>
      </div>
      <ErrorBox error={act.error} />
    </Card>
  );
}

/** 「explorer.exe は短い区間が 214 件（合計 16.5分）。無視しますか？」という無視の提案。 */
function IgnoreSuggestionCard({ sg, onDone }: { sg: IgnoreSuggestion; onDone: () => void }) {
  const act = useAction();
  const accept = async () => {
    if (await act.run(() => api.ignoreAdd(sg.exe))) onDone();
  };
  const dismiss = async () => {
    const r = await act.run(async () => {
      await api.ignoreSuggestDismiss(sg.exe);
      return true;
    });
    if (r) onDone();
  };
  return (
    <Card>
      <div class="row">
        <span>
          {suggestionLine(sg)}無視しますか？
          <span class="meta"> （{sg.reasons.map((r) => r.text).join("、")}。無視しても記録は消えず、いつでも戻せます）</span>
        </span>
        <span class="spacer" />
        <button class="ghost" disabled={act.busy} onClick={dismiss}>
          今回はしない
        </button>
        <button disabled={act.busy} onClick={accept}>
          無視する
        </button>
      </div>
      <ErrorBox error={act.error} />
    </Card>
  );
}

/** 複数選択して、同じタスク・種別・活動をまとめて割り当てる。 */
function BatchBar({
  ids,
  all,
  tasks,
  onSelect,
  onDone,
}: {
  ids: string[];
  all: string[];
  tasks: { id: string; title: string }[];
  onSelect: (ids: string[]) => void;
  onDone: () => void;
}) {
  const [task, setTask] = useState("");
  const [kind, setKind] = useState<SegmentKind | "">("");
  const [activity, setActivity] = useState("");
  const [rule, setRule] = useState(false);
  const act = useAction();

  const submit = async () => {
    const r = await act.run(() =>
      api.reviewSetMany({
        segments: ids,
        task: task || undefined,
        kind: kind || undefined,
        activity: activity.trim() || undefined,
        rule,
      }),
    );
    if (r) {
      setTask("");
      setKind("");
      setActivity("");
      setRule(false);
      onDone();
    }
  };

  const ignoreAll = async () => {
    if (await act.run(() => api.reviewIgnore(ids))) onDone();
  };

  return (
    <Card>
      <div class="row">
        <label class="check">
          <input
            type="checkbox"
            checked={ids.length === all.length}
            onChange={(e) => onSelect(e.currentTarget.checked ? all : [])}
          />
          すべて選ぶ
        </label>
        <span class="meta">{ids.length > 0 ? `${ids.length}件を選択中` : "まとめて仕分けるには、区間を選んでください"}</span>
      </div>
      {ids.length > 0 && (
        <>
          <div class="form-row">
            <label class="field">
              <span>タスク</span>
              <select value={task} onChange={(e) => setTask(e.currentTarget.value)}>
                <option value="">（割り当てない）</option>
                {tasks.map((t) => (
                  <option key={t.id} value={t.id}>
                    {t.title}
                  </option>
                ))}
              </select>
            </label>
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
            <button class="ghost" disabled={act.busy} onClick={ignoreAll} title="数えない時間にします。区間の操作からいつでも戻せます">
              {ids.length}件を無視する
            </button>
            <button class="primary" disabled={act.busy || (!task && !kind && !activity.trim())} onClick={submit}>
              {ids.length}件をまとめて確定
            </button>
          </div>
        </>
      )}
      <ErrorBox error={act.error} />
    </Card>
  );
}

function ReviewItem({
  seg,
  tasks,
  onDone,
  selected,
  onToggle,
}: {
  seg: SegmentView;
  tasks: { id: string; title: string }[];
  onDone: () => void;
  selected: boolean;
  onToggle: () => void;
}) {
  const [task, setTask] = useState(seg.task_id ?? "");
  const [kind, setKind] = useState<SegmentKind | "">("");
  const [activity, setActivity] = useState(seg.activity_label ?? "");
  const [rule, setRule] = useState(false);
  const act = useAction();

  const submit = async () => {
    const r = await act.run(() =>
      api.reviewSet({
        segment: seg.id,
        task: task || undefined,
        kind: kind || undefined,
        activity: activity.trim() || undefined,
        rule,
      }),
    );
    if (r) onDone();
  };

  // 無操作区間をワンタップで分類する。
  const asIdle = async (preset: IdlePreset) => {
    const r = await act.run(() => api.reviewIdle([seg.id], preset));
    if (r) onDone();
  };

  // この区間だけを無視する。
  const ignoreThis = async () => {
    if (await act.run(() => api.reviewIgnore([seg.id]))) onDone();
  };

  // このアプリを今後も無視する（無視リストに追加し、直近 7 日を再計算する）。
  const ignoreApp = async () => {
    if (seg.exe_name && (await act.run(() => api.ignoreAdd(seg.exe_name!)))) onDone();
  };

  // 割り込みを新しいタスクにして割り当てる。
  const asInterrupt = async () => {
    const title = (seg.title || seg.exe_name || "割り込み").slice(0, 60);
    const r = await act.run(async () => {
      const t = await api.taskAdd({ title, interrupt: true });
      return api.reviewSet({ segment: seg.id, task: t.id, rule });
    });
    if (r) onDone();
  };

  return (
    <Card>
      <div class="review-head">
        <label class="check" title="まとめて仕分けるために選ぶ">
          <input type="checkbox" checked={selected} onChange={onToggle} aria-label="この区間を選ぶ" />
        </label>
        <div>
          <h3 class="review-title">{seg.title || seg.exe_name || "（タイトルなし）"}</h3>
          <div class="meta">
            {fmtDateTime(seg.start_at)} – {fmtDur(seg.duration_ms)} ・ {KIND_LABEL[seg.kind]}
            {seg.exe_name ? ` ・ ${seg.exe_name}` : ""}
            {seg.confidence != null ? ` ・ 確信度 ${Math.round(seg.confidence * 100)}%` : ""}
          </div>
        </div>
      </div>
      {seg.kind === "idle" && (
        <div class="row" role="group" aria-label="この無操作の時間は">
          <span class="meta">この時間は</span>
          {IDLE_PRESETS.map((p) => (
            <button key={p.id} class="chip" disabled={act.busy} onClick={() => asIdle(p.id)}>
              {p.label}
            </button>
          ))}
        </div>
      )}
      <div class="form-row">
        <label class="field">
          <span>タスク</span>
          <select value={task} onChange={(e) => setTask(e.currentTarget.value)}>
            <option value="">（割り当てない）</option>
            {tasks.map((t) => (
              <option key={t.id} value={t.id}>
                {t.title}
              </option>
            ))}
          </select>
        </label>
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
        <OverflowMenu
          label="この区間の操作"
          items={[
            { label: "この区間を無視", onSelect: ignoreThis, disabled: act.busy },
            ...(seg.exe_name
              ? [{ label: `${seg.exe_name} を今後も無視`, onSelect: ignoreApp, disabled: act.busy }]
              : []),
            { label: "割り込みとしてタスク化", onSelect: asInterrupt, disabled: act.busy },
          ]}
        />
        <button disabled={act.busy || (!task && !kind && !activity.trim())} onClick={submit}>
          確定
        </button>
      </div>
      <ErrorBox error={act.error} />
    </Card>
  );
}
