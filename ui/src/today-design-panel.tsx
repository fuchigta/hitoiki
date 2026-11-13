// 今日のデザインのパネル。テンプレート、その日の予定、未完了のタスク（タイムラインへ置く候補）。
import type { CalendarEvent, Suggestion, TaskView } from "./api";
import { Empty, Section } from "./components";
import { fmtTime } from "./lib";
import { suggestionText, weekdayOf } from "./template-util";
import { ApplyTemplate, SaveAsTemplate } from "./templates";
import type { TaskDrag } from "./use-task-drag";

interface Props {
  /** 表示している日（YYYY-MM-DD）。 */
  date: string;
  isToday: boolean;
  /** 見出しに出す日の名前（「今日」か日付）。 */
  dayName: string;
  /** テンプレートの提案（届くまで undefined、取れなければ null）。 */
  suggest: Suggestion | null | undefined;
  events: CalendarEvent[];
  /** まだブロックにしていない未完了のタスク。 */
  candidates: TaskView[];
  /** 「配置するタスク」に選んでいるもの。 */
  armed: TaskView | null;
  /** 「配置するタスク」を選ぶ（同じものなら解除）。 */
  onArm: (task: TaskView | null) => void;
  /** タスクのチップを押した（ドラッグの開始）。 */
  startTaskDrag: (e: PointerEvent, task: TaskView) => void;
  /** ドラッグ中のタスク。 */
  taskDrag: TaskDrag | null;
  /** 「完了」。 */
  onFinish: () => void;
  /** テンプレートを適用した（ブロックの再取得）。 */
  onBlocksChanged: () => void;
}

/** その日のカレンダー予定の一覧。 */
function DayEvents(p: { isToday: boolean; events: CalendarEvent[] }) {
  const dayName = p.isToday ? "今日" : "この日";
  return (
    <Section title={`${dayName}の予定`} level={3}>
      {p.events.length === 0 ? (
        <Empty>{dayName}のカレンダー予定はありません。</Empty>
      ) : (
        <ul class="plain">
          {p.events.map((c) => (
            <li key={c.id}>
              <span class="meta">{c.all_day ? "終日" : `${fmtTime(c.start_at)}–${fmtTime(c.end_at)}`}</span> {c.title}
              {!c.is_busy && <span class="meta">（空き）</span>}
            </li>
          ))}
        </ul>
      )}
    </Section>
  );
}

/** 未完了のタスクのチップ。ドラッグでタイムラインへ置くか、選んでからタイムラインをクリックする。 */
function TaskChips(p: Pick<Props, "candidates" | "armed" | "onArm" | "startTaskDrag" | "taskDrag">) {
  return (
    <Section title="未完了のタスク" level={3}>
      <p class="hint">タイムラインへドラッグするか、選んでからタイムラインをクリックするとブロックにできます。</p>
      {p.candidates.length === 0 ? (
        <Empty>ブロックにしていない未完了のタスクはありません。</Empty>
      ) : (
        <ul class="plain task-chips">
          {p.candidates.map((t) => (
            <li key={t.id}>
              <button
                type="button"
                class={`chip task-chip${p.armed?.id === t.id ? " on" : ""}`}
                onPointerDown={(e) => p.startTaskDrag(e, t)}
                onKeyDown={(e) => {
                  if (e.key === "Enter" || e.key === " ") {
                    e.preventDefault();
                    p.onArm(p.armed?.id === t.id ? null : t);
                  }
                }}
                aria-pressed={p.armed?.id === t.id}
              >
                {t.title}
                {t.estimate_min ? <span class="meta"> {t.estimate_min}分</span> : null}
              </button>
            </li>
          ))}
        </ul>
      )}
      {p.taskDrag && <div class="hint">「{p.taskDrag.task.title}」をタイムラインの置きたい位置で離してください。</div>}
    </Section>
  );
}

/** デザインのパネル（サイドパネルに出す）。 */
export function DesignPanel(p: Props) {
  const suggested = p.suggest?.templates ?? [];
  return (
    <Section
      title={`${p.dayName}のデザイン`}
      help="空いている時間をドラッグ（クリックなら 1 時間）して、ブロックを置きます。置いたブロックはドラッグで動かし、上端・下端で長さを変え、クリックで編集できます。"
      aside={
        <button class="primary" onClick={p.onFinish}>
          完了
        </button>
      }
    >
      <div class="stack">
        <Section title="テンプレート" level={3}>
          <div class="stack">
            {suggested.length > 0 && p.suggest && (
              <p class="hint">{suggestionText(suggested.map((t) => t.name), p.suggest.weekday || weekdayOf(p.date))}</p>
            )}
            <ApplyTemplate date={p.date} suggested={suggested.map((t) => t.name)} onChanged={p.onBlocksChanged} />
            <SaveAsTemplate date={p.date} />
            <p class="hint">
              <a href="#/settings/templates">テンプレートを管理</a>
            </p>
          </div>
        </Section>
        <DayEvents isToday={p.isToday} events={p.events} />
        <TaskChips candidates={p.candidates} armed={p.armed} onArm={p.onArm} startTaskDrag={p.startTaskDrag} taskDrag={p.taskDrag} />
        <p class="hint">
          始業の振り返りは<a href="#/records/new/day_start">記録</a>から。
        </p>
      </div>
    </Section>
  );
}
