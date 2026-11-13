import { useEffect, useState } from "preact/hooks";
import { api } from "../api";
import type { TaskView } from "../api";
import { Empty, ErrorBox } from "../components";
import { dateInput, hrefOf, navigate, useAction, useLiveTick, useRoute } from "../lib";
import { currentBlock, digestSegments, visibleWindow, type TimeSpan } from "../plan-util";
import { TodayDateStrip } from "../today-date-strip";
import { DesignPanel } from "../today-design-panel";
import { TodaySidePanel } from "../today-side-panel";
import { BreakdownSection, StartGuideCard, TodayBand } from "../today-summary";
import { TimelineCard } from "../today-timeline";
import {
  breakdownOf,
  dateFromParam,
  dateParams,
  mergedDoingIds,
  shownPanel,
  startGuideFor,
  unplacedTasks,
  viewAnchors,
} from "../today-util";
import { useTaskDrag } from "../use-task-drag";
import { useClock, useTodayData } from "../use-today-data";
import { markDesignDone, useTodaySide } from "../use-today-side";

/** 今日（と、日付を移して見る前後の日）。予定・計画・実績のタイムラインと、右のサイドパネル 1 つ。 */
export function Today() {
  const route = useRoute();
  const tick = useLiveTick();
  const today = dateInput();
  const date = dateFromParam(route.params.get("date"), today);
  const isToday = date === today;
  const designRequested = route.path[0] === "design";

  const data = useTodayData(date, isToday, tick);
  const { st, log, blocks, cal, calHint } = data;
  const act = useAction();
  const clock = useClock();

  // サイドパネル（同時に 1 つ）。今日のデザインは「完了」まで保つ。
  const { side, panel, dispatch } = useTodaySide({ designRequested, isToday, date, blocks: blocks.data });

  // 日付を移したら、その日に紐づくパネル（ブロック・補正・区間）は閉じる。
  const [armed, setArmed] = useState<TaskView | null>(null);
  const [showAllDay, setShowAllDay] = useState(false);
  useEffect(() => {
    dispatch({ type: "dateChanged" });
    setArmed(null);
  }, [date]);

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

  const view: TimeSpan | null = range
    ? visibleWindow(range.start_at, range.end_at, viewAnchors(isToday ? now : undefined, blockList, segs, events), showAllDay)
    : null;

  // タスクのチップ。動かさずに離せば「配置するタスク」に選び、タイムラインの上で離せばそこに置く。
  const drag = useTaskDrag(
    view,
    (task) => setArmed((a) => (a?.id === task.id ? null : task)),
    async (task, over) => {
      const r = await act.run(() => api.blockAdd({ start: over.start, end: over.end, task: task.id }));
      if (r) blocks.reload();
    },
  );

  const go = (d: string) => navigate(hrefOf("today", route.path, { ...dateParams(d, today) }));

  const finish = () => {
    markDesignDone(date);
    setArmed(null);
    dispatch({ type: "finishDesign" });
    if (designRequested) navigate(hrefOf("today", [], dateParams(date, today)), { replace: true });
  };

  const dayName = isToday ? "今日" : date;
  const designOn = panel?.kind === "design" || side.design;
  const candidates = unplacedTasks(data.tasks.data?.tasks ?? [], blockList);
  const guide = startGuideFor(isToday, s, log.data?.segments, side.design);
  const breakdown = breakdownOf(isToday, s, data.otherKinds.data, data.otherTasks.data);
  const shown = shownPanel(panel, !!range);

  return (
    <div class="stack">
      <TodayDateStrip
        date={date}
        today={today}
        isToday={isToday}
        onGo={go}
        designOn={designOn}
        designMode={side.design}
        onToggleDesign={() => dispatch({ type: "toggleDesign" })}
        showAllDay={showAllDay}
        onShowAllDay={setShowAllDay}
      />

      <ErrorBox error={data.error ?? act.error} />
      {nothing && <Empty>状況を取得できませんでした。</Empty>}

      <TodayBand
        isToday={isToday}
        date={date}
        status={s}
        log={log.data}
        cur={cur}
        merged={merged}
        digest={digest}
        busy={act.busy}
        onTaskDone={(id) => act.run(() => api.taskDone(id)).then(st.reload)}
        onOpenNudges={() => dispatch({ type: "open", panel: { kind: "nudge" } })}
      />


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

      {guide && <StartGuideCard doing={guide.doing} segments={guide.segments} />}

      <div class={`today-body${shown ? " has-side" : ""}`}>
        <TimelineCard
          isToday={isToday}
          log={log.data}
          blocks={blocks.data}
          view={view}
          events={events}
          now={now}
          armed={armed}
          onDisarm={() => setArmed(null)}
          designOn={designOn}
          panel={panel}
          dispatch={dispatch}
          ghost={drag.ghost}
          laneRef={drag.laneRef}
          snapSec={data.snap.data}
          reloadBlocks={blocks.reload}
        />

        {shown && (
          <aside class="side-panel" aria-label="詳細">
            {shown.kind === "design" ? (
              <DesignPanel
                date={date}
                isToday={isToday}
                dayName={dayName}
                suggest={data.suggest.data}
                events={events}
                candidates={candidates}
                armed={armed}
                onArm={setArmed}
                startTaskDrag={drag.startTaskDrag}
                taskDrag={drag.taskDrag}
                onFinish={finish}
                onBlocksChanged={blocks.reload}
              />
            ) : (
              <TodaySidePanel panel={shown} date={date} today={today} data={data} dispatch={dispatch} />
            )}
          </aside>
        )}
      </div>

      {breakdown && <BreakdownSection dayName={dayName} breakdown={breakdown} />}
    </div>
  );
}
