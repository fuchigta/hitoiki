import { useEffect, useRef, useState } from "preact/hooks";
import { api } from "../api";
import type { CalendarEvent, TaskView } from "../api";
import { blockTitle } from "../block-form";
import { Bars, Card, Empty, ErrorBox } from "../components";
import { CorrectionPanel } from "../correction";
import { DayDesigner } from "../day-designer";
import { showStartGuide } from "../onboarding-util";
import { KIND_LABEL, dateInput, fmtDur, fmtTime, hrefOf, navigate, useAction, useAsync, useLiveTick } from "../lib";
import {
  currentBlock,
  digestSegments,
  placeTaskRange,
  shouldDesign,
  visibleWindow,
  type TimeSpan,
} from "../plan-util";
import { PushAnswer } from "../push-answer";
import { periodLabel } from "../push-answer-util";
import { ApplyTemplate, SaveAsTemplate } from "../templates";
import { suggestionText, weekdayOf } from "../template-util";

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

/** `#/today/design` で開かれたか（始業の通知・振り返りからの導線）。 */
function designRequested(): boolean {
  return /^#\/?today\/design/.test(location.hash);
}
function useDesignRequested(): boolean {
  const [r, setR] = useState(designRequested());
  useEffect(() => {
    const h = () => setR(designRequested());
    window.addEventListener("hashchange", h);
    return () => window.removeEventListener("hashchange", h);
  }, []);
  return r;
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

export function Today() {
  const tick = useLiveTick();
  const date = dateInput();
  const st = useAsync(() => api.status(), [tick]);
  const log = useAsync(() => api.log({ date: "today" }), [tick]);
  const blocks = useAsync(() => api.blockList({ date: "today" }), [tick]);
  const tasks = useAsync(() => api.taskList(["todo", "doing"]), [tick]);
  const cal = useAsync(() => api.calendarList({ date: "today" }).catch(() => [] as CalendarEvent[]), [tick]);
  const suggest = useAsync(() => api.templateSuggest("today").catch(() => null), [tick]);
  const calHint = useAsync(async () => {
    const [dismissed, src] = await Promise.all([
      api.hintDismissed("calendar").catch(() => true),
      api.configGet("calendar.sources").catch(() => ({ key: "", value: [] as unknown })),
    ]);
    return !dismissed && !(Array.isArray(src.value) && src.value.length > 0);
  }, []);
  const snap = useAsync(
    () => api.configGet("review.snap_sec").then((r) => (typeof r.value === "number" ? r.value : 120)).catch(() => 120),
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
  const requested = useDesignRequested();
  const clock = useClock();

  // 今日のデザインモード: ブロックが 0 件、または通知・振り返りから開いたときに入り、「完了」まで保つ。
  const [mode, setMode] = useState<"auto" | "on" | "off">("auto");
  const blocksLoaded = !!blocks.data;
  const blockCount = blocks.data?.blocks.length ?? 0;
  useEffect(() => {
    if (requested) setMode("on");
    else if (mode === "auto" && blocksLoaded && shouldDesign(blockCount, false, designDoneDate() === date)) setMode("on");
  }, [requested, blocksLoaded]);
  const designing = mode === "on";

  const [showAllDay, setShowAllDay] = useState(false);
  const [armed, setArmed] = useState<TaskView | null>(null);
  const [taskDrag, setTaskDrag] = useState<TaskDrag | null>(null);
  /** 「実績」の列で選んだ、補正する範囲。 */
  const [corr, setCorr] = useState<TimeSpan | null>(null);
  const laneRef = useRef<HTMLDivElement>(null);

  const s = st.data;
  const range = blocks.data?.range;
  const segs = log.data?.segments ?? [];
  const blockList = blocks.data?.blocks ?? [];
  const events = cal.data ?? [];
  const nothing = !st.loading && !s;
  const digest = digestSegments(segs);
  const now = s ? Math.max(s.now, clock) : clock;
  const cur = currentBlock(blockList, now);

  const finish = () => {
    markDesignDone(date);
    setArmed(null);
    setMode("off");
    if (designRequested()) location.hash = "#/today";
  };

  const view: TimeSpan | null = range
    ? visibleWindow(
        range.start_at,
        range.end_at,
        [
          now,
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

  return (
    <div class="stack">
      <ErrorBox error={st.error ?? log.error ?? blocks.error ?? act.error} />
      {nothing && <Empty>状況を取得できませんでした。</Empty>}

      {s && (
        <div class="stat-band">
          <div class="stat stat-now">
            <h2 class="stat-label">いま</h2>
            {cur && (
              <span class="stat-title">
                {blockTitle(cur)}
                {cur.focus && <span class="tag">集中</span>}
                <span class="meta">
                  {" "}
                  {fmtTime(cur.start_at)}–{fmtTime(cur.end_at)} の枠
                </span>
              </span>
            )}
            {s.doing.length === 0 ? (
              <span class="stat-sub">進行中のタスクはありません。</span>
            ) : (
              s.doing.map((d) => (
                <span key={d.task.id} class="stat-doing">
                  <span class="stat-title">{d.task.title}</span>
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
              エージェント対話 {fmtDur(digest.agentMs)} ・ 無操作 {fmtDur(s.today.idle_ms)} ・ 休憩 {fmtDur(s.today.break_ms)}
              {s.today.ignored_ms > 0 ? ` ・ 無視 ${fmtDur(s.today.ignored_ms)}` : ""}
            </span>
            <span class="stat-sub">無操作・休憩・無視は合計に含みません</span>
          </div>
          <div class="stat">
            <h2 class="stat-label">確認待ち</h2>
            <span class="stat-num">{s.needs_review_count}件</span>
            <span class="stat-sub">
              {s.needs_review_count > 0 ? (
                <>
                  {digest.reviewMs > 0 && `今日の分は約 ${Math.max(1, Math.round(digest.reviewMs / 60000))} 分 ・ `}
                  <a href={hrefOf("log", [], { status: "review", period: "today" })}>実績で確認する</a>
                </>
              ) : (
                "確認が必要な区間はありません"
              )}
              {s.needs_review_short_count > 0 && ` ・ 短い区間 ${s.needs_review_short_count} 件は実績で表示できます`}
            </span>
          </div>
        </div>
      )}

      {calHint.data && (
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

      {s && log.data && !designing && showStartGuide(s.doing.length, log.data.segments.length) && (
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
              使い方は<a href="#/onboarding">はじめに</a>でいつでも見られます。
            </li>
          </ul>
        </Card>
      )}

      <div class={designing ? "design" : ""}>
        <Card
          title={designing ? "今日のデザイン" : "タイムライン"}
          help={
            <>
              {designing
                ? "空いている時間をドラッグ（クリックなら 1 時間）して、ブロックを置きます。置いたブロックはドラッグで動かし、上端・下端で長さを変え、クリックで編集できます。"
                : "「計画」の列をドラッグすると、その場でブロックを作れます。"}{" "}
              「実績」の列をドラッグすると、その時間帯の実績をタスクへ付け替えられます。クリックすると、その区間を実績の画面で開きます。
            </>
          }
          aside={
            <div class="row">
              <label class="check">
                <input type="checkbox" checked={showAllDay} onChange={(e) => setShowAllDay(e.currentTarget.checked)} />
                0〜24 時を表示
              </label>
              {designing ? (
                <button class="primary" onClick={finish}>
                  完了
                </button>
              ) : (
                <button class="ghost" onClick={() => setMode("on")}>今日のデザイン</button>
              )}
            </div>
          }
        >
          {range && view && log.data && blocks.data ? (
            <>
              {armed && (
                <p class="hint" role="status">
                  「{armed.title}」を置く場所をタイムラインでクリックしてください。
                  <button type="button" class="link" onClick={() => setArmed(null)}>
                    やめる
                  </button>
                </p>
              )}
              <DayDesigner
                date={date}
                dayStart={range.start_at}
                dayEnd={range.end_at}
                view={view}
                blocks={blockList}
                segments={log.data.segments}
                events={events}
                gaps={log.data.gaps}
                tasks={tasks.data?.tasks ?? []}
                now={now}
                reload={blocks.reload}
                armedTask={armed}
                onPlaced={() => setArmed(null)}
                ghost={taskDrag?.over ?? null}
                laneRef={laneRef}
                onPickActual={setCorr}
                onClickActual={(id) => navigate(hrefOf("log", [id], { period: "today", status: "all" }))}
                actualSelection={corr}
                snapMs={(snap.data ?? 120) * 1000}
              />
              <ul class="legend">
                {[...new Set(log.data.segments.map((x) => x.kind))].map((k) => (
                  <li key={k}>
                    <i class={`dot k-${k}`} />
                    {KIND_LABEL[k]}
                  </li>
                ))}
                {(log.data.gaps ?? []).length > 0 && (<li><i class="dot gap" />記録なし</li>)}
              </ul>
            </>
          ) : (
            <Empty>読み込み中…</Empty>
          )}
        </Card>

        {designing && (
          <div class="stack design-side">
            <Card title="テンプレート">
              {suggested.length > 0 && suggest.data && (
                <p class="hint">{suggestionText(suggested.map((t) => t.name), suggest.data.weekday || weekdayOf(date))}</p>
              )}
              <ApplyTemplate date={date} suggested={suggested.map((t) => t.name)} onChanged={blocks.reload} />
              <SaveAsTemplate date={date} />
            </Card>
            <Card title="今日の予定">
              {events.length === 0 ? (
                <Empty>今日のカレンダー予定はありません。</Empty>
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
            </Card>
            <Card title="未完了のタスク" help="タイムラインへドラッグするか、選んでからタイムラインをクリックするとブロックにできます。">
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
            </Card>
            <p class="hint">
              始業の振り返りは<a href="#/records/new/day_start">記録</a>から。
            </p>
          </div>
        )}
      </div>

      {corr && (
        <CorrectionPanel
          span={corr}
          tasks={tasks.data?.tasks ?? []}
          labels={labels.data}
          onClose={() => setCorr(null)}
          onChanged={() => {
            log.reload();
            st.reload();
          }}
        />
      )}

      {s && (
        <div class="grid2">
          <Card title="種別ごと">
            <Bars rows={s.today.by_kind.map((r) => ({ ...r, label: KIND_LABEL[r.key as keyof typeof KIND_LABEL] ?? r.label }))} total={s.today.total_ms} />
          </Card>
          <Card title="タスクごと（上位）">
            <Bars rows={s.today.top_tasks} total={s.today.total_ms} />
          </Card>
        </div>
      )}

      {s && s.unanswered_pushes.length > 0 && (
        <Card title="未回答の問いかけ" help="答えなくても大丈夫です。「答えない」で一覧から外せます。">
          <ul class="plain push-list">
            {s.unanswered_pushes.map((p) => (
              <li key={p.id}>
                <div>
                  <span class="meta">{fmtTime(p.at)} {p.trigger_label}</span> <strong>{p.title}</strong>
                </div>
                {p.body && <p class="hint">{p.body}</p>}
                {periodLabel(p.payload, fmtTime) && <p class="meta">対象の時間帯: {periodLabel(p.payload, fmtTime)}</p>}
                {p.payload && <PushAnswer pushId={p.id} payload={p.payload} onDone={st.reload} />}
              </li>
            ))}
          </ul>
        </Card>
      )}
    </div>
  );
}
