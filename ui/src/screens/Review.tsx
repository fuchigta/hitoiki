import { useState } from "preact/hooks";
import { api, type SegmentKind, type SegmentView } from "../api";
import { Card, Empty, ErrorBox } from "../components";
import { KIND_LABEL, KIND_ORDER, fmtDateTime, fmtDur, useAction, useAsync, useLiveTick } from "../lib";

const PERIODS = [
  { id: "today", label: "今日" },
  { id: "yesterday", label: "昨日" },
  { id: "this-week", label: "今週" },
  { id: "last-week", label: "先週" },
];

export function Review() {
  const [period, setPeriod] = useState("this-week");
  const tick = useLiveTick();
  const list = useAsync(() => api.reviewList({ date: period }), [period, tick]);
  const tasks = useAsync(() => api.taskList(["todo", "doing"]), [tick]);

  return (
    <div class="stack">
      <div class="row">
        <select value={period} onChange={(e) => setPeriod(e.currentTarget.value)} aria-label="期間">
          {PERIODS.map((p) => (
            <option key={p.id} value={p.id}>
              {p.label}
            </option>
          ))}
        </select>
        <span class="muted">{list.data ? `${list.data.count}件` : ""}</span>
      </div>
      <ErrorBox error={list.error ?? tasks.error} />
      {list.data && list.data.segments.length === 0 && <Empty>確認が必要な区間はありません。</Empty>}
      {list.data?.segments.map((s) => (
        <ReviewItem key={s.id} seg={s} tasks={tasks.data?.tasks ?? []} onDone={list.reload} />
      ))}
    </div>
  );
}

function ReviewItem({
  seg,
  tasks,
  onDone,
}: {
  seg: SegmentView;
  tasks: { id: string; title: string }[];
  onDone: () => void;
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
        <div>
          <strong>{seg.title || seg.exe_name || "（タイトルなし）"}</strong>
          <div class="muted">
            {fmtDateTime(seg.start_at)} – {fmtDur(seg.duration_ms)} ・ {KIND_LABEL[seg.kind]}
            {seg.exe_name ? ` ・ ${seg.exe_name}` : ""}
            {seg.confidence != null ? ` ・ 確信度 ${Math.round(seg.confidence * 100)}%` : ""}
          </div>
        </div>
      </div>
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
        <button class="ghost" disabled={act.busy} onClick={asInterrupt} title="この区間をタイトルから新しい割り込みタスクにして割り当てます">
          割り込みとしてタスク化
        </button>
        <button class="primary" disabled={act.busy || (!task && !kind && !activity.trim())} onClick={submit}>
          確定
        </button>
      </div>
      <ErrorBox error={act.error} />
    </Card>
  );
}
