import { useMemo, useState } from "preact/hooks";
import { api, type AbcRecord, type ErgDistribution, type PushView } from "../api";
import { EmptyState, ErrorBox } from "../components";
import { ErgLine } from "../erg-label";
import { ERG_LEGEND, classifyMessage, labelMap } from "../erg-util";
import { fmtDateTime, fmtTime, hrefOf, navigate, useAction, useAsync, useLiveTick, useRoute } from "../lib";
import { Markdown } from "../markdown";
import { PushAnswer } from "../push-answer";
import { periodLabel } from "../push-answer-util";
import { AbcDetail, CheckinDetail, HassleDetail } from "../record-edit";
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
  pendingCount,
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
  const pendingOnly = route.params.get("erg") === "pending";
  const sel = recordsSelection(route.path);
  const range = { date: period };

  const checkins = useAsync(() => api.checkinList(range), [period]);
  const hassles = useAsync(() => api.hassleList(range), [period]);
  const abcs = useAsync(() => api.abcList(range), [period]);
  const reflections = useAsync(() => api.reflectList(undefined, REFLECTION_LIMIT), []);
  const erg = useAsync(() => api.ergOverview(range), [period]);
  const pushTick = useLiveTick(["push"]);
  const status = useAsync(() => api.status(), [pushTick]);
  // 作ったばかりの記録。一覧を読み直すまでの間も、詳細に出せるようにしておく。
  const [fresh, setFresh] = useState<RecordSource | null>(null);

  const reloadAll = () => {
    checkins.reload();
    hassles.reload();
    abcs.reload();
    reflections.reload();
    erg.reload();
  };

  const labels = useMemo(() => labelMap(erg.data?.labels ?? []), [erg.data]);
  const bounds = checkins.data ? { start: checkins.data.start_at, end: checkins.data.end_at } : undefined;
  const timeline = useMemo(
    () =>
      buildTimeline(
        {
          checkins: checkins.data?.items ?? [],
          hassles: hassles.data?.items ?? [],
          abcs: abcs.data?.items ?? [],
          reflections: bounds ? (reflections.data ?? []) : [],
        },
        labels,
        bounds,
      ),
    [checkins.data, hassles.data, abcs.data, reflections.data, labels],
  );
  const visible = filterTimeline(timeline, kind, pendingOnly);
  const pending = pendingCount(timeline);
  const loaded = !!(checkins.data && hassles.data && abcs.data && reflections.data);
  const loading = checkins.loading || hassles.loading || abcs.loading || reflections.loading;
  const error = checkins.error ?? hassles.error ?? abcs.error ?? reflections.error;

  // 一覧に出ている絞り込みを、選択を変えても保つ。
  const filters = {
    period: period === DEFAULT_PERIOD ? undefined : period,
    kind: kind === "all" ? undefined : kind,
    erg: pendingOnly ? "pending" : undefined,
  };
  const hrefFor = (path: string[], over: Record<string, string | undefined> = {}) =>
    hrefOf("records", path, { ...filters, ...over });
  const setFilter = (over: Record<string, string | undefined>) => navigate(hrefFor(route.path, over));

  const unanswered = status.data?.unanswered_pushes ?? [];

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
          {(pending > 0 || pendingOnly) && (
            <button
              type="button"
              class={pendingOnly ? "chip on" : "chip"}
              aria-pressed={pendingOnly}
              onClick={() => setFilter({ erg: pendingOnly ? undefined : "pending" })}
            >
              ERG 確認待ち {pending}
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

      {unanswered.length > 0 && <UnansweredPushes pushes={unanswered} onDone={status.reload} />}

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
      checkins: src.kind === "checkin" ? [src.rec] : [],
      hassles: src.kind === "hassle" ? [src.rec] : [],
      abcs: src.kind === "abc" ? [src.rec] : [],
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
          確信が持てないものは確認待ちになり、あなたが確定するまで集計に入りません。件数は事実を並べているだけで、多い少ないに良し悪しはありません。
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

/** 未回答の問いかけ（通知への答え）。帯を開くと、その場で答えられる。 */
function UnansweredPushes({ pushes, onDone }: { pushes: PushView[]; onDone: () => void }) {
  return (
    <details class="push-band">
      <summary>
        未回答の問いかけ {pushes.length} 件<Help label="未回答の問いかけの説明">答えなくても大丈夫です。「答えない」で一覧から外せます。</Help>
      </summary>
      <ul class="plain push-list">
        {pushes.map((p) => (
          <li key={p.id}>
            <div>
              <span class="meta">
                {fmtTime(p.at)} {p.trigger_label}
              </span>{" "}
              <strong>{p.title}</strong>
            </div>
            {p.body && <p class="hint">{p.body}</p>}
            {periodLabel(p.payload, fmtTime) && <p class="meta">対象の時間帯: {periodLabel(p.payload, fmtTime)}</p>}
            {p.payload && <PushAnswer pushId={p.id} payload={p.payload} onDone={onDone} />}
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
    s.kind === "checkin" ? !!s.rec.note?.trim() : s.kind === "reflection" ? reflectionAnswered(s.rec) : true;
  const erg =
    hasText || e.erg ? <ErgLine kind={e.kind} id={e.id} label={e.erg} onChanged={onErgChanged} /> : undefined;
  if (s.kind === "checkin") return <CheckinDetail c={s.rec} erg={erg} onChanged={onChanged} onDeleted={onDeleted} />;
  if (s.kind === "hassle") return <HassleDetail h={s.rec} erg={erg} onChanged={onChanged} onDeleted={onDeleted} />;
  if (s.kind === "abc") {
    return (
      <AbcDetail a={s.rec} erg={erg} onChanged={onChanged} onDeleted={onDeleted}>
        <AbcDialog abc={s.rec} onChanged={onChanged} />
      </AbcDetail>
    );
  }
  return <ReflectionDetail r={s.rec} erg={erg} onChanged={onChanged} />;
}

/** B（受け止め方）を掘り下げる AI の問い。生成には時間がかかるので、実行中は表示を変える。 */
function AbcDialog({ abc, onChanged }: { abc: AbcRecord; onChanged: () => void }) {
  const act = useAction();
  return (
    <div class="stack">
      {abc.ai_dialog_md && <Markdown text={abc.ai_dialog_md} />}
      <div class="row">
        <button
          type="button"
          disabled={act.busy}
          onClick={async () => {
            if (await act.run(() => api.abcDispute(abc.id))) onChanged();
          }}
        >
          {act.busy ? "AI が考えています…" : abc.ai_dialog_md ? "問いを追加する" : "受け止め方を掘り下げる問いを AI に聞く"}
        </button>
      </div>
      <ErrorBox error={act.error} />
    </div>
  );
}
