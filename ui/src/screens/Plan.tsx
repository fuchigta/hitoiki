import { useState } from "preact/hooks";
import { api } from "../api";
import type { CalendarEvent } from "../api";
import { blockTitle } from "../block-form";
import { Card, Empty, ErrorBox } from "../components";
import { DayDesigner } from "../day-designer";
import { dateInput, fmtTime, useAction, useAsync, useLiveTick } from "../lib";
import { TemplateManager } from "../templates";

/** 計画（タイムブロッキング）。ブロックの作成・編集はタイムライン上で直接行う（今日の画面と同じ部品）。 */
export function Plan() {
  const [date, setDate] = useState(dateInput());
  const tick = useLiveTick();
  const blocks = useAsync(() => api.blockList({ date }), [date, tick]);
  const log = useAsync(() => api.log({ date }), [date, tick]);
  const cal = useAsync(() => api.calendarList({ date }).catch(() => [] as CalendarEvent[]), [date, tick]);
  const tasks = useAsync(() => api.taskList(["todo", "doing"]), [tick]);
  const act = useAction();

  const range = blocks.data?.range;

  const remove = async (id: string) => {
    const r = await act.run(() => api.blockDelete(id));
    if (r !== undefined) blocks.reload();
  };

  return (
    <div class="stack">
      <div class="row">
        <input type="date" value={date} onChange={(e) => setDate(e.currentTarget.value)} aria-label="日付" />
        <span class="muted">
          {date === dateInput() && (
            <>
              今日のブロックは<a href="#/today">今日の画面</a>でも作れます。
            </>
          )}
        </span>
      </div>
      <ErrorBox error={blocks.error ?? log.error ?? act.error} />
      <Card title="計画と実績">
        {range && log.data && blocks.data ? (
          <>
            <p class="muted">
              「計画」の列を上下にドラッグすると、15 分刻みで時間を選んでブロックを作れます（クリックだけなら 1 時間）。ブロックはドラッグで動かし、上端・下端で長さを変え、クリックで編集します。
            </p>
            <DayDesigner
              date={date}
              dayStart={range.start_at}
              dayEnd={range.end_at}
              view={{ start: range.start_at, end: range.end_at }}
              blocks={blocks.data.blocks}
              segments={log.data.segments}
              events={cal.data ?? []}
              gaps={log.data.gaps}
              tasks={tasks.data?.tasks ?? []}
              reload={blocks.reload}
            />
            {blocks.data.blocks.length === 0 && log.data.segments.length === 0 && (
              <Empty>この日の記録はありません。</Empty>
            )}
          </>
        ) : (
          <Empty>読み込み中…</Empty>
        )}
        <ul class="plain">
          {blocks.data?.blocks.map((b) => (
            <li key={b.id} class="row">
              <span>
                {fmtTime(b.start_at)}–{fmtTime(b.end_at)} {blockTitle(b)}
                {b.focus && <span class="tag">集中</span>}
              </span>
              <span class="spacer" />
              <button type="button" onClick={() => remove(b.id)} disabled={act.busy}>
                削除
              </button>
            </li>
          ))}
        </ul>
      </Card>
      <Card title="テンプレート（働き方のパターン）">
        <p class="muted">
          よく使う 1 日の配置を保存しておき、曜日の既定にすると、今日の画面の「今日のデザイン」で提案されます。
        </p>
        <TemplateManager date={date} tasks={tasks.data?.tasks ?? []} />
      </Card>
    </div>
  );
}
