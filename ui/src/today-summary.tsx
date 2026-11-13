// 今日の画面の集計の節。上の帯（いま・合計・要確認・問いかけ）、今日以外の日の合計、「次にやること」、内訳。
import type { ActualList, BlockView, Millis, SegmentView, Status } from "./api";
import { blockTitle } from "./block-form";
import { Bars, Card, Section } from "./components";
import { KIND_LABEL, fmtDur, fmtTime, hrefOf } from "./lib";
import type { TodayDigest } from "./plan-util";
import { daySum, type Breakdown } from "./today-util";

interface StatBandProps {
  status: Status;
  /** いまの計画ブロック。 */
  cur: BlockView | undefined;
  /** 進行中のタスクのうち、いまのブロックと重なるもの。 */
  merged: Set<string>;
  digest: TodayDigest;
  busy: boolean;
  /** 進行中のタスクを完了にする。 */
  onTaskDone: (taskId: string) => void;
  /** 未回答の問いかけのパネルを開く。 */
  onOpenNudges: () => void;
}

/** 「いま」。計画ブロックと進行中のタスク（同じものなら 1 行にまとめる）。 */
function NowStat(p: StatBandProps) {
  const { status: s, cur, merged } = p;
  return (
    <div class="stat stat-now">
      <h2 class="stat-label">いま</h2>
      {cur && merged.size === 0 && (
        <span class="stat-title">
          {blockTitle(cur)}
          {cur.focus && <span class="tag">集中</span>}
          <span class="meta">
            {" "}
            {fmtTime(cur.start_at)}–{fmtTime(cur.end_at)} の計画ブロック
          </span>
        </span>
      )}
      {s.doing.length === 0 ? (
        <span class="stat-sub">進行中のタスクはありません。</span>
      ) : (
        s.doing.map((d) => (
          <span key={d.task.id} class="stat-doing">
            <span class="stat-title">{d.task.title}</span>
            {cur && merged.has(d.task.id) && (
              <>
                {cur.focus && <span class="tag">集中</span>}
                <span class="meta">
                  {fmtTime(cur.start_at)}–{fmtTime(cur.end_at)} の計画ブロック
                </span>
              </>
            )}
            {d.since && (
              <span class="meta">
                {fmtTime(d.since)} から（{fmtDur(s.now - d.since)}）
              </span>
            )}
            <button disabled={p.busy} onClick={() => p.onTaskDone(d.task.id)}>
              完了にする
            </button>
          </span>
        ))
      )}
    </div>
  );
}

/** 今日の上の帯。いま・今日の合計・要確認・未回答の問いかけ。 */
function TodayStatBand(p: StatBandProps) {
  const { status: s, digest } = p;
  const unanswered = s.unanswered_nudges;
  return (
    <div class="stat-band">
      <NowStat {...p} />
      <div class="stat">
        <h2 class="stat-label">今日の合計</h2>
        <span class="stat-num">{fmtDur(s.today.total_ms)}</span>
        <span class="stat-sub">
          AI との対話 {fmtDur(digest.agentMs)} ・ 無操作 {fmtDur(s.today.idle_ms)} ・ 休憩 {fmtDur(s.today.break_ms)}
          {s.today.ignored_ms > 0 ? ` ・ 無視 ${fmtDur(s.today.ignored_ms)}` : ""}
        </span>
        <span class="stat-sub">無操作・休憩・無視は合計に含みません</span>
      </div>
      <div class="stat">
        <h2 class="stat-label">要確認</h2>
        <span class="stat-num">{s.needs_review_count}件</span>
        <span class="stat-sub">
          {s.needs_review_count > 0 ? (
            <>
              {digest.reviewMs > 0 && `今日の分は約 ${Math.max(1, Math.round(digest.reviewMs / 60000))} 分 ・ `}
              <a href={hrefOf("actual", [], { status: "review", period: "today" })}>実績で確認する</a>
            </>
          ) : (
            "確認が必要な区間はありません"
          )}
          {s.needs_review_short_count > 0 && ` ・ 短い区間 ${s.needs_review_short_count} 件は実績で表示できます`}
        </span>
      </div>
      <div class="stat">
        <h2 class="stat-label">未回答の問いかけ</h2>
        <span class="stat-num">{unanswered.length}件</span>
        <span class="stat-sub">
          {unanswered.length > 0 ? (
            <button type="button" class="link" onClick={p.onOpenNudges}>
              答える
            </button>
          ) : (
            "答えていない問いかけはありません"
          )}
        </span>
      </div>
    </div>
  );
}

/** 今日以外の日の上の帯。その日の合計だけ。 */
function DayTotalBand(p: { date: string; segments: SegmentView[]; agentMs: Millis }) {
  return (
    <div class="stat-band">
      <div class="stat">
        <h2 class="stat-label">{p.date} の合計</h2>
        <span class="stat-num">{fmtDur(daySum(p.segments))}</span>
        <span class="stat-sub">AI との対話 {fmtDur(p.agentMs)} ・ 無操作・休憩・無視は含みません</span>
      </div>
    </div>
  );
}

interface BandProps extends Omit<StatBandProps, "status"> {
  isToday: boolean;
  /** 表示している日（YYYY-MM-DD）。 */
  date: string;
  /** 状況（届くまで undefined）。今日の帯に使う。 */
  status: Status | undefined;
  /** その日の実績（届くまで undefined）。今日以外の帯に使う。 */
  log: ActualList | undefined;
}

/** 上の帯。今日は状況（届いてから）、それ以外の日はその日の合計（実績が届いてから）。 */
export function TodayBand(p: BandProps) {
  if (p.isToday) return p.status ? <TodayStatBand {...p} status={p.status} /> : null;
  return p.log ? <DayTotalBand date={p.date} segments={p.log.segments} agentMs={p.digest.agentMs} /> : null;
}

/** 使い始めの案内。`doing` は進行中のタスクの数、`segments` はその日の区間の数。 */
export function StartGuideCard(p: { doing: number; segments: number }) {
  return (
    <Card title="次にやること">
      <ul>
        {p.doing === 0 && (
          <li>
            <a href="#/tasks">タスク</a>を 1 つ作って開始すると、ルールに当たらない時間の候補になります。
          </li>
        )}
        {p.segments === 0 && <li>記録はもう始まっています。しばらく使うと、ここにタイムラインが並びます。</li>}
        <li>
          使い方は<a href="#/welcome">はじめに</a>でいつでも見られます。
        </li>
      </ul>
    </Card>
  );
}

/** 内訳。種別ごと・タスクごと（上位）。 */
export function BreakdownSection(p: { dayName: string; breakdown: Breakdown }) {
  const { kindRows, taskRows, total } = p.breakdown;
  return (
    <Section title={`${p.dayName}の内訳`} aside={<a href="#/report">レポートで見る</a>}>
      <div class="breakdown">
        <div class="stack">
          <h3>種別ごと</h3>
          <Bars rows={kindRows.map((r) => ({ ...r, label: KIND_LABEL[r.key as keyof typeof KIND_LABEL] ?? r.label }))} total={total} />
        </div>
        <div class="stack">
          <h3>タスクごと（上位）</h3>
          <Bars rows={taskRows} total={total} />
        </div>
      </div>
    </Section>
  );
}
