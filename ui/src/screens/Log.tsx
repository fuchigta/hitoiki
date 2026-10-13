import { useState } from "preact/hooks";
import { api, type SegmentView } from "../api";
import { EmptyState, ErrorBox } from "../components";
import { SuggestionBand } from "../log-suggestions";
import { LOG_STATUSES, filterSegments, nextToReview, statusFromParam, toggleId, type LogStatus } from "../log-util";
import { KIND_LABEL, fmtDateTime, fmtDur, fmtTime, hrefOf, navigate, useAsync, useLiveTick, useRoute } from "../lib";
import { SegmentPanel } from "../segment-panel";
import type { PeriodId } from "../ui/period-util";
import { ListDetail, ObjectList, ObjectRow, PeriodSelect } from "../ui/parts";

const STATUS_LABEL_JA: Record<LogStatus, string> = { review: "要確認", untasked: "タスクなし", all: "すべて" };
const PERIODS: PeriodId[] = ["today", "yesterday", "this-week", "last-week"];

/** 短い区間のしきい値の表記（「1分」「90秒」）。 */
function fmtShortLimit(sec: number): string {
  return sec >= 60 && sec % 60 === 0 ? `${sec / 60}分` : `${sec}秒`;
}

export function Log() {
  const route = useRoute();
  const tick = useLiveTick();
  const selectedId = route.path[0] ?? null;
  const status = statusFromParam(route.params.get("status"));
  const periodParam = route.params.get("period");
  const period: PeriodId = PERIODS.find((p) => p === periodParam) ?? "this-week";
  const taskFilter = route.params.get("task");
  const [showShort, setShowShort] = useState(false);
  const [checked, setChecked] = useState<string[]>([]);

  // 要確認の件数は、どの絞り込みでもチップに出すので常に取る。
  const review = useAsync(() => api.reviewList({ date: period }, showShort), [period, showShort, tick]);
  const all = useAsync(() => (status === "review" ? Promise.resolve(null) : api.log({ date: period })), [period, status, tick]);
  const tasks = useAsync(() => api.taskList(["todo", "doing"]), [tick]);
  const suggestions = useAsync(() => api.reviewSuggestions(), [tick]);
  const ignoreSuggestions = useAsync(() => api.ignoreSuggestions(), [tick]);

  const hrefWith = (id: string | null, over: Record<string, string | undefined> = {}) =>
    hrefOf("log", id ? [id] : [], {
      period: period === "this-week" ? undefined : period,
      status: status === "review" ? undefined : status,
      task: taskFilter ?? undefined,
      ...over,
    });
  /** 絞り込みを変える（選択は外す）。 */
  const setFilter = (over: Record<string, string | undefined>) => {
    setChecked([]);
    navigate(hrefWith(null, over));
  };

  const source: SegmentView[] | undefined =
    status === "review" ? review.data?.segments : all.data ? filterSegments(all.data.segments, status, taskFilter) : undefined;
  const visible = source ?? [];
  const chosen = checked.filter((id) => visible.some((s) => s.id === id));
  const single = selectedId ? visible.find((s) => s.id === selectedId) : undefined;
  const panelSegs = chosen.length > 0 ? visible.filter((s) => chosen.includes(s.id)) : single ? [single] : [];

  const refresh = () => {
    review.reload();
    all.reload();
    suggestions.reload();
    ignoreSuggestions.reload();
  };

  /** 確定・無視が済んだ。要確認の絞り込み中は、次の要確認の区間を選ぶ。 */
  const done = (ids: string[]) => {
    setChecked([]);
    if (status === "review") {
      const next = nextToReview(visible.map((s) => s.id), ids);
      navigate(hrefWith(next), { replace: true });
    }
    refresh();
  };

  const taskTitle = taskFilter ? (all.data?.segments.find((s) => s.task_id === taskFilter)?.task_title ?? null) : null;
  const sameDay = period === "today" || period === "yesterday";
  const loaded = source !== undefined;
  const reviewCount = review.data?.count;

  const list = (
    <div class="stack">
      <div class="list-head">
        <PeriodSelect value={period} onChange={(p) => setFilter({ period: p === "this-week" ? undefined : p })} />
        {LOG_STATUSES.map((s) => (
          <button
            key={s}
            type="button"
            class={status === s ? "chip on" : "chip"}
            aria-pressed={status === s}
            onClick={() => setFilter({ status: s === "review" ? undefined : s })}
          >
            {STATUS_LABEL_JA[s]}
            {s === "review" && reviewCount != null && reviewCount > 0 ? ` ${reviewCount}` : ""}
          </button>
        ))}
      </div>
      {taskFilter && (
        <p class="hint">
          <span class="tag">タスク: {taskTitle ?? "指定あり"}</span>{" "}
          <button type="button" class="link" onClick={() => setFilter({ task: undefined })}>
            絞り込みを解除
          </button>
        </p>
      )}
      {status === "review" && review.data && (review.data.hidden_short > 0 || showShort) && (
        <label class="row hint">
          <input type="checkbox" checked={showShort} onChange={(e) => setShowShort(e.currentTarget.checked)} />
          {showShort
            ? `短い区間（${fmtShortLimit(review.data.min_sec)}未満）も表示中`
            : `短い区間 ${review.data.hidden_short} 件を表示（${fmtShortLimit(review.data.min_sec)}未満）`}
        </label>
      )}
      <ErrorBox error={review.error ?? all.error ?? tasks.error ?? suggestions.error ?? ignoreSuggestions.error} />
      {loaded && visible.length === 0 && (
        <EmptyState>
          {status === "review"
            ? review.data && review.data.hidden_short > 0
              ? "確認が必要な区間はありません（短い区間のみ隠れています）。"
              : "確認が必要な区間はありません。"
            : "該当する区間はありません。"}
        </EmptyState>
      )}
      {visible.length > 1 && (
        <label class="check">
          <input
            type="checkbox"
            checked={chosen.length === visible.length}
            onChange={(e) => setChecked(e.currentTarget.checked ? visible.map((s) => s.id) : [])}
          />
          すべて選ぶ
          <span class="meta">{visible.length} 件</span>
        </label>
      )}
      <ObjectList label="区間">
        {visible.map((s) => (
          <ObjectRow
            key={s.id}
            selected={chosen.length === 0 && selectedId === s.id}
            leading={
              <label class="check" title="まとめて確定するために選ぶ">
                <input
                  type="checkbox"
                  checked={chosen.includes(s.id)}
                  aria-label="この区間を選ぶ"
                  onChange={() => setChecked(toggleId(chosen, s.id))}
                />
              </label>
            }
            title={
              <>
                <i class={`dot k-${s.kind}`} title={KIND_LABEL[s.kind]} />
                {s.title || s.exe_name || "（タイトルなし）"}
              </>
            }
            tags={[
              ...(s.task_title ? [s.task_title] : []),
              ...(s.status === "needs_review" ? ["要確認"] : []),
              ...(s.offline ? ["PC 外"] : []),
            ]}
            meta={`${sameDay ? fmtTime(s.start_at) : fmtDateTime(s.start_at)}–${fmtTime(s.end_at)} ${fmtDur(s.duration_ms)}`}
            onSelect={() => {
              setChecked([]);
              navigate(hrefWith(s.id));
            }}
          />
        ))}
      </ObjectList>
    </div>
  );

  return (
    <div class="stack">
      <SuggestionBand rules={suggestions.data ?? []} ignores={(ignoreSuggestions.data ?? []).slice(0, 3)} onDone={refresh} />
      <ListDetail
        list={list}
        detail={
          panelSegs.length > 0 ? (
            <SegmentPanel
              key={panelSegs.map((s) => s.id).join(",")}
              segs={panelSegs}
              tasks={tasks.data?.tasks ?? []}
              onDone={done}
              onTasksChanged={tasks.reload}
            />
          ) : (
            visible.length > 0 && (
              <EmptyState>
                {selectedId && loaded
                  ? "この区間は、いまの絞り込みの一覧にありません。"
                  : "区間を選ぶと、タスクの割り当てなどができます。"}
              </EmptyState>
            )
          )
        }
      />
    </div>
  );
}
