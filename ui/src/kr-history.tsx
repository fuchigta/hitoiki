import { useState } from "preact/hooks";
import { api, type KeyResult, type KrHistory as KrHistoryData } from "./api";
import { ErrorBox } from "./components";
import { isEmptyHistory, valueLevels, valueSegments, weekValueText } from "./kr-history-util";
import { barHeights, weekLabel } from "./kr-util";
import { fmtDateTime, fmtDur, useAction, useAsync, useLiveTick } from "./lib";

const COL_W = 24;
const CHART_H = 72;

/**
 * KR の現在値の履歴: 値の推移（折れ線）と週ごとの時間（棒）を同じ週の軸で並べ、値を記録するフォームを添える。
 * 目標との比較や評価はしない。並べるだけ。
 */
export function KrHistory({ k, onChanged }: { k: KeyResult; onChanged: () => void }) {
  const tick = useLiveTick(["segments-updated"]);
  const hist = useAsync(() => api.okrHistory(k.id), [k.id, k.current, tick]);
  const h = hist.data;
  return (
    <details class="kr-history">
      <summary>値の履歴</summary>
      <div class="stack">
        <ErrorBox error={hist.error} />
        {h && <HistoryChart h={h} />}
        <ProgressForm
          k={k}
          onDone={() => {
            hist.reload();
            onChanged();
          }}
        />
        {h && <EntryList h={h} />}
      </div>
    </details>
  );
}

function HistoryChart({ h }: { h: KrHistoryData }) {
  if (isEmptyHistory(h.weeks)) {
    return <p class="muted">値の記録も、紐付いたタスクの時間も、この期間にはまだありません。</p>;
  }
  const heights = barHeights(h.weeks);
  const segs = valueSegments(valueLevels(h.weeks), COL_W, CHART_H);
  const width = h.weeks.length * COL_W;
  return (
    <div class="stack">
      <svg
        class="kr-history-chart"
        viewBox={`0 0 ${width} ${CHART_H}`}
        role="img"
        aria-label="週ごとの時間（棒）と現在値の推移（折れ線）"
      >
        {h.weeks.map((w, i) => {
          const bh = (heights[i] / 100) * (CHART_H - 8);
          return (
            <rect
              key={w.week_start}
              class="kr-history-bar"
              x={i * COL_W + 4}
              y={CHART_H - bh}
              width={COL_W - 8}
              height={bh}
            >
              <title>{`${w.week_start} の週: ${fmtDur(w.duration_ms)}`}</title>
            </rect>
          );
        })}
        {segs.map((pts, i) => (
          <polyline
            key={i}
            class="kr-history-line"
            fill="none"
            points={pts.map((p) => `${p.x},${p.y}`).join(" ")}
          />
        ))}
        {segs.flat().map((p) => (
          <circle key={`${p.x}`} class="kr-history-dot" cx={p.x} cy={p.y} r={2.5} />
        ))}
      </svg>
      <table class="kr-history-table">
        <thead>
          <tr>
            <th>週</th>
            <th>現在値</th>
            <th>時間</th>
          </tr>
        </thead>
        <tbody>
          {h.weeks.map((w) => (
            <tr key={w.week_start}>
              <td>{weekLabel(w.week_start)}</td>
              <td>{weekValueText(w)}</td>
              <td>{w.duration_ms > 0 ? fmtDur(w.duration_ms) : "-"}</td>
            </tr>
          ))}
        </tbody>
      </table>
      <p class="muted kr-range">
        棒は週ごとの時間、線は各週の終わり時点の現在値（数として読める値だけ。最小〜最大を縦に伸ばして表示）。
      </p>
    </div>
  );
}

function ProgressForm({ k, onDone }: { k: KeyResult; onDone: () => void }) {
  const [value, setValue] = useState("");
  const [note, setNote] = useState("");
  const act = useAction();
  const submit = async (e: Event) => {
    e.preventDefault();
    const v = value.trim();
    if (!v) return;
    const r = await act.run(() => api.okrProgress(k.id, v, note.trim() || undefined));
    if (r) {
      setValue("");
      setNote("");
      onDone();
    }
  };
  return (
    <form class="row" onSubmit={submit}>
      <input
        class="narrow"
        value={value}
        onInput={(e) => setValue(e.currentTarget.value)}
        placeholder={k.current ? `現在値（今は ${k.current}）` : "現在値"}
        aria-label="現在値を記録"
      />
      <input
        class="grow"
        value={note}
        onInput={(e) => setNote(e.currentTarget.value)}
        placeholder="メモ（任意）"
        aria-label="現在値に添えるメモ"
      />
      <button disabled={act.busy || !value.trim()}>記録</button>
      <ErrorBox error={act.error} />
    </form>
  );
}

function EntryList({ h }: { h: KrHistoryData }) {
  if (h.entries.length === 0) return <p class="muted">値の記録はまだありません。</p>;
  return (
    <ul class="plain kr-history-entries">
      {[...h.entries].reverse().map((e) => (
        <li key={e.id}>
          <span class="muted">{fmtDateTime(e.at)}</span> {e.text ?? "（消した）"}
          {e.note && <span class="muted">　{e.note}</span>}
        </li>
      ))}
    </ul>
  );
}
