// デザインのパネルのタスクのチップを、タイムラインへドラッグして置く操作。
import type { RefObject } from "preact";
import { useRef, useState } from "preact/hooks";
import type { TaskView } from "./api";
import { placeTaskRange, type TimeSpan } from "./plan-util";

export interface TaskDrag {
  task: TaskView;
  /** 配置先のプレビュー（タイムラインの上にいるとき）。 */
  over: TimeSpan | null;
}

export interface TaskDragHandle {
  /** ドラッグ中のタスクと配置先のプレビュー。 */
  taskDrag: TaskDrag | null;
  /** 配置先のプレビュー（タイムラインに重ねて出す）。 */
  ghost: TimeSpan | null;
  /** タイムラインの列（配置先の時刻を座標から求める）。 */
  laneRef: RefObject<HTMLDivElement>;
  /** タスクのチップを押した。 */
  startTaskDrag: (e: PointerEvent, task: TaskView) => void;
}

/**
 * チップを押して、動かさずに離せば `onTap`（「配置するタスク」の選択）、タイムラインの上で離せば `onDrop`（そこに置く）。
 * `view` はタイムラインの表示範囲（届くまで null。その間は何もしない）。
 */
export function useTaskDrag(
  view: TimeSpan | null,
  onTap: (task: TaskView) => void,
  onDrop: (task: TaskView, over: TimeSpan) => void | Promise<void>,
): TaskDragHandle {
  const [taskDrag, setTaskDrag] = useState<TaskDrag | null>(null);
  const laneRef = useRef<HTMLDivElement>(null);

  const startTaskDrag = (e: PointerEvent, task: TaskView) => {
    if (e.button > 0 || !e.isPrimary || !view) return;
    const x0 = e.clientX;
    const y0 = e.clientY;
    let moved = false;
    const spanAt = (x: number, y: number): TimeSpan | null => {
      const el = laneRef.current;
      if (!el) return null;
      const r = el.getBoundingClientRect();
      if (x < r.left || x > r.right || y < r.top || y > r.bottom) return null;
      return placeTaskRange(y - r.top, r.height, view.start, view.end, task.estimate_min);
    };
    const move = (ev: PointerEvent) => {
      if (!moved && Math.hypot(ev.clientX - x0, ev.clientY - y0) < 5) return;
      moved = true;
      setTaskDrag({ task, over: spanAt(ev.clientX, ev.clientY) });
    };
    const end = async (ev: PointerEvent) => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", end);
      window.removeEventListener("pointercancel", end);
      setTaskDrag(null);
      if (!moved) {
        onTap(task);
        return;
      }
      const over = ev.type === "pointerup" ? spanAt(ev.clientX, ev.clientY) : null;
      if (over) await onDrop(task, over);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", end);
    window.addEventListener("pointercancel", end);
  };

  return { taskDrag, ghost: taskDrag?.over ?? null, laneRef, startTaskDrag };
}
