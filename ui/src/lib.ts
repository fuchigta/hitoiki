import { useCallback, useEffect, useState } from "preact/hooks";
import { api, on, toAppError, type AppError, type EventName, type Millis, type SegmentKind } from "./api";

// ===== 表示用の整形 =====

export function fmtDur(ms: number): string {
  const min = Math.round(Math.max(0, ms) / 60000);
  if (min < 60) return `${min}分`;
  const h = Math.floor(min / 60);
  const m = min % 60;
  return m === 0 ? `${h}時間` : `${h}時間${m}分`;
}

const pad = (n: number) => String(n).padStart(2, "0");

// ===== タイムゾーン =====
// 表示は設定 `general.timezone`（IANA 名）に従う。取得できない・不正なときは OS のローカル時刻。

let TZ: string | undefined;
const TZ_EVENT = "hitoiki-tz-changed";

/** 表示に使うタイムゾーンを設定する（不正な名前は無視してローカル時刻に戻す）。 */
export function setTimeZone(tz: string | null | undefined): void {
  let next: string | undefined;
  if (tz) {
    try {
      new Intl.DateTimeFormat("ja-JP", { timeZone: tz });
      next = tz;
    } catch {
      next = undefined;
    }
  }
  if (next !== TZ) {
    TZ = next;
    window.dispatchEvent(new Event(TZ_EVENT));
  }
}

/** 設定からタイムゾーンを読み込む。 */
export async function loadTimeZone(): Promise<void> {
  try {
    const r = await api.configGet("general.timezone");
    setTimeZone(typeof r.value === "string" ? r.value : undefined);
  } catch {
    setTimeZone(undefined);
  }
}

/** タイムゾーンが変わるたびに増える値。画面の再描画に使う。 */
export function useTimeZoneVersion(): number {
  const [v, setV] = useState(0);
  useEffect(() => {
    const h = () => setV((x) => x + 1);
    window.addEventListener(TZ_EVENT, h);
    return () => window.removeEventListener(TZ_EVENT, h);
  }, []);
  return v;
}

interface Wall {
  y: number;
  mo: number;
  d: number;
  h: number;
  mi: number;
}

/** UTC ミリ秒を、表示タイムゾーンの壁時計に分解する。 */
export function wallClock(ms: Millis): Wall {
  const f = new Intl.DateTimeFormat("en-CA", {
    timeZone: TZ,
    hourCycle: "h23",
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  });
  const g: Record<string, number> = {};
  for (const p of f.formatToParts(new Date(ms))) if (p.type !== "literal") g[p.type] = Number(p.value);
  return { y: g.year, mo: g.month, d: g.day, h: g.hour % 24, mi: g.minute };
}

/** 表示タイムゾーンの HH:mm。 */
export function fmtTime(ms: Millis): string {
  const w = wallClock(ms);
  return `${pad(w.h)}:${pad(w.mi)}`;
}

export function fmtDateTime(ms: Millis): string {
  const w = wallClock(ms);
  return `${w.mo}/${w.d} ${pad(w.h)}:${pad(w.mi)}`;
}

/** `<input type="date">` 用の YYYY-MM-DD（表示タイムゾーン）。 */
export function dateInput(ms: Millis = Date.now()): string {
  const w = wallClock(ms);
  return `${w.y}-${pad(w.mo)}-${pad(w.d)}`;
}

/** 表示タイムゾーンの日付 + 時刻（HH:mm）を UTC ミリ秒に。 */
export function toMillis(date: string, time: string): Millis {
  const [y, mo, d] = date.split("-").map(Number);
  const [h, mi] = time.split(":").map(Number);
  const asUtc = Date.UTC(y, mo - 1, d, h, mi);
  // 壁時計 → UTC は、その時刻付近のオフセットを引いて求める（夏時間の境目は 2 回目の補正で収束）。
  const offsetAt = (t: Millis) => {
    const w = wallClock(t);
    return Date.UTC(w.y, w.mo - 1, w.d, w.h, w.mi) - t;
  };
  let t = asUtc - offsetAt(asUtc);
  t = asUtc - offsetAt(t);
  return t;
}

/** 日時を `YYYY-MM-DD HH:mm`（表示タイムゾーン）で。 */
export function fmtFull(ms: Millis): string {
  return `${dateInput(ms)} ${fmtTime(ms)}`;
}

export const KIND_LABEL: Record<SegmentKind, string> = {
  work: "作業",
  agent_dialog: "エージェント対話",
  agent_watch: "エージェント待機",
  meeting: "会議",
  idle: "無操作",
  break: "休憩",
  interrupt: "割り込み",
};

export const KIND_ORDER: SegmentKind[] = [
  "work",
  "agent_dialog",
  "agent_watch",
  "meeting",
  "interrupt",
  "idle",
  "break",
];

export const STATUS_LABEL: Record<string, string> = {
  todo: "未着手",
  doing: "進行中",
  done: "完了",
  dropped: "やめた",
};

// ===== 画面 =====

export const SCREENS = [
  { id: "today", label: "今日" },
  { id: "review", label: "仕分け" },
  { id: "tasks", label: "タスク" },
  { id: "plan", label: "計画" },
  { id: "reflect", label: "振り返り" },
  { id: "report", label: "レポート" },
  { id: "records", label: "記録" },
  { id: "okr", label: "OKR・スキル" },
  { id: "settings", label: "設定" },
] as const;

export type ScreenId = (typeof SCREENS)[number]["id"];

/** `#/today` 形式のハッシュから画面を得る。 */
export function screenFromHash(hash: string): ScreenId {
  const id = hash.replace(/^#\/?/, "").split(/[/?]/)[0];
  return SCREENS.some((s) => s.id === id) ? (id as ScreenId) : "today";
}

export function useScreen(): ScreenId {
  const [s, setS] = useState<ScreenId>(screenFromHash(location.hash));
  useEffect(() => {
    const h = () => setS(screenFromHash(location.hash));
    window.addEventListener("hashchange", h);
    return () => window.removeEventListener("hashchange", h);
  }, []);
  return s;
}

// ===== データ取得 =====

/** 区間・doing・取り込みの更新イベントで増える値。依存に入れると自動で再取得される。 */
export function useLiveTick(events: EventName[] = ["segments-updated", "doing-changed", "import-completed"]): number {
  const [tick, setTick] = useState(0);
  useEffect(() => {
    const offs: Promise<() => void>[] = events.map((e) =>
      on(e, () => setTick((t) => t + 1)).catch(() => () => {}),
    );
    return () => {
      offs.forEach((p) => p.then((off) => off()));
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  return tick;
}

export interface Async<R> {
  data: R | undefined;
  error: AppError | null;
  loading: boolean;
  reload: () => void;
}

export function useAsync<R>(fn: () => Promise<R>, deps: unknown[]): Async<R> {
  const [data, setData] = useState<R>();
  const [error, setError] = useState<AppError | null>(null);
  const [loading, setLoading] = useState(true);
  const [n, setN] = useState(0);
  useEffect(() => {
    let alive = true;
    setLoading(true);
    fn()
      .then((d) => {
        if (!alive) return;
        setData(d);
        setError(null);
      })
      .catch((e) => alive && setError(toAppError(e)))
      .finally(() => alive && setLoading(false));
    return () => {
      alive = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [...deps, n]);
  const reload = useCallback(() => setN((x) => x + 1), []);
  return { data, error, loading, reload };
}

/** 操作の実行とエラー表示の状態をまとめる。 */
export function useAction() {
  const [error, setError] = useState<AppError | null>(null);
  const [busy, setBusy] = useState(false);
  const run = useCallback(async <R,>(fn: () => Promise<R>): Promise<R | undefined> => {
    setBusy(true);
    setError(null);
    try {
      return await fn();
    } catch (e) {
      setError(toAppError(e));
      return undefined;
    } finally {
      setBusy(false);
    }
  }, []);
  return { error, busy, run, clear: () => setError(null) };
}
