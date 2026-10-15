import { useEffect, useReducer, useRef, useState } from "preact/hooks";
import { api } from "../api";
import type { CalendarEvent, ReportRow, TaskView } from "../api";
import { BlockPanel } from "../block-panel";
import { blockTitle } from "../block-form";
import { Bars, Card, Empty, ErrorBox, Section } from "../components";
import { CorrectionPanel } from "../correction";
import { DayDesigner } from "../day-designer";
import { showStartGuide } from "../welcome-util";
import {
  KIND_LABEL,
  dateInput,
  fmtDur,
  fmtTime,
  hrefOf,
  navigate,
  useAction,
  useAsync,
  useLiveTick,
  useRoute,
} from "../lib";
import {
  currentBlock,
  digestSegments,
  placeTaskRange,
  shouldDesign,
  visibleWindow,
  type TimeSpan,
} from "../plan-util";
import { NudgeList } from "../nudge-answer";
import { SegmentPanel } from "../segment-panel";
import { ApplyTemplate, SaveAsTemplate } from "../templates";
import { suggestionText, weekdayOf } from "../template-util";
import {
  SIDE_INITIAL,
  dateFromParam,
  dateParams,
  daySum,
  actualPeriodFor,
  mergedDoingIds,
  shiftDate,
  sideReducer,
  visiblePanel,
  weekdayLabel,
} from "../today-util";

const DONE_KEY = "hitoiki.design.done";

/** 今日のデザインを済ませた日（YYYY-MM-DD）。保存できない環境では常に未完了。 */
function designDoneDate(): string | null {
  try {
    return localStorage.getItem(DONE_KEY);
  } catch {
    return null;
  }
}
function markDesignDone(date: string) {
  try {
    localStorage.setItem(DONE_KEY, date);
  } catch {
    /* 保存できなくても動く */
  }
}

/** 30 秒ごとに更新する現在時刻（現在時刻の線用）。 */
function useClock(): number {
  const [t, setT] = useState(Date.now());
  useEffect(() => {
    const id = setInterval(() => setT(Date.now()), 30_000);
    return () => clearInterval(id);
  }, []);
  return t;
}

interface TaskDrag {
  task: TaskView;
  /** 配置先のプレビュー（タイムラインの上にいるとき）。 */
  over: TimeSpan | null;
}

/** 今日（と、日付を移して見る前後の日）。予定・計画・実績のタイムラインと、右のサイドパネル 1 つ。 */
export function Today() {
  const route = useRoute();
  const tick = useLiveTick();
  const today = dateInput();
  const date = dateFromParam(route.params.get("date"), today);
  const isToday = date === today;
  const designRequested = route.path[0] === "design";

  const st = useAsync(() => api.status(), [tick]);
  const log = useAsync(() => api.actualList({ date }), [date, tick]);
  const blocks = useAsync(() => api.blockList({ date }), [date, tick]);
  const tasks = useAsync(() => api.taskList(["todo", "doing"]), [tick]);
  const cal = useAsync(() => api.calendarList({ date }).catch(() => [] as CalendarEvent[]), [date, tick]);
  const suggest = useAsync(() => api.templateSuggest(date).catch(() => null), [date, tick]);
  // 今日以外の内訳は、レポートと同じ集計から引く（今日は status の集計）。
  const otherKinds = useAsync(() => (isToday ? Promise.resolve(null) : api.report("kind", { date })), [date, isToday, tick]);
  const otherTasks = useAsync(() => (isToday ? Promise.resolve(null) : api.report("task", { date })), [date, isToday, tick]);
  const calHint = useAsync(async () => {
    const [dismissed, src] = await Promise.all([
      api.hintDismissed("calendar").catch(() => true),
      api.configGet("calendar.sources").catch(() => ({ key: "", value: [] as unknown })),
    ]);
    return !dismissed && !(Array.isArray(src.value) && src.value.length > 0);
  }, []);
  const snap = useAsync(
    () => api.configGet("actual.snap_sec").then((r) => (typeof r.value === "number" ? r.value : 120)).catch(() => 120),
    [],
  );
  const labels = useAsync(
    () =>
      api
        .configGet("activity.labels")
        .then((r) => (Array.isArray(r.value) ? (r.value as string[]) : []))
        .catch(() => [] as string[]),
    [],
  );
  const act = useAction();
  const clock = useClock();

  // サイドパネル（同時に 1 つ）。今日のデザインは「完了」まで保つ。
  const [side, dispatch] = useReducer(sideReducer, SIDE_INITIAL);
  const panel = visiblePanel(side);

  // 通知・振り返りから `#/today/design` で開かれたら、デザインのパネルを出す。
  useEffect(() => {
    if (designRequested) dispatch({ type: "open", panel: { kind: "design" } });
  }, [designRequested]);

  // 今日のブロックが 0 件で、まだデザインを済ませていなければ、最初に 1 度だけ自動で開く。
  const autoChecked = useRef(false);
  const blocksLoaded = !!blocks.data;
  const blockCount = blocks.data?.blocks.length ?? 0;
  useEffect(() => {
    if (autoChecked.current || !blocksLoaded) return;
    autoChecked.current = true;
    if (isToday && shouldDesign(blockCount, false, designDoneDate() === date)) dispatch({ type: "open", panel: { kind: "design" } });
  }, [blocksLoaded]);

  // 日付を移したら、その日に紐づくパネル（ブロック・補正・区間）は閉じる。
  const [armed, setArmed] = useState<TaskView | null>(null);
  const [showAllDay, setShowAllDay] = useState(false);
  useEffect(() => {
    dispatch({ type: "dateChanged" });
    setArmed(null);
  }, [date]);

  const [taskDrag, setTaskDrag] = useState<TaskDrag | null>(null);
  const laneRef = useRef<HTMLDivElement>(null);

  const s = st.data;
  const range = blocks.data?.range;
  const segs = log.data?.segments ?? [];
  const blockList = blocks.data?.blocks ?? [];
  const events = cal.data ?? [];
  const nothing = !st.loading && !s;
  const digest = digestSegments(segs);
  const now = s ? Math.max(s.now, clock) : clock;
  const cur = isToday ? currentBlock(blockList, now) : undefined;
  const merged = mergedDoingIds(cur, s?.doing ?? []);

  const go = (d: string) => navigate(hrefOf("today", route.path, { ...dateParams(d, today) }));

  const finish = () => {
    markDesignDone(date);
    setArmed(null);
    dispatch({ type: "finishDesign" });
    if (designRequested) navigate(hrefOf("today", [], dateParams(date, today)), { replace: true });
  };

  const view: TimeSpan | null = range
    ? visibleWindow(
        range.start_at,
        range.end_at,
        [
          ...(isToday ? [now] : []),
          ...blockList.flatMap((b) => [b.start_at, b.end_at]),
          ...segs.flatMap((x) => [x.start_at, x.end_at]),
          ...events.filter((c) => !c.all_day).flatMap((c) => [c.start_at, c.end_at]),
        ],
        showAllDay,
      )
    : null;

  const placed = new Set(blockList.map((b) => b.task_id).filter(Boolean));
  const candidates = (tasks.data?.tasks ?? []).filter((t) => !placed.has(t.id));

  /** タスクのチップを押した。動かさずに離せば「配置するタスク」に選び、タイムラインの上で離せばそこに置く。 */
  const startTaskDrag = (e: PointerEvent, task: TaskView) => {
    if (e.button > 0 || !e.isPrimary || !range || !view) return;
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
        setArmed((a) => (a?.id === task.id ? null : task));
        return;
      }
      const over = ev.type === "pointerup" ? spanAt(ev.clientX, ev.clientY) : null;
      if (over) {
        const r = await act.run(() => api.blockAdd({ start: over.start, end: over.end, task: task.id }));
        if (r) blocks.reload();
      }
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", end);
    window.addEventListener("pointercancel", end);
  };

  const suggested = suggest.data?.templates ?? [];
  const dayName = isToday ? "今日" : date;
  const unanswered = s?.unanswered_nudges ?? [];

  // 内訳。今日は status の集計、それ以外はレポートの集計。
  const kindRows: ReportRow[] | null = isToday ? (s?.today.by_kind ?? null) : (otherKinds.data?.rows ?? null);
  const taskRows: ReportRow[] | null = isToday ? (s?.today.top_tasks ?? null) : (otherTasks.data?.rows.slice(0, 8) ?? null);
  const breakdownTotal = isToday ? (s?.today.total_ms ?? 0) : (otherKinds.data?.total_ms ?? 0);

  const segPanelId = panel?.kind === "segment" ? panel.id : null;
  const openedSeg = segPanelId ? segs.find((x) => x.id === segPanelId) : undefined;

  const sidePanel = (() => {
    if (!panel) return null;
    switch (panel.kind) {
      case "block":
        return range ? (
          <BlockPanel
            draft={panel.draft}
            date={date}
            dayEnd={range.end_at}
            tasks={tasks.data?.tasks ?? []}
            blocks={blockList}
            events={events}
            onChanged={blocks.reload}
            onClose={() => dispatch({ type: "close" })}
          />
        ) : null;
      case "correct":
        return (
          <CorrectionPanel
            span={panel.span}
            tasks={tasks.data?.tasks ?? []}
            labels={labels.data}
            onClose={() => dispatch({ type: "close" })}
            onChanged={() => {
              log.reload();
              st.reload();
            }}
          />
        );
      case "segment":
        return openedSeg ? (
          <div class="stack">
            <SegmentPanel
              key={openedSeg.id}
              segs={[openedSeg]}
              tasks={tasks.data?.tasks ?? []}
              onDone={() => {
                dispatch({ type: "close" });
                log.reload();
                st.reload();
              }}
              onTasksChanged={tasks.reload}
            />
            <p class="row hint">
              <a href={hrefOf("actual", [openedSeg.id], { period: actualPeriodFor(date, today), status: "all" })}>実績で開く</a>
              <span class="spacer" />
              <button type="button" class="ghost" onClick={() => dispatch({ type: "close" })}>
                閉じる
              </button>
            </p>
          </div>
        ) : (
          <Empty>この区間は見つかりませんでした。</Empty>
        );
      case "nudge":
        return (
          <Section
            title="未回答の問いかけ"
            help="答えなくても大丈夫です。「答えない」で一覧から外せます。"
            aside={
              <button type="button" class="ghost" onClick={() => dispatch({ type: "close" })}>
                閉じる
              </button>
            }
          >
            {unanswered.length === 0 ? <Empty>未回答の問いかけはありません。</Empty> : <NudgeList nudges={unanswered} onDone={st.reload} />}
          </Section>
        );
      case "design":
        return (
          <Section
            title={`${dayName}のデザイン`}
            help="空いている時間をドラッグ（クリックなら 1 時間）して、ブロックを置きます。置いたブロックはドラッグで動かし、上端・下端で長さを変え、クリックで編集できます。"
            aside={
              <button class="primary" onClick={finish}>
                完了
              </button>
            }
          >
            <div class="stack">
              <Section title="テンプレート" level={3}>
                <div class="stack">
                  {suggested.length > 0 && suggest.data && (
                    <p class="hint">{suggestionText(suggested.map((t) => t.name), suggest.data.weekday || weekdayOf(date))}</p>
                  )}
                  <ApplyTemplate date={date} suggested={suggested.map((t) => t.name)} onChanged={blocks.reload} />
                  <SaveAsTemplate date={date} />
                  <p class="hint">
                    <a href="#/settings/templates">テンプレートを管理</a>
                  </p>
                </div>
              </Section>
              <Section title={`${isToday ? "今日" : "この日"}の予定`} level={3}>
                {events.length === 0 ? (
                  <Empty>{isToday ? "今日" : "この日"}のカレンダー予定はありません。</Empty>
                ) : (
                  <ul class="plain">
                    {events.map((c) => (
                      <li key={c.id}>
                        <span class="meta">{c.all_day ? "終日" : `${fmtTime(c.start_at)}–${fmtTime(c.end_at)}`}</span> {c.title}
                        {!c.is_busy && <span class="meta">（空き）</span>}
                      </li>
                    ))}
                  </ul>
                )}
              </Section>
              <Section title="未完了のタスク" level={3}>
                <p class="hint">タイムラインへドラッグするか、選んでからタイムラインをクリックするとブロックにできます。</p>
                {candidates.length === 0 ? (
                  <Empty>ブロックにしていない未完了のタスクはありません。</Empty>
                ) : (
                  <ul class="plain task-chips">
                    {candidates.map((t) => (
                      <li key={t.id}>
                        <button
                          type="button"
                          class={`chip task-chip${armed?.id === t.id ? " on" : ""}`}
                          onPointerDown={(e) => startTaskDrag(e, t)}
                          onKeyDown={(e) => {
                            if (e.key === "Enter" || e.key === " ") {
                              e.preventDefault();
                              setArmed(armed?.id === t.id ? null : t);
                            }
                          }}
                          aria-pressed={armed?.id === t.id}
                        >
                          {t.title}
                          {t.estimate_min ? <span class="meta"> {t.estimate_min}分</span> : null}
                        </button>
                      </li>
                    ))}
                  </ul>
                )}
                {taskDrag && <div class="hint">「{taskDrag.task.title}」をタイムラインの置きたい位置で離してください。</div>}
              </Section>
              <p class="hint">
                始業の振り返りは<a href="#/records/new/day_start">記録</a>から。
              </p>
            </div>
          </Section>
        );
    }
  })();

  return (
    <div class="stack">
      <div class="date-strip" role="group" aria-label="日付">
        <button type="button" aria-label="前の日" onClick={() => go(shiftDate(date, -1))}>
          ‹
        </button>
        <button type="button" disabled={isToday} onClick={() => go(today)}>
          今日
        </button>
        <button type="button" aria-label="次の日" onClick={() => go(shiftDate(date, 1))}>
          ›
        </button>
        <input
          type="date"
          value={date}
          aria-label="日付"
          onChange={(e) => {
            const v = e.currentTarget.value;
            if (v) go(dateFromParam(v, today));
          }}
        />
        <span class="meta">{weekdayLabel(date)}</span>
        <span class="spacer" />
        <button
          type="button"
          class={panel?.kind === "design" || side.design ? "chip on" : "chip"}
          aria-pressed={side.design}
          onClick={() => dispatch({ type: "toggleDesign" })}
        >
          デザイン
        </button>
        <label class="check">
          <input type="checkbox" checked={showAllDay} onChange={(e) => setShowAllDay(e.currentTarget.checked)} />
          0〜24 時を表示
        </label>
      </div>

      <ErrorBox error={st.error ?? log.error ?? blocks.error ?? act.error} />
      {nothing && <Empty>状況を取得できませんでした。</Empty>}

      {isToday && s && (
        <div class="stat-band">
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
                  <button disabled={act.busy} onClick={() => act.run(() => api.taskDone(d.task.id)).then(st.reload)}>
                    完了にする
                  </button>
                </span>
              ))
            )}
          </div>
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
                <button type="button" class="link" onClick={() => dispatch({ type: "open", panel: { kind: "nudge" } })}>
                  答える
                </button>
              ) : (
                "答えていない問いかけはありません"
              )}
            </span>
          </div>
        </div>
      )}
      {!isToday && log.data && (
        <div class="stat-band">
          <div class="stat">
            <h2 class="stat-label">{date} の合計</h2>
            <span class="stat-num">{fmtDur(daySum(segs))}</span>
            <span class="stat-sub">AI との対話 {fmtDur(digest.agentMs)} ・ 無操作・休憩・無視は含みません</span>
          </div>
        </div>
      )}

      {isToday && calHint.data && (
        <p class="hint-note" role="note">
          カレンダーをつなぐと会議を自動判定できます（<a href="#/settings">設定</a>）。
          <button
            type="button"
            class="link"
            onClick={() => api.hintDismiss("calendar").finally(calHint.reload)}
            aria-label="この案内を閉じる"
          >
            閉じる
          </button>
        </p>
      )}

      {isToday && s && log.data && !side.design && showStartGuide(s.doing.length, log.data.segments.length) && (
        <Card title="次にやること">
          <ul>
            {s.doing.length === 0 && (
              <li>
                <a href="#/tasks">タスク</a>を 1 つ作って開始すると、ルールに当たらない時間の候補になります。
              </li>
            )}
            {log.data.segments.length === 0 && (
              <li>記録はもう始まっています。しばらく使うと、ここにタイムラインが並びます。</li>
            )}
            <li>
              使い方は<a href="#/welcome">はじめに</a>でいつでも見られます。
            </li>
          </ul>
        </Card>
      )}

      <div class={`today-body${sidePanel ? " has-side" : ""}`}>
        <Card
          title="タイムライン"
          help={
            <>
              「計画」の列をドラッグすると、右のパネルでブロックを作れます（クリックなら 1 時間）。ブロックはドラッグで動かし、上端・下端で長さを変え、クリックで編集します。
              「実績」の列をドラッグすると、その時間帯の実績をタスクへ付け替えられます。クリックすると、その区間を右のパネルで開きます。
            </>
          }
        >
          {range && view && log.data && blocks.data ? (
            <>
              {(armed || panel?.kind === "design" || side.design) && (
                // デザインを開いている間は 1 行ぶん場所を空けておく（タスクを選んだときに、下のタイムラインが動かないように）。
                <p class="hint hint-slot" role="status">
                  {armed && (
                    <>
                      「<span class="hint-title">{armed.title}</span>」を置く場所をタイムラインでクリックしてください。
                      <button type="button" class="link" onClick={() => setArmed(null)}>
                        やめる
                      </button>
                    </>
                  )}
                </p>
              )}
              <DayDesigner
                dayStart={range.start_at}
                dayEnd={range.end_at}
                view={view}
                blocks={blockList}
                segments={log.data.segments}
                events={events}
                uncollected={log.data.uncollected}
                now={isToday ? now : undefined}
                reload={blocks.reload}
                draft={panel?.kind === "block" ? panel.draft : null}
                onDraft={(d) => dispatch(d ? { type: "open", panel: { kind: "block", draft: d } } : { type: "close" })}
                armedTask={armed}
                onPlaced={() => setArmed(null)}
                ghost={taskDrag?.over ?? null}
                laneRef={laneRef}
                onPickActual={(span) => dispatch({ type: "open", panel: { kind: "correct", span } })}
                onClickActual={(id) => dispatch({ type: "open", panel: { kind: "segment", id } })}
                actualSelection={panel?.kind === "correct" ? panel.span : null}
                selectedSegmentId={segPanelId}
                snapMs={(snap.data ?? 120) * 1000}
              />
              <ul class="legend">
                {[...new Set(log.data.segments.map((x) => x.kind))].map((k) => (
                  <li key={k}>
                    <i class={`dot k-${k}`} />
                    {KIND_LABEL[k]}
                  </li>
                ))}
                {(log.data.uncollected ?? []).length > 0 && (<li><i class="dot uncollected" />記録なし</li>)}
              </ul>
              {!isToday && blockList.length === 0 && segs.length === 0 && <Empty>この日の記録はありません。</Empty>}
            </>
          ) : (
            <Empty>読み込み中…</Empty>
          )}
        </Card>

        {sidePanel && (
          <aside class="side-panel" aria-label="詳細">
            {sidePanel}
          </aside>
        )}
      </div>

      {kindRows && taskRows && (
        <Section title={`${dayName}の内訳`} aside={<a href="#/report">レポートで見る</a>}>
          <div class="breakdown">
            <div class="stack">
              <h3>種別ごと</h3>
              <Bars rows={kindRows.map((r) => ({ ...r, label: KIND_LABEL[r.key as keyof typeof KIND_LABEL] ?? r.label }))} total={breakdownTotal} />
            </div>
            <div class="stack">
              <h3>タスクごと（上位）</h3>
              <Bars rows={taskRows} total={breakdownTotal} />
            </div>
          </div>
        </Section>
      )}
    </div>
  );
}
