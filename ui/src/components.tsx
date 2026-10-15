import type { ComponentChildren } from "preact";
import { useEffect, useRef, useState } from "preact/hooks";
import { api, type AppError, type BlockView, type UncollectedView, type Millis, type ReportRow, type RuntimeMode, type SegmentView } from "./api";
import { leaveGuard } from "./edit-util";
import { Help } from "./ui/parts";
import { KIND_LABEL, KIND_ORDER, fmtDur, fmtTime, useAction } from "./lib";

export function ErrorBox({ error }: { error: AppError | null }) {
  if (!error) return null;
  return (
    <div class="error" role="alert">
      {error.message}
    </div>
  );
}

/** 常駐処理が自前で動いていないとき（別プロセスが収集中／起動失敗）に理由を表示する。 */
export function RuntimeBanner() {
  const [m, setM] = useState<RuntimeMode | null>(null);
  useEffect(() => {
    api.runtimeState().then(setM).catch(() => {});
  }, []);
  if (!m || (m.mode !== "external" && m.mode !== "failed")) return null;
  return (
    <div class={`banner${m.mode === "failed" ? " warn" : ""}`} role="status">
      {m.message}
    </div>
  );
}

interface SectionProps {
  title?: string;
  /** 見出しの階層。2 = 画面内の区画（カード）、3 = 区画の中の小見出し。 */
  level?: 2 | 3;
  /** 見出しの右に置く要素（操作ボタンなど）。 */
  aside?: ComponentChildren;
  /** 見出しの横の「?」に入れる説明文。 */
  help?: ComponentChildren;
  children: ComponentChildren;
}

/** 見出しつきの区画。level 2 はカード枠つき（h2）、level 3 は枠なしの小見出し（h3）。 */
export function Section({ title, level = 2, aside, help, children }: SectionProps) {
  const head =
    title || aside ? (
      <header class="card-head">
        {level === 2 ? <h2>{title}</h2> : <h3>{title}</h3>}
        {help && <Help>{help}</Help>}
        <div class="card-aside">{aside}</div>
      </header>
    ) : null;
  return (
    <section class={level === 2 ? "card" : "sec"}>
      {head}
      {children}
    </section>
  );
}

/** Section（level 2）の別名。既存の呼び出し用。 */
export function Card(props: Omit<SectionProps, "level">) {
  return <Section {...props} level={2} />;
}

/** 未保存の変更があるまま移動しようとしたときの確認（`window.confirm` の代わりのアプリ内 UI）。 */
export function LeaveConfirm() {
  const [, setN] = useState(0);
  useEffect(() => leaveGuard.subscribe(() => setN((x) => x + 1)), []);
  const open = leaveGuard.hasPending();
  useEffect(() => {
    if (!open) return;
    const h = (e: KeyboardEvent) => {
      if (e.key === "Escape") leaveGuard.cancel();
    };
    window.addEventListener("keydown", h);
    return () => window.removeEventListener("keydown", h);
  }, [open]);
  if (!open) return null;
  return (
    <div class="modal-back">
      <div class="modal" role="alertdialog" aria-modal="true" aria-labelledby="leave-title">
        <p id="leave-title">未保存の変更があります。破棄して移動しますか？</p>
        <div class="row">
          <span class="spacer" />
          <button autoFocus onClick={() => leaveGuard.cancel()}>
            キャンセル
          </button>
          <button class="primary" onClick={() => leaveGuard.discard()}>
            破棄して移動
          </button>
        </div>
      </div>
    </div>
  );
}

/** 空のときの案内。短い 1 文と、任意の操作ボタン。 */
export function EmptyState({ children, action }: { children: ComponentChildren; action?: ComponentChildren }) {
  return (
    <div class="empty">
      <p>{children}</p>
      {action && <div class="row">{action}</div>}
    </div>
  );
}

/** EmptyState の別名。既存の呼び出し用。 */
export const Empty = EmptyState;

/**
 * 一覧の上の「＋ ○○」。押すとタイトルの入力欄が出て、Enter で作成する（Esc でやめる）。
 * `onCreate` は作成に成功したら true 相当の値を返す（失敗は undefined で、エラーを出して欄を残す）。
 */
export function InlineNew({
  label,
  inputLabel,
  placeholder = "タイトル（Enter で作成）",
  onCreate,
}: {
  label: string;
  inputLabel: string;
  placeholder?: string;
  onCreate: (title: string) => Promise<unknown>;
}) {
  const [open, setOpen] = useState(false);
  const [title, setTitle] = useState("");
  const act = useAction();
  const input = useRef<HTMLInputElement>(null);
  useEffect(() => {
    if (open) input.current?.focus();
  }, [open]);
  const close = () => {
    setOpen(false);
    setTitle("");
  };
  if (!open) {
    return (
      <button type="button" onClick={() => setOpen(true)}>
        {label}
      </button>
    );
  }
  return (
    <form
      class="list-new grow"
      onSubmit={async (e) => {
        e.preventDefault();
        if (!title.trim()) return;
        const r = await act.run(() => onCreate(title.trim()));
        if (r !== undefined) close();
      }}
    >
      <input
        ref={input}
        class="grow"
        value={title}
        aria-label={inputLabel}
        placeholder={placeholder}
        onInput={(e) => setTitle(e.currentTarget.value)}
        onKeyDown={(e) => {
          if (e.key === "Escape") {
            e.stopPropagation();
            close();
          }
        }}
      />
      <ErrorBox error={act.error} />
    </form>
  );
}

export function Field({ label, children }: { label: string; children: ComponentChildren }) {
  return (
    <label class="field">
      <span>{label}</span>
      {children}
    </label>
  );
}

// ===== 横棒グラフ =====

export function Bars({ rows, total }: { rows: ReportRow[]; total: number }) {
  if (rows.length === 0) return <Empty>この期間の記録はありません。</Empty>;
  const max = Math.max(...rows.map((r) => r.duration_ms), 1);
  return (
    <ul class="bars">
      {rows.map((r) => (
        <li key={r.key ?? r.label}>
          <span class="bar-label" title={r.label}>
            {r.label}
          </span>
          <span class="bar-track">
            <span class="bar-fill" style={{ width: `${(r.duration_ms / max) * 100}%` }} />
          </span>
          <span class="bar-value">
            {fmtDur(r.duration_ms)}
            {total > 0 && <small> {Math.round((r.duration_ms / total) * 100)}%</small>}
          </span>
        </li>
      ))}
    </ul>
  );
}

// ===== タイムライン（計画ブロックと実区間の重ね合わせ） =====

interface TimelineProps {
  segments: SegmentView[];
  blocks: BlockView[];
  now: Millis;
  /** 1 日の範囲（その日の 0 時〜翌 0 時）。 */
  dayStart: Millis;
  dayEnd: Millis;
  /** 記録なしの時間帯。実績の行に斜線で重ねる。 */
  uncollected?: UncollectedView[];
}

const HOUR = 3600_000;

export function Timeline({ segments, blocks, now, dayStart, dayEnd, uncollected = [] }: TimelineProps) {
  const starts = [
    ...segments.map((s) => s.start_at),
    ...blocks.map((b) => b.start_at),
    ...uncollected.map((g) => g.start_at),
    now,
  ];
  const ends = [
    ...segments.map((s) => s.end_at),
    ...blocks.map((b) => b.end_at),
    ...uncollected.map((g) => g.end_at),
    now,
  ];
  let lo = Math.floor((Math.min(...starts) - dayStart) / HOUR) * HOUR + dayStart;
  let hi = Math.ceil((Math.max(...ends) - dayStart) / HOUR) * HOUR + dayStart;
  // 少なくとも 8 時間の幅で表示する。
  if (hi - lo < 8 * HOUR) hi = lo + 8 * HOUR;
  lo = Math.max(dayStart, lo);
  hi = Math.min(dayEnd, Math.max(hi, lo + HOUR));
  const span = hi - lo;
  const pos = (t: Millis) => `${(Math.min(Math.max(t, lo), hi) - lo) / span * 100}%`;
  const width = (a: Millis, b: Millis) =>
    `${Math.max(0, (Math.min(b, hi) - Math.max(a, lo)) / span * 100)}%`;
  const ticks: Millis[] = [];
  for (let t = Math.ceil((lo - dayStart) / HOUR) * HOUR + dayStart; t <= hi; t += HOUR) ticks.push(t);
  const step = ticks.length > 14 ? 2 : 1;
  const kinds = KIND_ORDER.filter((k) => segments.some((s) => s.kind === k));

  return (
    <div class="timeline">
      <div class="tl-axis">
        {ticks.map((t, i) =>
          i % step === 0 ? (
            <span key={t} style={{ left: pos(t) }}>
              {fmtTime(t)}
            </span>
          ) : null,
        )}
      </div>
      <div class="tl-row">
        <span class="tl-name">計画</span>
        <div class="tl-lane">
          {blocks.map((b) => (
            <div
              key={b.id}
              class={`tl-item plan${b.focus ? " focus" : ""}`}
              style={{ left: pos(b.start_at), width: width(b.start_at, b.end_at) }}
              title={`${fmtTime(b.start_at)}–${fmtTime(b.end_at)} ${b.task_title ?? b.label ?? ""}${b.focus ? "（集中）" : ""}`}
            >
              <span>{b.task_title ?? b.label ?? ""}</span>
            </div>
          ))}
        </div>
      </div>
      <div class="tl-row">
        <span class="tl-name">実績</span>
        <div class="tl-lane">
          {segments.map((s) => (
            <div
              key={s.id}
              class={`tl-item seg k-${s.kind}${s.status === "needs_review" ? " review" : ""}`}
              style={{ left: pos(s.start_at), width: width(s.start_at, s.end_at) }}
              title={`${fmtTime(s.start_at)}–${fmtTime(s.end_at)} ${KIND_LABEL[s.kind]}${s.task_title ? " / " + s.task_title : ""}${s.exe_name ? " / " + s.exe_name : ""}${s.status === "needs_review" ? "（要確認）" : ""}`}
            />
          ))}
          {uncollected.map((g) => (
            <div
              key={`uncollected-${g.start_at}`}
              class="tl-item uncollected"
              style={{ left: pos(g.start_at), width: width(g.start_at, g.end_at) }}
              title={`${fmtTime(g.start_at)}–${fmtTime(g.end_at)} 記録なし（${fmtDur(g.duration_ms)}。収集が止まっていた時間帯です）`}
            />
          ))}
          {now >= lo && now <= hi && <div class="tl-now" style={{ left: pos(now) }} title="いま" />}
        </div>
      </div>
      <ul class="legend">
        {kinds.map((k) => (
          <li key={k}>
            <i class={`dot k-${k}`} />
            {KIND_LABEL[k]}
          </li>
        ))}
        {segments.some((s) => s.status === "needs_review") && (
          <li>
            <i class="dot review" />
            要確認
          </li>
        )}
        {uncollected.length > 0 && (
          <li>
            <i class="dot uncollected" />
            記録なし
          </li>
        )}
      </ul>
    </div>
  );
}
