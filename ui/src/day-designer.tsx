import type { RefObject } from "preact";
import { useState } from "preact/hooks";
import { api } from "./api";
import type { BlockInput, BlockView, CalendarEvent, GapView, Millis, SegmentView, TaskView } from "./api";
import { BlockForm, blockTitle, type BlockDraft } from "./block-form";
import { ErrorBox } from "./components";
import { DayGrid } from "./day-grid";
import { useAction } from "./lib";
import type { TimeSpan } from "./plan-util";

interface Props {
  /** 日付（YYYY-MM-DD）。 */
  date: string;
  dayStart: Millis;
  dayEnd: Millis;
  view: TimeSpan;
  blocks: BlockView[];
  segments: SegmentView[];
  events: CalendarEvent[];
  gaps?: GapView[];
  tasks: TaskView[];
  now?: Millis;
  /** ブロックを変えた後に呼ぶ（一覧の再取得）。 */
  reload: () => void;
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
  snapMs?: number;
}

/**
 * 1 日のタイムラインと、その上でのブロックの作成・編集・移動・伸縮・削除。
 * 計画画面と今日画面が同じ部品を使う（ロジックは plan-util に集めている）。
 */
export function DayDesigner(p: Props) {
  const [draft, setDraft] = useState<BlockDraft | null>(null);
  const act = useAction();

  const others = [
    ...p.blocks.map((b) => ({ id: b.id, start_at: b.start_at, end_at: b.end_at, title: blockTitle(b) })),
    ...p.events
      .filter((c) => c.is_busy && !c.all_day)
      .map((c) => ({ start_at: c.start_at, end_at: c.end_at, title: c.title })),
  ];

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
    setDraft({ start: span.start, end: span.end, task: "", label: "", focus: false });
  };

  const submit = async (input: BlockInput, id?: string) => {
    const r = await act.run(() => (id ? api.blockUpdate(id, input) : api.blockAdd(input)));
    if (r) {
      setDraft(null);
      p.reload();
    }
  };

  const remove = async (b: BlockView) => {
    const r = await act.run(() => api.blockDelete(b.id));
    if (r !== undefined) {
      if (draft?.id === b.id) setDraft(null);
      p.reload();
    }
  };

  const change = async (b: BlockView, span: TimeSpan) => {
    setDraft(null);
    const r = await act.run(() => api.blockUpdate(b.id, blockInput(b, span)));
    if (r) p.reload();
  };

  const edit = (b: BlockView) => {
    act.clear();
    setDraft({
      id: b.id,
      start: b.start_at,
      end: b.end_at,
      task: b.task_id ?? "",
      label: b.label ?? "",
      focus: b.focus,
    });
  };

  const selection = draft ? { start: draft.start, end: draft.end } : (p.ghost ?? null);

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
        editingId={draft?.id ?? null}
        clickLenMin={p.clickLenMin ?? (p.armedTask?.estimate_min ?? 60)}
        laneRef={p.laneRef}
        onPickActual={p.onPickActual}
        onClickActual={p.onClickActual}
        actualSelection={p.actualSelection}
        snapMs={p.snapMs}
        onSelect={create}
        onEditBlock={edit}
        onChangeBlock={change}
        onDeleteBlock={remove}
        disabled={act.busy}
        popover={
          draft && (
            <BlockForm
              key={`${draft.id ?? "new"}-${draft.start}-${draft.end}`}
              draft={draft}
              date={p.date}
              dayEnd={p.dayEnd}
              tasks={p.tasks}
              others={others}
              busy={act.busy}
              error={act.error}
              onSubmit={submit}
              onCancel={() => setDraft(null)}
              onDelete={draft.id ? () => remove(p.blocks.find((b) => b.id === draft.id)!) : undefined}
            />
          )
        }
      />
      {!draft && <ErrorBox error={act.error} />}
    </div>
  );
}
