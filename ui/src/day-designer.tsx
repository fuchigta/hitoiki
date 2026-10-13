import type { RefObject } from "preact";
import { api } from "./api";
import type { BlockInput, BlockView, CalendarEvent, GapView, Millis, SegmentView, TaskView } from "./api";
import type { BlockDraft } from "./block-form";
import { ErrorBox } from "./components";
import { DayGrid } from "./day-grid";
import { useAction } from "./lib";
import type { TimeSpan } from "./plan-util";

interface Props {
  dayStart: Millis;
  dayEnd: Millis;
  view: TimeSpan;
  blocks: BlockView[];
  segments: SegmentView[];
  events: CalendarEvent[];
  gaps?: GapView[];
  now?: Millis;
  /** ブロックを変えた後に呼ぶ（一覧の再取得）。 */
  reload: () => void;
  /** サイドパネルで作成・編集中のブロック（タイムライン上で強調する）。 */
  draft: BlockDraft | null;
  /** ブロックの作成・編集を始める（サイドパネルに BlockForm を出す）。null で閉じる。 */
  onDraft: (d: BlockDraft | null) => void;
  /** 「配置するタスク」を選んでいる間は、範囲を選ぶとそのタスクのブロックをすぐ作る。 */
  armedTask?: TaskView | null;
  /** 配置し終えた。 */
  onPlaced?: () => void;
  /** タスクをドラッグ中の配置位置のプレビュー。 */
  ghost?: TimeSpan | null;
  laneRef?: RefObject<HTMLDivElement>;
  clickLenMin?: number;
  /** 「実績」の列で範囲を選んだ（実績の補正）。 */
  onPickActual?: (span: TimeSpan) => void;
  actualSelection?: TimeSpan | null;
  /** 「実績」の列をクリックした（その位置の区間 ID）。 */
  onClickActual?: (segmentId: string) => void;
  /** サイドパネルで開いている区間。 */
  selectedSegmentId?: string | null;
  snapMs?: number;
}

/**
 * 1 日のタイムラインと、その上でのブロックの作成・移動・伸縮・削除。
 * ブロックの入力フォームはサイドパネル（`BlockPanel`）に出すので、ここでは下書きを親に渡すだけ。
 */
export function DayDesigner(p: Props) {
  const act = useAction();

  const blockInput = (b: BlockView, span: TimeSpan): BlockInput => ({
    start: span.start,
    end: span.end,
    task: b.task_id ?? undefined,
    label: b.label ?? undefined,
    focus: b.focus,
  });

  const create = async (span: TimeSpan) => {
    const t = p.armedTask;
    if (t) {
      const r = await act.run(() => api.blockAdd({ start: span.start, end: span.end, task: t.id }));
      if (r) {
        p.onPlaced?.();
        p.reload();
      }
      return;
    }
    act.clear();
    p.onDraft({ start: span.start, end: span.end, task: "", label: "", focus: false });
  };

  const remove = async (b: BlockView) => {
    const r = await act.run(() => api.blockDelete(b.id));
    if (r !== undefined) {
      if (p.draft?.id === b.id) p.onDraft(null);
      p.reload();
    }
  };

  const change = async (b: BlockView, span: TimeSpan) => {
    p.onDraft(null);
    const r = await act.run(() => api.blockUpdate(b.id, blockInput(b, span)));
    if (r) p.reload();
  };

  const edit = (b: BlockView) => {
    act.clear();
    p.onDraft({
      id: b.id,
      start: b.start_at,
      end: b.end_at,
      task: b.task_id ?? "",
      label: b.label ?? "",
      focus: b.focus,
    });
  };

  const selection = p.draft ? { start: p.draft.start, end: p.draft.end } : (p.ghost ?? null);

  return (
    <div class="stack">
      <DayGrid
        dayStart={p.dayStart}
        dayEnd={p.dayEnd}
        view={p.view}
        blocks={p.blocks}
        segments={p.segments}
        events={p.events}
        gaps={p.gaps}
        now={p.now}
        selection={selection}
        editingId={p.draft?.id ?? null}
        clickLenMin={p.clickLenMin ?? (p.armedTask?.estimate_min ?? 60)}
        laneRef={p.laneRef}
        onPickActual={p.onPickActual}
        onClickActual={p.onClickActual}
        actualSelection={p.actualSelection}
        selectedSegmentId={p.selectedSegmentId}
        snapMs={p.snapMs}
        onSelect={create}
        onEditBlock={edit}
        onChangeBlock={change}
        onDeleteBlock={remove}
        disabled={act.busy}
      />
      <ErrorBox error={act.error} />
    </div>
  );
}
