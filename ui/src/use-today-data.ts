// 今日の画面のデータ取得。呼び出し先・引数・依存配列・エラー時のフォールバックは画面側の規約そのまま
// （画面の表示部品は、ここで取ったものを受け取るだけ）。
import { useEffect, useState } from "preact/hooks";
import { api } from "./api";
import type { ActualList, AppError, BlockList, CalendarEvent, Report, Status, Suggestion, TaskView } from "./api";
import { useAsync, type Async } from "./lib";

export interface TodayData {
  /** 画面に出す取得エラー（状況・実績・ブロックの順で最初のもの。予定やテンプレートは取れなくても黙って空にする）。 */
  error: AppError | null;
  /** 状況（いま・今日の合計・要確認・未回答の問いかけ）。 */
  st: Async<Status>;
  /** その日の実績の区間。 */
  log: Async<ActualList>;
  /** その日の計画ブロックと日の範囲。 */
  blocks: Async<BlockList>;
  /** 未着手・進行中のタスク。 */
  tasks: Async<{ tasks: TaskView[] }>;
  /** その日のカレンダー予定（取れなければ空）。 */
  cal: Async<CalendarEvent[]>;
  /** テンプレートの提案（取れなければ null）。 */
  suggest: Async<Suggestion | null>;
  /** 今日以外の日の内訳（種別ごと）。今日は null。 */
  otherKinds: Async<Report | null>;
  /** 今日以外の日の内訳（タスクごと）。今日は null。 */
  otherTasks: Async<Report | null>;
  /** カレンダー接続の案内を出すか。 */
  calHint: Async<boolean>;
  /** 実績の補正で使う吸着の秒数（既定 120）。 */
  snap: Async<number>;
  /** 作業内容のラベル候補。 */
  labels: Async<string[]>;
}

/** 今日の画面で使うデータをまとめて取る。`tick` は区間・doing・取り込みの更新で増える値（再取得のきっかけ）。 */
export function useTodayData(date: string, isToday: boolean, tick: number): TodayData {
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
  const error = st.error ?? log.error ?? blocks.error;
  return { error, st, log, blocks, tasks, cal, suggest, otherKinds, otherTasks, calHint, snap, labels };
}

/** 30 秒ごとに更新する現在時刻（現在時刻の線用）。 */
export function useClock(): number {
  const [t, setT] = useState(Date.now());
  useEffect(() => {
    const id = setInterval(() => setT(Date.now()), 30_000);
    return () => clearInterval(id);
  }, []);
  return t;
}
