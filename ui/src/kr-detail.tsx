import { useState } from "preact/hooks";
import { api, type KeyResult, type KrSummary } from "./api";
import { ErrorBox } from "./components";
import {
  barHeights,
  linkCandidates,
  rangeSourceLabel,
  relinkCount,
  splitMeasures,
  weekLabel,
} from "./kr-util";
import { STATUS_LABEL, fmtDur, useAction, useAsync, useLiveTick } from "./lib";

/**
 * KR のカードの中身: 選んだ軸の値（大きく）と補助の値、週ごとの時間、スキル別、見積と実績、紐付いたタスク。
 * 目標・現在値は自由記述のまま KR の行に並べるだけで、達成率の計算や評価はしない。
 */
export function KrDetail({ k, onChanged }: { k: KeyResult; onChanged: () => void }) {
  const tick = useLiveTick(["segments-updated"]);
  const sum = useAsync(() => api.okrShow(k.id), [k.id, k.measure, k.day_min, tick]);
  const s = sum.data;
  return (
    <div class="kr-detail">
      <ErrorBox error={sum.error} />
      {s && <Summary s={s} />}
      <KrTasks
        k={k}
        summary={s ?? null}
        onChanged={() => {
          sum.reload();
          onChanged();
        }}
      />
    </div>
  );
}

function Summary({ s }: { s: KrSummary }) {
  const { main, aux } = splitMeasures(s);
  const heights = barHeights(s.weekly);
  const hasTime = s.time_ms > 0;
  return (
    <div class="stack">
      <div class="kr-main-row">
        <div>
          <div class="kr-main" aria-label={main.label}>
            {main.text}
          </div>
          <div class="muted kr-main-label">{main.label}</div>
        </div>
        <div class="kr-aux muted">
          {aux.map((a) => (
            <span key={a.measure}>
              {a.label}: {a.text}
            </span>
          ))}
        </div>
      </div>
      <p class="muted kr-range">
        期間: {s.range.start.slice(0, 10)} 〜 {s.range.end.slice(0, 10)}（{rangeSourceLabel(s.range_source)}）
      </p>
      {hasTime && (
        <>
          <div class="kr-bars" role="img" aria-label="週ごとの時間">
            {s.weekly.map((w, i) => (
              <div class="kr-bar" key={w.week_start} title={`${w.week_start} の週: ${fmtDur(w.duration_ms)}`}>
                <div class="kr-bar-track">
                  <div class="kr-bar-fill" style={{ height: `${heights[i]}%` }} />
                </div>
                <span class="kr-bar-label">{weekLabel(w.week_start)}</span>
              </div>
            ))}
          </div>
          {s.skills.length > 0 && (
            <p class="muted kr-skills">
              スキル別:{" "}
              {s.skills.map((sk, i) => (
                <span key={sk.name}>
                  {i > 0 && "、"}
                  {sk.name} {fmtDur(sk.duration_ms)}
                </span>
              ))}
              {s.skills.length > 1 && "（複数のスキルが付いたタスクは各スキルに全時間を計上）"}
            </p>
          )}
        </>
      )}
      {s.estimate.estimated_tasks > 0 && (
        <p class="muted">
          見積の合計 {fmtDur(s.estimate.estimate_min_total * 60_000)}（{s.estimate.estimated_tasks} 件）・
          そのタスクの期間内の実績 {fmtDur(s.estimate.actual_ms_of_estimated)}
        </p>
      )}
    </div>
  );
}

/** 紐付いたタスクの一覧（個別に外せる）と、タスクの追加・既存タスクの紐付け。 */
function KrTasks({
  k,
  summary,
  onChanged,
}: {
  k: KeyResult;
  summary: KrSummary | null;
  onChanged: () => void;
}) {
  const [title, setTitle] = useState("");
  const [picking, setPicking] = useState(false);
  const act = useAction();
  const tasks = summary?.tasks ?? [];

  const add = async (e: Event) => {
    e.preventDefault();
    const t = title.trim();
    if (!t) return;
    const r = await act.run(() => api.taskAdd({ title: t, key_result: k.id }));
    if (r) {
      setTitle("");
      onChanged();
    }
  };
  const unlink = async (id: string) => {
    const r = await act.run(() => api.okrUnlink([id]));
    if (r) onChanged();
  };

  return (
    <div class="stack kr-tasks">
      {tasks.length > 0 && (
        <ul class="plain">
          {tasks.map((t) => (
            <li key={t.id}>
              <span class="row">
                <span class="grow">
                  {t.title}
                  <span class="muted">
                    {" "}
                    （{STATUS_LABEL[t.status] ?? t.status}
                    {t.tracked_ms > 0 ? `・${fmtDur(t.tracked_ms)}` : ""}）
                  </span>
                </span>
                <button
                  type="button"
                  class="ghost"
                  disabled={act.busy}
                  aria-label={`タスク「${t.title}」を KR から外す`}
                  onClick={() => unlink(t.id)}
                >
                  外す
                </button>
              </span>
            </li>
          ))}
        </ul>
      )}
      {summary && tasks.length === 0 && <p class="muted">紐付いたタスクはまだありません。</p>}
      <form class="row" onSubmit={add}>
        <input
          class="grow"
          value={title}
          onInput={(e) => setTitle(e.currentTarget.value)}
          placeholder="この KR のタスクを追加（タイトル）"
          aria-label="この KR のタスクを追加"
        />
        <button disabled={act.busy || !title.trim()}>追加</button>
        <button type="button" class="ghost" onClick={() => setPicking((v) => !v)} aria-expanded={picking}>
          既存のタスクを紐づける
        </button>
      </form>
      {picking && (
        <LinkPicker
          k={k}
          onDone={() => {
            setPicking(false);
            onChanged();
          }}
          onCancel={() => setPicking(false)}
        />
      )}
      <ErrorBox error={act.error} />
    </div>
  );
}

/** 未完了のタスクから複数選んで紐付ける。他の KR に紐付いているものは、付け替えになることを示す。 */
function LinkPicker({ k, onDone, onCancel }: { k: KeyResult; onDone: () => void; onCancel: () => void }) {
  const all = useAsync(() => api.taskList(["todo", "doing"]), []);
  const [query, setQuery] = useState("");
  const [sel, setSel] = useState<Set<string>>(new Set());
  const act = useAction();
  const cands = linkCandidates(all.data?.tasks ?? [], k, query);
  const relinks = relinkCount(cands, sel);

  const toggle = (id: string) =>
    setSel((s) => {
      const n = new Set(s);
      if (n.has(id)) n.delete(id);
      else n.add(id);
      return n;
    });
  const submit = async () => {
    const r = await act.run(() => api.okrLink(k.id, [...sel]));
    if (r) onDone();
  };

  return (
    <div class="stack kr-link-picker">
      <ErrorBox error={all.error} />
      <input
        value={query}
        onInput={(e) => setQuery(e.currentTarget.value)}
        placeholder="タスクを絞り込む"
        aria-label="紐付けるタスクを絞り込む"
      />
      {cands.length === 0 && <p class="muted">紐付けられる未完了のタスクがありません。</p>}
      <ul class="plain kr-cands">
        {cands.map((c) => (
          <li key={c.task.id}>
            <label class="check">
              <input type="checkbox" checked={sel.has(c.task.id)} onChange={() => toggle(c.task.id)} />
              {c.task.title}
              {c.fromKr && <span class="muted">（KR「{c.fromKr}」に紐付いています。選ぶと付け替えます）</span>}
            </label>
          </li>
        ))}
      </ul>
      <div class="row">
        {relinks > 0 && <span class="muted note">{relinks} 件は別の KR からの付け替えになります。</span>}
        <span class="spacer" />
        <button type="button" class="ghost" onClick={onCancel}>
          キャンセル
        </button>
        <button type="button" class="primary" disabled={act.busy || sel.size === 0} onClick={submit}>
          {sel.size} 件を紐づける
        </button>
      </div>
      <ErrorBox error={act.error} />
    </div>
  );
}
