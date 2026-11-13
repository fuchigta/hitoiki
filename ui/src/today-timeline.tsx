// 今日の画面のタイムラインのカード。予定・計画・実績の列と凡例。
import type { RefObject } from "preact";
import type { ActualList, BlockList, CalendarEvent, Millis, SegmentView, TaskView, UncollectedView } from "./api";
import { Card, Empty } from "./components";
import { DayDesigner } from "./day-designer";
import { KIND_LABEL } from "./lib";
import type { TimeSpan } from "./plan-util";
import { legendKinds, timelineSelection, type SideAction, type SidePanel } from "./today-util";

interface Props {
  isToday: boolean;
  /** その日の実績（届くまで undefined）。 */
  log: ActualList | undefined;
  /** その日のブロックと日の範囲（届くまで undefined）。 */
  blocks: BlockList | undefined;
  /** タイムラインの表示範囲（日の範囲が届くまで null）。 */
  view: TimeSpan | null;
  events: CalendarEvent[];
  /** 現在時刻（現在時刻の線。今日だけ出す）。 */
  now: Millis;
  /** 「配置するタスク」に選んでいるもの。 */
  armed: TaskView | null;
  /** 「配置するタスク」の選択をやめる・置き終えた。 */
  onDisarm: () => void;
  /** デザインのパネルが出ているか、デザイン中（案内の行の場所を空けておく）。 */
  designOn: boolean;
  /** サイドパネルの内容（タイムラインで強調するものを決める）。 */
  panel: SidePanel | null;
  dispatch: (a: SideAction) => void;
  /** タスクをドラッグ中の配置位置のプレビュー。 */
  ghost: TimeSpan | null;
  laneRef: RefObject<HTMLDivElement>;
  /** 実績の補正で使う吸着（秒）。 */
  snapSec: number | undefined;
  /** ブロックを変えた後に呼ぶ（一覧の再取得）。 */
  reloadBlocks: () => void;
}

/** 凡例。区間に出てきた種別と、記録なしの時間帯。 */
function TimelineLegend(p: { segments: SegmentView[]; uncollected: UncollectedView[] | undefined }) {
  return (
    <ul class="legend">
      {legendKinds(p.segments).map((k) => (
        <li key={k}>
          <i class={`dot k-${k}`} />
          {KIND_LABEL[k]}
        </li>
      ))}
      {(p.uncollected ?? []).length > 0 && (<li><i class="dot uncollected" />記録なし</li>)}
    </ul>
  );
}

/** タイムラインの中身（実績とブロックが届いてから）。 */
function TimelineBody(p: Props & { log: ActualList; blocks: BlockList; view: TimeSpan }) {
  const range = p.blocks.range;
  const sel = timelineSelection(p.panel);
  return (
    <>
      {(p.armed || p.designOn) && (
        // デザインを開いている間は 1 行ぶん場所を空けておく（タスクを選んだときに、下のタイムラインが動かないように）。
        <p class="hint hint-slot" role="status">
          {p.armed && (
            <>
              「<span class="hint-title">{p.armed.title}</span>」を置く場所をタイムラインでクリックしてください。
              <button type="button" class="link" onClick={p.onDisarm}>
                やめる
              </button>
            </>
          )}
        </p>
      )}
      <DayDesigner
        dayStart={range.start_at}
        dayEnd={range.end_at}
        view={p.view}
        blocks={p.blocks.blocks}
        segments={p.log.segments}
        events={p.events}
        uncollected={p.log.uncollected}
        now={p.isToday ? p.now : undefined}
        reload={p.reloadBlocks}
        draft={sel.draft}
        onDraft={(d) => p.dispatch(d ? { type: "open", panel: { kind: "block", draft: d } } : { type: "close" })}
        armedTask={p.armed}
        onPlaced={p.onDisarm}
        ghost={p.ghost}
        laneRef={p.laneRef}
        onPickActual={(span) => p.dispatch({ type: "open", panel: { kind: "correct", span } })}
        onClickActual={(id) => p.dispatch({ type: "open", panel: { kind: "segment", id } })}
        actualSelection={sel.actualSpan}
        selectedSegmentId={sel.segmentId}
        snapMs={(p.snapSec ?? 120) * 1000}
      />
      <TimelineLegend segments={p.log.segments} uncollected={p.log.uncollected} />
      {!p.isToday && p.blocks.blocks.length === 0 && p.log.segments.length === 0 && <Empty>この日の記録はありません。</Empty>}
    </>
  );
}

/** タイムラインのカード。実績とブロックが届くまでは「読み込み中」。 */
export function TimelineCard(p: Props) {
  const range = p.blocks?.range;
  return (
    <Card
      title="タイムライン"
      help={
        <>
          「計画」の列をドラッグすると、右のパネルでブロックを作れます（クリックなら 1 時間）。ブロックはドラッグで動かし、上端・下端で長さを変え、クリックで編集します。
          「実績」の列をドラッグすると、その時間帯の実績をタスクへ付け替えられます。クリックすると、その区間を右のパネルで開きます。
        </>
      }
    >
      {range && p.view && p.log && p.blocks ? <TimelineBody {...p} log={p.log} blocks={p.blocks} view={p.view} /> : <Empty>読み込み中…</Empty>}
    </Card>
  );
}
