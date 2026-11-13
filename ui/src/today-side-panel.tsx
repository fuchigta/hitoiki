// 今日の画面のサイドパネルのうち、データに紐づくもの（ブロックの編集・実績の補正・区間・未回答の問いかけ）。
// デザインのパネルは `today-design-panel.tsx`。
import type { BlockDraft } from "./block-form";
import { BlockPanel } from "./block-panel";
import { Empty, Section } from "./components";
import { CorrectionPanel } from "./correction";
import { hrefOf } from "./lib";
import { NudgeList } from "./nudge-answer";
import { SegmentPanel } from "./segment-panel";
import { actualPeriodFor, type SideAction, type SidePanel } from "./today-util";
import type { TodayData } from "./use-today-data";

interface Props {
  panel: Exclude<SidePanel, { kind: "design" }>;
  /** 表示している日（YYYY-MM-DD）。 */
  date: string;
  today: string;
  data: TodayData;
  dispatch: (a: SideAction) => void;
}

interface CaseProps {
  date: string;
  today: string;
  data: TodayData;
  /** いま出ているパネルを閉じる。 */
  close: () => void;
}

/** ブロックの作成・編集。日の範囲が届くまでは出さない（出すかどうかは `shownPanel` でも判定している）。 */
function BlockCase(p: CaseProps & { draft: BlockDraft }) {
  const { blocks, tasks, cal } = p.data;
  const range = blocks.data?.range;
  return range ? (
    <BlockPanel
      draft={p.draft}
      date={p.date}
      dayEnd={range.end_at}
      tasks={tasks.data?.tasks ?? []}
      blocks={blocks.data?.blocks ?? []}
      events={cal.data ?? []}
      onChanged={blocks.reload}
      onClose={p.close}
    />
  ) : null;
}

/** 実績の区間 1 つ。見つからなければその旨。 */
function SegmentCase(p: CaseProps & { id: string }) {
  const { st, log, tasks } = p.data;
  const seg = log.data?.segments.find((x) => x.id === p.id);
  return seg ? (
    <div class="stack">
      <SegmentPanel
        key={seg.id}
        segs={[seg]}
        tasks={tasks.data?.tasks ?? []}
        onDone={() => {
          p.close();
          log.reload();
          st.reload();
        }}
        onTasksChanged={tasks.reload}
      />
      <p class="row hint">
        <a href={hrefOf("actual", [seg.id], { period: actualPeriodFor(p.date, p.today), status: "all" })}>実績で開く</a>
        <span class="spacer" />
        <button type="button" class="ghost" onClick={p.close}>
          閉じる
        </button>
      </p>
    </div>
  ) : (
    <Empty>この区間は見つかりませんでした。</Empty>
  );
}

/** 未回答の問いかけ。 */
function NudgeCase(p: CaseProps) {
  const { st } = p.data;
  const unanswered = st.data?.unanswered_nudges ?? [];
  return (
    <Section
      title="未回答の問いかけ"
      help="答えなくても大丈夫です。「答えない」で一覧から外せます。"
      aside={
        <button type="button" class="ghost" onClick={p.close}>
          閉じる
        </button>
      }
    >
      {unanswered.length === 0 ? <Empty>未回答の問いかけはありません。</Empty> : <NudgeList nudges={unanswered} onDone={st.reload} />}
    </Section>
  );
}

/** データに紐づくサイドパネル。 */
export function TodaySidePanel({ panel, date, today, data, dispatch }: Props) {
  const c: CaseProps = { date, today, data, close: () => dispatch({ type: "close" }) };
  switch (panel.kind) {
    case "block":
      return <BlockCase {...c} draft={panel.draft} />;
    case "correct":
      return (
        <CorrectionPanel
          span={panel.span}
          tasks={data.tasks.data?.tasks ?? []}
          labels={data.labels.data}
          onClose={c.close}
          onChanged={() => {
            data.log.reload();
            data.st.reload();
          }}
        />
      );
    case "segment":
      return <SegmentCase {...c} id={panel.id} />;
    case "nudge":
      return <NudgeCase {...c} />;
  }
}
