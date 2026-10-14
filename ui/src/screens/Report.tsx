import { useState } from "preact/hooks";
import { api, type ReportBy } from "../api";
import { Bars, Card, ErrorBox } from "../components";
import { HeatmapCard } from "../heatmap";
import { Help, PeriodSelect } from "../ui/parts";
import { fmtDur, useAsync, useLiveTick } from "../lib";

const BYS: { id: ReportBy; label: string }[] = [
  { id: "task", label: "タスク" },
  { id: "activity", label: "活動" },
  { id: "kind", label: "種別" },
  { id: "quadrant", label: "緊急・重要" },
  { id: "okr", label: "OKR" },
  { id: "skill", label: "スキル" },
  { id: "agent", label: "AI エージェント" },
];

/** 内訳によっては意味が伝わりにくいので、選んだときだけ「?」で説明を添える。 */
const BY_HELP: Partial<Record<ReportBy, string>> = {
  quadrant:
    "タスクの「緊急かどうか・重要かどうか」で時間を 4 つに分けます（アイゼンハワー・マトリクス）。緊急・重要のどちらかが未設定のタスクは「未設定」に入ります。",
  agent: "Claude Code や Codex などの AI エージェントを使っていた時間です。AI との対話（指示を書いている時間）と AI の作業待ち（返答や作業が終わるのを待っている時間）に分かれます。",
};

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
        {BY_HELP[by] && <Help label={`${BYS.find((b) => b.id === by)?.label}の説明`}>{BY_HELP[by]}</Help>}
        <span class="spacer" />
        <PeriodSelect value={period} onChange={setPeriod} />
        <label class="check">
          <input type="checkbox" checked={idle} onChange={(e) => setIdle(e.currentTarget.checked)} />
          無操作・休憩も含める
        </label>
      </div>
      <HeatmapCard />
      <ErrorBox error={r.error} />
      {r.data && (
        <Card title={`合計 ${fmtDur(r.data.total_ms)}`}>
          <Bars rows={r.data.rows} total={r.data.total_ms} />
          {r.data.multi_attribution && (
            <p class="meta">複数のスキルが付いた時間は、それぞれに全時間を数えています。</p>
          )}
          {!r.data.include_idle && (r.data.excluded.idle_ms > 0 || r.data.excluded.break_ms > 0) && (
            <p class="meta">
              別枠: 無操作 {fmtDur(r.data.excluded.idle_ms)} ・ 休憩 {fmtDur(r.data.excluded.break_ms)}
            </p>
          )}
          {r.data.excluded.ignored_ms > 0 && (
            <p class="meta">無視した区間: {fmtDur(r.data.excluded.ignored_ms)}（合計には含めません）</p>
          )}
          {r.data.note && <p class="hint">{r.data.note}</p>}
        </Card>
      )}
    </div>
  );
}
