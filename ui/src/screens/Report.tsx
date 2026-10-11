import { useState } from "preact/hooks";
import { api, type ReportBy } from "../api";
import { Bars, Card, ErrorBox } from "../components";
import { fmtDur, useAsync, useLiveTick } from "../lib";

const BYS: { id: ReportBy; label: string }[] = [
  { id: "task", label: "タスク" },
  { id: "activity", label: "活動" },
  { id: "kind", label: "種別" },
  { id: "quadrant", label: "象限" },
  { id: "okr", label: "OKR" },
  { id: "skill", label: "スキル" },
  { id: "agent", label: "エージェント" },
];

const PERIODS = [
  { id: "today", label: "今日" },
  { id: "yesterday", label: "昨日" },
  { id: "this-week", label: "今週" },
  { id: "last-week", label: "先週" },
];

export function Report() {
  const [by, setBy] = useState<ReportBy>("task");
  const [period, setPeriod] = useState("this-week");
  const [idle, setIdle] = useState(false);
  const tick = useLiveTick();
  const r = useAsync(() => api.report(by, { date: period }, idle), [by, period, idle, tick]);

  return (
    <div class="stack">
      <div class="row">
        <div class="row">
          {BYS.map((b) => (
            <button key={b.id} class={by === b.id ? "chip on" : "chip"} onClick={() => setBy(b.id)}>
              {b.label}
            </button>
          ))}
        </div>
        <span class="spacer" />
        <select value={period} onChange={(e) => setPeriod(e.currentTarget.value)} aria-label="期間">
          {PERIODS.map((p) => (
            <option key={p.id} value={p.id}>
              {p.label}
            </option>
          ))}
        </select>
        <label class="check">
          <input type="checkbox" checked={idle} onChange={(e) => setIdle(e.currentTarget.checked)} />
          無操作・休憩も含める
        </label>
      </div>
      <ErrorBox error={r.error} />
      {r.data && (
        <Card title={`合計 ${fmtDur(r.data.total_ms)}`}>
          <Bars rows={r.data.rows} total={r.data.total_ms} />
          {r.data.multi_attribution && (
            <p class="muted">複数のスキルが付いた時間は、それぞれに全時間を数えています。</p>
          )}
          {!r.data.include_idle && (r.data.excluded.idle_ms > 0 || r.data.excluded.break_ms > 0) && (
            <p class="muted">
              別枠: 無操作 {fmtDur(r.data.excluded.idle_ms)} ・ 休憩 {fmtDur(r.data.excluded.break_ms)}
            </p>
          )}
          {r.data.excluded.ignored_ms > 0 && (
            <p class="muted">無視した区間: {fmtDur(r.data.excluded.ignored_ms)}（合計には含めません）</p>
          )}
          {r.data.note && <p class="muted">{r.data.note}</p>}
        </Card>
      )}
    </div>
  );
}
