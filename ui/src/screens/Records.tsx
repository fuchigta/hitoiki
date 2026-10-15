import { useMemo, useState } from "preact/hooks";
import { api, type Reframe, type ErgDistribution, type NudgeView } from "../api";
import { EmptyState, ErrorBox } from "../components";
import { ErgLine } from "../erg-label";
import { ERG_LEGEND, classifyMessage, labelMap } from "../erg-util";
import { fmtDateTime, fmtTime, hrefOf, navigate, useAction, useAsync, useLiveTick, useRoute } from "../lib";
import { Markdown } from "../markdown";
import { NudgeAnswer } from "../nudge-answer";
import { periodLabel } from "../nudge-answer-util";
import { ReframeDetail, ConditionDetail, ConcernDetail } from "../record-edit";
import { NewRecord } from "../records-new";
import {
  DEFAULT_PERIOD,
  KIND_FILTERS,
  NEW_KINDS,
  RECORD_KIND_LABEL,
  buildTimeline,
  ergBadge,
  ergCountLine,
  filterTimeline,
  kindFilter,
  unconfirmedCount,
  recordsPeriod,
  recordsSelection,
  reflectionAnswered,
  type RecordSource,
  type RecordsSelection,
  type TimelineEntry,
} from "../records-util";
import { ReflectionDetail } from "../reflection-detail";
import { Help, ListDetail, ObjectList, ObjectRow, OverflowMenu, PeriodSelect } from "../ui/parts";
import type { PeriodId } from "../ui/period-util";

/** 振り返りは API が期間で絞れないので、新しいものから十分な件数を取って画面側で期間に絞る。 */
const REFLECTION_LIMIT = 300;

export function Records() {
  const route = useRoute();
  const period = recordsPeriod(route.params.get("period"));
  const kind = kindFilter(route.params.get("kind"));
  const unconfirmedOnly = route.params.get("erg") === "unconfirmed";
  const sel = recordsSelection(route.path);
  const range = { date: period };

  const conditions = useAsync(() => api.conditionList(range), [period]);
  const concerns = useAsync(() => api.concernList(range), [period]);
  const reframes = useAsync(() => api.reframeList(range), [period]);
  const reflections = useAsync(() => api.reflectList(undefined, REFLECTION_LIMIT), []);
  const erg = useAsync(() => api.ergOverview(range), [period]);
  const nudgeTick = useLiveTick(["nudge"]);
  const status = useAsync(() => api.status(), [nudgeTick]);
  // 作ったばかりの記録。一覧を読み直すまでの間も、詳細に出せるようにしておく。
  const [fresh, setFresh] = useState<RecordSource | null>(null);

  const reloadAll = () => {
    conditions.reload();
    concerns.reload();
    reframes.reload();
    reflections.reload();
    erg.reload();
  };

  const labels = useMemo(() => labelMap(erg.data?.labels ?? []), [erg.data]);
  const bounds = conditions.data ? { start: conditions.data.start_at, end: conditions.data.end_at } : undefined;
  const timeline = useMemo(
    () =>
      buildTimeline(
        {
          conditions: conditions.data?.items ?? [],
          concerns: concerns.data?.items ?? [],
          reframes: reframes.data?.items ?? [],
          reflections: bounds ? (reflections.data ?? []) : [],
        },
        labels,
        bounds,
      ),
    [conditions.data, concerns.data, reframes.data, reflections.data, labels],
  );
  const visible = filterTimeline(timeline, kind, unconfirmedOnly);
  const unconfirmed = unconfirmedCount(timeline);
  const loaded = !!(conditions.data && concerns.data && reframes.data && reflections.data);
  const loading = conditions.loading || concerns.loading || reframes.loading || reflections.loading;
  const error = conditions.error ?? concerns.error ?? reframes.error ?? reflections.error;

  // 一覧に出ている絞り込みを、選択を変えても保つ。
  const filters = {
    period: period === DEFAULT_PERIOD ? undefined : period,
    kind: kind === "all" ? undefined : kind,
    erg: unconfirmedOnly ? "unconfirmed" : undefined,
  };
  const hrefFor = (path: string[], over: Record<string, string | undefined> = {}) =>
    hrefOf("records", path, { ...filters, ...over });
  const setFilter = (over: Record<string, string | undefined>) => navigate(hrefFor(route.path, over));

  const unanswered = status.data?.unanswered_nudges ?? [];

  const created = (src: RecordSource) => {
    setFresh(src);
    reloadAll();
    // 今日・今週の一覧なら絞り込みを残す。それ以外（昨日・先週）は、作った記録が見えるよう期間を既定に戻す。
    const keep = period === "today" || period === "this-week" ? { period: filters.period } : {};
    navigate(hrefOf("records", [src.kind, src.rec.id], keep));
  };
  const deleted = () => {
    setFresh(null);
    reloadAll();
    navigate(hrefFor([]), { replace: true });
  };

  const entry = (s: Extract<RecordsSelection, { mode: "detail" }>): TimelineEntry | undefined =>
    timeline.find((e) => e.kind === s.kind && e.id === s.id) ??
    (fresh && fresh.kind === s.kind && fresh.rec.id === s.id ? freshEntry(fresh, labels) : undefined);

  return (
    <div class="stack">
      <div class="list-head records-bar">
        <PeriodSelect value={period} onChange={(p: PeriodId) => setFilter({ period: p === DEFAULT_PERIOD ? undefined : p })} />
        <div class="row" role="group" aria-label="種類">
          {KIND_FILTERS.map((k) => (
            <button
              key={k.id}
              type="button"
              class={kind === k.id ? "chip on" : "chip"}
              aria-pressed={kind === k.id}
              onClick={() => setFilter({ kind: k.id === "all" ? undefined : k.id })}
            >
              {k.label}
            </button>
          ))}
          {(unconfirmed > 0 || unconfirmedOnly) && (
            <button
              type="button"
              class={unconfirmedOnly ? "chip on" : "chip"}
              aria-pressed={unconfirmedOnly}
              onClick={() => setFilter({ erg: unconfirmedOnly ? undefined : "unconfirmed" })}
            >
              ERG 未確定 {unconfirmed}
            </button>
          )}
        </div>
        <span class="spacer" />
        <OverflowMenu
          label="記録の種類"
          trigger={{ text: "＋ 記録", primary: true }}
          items={NEW_KINDS.map((k) => ({ label: k.label, onSelect: () => navigate(hrefFor(["new", k.id])) }))}
        />
      </div>

      {erg.data && <ErgBar distribution={erg.data.distribution} range={range} onChanged={erg.reload} />}
      <ErrorBox error={erg.error} />

      {unanswered.length > 0 && <UnansweredNudges nudges={unanswered} onDone={status.reload} />}

      <ListDetail
        list={
          <div class="stack">
            <ErrorBox error={error} />
            {loaded && visible.length === 0 && (
              <EmptyState>
                {timeline.length === 0
                  ? "この期間の記録はありません。「＋ 記録」から書けます。"
                  : "この絞り込みに合う記録はありません。"}
              </EmptyState>
            )}
            <ObjectList label="記録の時系列">
              {visible.map((e) => (
                <ObjectRow
                  key={`${e.kind}:${e.id}`}
                  selected={sel?.mode === "detail" && sel.kind === e.kind && sel.id === e.id}
                  title={e.summary}
                  tags={[RECORD_KIND_LABEL[e.kind], ergBadge(e.erg)].filter((t): t is string => !!t)}
                  meta={fmtDateTime(e.at)}
                  onSelect={() => navigate(hrefFor([e.kind, e.id]))}
                />
              ))}
            </ObjectList>
          </div>
        }
        detail={
          sel?.mode === "new" ? (
            <NewRecord key={sel.kind} kind={sel.kind} onCreated={created} />
          ) : sel ? (
            <Detail
              key={`${sel.kind}:${sel.id}`}
              e={entry(sel)}
              loading={loading || !loaded}
              onChanged={reloadAll}
              onDeleted={deleted}
              onErgChanged={erg.reload}
            />
          ) : (
            <EmptyState>
              記録を選ぶと詳細が出ます。
              {loaded && timeline.length > 0 && <> いまの一覧は {visible.length} 件です。</>}
            </EmptyState>
          )
        }
      />
    </div>
  );
}

function freshEntry(src: RecordSource, labels: ReturnType<typeof labelMap>): TimelineEntry | undefined {
  return buildTimeline(
    {
      conditions: src.kind === "condition" ? [src.rec] : [],
      concerns: src.kind === "concern" ? [src.rec] : [],
      reframes: src.kind === "reframe" ? [src.rec] : [],
      reflections: src.kind === "reflection" ? [src.rec] : [],
    },
    labels,
  )[0];
}

/** 期間内の ERG の分布（件数だけ。評価や助言は添えない）と、未判定の記録を AI で判定する操作。 */
function ErgBar({
  distribution,
  range,
  onChanged,
}: {
  distribution: ErgDistribution;
  range: { date: string };
  onChanged: () => void;
}) {
  const act = useAction();
  const [msg, setMsg] = useState("");
  const classify = async () => {
    setMsg("");
    const o = await act.run(() => api.ergClassify(range));
    if (o) {
      setMsg(classifyMessage(o));
      onChanged();
    }
  };
  return (
    <div class="erg-bar">
      <div class="row">
        <span class="meta">ERG: {ergCountLine(distribution)}</span>
        <Help label="ERG の説明">
          ERG は「{ERG_LEGEND}」の 3 つの観点です。記録の内容から AI が判定し、AI の判定は小さく「AI」と表示されます。
          確信が持てないものは未確定になり、あなたが確定するまで集計に入りません。件数は事実を並べているだけで、多い少ないに良し悪しはありません。
        </Help>
        <span class="spacer" />
        {msg && (
          <span class="meta" role="status">
            {msg}
          </span>
        )}
        <button type="button" class="ghost" disabled={act.busy || distribution.unlabeled === 0} onClick={classify}>
          {act.busy ? "AI が判定しています…" : "未判定の記録を AI で判定する"}
        </button>
      </div>
      <ErrorBox error={act.error} />
    </div>
  );
}

/** 未回答の問いかけ（問いかけへの答え）。帯を開くと、その場で答えられる。 */
function UnansweredNudges({ nudges, onDone }: { nudges: NudgeView[]; onDone: () => void }) {
  return (
    <details class="nudge-band">
      <summary>
        未回答の問いかけ {nudges.length} 件<Help label="未回答の問いかけの説明">答えなくても大丈夫です。「答えない」で一覧から外せます。</Help>
      </summary>
      <ul class="plain nudge-list">
        {nudges.map((p) => (
          <li key={p.id}>
            <div>
              <span class="meta">
                {fmtTime(p.at)} {p.trigger_label}
              </span>{" "}
              <strong>{p.title}</strong>
            </div>
            {p.body && <p class="hint">{p.body}</p>}
            {periodLabel(p.payload, fmtTime) && <p class="meta">対象の時間帯: {periodLabel(p.payload, fmtTime)}</p>}
            {p.payload && <NudgeAnswer nudgeId={p.id} payload={p.payload} onDone={onDone} />}
          </li>
        ))}
      </ul>
    </details>
  );
}

/** 選んだ記録の詳細。 */
function Detail({
  e,
  loading,
  onChanged,
  onDeleted,
  onErgChanged,
}: {
  e: TimelineEntry | undefined;
  loading: boolean;
  onChanged: () => void;
  onDeleted: () => void;
  onErgChanged: () => void;
}) {
  if (!e) {
    return loading ? (
      <EmptyState>読み込み中…</EmptyState>
    ) : (
      <EmptyState>この期間の一覧に見つかりません。期間を変えると見つかるかもしれません。</EmptyState>
    );
  }
  const s = e.source;
  // 文章のある記録には ERG の行を出す。文章が無くラベルも無い記録には出さない。
  const hasText =
    s.kind === "condition" ? !!s.rec.note?.trim() : s.kind === "reflection" ? reflectionAnswered(s.rec) : true;
  const erg =
    hasText || e.erg ? <ErgLine kind={e.kind} id={e.id} label={e.erg} onChanged={onErgChanged} /> : undefined;
  if (s.kind === "condition") return <ConditionDetail c={s.rec} erg={erg} onChanged={onChanged} onDeleted={onDeleted} />;
  if (s.kind === "concern") return <ConcernDetail h={s.rec} erg={erg} onChanged={onChanged} onDeleted={onDeleted} />;
  if (s.kind === "reframe") {
    return (
      <ReframeDetail a={s.rec} erg={erg} onChanged={onChanged} onDeleted={onDeleted}>
        <ReframeDeepening reframe={s.rec} onChanged={onChanged} />
      </ReframeDetail>
    );
  }
  return <ReflectionDetail r={s.rec} erg={erg} onChanged={onChanged} />;
}

/** B（受け止め方）を掘り下げる AI の問い。生成には時間がかかるので、実行中は表示を変える。 */
function ReframeDeepening({ reframe, onChanged }: { reframe: Reframe; onChanged: () => void }) {
  const act = useAction();
  return (
    <div class="stack">
      {reframe.deepening_md && <Markdown text={reframe.deepening_md} />}
      <div class="row">
        <button
          type="button"
          disabled={act.busy}
          onClick={async () => {
            if (await act.run(() => api.reframeDeepen(reframe.id))) onChanged();
          }}
        >
          {act.busy ? "AI が考えています…" : reframe.deepening_md ? "問いを追加する" : "受け止め方を掘り下げる問いを AI に聞く"}
        </button>
      </div>
      <ErrorBox error={act.error} />
    </div>
  );
}
