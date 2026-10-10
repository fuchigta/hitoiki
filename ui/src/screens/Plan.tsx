import { useState } from "preact/hooks";
import { api } from "../api";
import { Card, Empty, ErrorBox, Field, Timeline } from "../components";
import { dateInput, fmtTime, toMillis, useAction, useAsync, useLiveTick } from "../lib";

export function Plan() {
  const [date, setDate] = useState(dateInput());
  const tick = useLiveTick();
  const blocks = useAsync(() => api.blockList({ date }), [date, tick]);
  const log = useAsync(() => api.log({ date }), [date, tick]);
  const tasks = useAsync(() => api.taskList(["todo", "doing"]), [tick]);

  const [start, setStart] = useState("09:00");
  const [end, setEnd] = useState("10:00");
  const [task, setTask] = useState("");
  const [label, setLabel] = useState("");
  const [focus, setFocus] = useState(false);
  const act = useAction();

  const submit = async (e: Event) => {
    e.preventDefault();
    const r = await act.run(() =>
      api.blockAdd({
        start: toMillis(date, start),
        end: toMillis(date, end),
        task: task || undefined,
        label: label.trim() || undefined,
        focus,
      }),
    );
    if (r) {
      setLabel("");
      blocks.reload();
    }
  };

  const range = blocks.data?.range;

  return (
    <div class="stack">
      <div class="row">
        <input type="date" value={date} onChange={(e) => setDate(e.currentTarget.value)} aria-label="日付" />
      </div>
      <Card title="ブロックを作る">
        <form class="stack" onSubmit={submit}>
          <div class="form-row">
            <Field label="開始">
              <input type="time" value={start} onInput={(e) => setStart(e.currentTarget.value)} />
            </Field>
            <Field label="終了">
              <input type="time" value={end} onInput={(e) => setEnd(e.currentTarget.value)} />
            </Field>
            <Field label="タスク">
              <select value={task} onChange={(e) => setTask(e.currentTarget.value)}>
                <option value="">（なし）</option>
                {tasks.data?.tasks.map((t) => (
                  <option key={t.id} value={t.id}>
                    {t.title}
                  </option>
                ))}
              </select>
            </Field>
            <Field label="ラベル">
              <input value={label} onInput={(e) => setLabel(e.currentTarget.value)} placeholder="任意" />
            </Field>
          </div>
          <div class="row">
            <label class="check">
              <input type="checkbox" checked={focus} onChange={(e) => setFocus(e.currentTarget.checked)} />
              集中の時間（この間は通知を控えます）
            </label>
            <span class="spacer" />
            <button class="primary" disabled={act.busy}>
              追加
            </button>
          </div>
          <ErrorBox error={act.error} />
        </form>
      </Card>
      <ErrorBox error={blocks.error ?? log.error} />
      <Card title="計画と実績">
        {range && log.data && blocks.data ? (
          blocks.data.blocks.length === 0 && log.data.segments.length === 0 ? (
            <Empty>この日の記録はありません。</Empty>
          ) : (
            <Timeline
              segments={log.data.segments}
              blocks={blocks.data.blocks}
              now={Date.now()}
              dayStart={range.start_at}
              dayEnd={range.end_at}
            />
          )
        ) : (
          <Empty>読み込み中…</Empty>
        )}
        <ul class="plain">
          {blocks.data?.blocks.map((b) => (
            <li key={b.id}>
              {fmtTime(b.start_at)}–{fmtTime(b.end_at)} {b.task_title ?? b.label ?? "（ラベルなし）"}
              {b.focus && <span class="tag">集中</span>}
            </li>
          ))}
        </ul>
      </Card>
    </div>
  );
}
