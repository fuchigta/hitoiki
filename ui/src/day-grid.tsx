import type { ComponentChildren, RefObject } from "preact";
import { useRef, useState } from "preact/hooks";
import type { BlockView, CalendarEvent, Millis, SegmentView } from "./api";
import { KIND_LABEL, fmtTime } from "./lib";
import { clickRange, currentBlock, dragRange, moveRange, nowPercent, resizeRange, timeToPercent, type TimeSpan } from "./plan-util";

const HOUR = 3_600_000;
/** 1 時間あたりの高さ（px）。 */
export const HOUR_PX = 36;
/** これ未満の動きはクリックとして扱う（px）。 */
const CLICK_SLOP = 4;

type DragMode = "create" | "move" | "start" | "end";

interface Drag {
  mode: DragMode;
  pointerId: number;
  y1: number;
  y2: number;
  block?: BlockView;
}

export interface DayGridProps {
  /** 1 日の範囲（ブロックの作成・移動はこの中に収める）。 */
  dayStart: Millis;
  dayEnd: Millis;
  /** 表示する時間帯（1 日の一部でもよい）。 */
  view: TimeSpan;
  blocks: BlockView[];
  segments: SegmentView[];
  events: CalendarEvent[];
  /** 現在時刻の線と「いまのブロック」を出すときに渡す。 */
  now?: Millis;
  /** 選択中の範囲（作成・編集のポップオーバーを開いている間、または配置のプレビュー）。 */
  selection?: TimeSpan | null;
  /** 編集中のブロック（ドラッグ中のように目立たせる）。 */
  editingId?: string | null;
  /** クリックだけで作る範囲の長さ（分）。 */
  clickLenMin?: number;
  /** 範囲を選んだ（クリック・ドラッグ）。 */
  onSelect: (span: TimeSpan) => void;
  /** ブロックをクリックした。 */
  onEditBlock: (b: BlockView) => void;
  /** ブロックを移動・伸縮した。 */
  onChangeBlock: (b: BlockView, span: TimeSpan) => void;
  onDeleteBlock: (b: BlockView) => void;
  /** 計画の列の要素（外から座標を求めるため）。 */
  laneRef?: RefObject<HTMLDivElement>;
  /** 選択範囲の位置に重ねて出す内容（ポップオーバー）。 */
  popover?: ComponentChildren;
  disabled?: boolean;
}

/** 予定・計画・実績を縦に並べた 1 日のタイムライン。計画の列で作成・移動・伸縮・削除ができる。 */
export function DayGrid(p: DayGridProps) {
  const innerRef = useRef<HTMLDivElement>(null);
  const laneRef = p.laneRef ?? innerRef;
  const [drag, setDrag] = useState<Drag | null>(null);

  const { view } = p;
  const hours = (view.end - view.start) / HOUR;
  const height = Math.max(1, hours * HOUR_PX);

  const laneHeight = () => laneRef.current?.clientHeight || height;
  const localY = (e: PointerEvent) => e.clientY - laneRef.current!.getBoundingClientRect().top;

  /** ドラッグ中の見た目の範囲。 */
  const live = (d: Drag): TimeSpan => {
    const h = laneHeight();
    if (d.mode === "create") return dragRange(d.y1, d.y2, h, view.start, view.end);
    const base = { start: d.block!.start_at, end: d.block!.end_at };
    if (d.mode === "move") return moveRange(base, d.y2 - d.y1, h, view.start, view.end);
    return resizeRange(base, d.mode, d.y2, h, view.start, view.end);
  };

  const onDown = (e: PointerEvent) => {
    if (p.disabled || e.button > 0 || !e.isPrimary) return;
    const target = e.target as HTMLElement;
    if (target.closest("[data-del]")) return;
    const el = target.closest<HTMLElement>("[data-block-id]");
    const y = localY(e);
    laneRef.current!.setPointerCapture(e.pointerId);
    if (el) {
      const block = p.blocks.find((b) => b.id === el.dataset.blockId);
      if (!block) return;
      const edge = target.closest<HTMLElement>("[data-edge]")?.dataset.edge;
      setDrag({ mode: edge === "start" || edge === "end" ? edge : "move", pointerId: e.pointerId, y1: y, y2: y, block });
      return;
    }
    setDrag({ mode: "create", pointerId: e.pointerId, y1: y, y2: y });
  };
  const onMove = (e: PointerEvent) => {
    if (drag && e.pointerId === drag.pointerId) setDrag({ ...drag, y2: localY(e) });
  };
  const onUp = (e: PointerEvent) => {
    if (!drag || e.pointerId !== drag.pointerId) return;
    const d = { ...drag, y2: localY(e) };
    setDrag(null);
    const moved = Math.abs(d.y2 - d.y1) >= CLICK_SLOP;
    const h = laneHeight();
    if (d.mode === "create") {
      const r = moved
        ? dragRange(d.y1, d.y2, h, view.start, view.end)
        : clickRange(d.y1, h, view.start, view.end, p.clickLenMin ?? 60);
      p.onSelect(r);
    } else if (!moved) {
      p.onEditBlock(d.block!);
    } else {
      const r = live(d);
      if (r.start !== d.block!.start_at || r.end !== d.block!.end_at) p.onChangeBlock(d.block!, r);
    }
  };

  const liveSpan = drag && (drag.mode === "create" ? Math.abs(drag.y2 - drag.y1) >= CLICK_SLOP : true) ? live(drag) : null;
  const sel = drag?.mode === "create" ? liveSpan : (p.selection ?? null);

  const cur = p.now !== undefined ? currentBlock(p.blocks, p.now) : undefined;
  const nowPct = p.now !== undefined ? nowPercent(p.now, view.start, view.end) : null;

  const hourLabels: Millis[] = [];
  for (let t = Math.ceil(view.start / HOUR) * HOUR; t <= view.end; t += HOUR) hourLabels.push(t);
  // 表示開始がちょうど時刻の境目でない場合（時差が 30 分単位のタイムゾーン）でも、ラベルはその位置に出す。
  const labelOf = (t: Millis) => fmtTime(t);

  const blockTitle = (b: BlockView) =>
    `${fmtTime(b.start_at)}–${fmtTime(b.end_at)} ${b.task_title ?? b.label ?? ""}${b.focus ? "（集中）" : ""}`;

  const pos = (s: Millis, e: Millis) => {
    const top = timeToPercent(s, view.start, view.end);
    const bottom = timeToPercent(e, view.start, view.end);
    return { top: `${top}%`, height: `${Math.max(bottom - top, 0.4)}%` };
  };

  const now = nowPct !== null && <div class="vtl-now" style={{ top: `${nowPct}%` }} title="いま" />;

  return (
    <div class="vtl" style={{ height: `${height + 22}px`, "--hours": String(hours) }}>
      <div class="vtl-hours">
        {hourLabels.map((t) => (
          <span key={t} style={{ top: `${timeToPercent(t, view.start, view.end)}%` }}>
            {labelOf(t)}
          </span>
        ))}
      </div>
      <Lane name="予定">
        {p.events
          .filter((c) => !c.all_day)
          .map((c) => (
            <div
              key={c.id}
              class={`vtl-item cal${c.is_busy ? "" : " free"}`}
              style={pos(c.start_at, c.end_at)}
              title={`${fmtTime(c.start_at)}–${fmtTime(c.end_at)} ${c.title}`}
            >
              {c.title}
            </div>
          ))}
        {now}
      </Lane>
      <Lane
        name="計画"
        laneRef={laneRef}
        handlers={{ onDown, onMove, onUp, onCancel: () => setDrag(null) }}
        disabled={p.disabled}
      >
        {p.blocks.map((b) => {
          const dragging = drag?.block?.id === b.id && liveSpan;
          const s = dragging ? liveSpan.start : b.start_at;
          const e = dragging ? liveSpan.end : b.end_at;
          return (
            <div
              key={b.id}
              data-block-id={b.id}
              role="button"
              tabIndex={0}
              class={`vtl-item plan${b.focus ? " focus" : ""}${cur?.id === b.id ? " now" : ""}${dragging || p.editingId === b.id ? " active" : ""}`}
              style={pos(s, e)}
              title={blockTitle(b)}
              onKeyDown={(ev) => {
                if (ev.key === "Enter" || ev.key === " ") {
                  ev.preventDefault();
                  p.onEditBlock(b);
                }
              }}
            >
              <div class="vtl-grip top" data-edge="start" />
              <span class="vtl-label">{b.task_title ?? b.label ?? ""}</span>
              <button type="button" data-del class="vtl-del" aria-label="このブロックを削除" onClick={() => p.onDeleteBlock(b)}>
                ×
              </button>
              <div class="vtl-grip bottom" data-edge="end" />
            </div>
          );
        })}
        {sel && (
          <div class="vtl-sel" style={pos(sel.start, sel.end)}>
            {fmtTime(sel.start)}–{fmtTime(sel.end)}
          </div>
        )}
        {now}
      </Lane>
      <Lane name="実績">
        {p.segments.map((s) => (
          <div
            key={s.id}
            class={`vtl-item seg k-${s.kind}${s.status === "needs_review" ? " review" : ""}`}
            style={pos(s.start_at, s.end_at)}
            title={`${fmtTime(s.start_at)}–${fmtTime(s.end_at)} ${KIND_LABEL[s.kind]}${s.task_title ? " / " + s.task_title : ""}${s.exe_name ? " / " + s.exe_name : ""}`}
          />
        ))}
        {now}
      </Lane>
      {p.popover && sel && (
        <div class="vtl-pop" style={{ top: `${Math.min(timeToPercent(sel.start, view.start, view.end), 55)}%` }} role="dialog" aria-label="ブロック">
          {p.popover}
        </div>
      )}
    </div>
  );
}

interface Handlers {
  onDown: (e: PointerEvent) => void;
  onMove: (e: PointerEvent) => void;
  onUp: (e: PointerEvent) => void;
  onCancel: () => void;
}

function Lane(p: {
  name: string;
  children: ComponentChildren;
  laneRef?: RefObject<HTMLDivElement>;
  handlers?: Handlers;
  disabled?: boolean;
}) {
  const h = p.handlers;
  return (
    <div class="vtl-col">
      <div class="vtl-name">{p.name}</div>
      <div
        ref={p.laneRef}
        class={`vtl-lane${h && !p.disabled ? " drag" : ""}`}
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
