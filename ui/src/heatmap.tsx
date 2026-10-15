import { useState } from "preact/hooks";
import { api, type SegmentKind } from "./api";
import { Card, Empty, ErrorBox } from "./components";
import { Help } from "./ui/parts";
import { buildBy, buildWeeks, cellTitle, fmtHeatValue, monthLabels, WEEKDAY_LABELS, type HeatBy } from "./heatmap-util";
import { keyResultLabel, keyResultOptions } from "./key-result-util";
import { KIND_LABEL, KIND_ORDER, useAsync, useLiveTick } from "./lib";

const BYS: { id: HeatBy; label: string }[] = [
  { id: "time", label: "時間" },
  { id: "kind", label: "種別" },
  { id: "task", label: "タスク" },
  { id: "skill", label: "スキル" },
  { id: "key_result", label: "成果の指標" },
  { id: "records", label: "記録" },
];

const RECORD_KINDS = [
  { id: "", label: "すべて" },
  { id: "checkin", label: "調子" },
  { id: "hassle", label: "気になったこと" },
  { id: "abc", label: "捉え直し" },
  { id: "reflection", label: "振り返り" },
];

/** 活動量ヒートマップ（直近 53 週）。濃淡は値のある日の分布に対する相対的な段階で、良し悪しは表さない。 */
export function HeatmapCard() {
  const [by, setBy] = useState<HeatBy>("time");
  const [arg, setArg] = useState("");
  const [picked, setPicked] = useState<string | null>(null);
  const tick = useLiveTick();
  const tasks = useAsync(() => api.taskList(), []);
  const skills = useAsync(() => api.skillList(), []);
  const goals = useAsync(() => api.goalList(), []);
  const keyResults = keyResultOptions(goals.data?.goals ?? []);
  const query = buildBy(by, arg);
  const hm = useAsync(
    () => (query ? api.heatmap(query) : Promise.resolve(null)),
    [query, tick],
  );

  const choose = (b: HeatBy) => {
    setBy(b);
    setArg(b === "kind" ? "work" : "");
    setPicked(null);
  };

  const data = hm.data;
  const weeks = data ? buildWeeks(data.days, data.from_weekday) : [];
  const months = monthLabels(weeks);
  const selected = data?.days.find((d) => d.date === picked);

  return (
    <Card title="活動量">
      <div class="row">
        <div class="row">
          {BYS.map((b) => (
            <button key={b.id} class={by === b.id ? "chip on" : "chip"} onClick={() => choose(b.id)}>
              {b.label}
            </button>
          ))}
        </div>
        {by === "kind" && (
          <select value={arg} onChange={(e) => setArg(e.currentTarget.value)} aria-label="区間の種別">
            {KIND_ORDER.map((k: SegmentKind) => (
              <option key={k} value={k}>
                {KIND_LABEL[k]}
              </option>
            ))}
          </select>
        )}
        {by === "task" && (
          <select value={arg} onChange={(e) => setArg(e.currentTarget.value)} aria-label="タスク">
            <option value="">タスクを選ぶ</option>
            {(tasks.data?.tasks ?? []).map((t) => (
              <option key={t.id} value={t.id}>
                {t.title}
              </option>
            ))}
          </select>
        )}
        {by === "skill" && (
          <select value={arg} onChange={(e) => setArg(e.currentTarget.value)} aria-label="スキル">
            <option value="">スキルを選ぶ</option>
            {(skills.data?.skills ?? []).map((s) => (
              <option key={s.id} value={s.name}>
                {s.name}
              </option>
            ))}
          </select>
        )}
        {by === "key_result" && (
          <select value={arg} onChange={(e) => setArg(e.currentTarget.value)} aria-label="成果の指標">
            <option value="">成果の指標を選ぶ</option>
            {keyResults.map((k) => (
              <option key={k.id} value={k.id}>
                {keyResultLabel(k)}
              </option>
            ))}
          </select>
        )}
        {by === "records" && (
          <select value={arg} onChange={(e) => setArg(e.currentTarget.value)} aria-label="記録の種類">
            {RECORD_KINDS.map((k) => (
              <option key={k.id} value={k.id}>
                {k.label}
              </option>
            ))}
          </select>
        )}
      </div>
      <ErrorBox error={hm.error} />
      {!query && <Empty>{by === "task" ? "タスク" : by === "key_result" ? "成果の指標" : "スキル"}を選ぶと表示します。</Empty>}
      {data && query && (
        <>
          <div class="heatmap-scroll">
            <div class="heatmap" role="img" aria-label={`${data.label}の日別ヒートマップ`}>
              <div class="hm-months" style={{ gridTemplateColumns: `repeat(${weeks.length}, 1fr)` }}>
                {months.map((m) => (
                  <span key={m.week} style={{ gridColumn: m.week + 1 }}>
                    {m.label}
                  </span>
                ))}
              </div>
              <div class="hm-body">
                <div class="hm-wd">
                  {WEEKDAY_LABELS.map((l, i) => (
                    <span key={i}>{l}</span>
                  ))}
                </div>
                <div class="hm-grid" style={{ gridTemplateColumns: `repeat(${weeks.length}, 1fr)` }}>
                  {weeks.map((w, wi) => (
                    <div class="hm-col" key={wi}>
                      {w.map((d, di) =>
                        d ? (
                          <button
                            key={di}
                            class={`hm-cell hm-l${d.level}${picked === d.date ? " sel" : ""}`}
                            title={cellTitle(d, data.unit)}
                            aria-label={cellTitle(d, data.unit)}
                            onClick={() => setPicked(picked === d.date ? null : d.date)}
                          />
                        ) : (
                          <span key={di} class="hm-cell hm-none" />
                        ),
                      )}
                    </div>
                  ))}
                </div>
              </div>
            </div>
          </div>
          <div class="hm-legend">
            <span class="meta">少</span>
            {[0, 1, 2, 3, 4].map((l) => (
              <span key={l} class={`hm-cell hm-l${l}`} />
            ))}
            <span class="meta">多</span>
            <Help>濃さは、値のある日の分布に対する相対的な段階です。</Help>
          </div>
          <p class="meta">
            {data.label}: 合計 {fmtHeatValue(data.unit, data.total)} ・ 値のあった日 {data.active_days} 日
            {selected && <> ・ 選択: {cellTitle(selected, data.unit)}</>}
          </p>
        </>
      )}
    </Card>
  );
}
