import type { ComponentChildren, RefObject } from "preact";
import { useRef, useState } from "preact/hooks";
import { api } from "../api";
import type { BlockView, CalendarEvent, SegmentView } from "../api";
import { Card, Empty, ErrorBox, Field } from "../components";
import { dateInput, fmtTime, KIND_LABEL, toMillis, useAction, useAsync, useLiveTick } from "../lib";
import { dragRange, timeToPercent } from "../plan-util";

const HOURS = Array.from({ length: 25 }, (_, i) => i);
const GRID_HEIGHT = 24 * 36;

interface Drag {
  pointerId: number;
  y1: number;
  y2: number;
}

export function Plan() {
  const [date, setDate] = useState(dateInput());
  const tick = useLiveTick();
  const blocks = useAsync(() => api.blockList({ date }), [date, tick]);
  const log = useAsync(() => api.log({ date }), [date, tick]);
  const cal = useAsync(() => api.calendarList({ date }).catch(() => [] as CalendarEvent[]), [date, tick]);
  const tasks = useAsync(() => api.taskList(["todo", "doing"]), [tick]);

  const [start, setStart] = useState("09:00");
  const [end, setEnd] = useState("10:00");
  const [task, setTask] = useState("");
  const [label, setLabel] = useState("");
  const [focus, setFocus] = useState(false);
  const [popover, setPopover] = useState(false);
  const [drag, setDrag] = useState<Drag | null>(null);
  const laneRef = useRef<HTMLDivElement>(null);
  const act = useAction();

  const range = blocks.data?.range;
  const dayStart = range?.start_at ?? 0;
  const dayEnd = range?.end_at ?? 0;

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
      setPopover(false);
      blocks.reload();
    }
  };

  const remove = async (b: BlockView) => {
    const r = await act.run(() => api.blockDelete(b.id));
    if (r !== undefined) blocks.reload();
  };

  // ===== ドラッグ（Pointer Events: マウス・タッチ・ペン共通） =====
  const localY = (e: PointerEvent) => {
    const r = laneRef.current!.getBoundingClientRect();
    return e.clientY - r.top;
  };
  const onDown = (e: PointerEvent) => {
    if (!range || e.button > 0 || !e.isPrimary) return;
    laneRef.current!.setPointerCapture(e.pointerId);
    const y = localY(e);
    setPopover(false);
    setDrag({ pointerId: e.pointerId, y1: y, y2: y });
  };
  const onMove = (e: PointerEvent) => {
    if (drag && e.pointerId === drag.pointerId) setDrag({ ...drag, y2: localY(e) });
  };
  const onUp = (e: PointerEvent) => {
    if (!drag || e.pointerId !== drag.pointerId || !range) return;
    const h = laneRef.current!.clientHeight;
    const r = dragRange(drag.y1, localY(e), h, dayStart, dayEnd);
    setDrag(null);
    setStart(fmtTime(r.start));
    // 24:00 は time 入力に入らないので 23:59 に寄せる。
    setEnd(r.end >= dayEnd ? "23:59" : fmtTime(r.end));
    setPopover(true);
  };
  const onCancel = () => setDrag(null);

  const sel = drag
    ? dragRange(drag.y1, drag.y2, laneRef.current?.clientHeight ?? GRID_HEIGHT, dayStart, dayEnd)
    : popover && range
      ? { start: toMillis(date, start), end: Math.min(dayEnd, toMillis(date, end)) }
      : null;

  const form = (
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
        {popover && (
          <button type="button" onClick={() => setPopover(false)}>
            閉じる
          </button>
        )}
        <button class="primary" disabled={act.busy}>
          追加
        </button>
      </div>
      <ErrorBox error={act.error} />
    </form>
  );

  return (
    <div class="stack">
      <div class="row">
        <input type="date" value={date} onChange={(e) => setDate(e.currentTarget.value)} aria-label="日付" />
      </div>
      <Card title="ブロックを作る">{form}</Card>
      <ErrorBox error={blocks.error ?? log.error} />
      <Card title="計画と実績">
        {range && log.data && blocks.data ? (
          <>
            <p class="muted">「計画」の列を上下にドラッグすると、15 分刻みで時間を選んでブロックを作れます。</p>
            <div class="vtl" style={{ height: `${GRID_HEIGHT}px` }}>
              <div class="vtl-hours">
                {HOURS.map((h) => (
                  <span key={h} style={{ top: `${(h / 24) * 100}%` }}>
                    {h}:00
                  </span>
                ))}
              </div>
              <Lane name="予定">
                {(cal.data ?? [])
                  .filter((c) => !c.all_day)
                  .map((c) => (
                    <VItem
                      key={c.id}
                      cls="cal"
                      s={c.start_at}
                      e={c.end_at}
                      dayStart={dayStart}
                      dayEnd={dayEnd}
                      title={`${fmtTime(c.start_at)}–${fmtTime(c.end_at)} ${c.title}`}
                    >
                      {c.title}
                    </VItem>
                  ))}
              </Lane>
              <Lane name="計画" laneRef={laneRef} handlers={{ onDown, onMove, onUp, onCancel }}>
                {blocks.data.blocks.map((b) => (
                  <VItem
                    key={b.id}
                    cls={`plan${b.focus ? " focus" : ""}`}
                    s={b.start_at}
                    e={b.end_at}
                    dayStart={dayStart}
                    dayEnd={dayEnd}
                    title={`${fmtTime(b.start_at)}–${fmtTime(b.end_at)} ${b.task_title ?? b.label ?? ""}${b.focus ? "（集中）" : ""}`}
                  >
                    <span>{b.task_title ?? b.label ?? ""}</span>
                    <button
                      type="button"
                      class="vtl-del"
                      aria-label="このブロックを削除"
                      onPointerDown={(e) => e.stopPropagation()}
                      onClick={() => remove(b)}
                    >
                      ×
                    </button>
                  </VItem>
                ))}
                {sel && (
                  <div
                    class="vtl-sel"
                    style={{
                      top: `${timeToPercent(sel.start, dayStart, dayEnd)}%`,
                      height: `${timeToPercent(sel.end, dayStart, dayEnd) - timeToPercent(sel.start, dayStart, dayEnd)}%`,
                    }}
                  >
                    {fmtTime(sel.start)}–{fmtTime(sel.end)}
                  </div>
                )}
              </Lane>
              <Lane name="実績">
                {log.data.segments.map((s: SegmentView) => (
                  <VItem
                    key={s.id}
                    cls={`seg k-${s.kind}${s.status === "needs_review" ? " review" : ""}`}
                    s={s.start_at}
                    e={s.end_at}
                    dayStart={dayStart}
                    dayEnd={dayEnd}
                    title={`${fmtTime(s.start_at)}–${fmtTime(s.end_at)} ${KIND_LABEL[s.kind]}${s.task_title ? " / " + s.task_title : ""}${s.exe_name ? " / " + s.exe_name : ""}`}
                  />
                ))}
              </Lane>
              {popover && sel && (
                <div
                  class="vtl-pop"
                  style={{ top: `${Math.min(timeToPercent(sel.start, dayStart, dayEnd), 70)}%` }}
                  role="dialog"
                  aria-label="ブロックを作る"
                >
                  {form}
                </div>
              )}
            </div>
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
                {fmtTime(b.start_at)}–{fmtTime(b.end_at)} {b.task_title ?? b.label ?? "（ラベルなし）"}
                {b.focus && <span class="tag">集中</span>}
              </span>
              <span class="spacer" />
              <button type="button" onClick={() => remove(b)} disabled={act.busy}>
                削除
              </button>
            </li>
          ))}
        </ul>
      </Card>
    </div>
  );
}

interface Handlers {
  onDown: (e: PointerEvent) => void;
  onMove: (e: PointerEvent) => void;
  onUp: (e: PointerEvent) => void;
  onCancel: () => void;
}

function Lane(p: { name: string; children: ComponentChildren; laneRef?: RefObject<HTMLDivElement>; handlers?: Handlers }) {
  const h = p.handlers;
  return (
    <div class="vtl-col">
      <div class="vtl-name">{p.name}</div>
      <div
        ref={p.laneRef}
        class={`vtl-lane${h ? " drag" : ""}`}
        onPointerDown={h?.onDown}
        onPointerMove={h?.onMove}
        onPointerUp={h?.onUp}
        onPointerCancel={h?.onCancel}
      >
        {p.children}
      </div>
    </div>
  );
}

function VItem(p: {
  cls: string;
  s: number;
  e: number;
  dayStart: number;
  dayEnd: number;
  title: string;
  children?: ComponentChildren;
}) {
  const top = timeToPercent(p.s, p.dayStart, p.dayEnd);
  const bottom = timeToPercent(p.e, p.dayStart, p.dayEnd);
  return (
    <div class={`vtl-item ${p.cls}`} style={{ top: `${top}%`, height: `${Math.max(bottom - top, 0.4)}%` }} title={p.title}>
      {p.children}
    </div>
  );
}
