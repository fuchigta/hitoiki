import { useCallback, useEffect, useState } from "preact/hooks";
import { leaveGuard } from "./edit-util";
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
  ignored: "無視",
};

export const KIND_ORDER: SegmentKind[] = [
  "work",
  "agent_dialog",
  "agent_watch",
  "meeting",
  "interrupt",
  "idle",
  "break",
  "ignored",
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
  { id: "log", label: "実績" },
  { id: "tasks", label: "タスク" },
  { id: "reflect", label: "振り返り" },
  { id: "report", label: "レポート" },
  { id: "records", label: "記録" },
  { id: "okr", label: "目標" },
  { id: "settings", label: "設定" },
  { id: "onboarding", label: "はじめに" },
] as const;

export type ScreenId = (typeof SCREENS)[number]["id"];

export interface Route {
  screen: ScreenId;
  /** 画面より後ろのパス（`#/tasks/<ID>` なら `[ID]`）。 */
  path: string[];
  params: URLSearchParams;
}

function safeDecode(s: string): string {
  try {
    return decodeURIComponent(s);
  } catch {
    return s;
  }
}

/** 画面としては無くなったが、ハッシュとしては受け付ける旧い ID（`legacyRoute` で新しいルートに写す）。 */
const LEGACY_IDS: string[] = ["review", "plan"];

/**
 * `#/tasks/<ID>?tab=time` 形式のハッシュをルートに分解する。知らない画面は `today`。
 * 記録の旧名（`checkin` / `hassle` / `reflection/<kind>`）は、ここで受け付けて記録・振り返りに寄せる（`legacyRoute` が仕上げる）。
 */
export function routeFromHash(hash: string): Route {
  const body = hash.replace(/^#\/?/, "");
  const q = body.indexOf("?");
  const pathPart = q >= 0 ? body.slice(0, q) : body;
  const params = new URLSearchParams(q >= 0 ? body.slice(q + 1) : "");
  const segs = pathPart.split("/").filter((x) => x !== "");
  if (segs[0] === "checkin" || segs[0] === "hassle") return { screen: "records", path: ["new", segs[0]], params };
  if (segs[0] === "reflection") return { screen: "reflect", path: segs.slice(1).map(safeDecode), params };
  if (!SCREENS.some((s) => s.id === segs[0]) && !LEGACY_IDS.includes(segs[0])) return { screen: "today", path: [], params };
  return { screen: segs[0] as ScreenId, path: segs.slice(1).map(safeDecode), params };
}

/** ルートからハッシュを作る。`params` は値が空のものを除く。 */
export function hrefOf(
  screen: string,
  path: string[] = [],
  params?: Record<string, string | undefined> | URLSearchParams,
): string {
  const sp = new URLSearchParams();
  if (params instanceof URLSearchParams) params.forEach((v, k) => v && sp.set(k, v));
  else for (const [k, v] of Object.entries(params ?? {})) if (v) sp.set(k, v);
  const qs = sp.toString();
  return `#/${[screen, ...path.map((x) => encodeURIComponent(x))].join("/")}${qs ? `?${qs}` : ""}`;
}

/**
 * 旧い画面 ID のルートを、いまのルートへ写す（`hitoiki ui <画面>`・通知の `open_screen`・起動引数が旧名を使うため）。
 * 画面を統合する段階で、ここに足す。
 * - `review` → `log?status=review`（ほかのクエリは残す）
 * - `plan` → `today`（`date` などのクエリは残す。計画は今日の画面に統合された）
 * - `reflect` → `records?kind=reflection`
 * - `reflect/<kind>`（day_start・day_end・weekly）→ `records/new/<kind>`（振り返りの作成フォーム）
 */
export function legacyRoute(route: Route): Route {
  if ((route.screen as string) === "review") {
    const params = new URLSearchParams(route.params);
    if (!params.has("status")) params.set("status", "review");
    return { screen: "log", path: route.path, params };
  }
  if ((route.screen as string) === "plan") {
    return { screen: "today", path: route.path, params: route.params };
  }
  if (route.screen === "reflect") {
    const kind = route.path[0];
    if (kind === "day_start" || kind === "day_end" || kind === "weekly") {
      return { screen: "records", path: ["new", kind], params: route.params };
    }
    const params = new URLSearchParams(route.params);
    params.set("kind", "reflection");
    return { screen: "records", path: [], params };
  }
  return route;
}

/** `#/today` 形式のハッシュから画面を得る。 */
export function screenFromHash(hash: string): ScreenId {
  return legacyRoute(routeFromHash(hash)).screen;
}

/**
 * ハッシュを移す。未保存の変更があれば確認してから。
 * 同じハッシュなら何もしない。`replace` は履歴を増やさずに置き換える。
 */
export function navigate(href: string, opts: { replace?: boolean } = {}): void {
  if (href === location.hash) return;
  leaveGuard.requestLeave(() => {
    if (opts.replace) location.replace(href);
    else location.hash = href;
  });
}

const hashOf = (r: Route) => hrefOf(r.screen, r.path, r.params);

/** 現在のルート。ハッシュが変わると更新する。旧名のハッシュは、履歴を増やさず正規のルートに置き換える。 */
export function useRoute(): Route {
  const [r, setR] = useState<Route>(() => legacyRoute(routeFromHash(location.hash)));
  useEffect(() => {
    const h = () => {
      const raw = routeFromHash(location.hash);
      const next = legacyRoute(raw);
      if (hashOf(next) !== hashOf(raw)) {
        location.replace(hashOf(next));
        return;
      }
      setR(next);
    };
    h();
    window.addEventListener("hashchange", h);
    return () => window.removeEventListener("hashchange", h);
  }, []);
  return r;
}

export function useScreen(): ScreenId {
  return useRoute().screen;
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
