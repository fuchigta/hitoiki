import { useEffect, useState } from "preact/hooks";
import { api } from "./api";
import type { CorrectionApplied, CorrectionCandidate, TaskView } from "./api";
import { Card, Empty, ErrorBox } from "./components";
import {
  describeTotals,
  idleIncluded,
  matchesFilter,
  setIdleIncluded,
  sumSelected,
  targetInput,
  targetLabel,
  toggleChecked,
  type CandidateFilter,
  type TargetChoice,
} from "./correct-util";
import { ignorableIds } from "./ignore-util";
import { KIND_LABEL, fmtDur, fmtTime, useAction, useAsync } from "./lib";
import type { TimeSpan } from "./plan-util";

const FILTERS: { id: CandidateFilter; label: string }[] = [
  { id: "unconfirmed", label: "未確定だけ" },
  { id: "no_task", label: "タスクなしだけ" },
];

const CATEGORY_LABEL: Record<CorrectionCandidate["category"], string> = {
  work: "作業",
  agent_dialog: "AI との対話",
  confirmed: "確定済み",
  meeting: "会議",
  idle: "無操作",
  uncollected: "記録なし",
};

function candidateTitle(c: CorrectionCandidate): string {
  if (c.uncollected) return "記録なし（収集が止まっていた時間帯）";
  const parts = [c.kind ? KIND_LABEL[c.kind] : "", c.task_title ?? "", c.activity ?? "", c.exe_name ?? ""];
  return parts.filter(Boolean).join(" / ");
}

interface Props {
  /** 選んだ範囲（「実績」の列でドラッグした範囲）。 */
  span: TimeSpan;
  /** 割り当て先に選べるタスク。 */
  tasks: TaskView[];
  /** 活動の候補。 */
  labels?: string[];
  onClose: () => void;
  /** 補正した（または取り消した）あとに呼ぶ。一覧の再取得に使う。 */
  onChanged: () => void;
}

/** 実績の補正: 範囲に含まれる区間を一覧し、チェックしたものを 1 つのタスクへ付け替える。 */
export function CorrectionPanel(p: Props) {
  const list = useAsync(() => api.correctionCandidates(p.span.start, p.span.end), [p.span.start, p.span.end]);
  // 範囲内で無視にした区間（元に戻せるように並べる）。
  const ignored = useAsync(
    () => api.actualList({ from: new Date(p.span.start).toISOString(), to: new Date(p.span.end).toISOString() }),
    [p.span.start, p.span.end],
  );
  const [cands, setCands] = useState<CorrectionCandidate[]>([]);
  const [filter, setFilter] = useState<CandidateFilter>("all");
  const [choice, setChoice] = useState<TargetChoice>("none");
  const [activity, setActivity] = useState("");
  const [applied, setApplied] = useState<CorrectionApplied | null>(null);
  const act = useAction();

  // 範囲を選び直したら、候補と結果を読み直す。
  useEffect(() => {
    setApplied(null);
    setFilter("all");
  }, [p.span.start, p.span.end]);
  useEffect(() => {
    if (list.data) setCands(list.data.candidates);
  }, [list.data]);

  const task = p.tasks.find((t) => t.id === choice);
  const totals = sumSelected(cands, filter);
  const label = targetLabel(choice, task?.title);
  const needTask = choice !== "none" && choice !== "rest" && !task;
  const visible = cands.filter((c) => matchesFilter(c, filter));
  const hasIdle = cands.some((c) => c.becomes_off_pc);

  const confirm = async () => {
    const selected = cands.filter((c) => c.checked && matchesFilter(c, filter)).map((c) => c.id);
    const r = await act.run(() =>
      api.correctionApply({
        start_at: p.span.start,
        end_at: p.span.end,
        ...targetInput(choice),
        activity: activity.trim() || undefined,
        selected,
      }),
    );
    if (r) {
      setApplied(r);
      p.onChanged();
    }
  };

  // チェックした区間を、付け替えずに無視にする（区間全体が対象）。
  const ignoreChecked = async () => {
    const ids = ignorableIds(cands.filter((c) => matchesFilter(c, filter)));
    if (ids.length === 0) return;
    if (await act.run(() => api.actualIgnore(ids))) {
      list.reload();
      ignored.reload();
      p.onChanged();
    }
  };

  // 無視にした区間を元に戻す。
  const restore = async (id: string) => {
    if (await act.run(() => api.actualUnignore([id]))) {
      list.reload();
      ignored.reload();
      p.onChanged();
    }
  };

  const undo = async () => {
    if (!applied) return;
    const r = await act.run(() => api.correctionUndo(applied.correction.id));
    if (r) {
      setApplied(null);
      list.reload();
      p.onChanged();
    }
  };

  return (
    <Card
      title={`実績の付け替え ${fmtTime(p.span.start)}–${fmtTime(p.span.end)}`}
      aside={
        <button type="button" class="ghost" onClick={p.onClose}>
          閉じる
        </button>
      }
    >
      <ErrorBox error={list.error ?? ignored.error ?? act.error} />
      {applied ? (
        <div class="stack">
          <p role="status">
            <strong>付け替えました。</strong> {applied.preview.summary}
          </p>
          <p class="hint">取り消しは、ここか、タスクの詳細の「補正した時間帯」から行えます。</p>
          <div class="row">
            <button type="button" disabled={act.busy} onClick={undo}>
              取り消す
            </button>
            <button type="button" class="primary" onClick={p.onClose}>
              閉じる
            </button>
          </div>
        </div>
      ) : (
        <div class="stack">
          <div class="row corr-target">
            <label>
              割り当て先{" "}
              <select value={choice} onChange={(e) => setChoice(e.currentTarget.value)}>
                <option value="none">タスクなし</option>
                <option value="rest">休憩</option>
                {p.tasks.map((t) => (
                  <option key={t.id} value={t.id}>
                    {t.title}
                  </option>
                ))}
              </select>
            </label>
            <label>
              活動{" "}
              <input
                type="text"
                list="corr-labels"
                value={activity}
                placeholder="（任意）"
                onInput={(e) => setActivity(e.currentTarget.value)}
              />
              <datalist id="corr-labels">
                {(p.labels ?? []).map((l) => (
                  <option key={l} value={l} />
                ))}
              </datalist>
            </label>
          </div>

          <div class="row corr-filters">
            {FILTERS.map((f) => (
              <button
                key={f.id}
                type="button"
                class={filter === f.id ? "chip on" : "chip"}
                aria-pressed={filter === f.id}
                onClick={() => setFilter(filter === f.id ? "all" : f.id)}
              >
                {f.label}
              </button>
            ))}
            <label class="check">
              <input
                type="checkbox"
                role="switch"
                checked={idleIncluded(cands)}
                disabled={!hasIdle}
                onChange={(e) => setCands(setIdleIncluded(cands, e.currentTarget.checked))}
              />
              無操作・記録なしも含める
            </label>
          </div>

          {list.loading && cands.length === 0 ? (
            <Empty>読み込み中…</Empty>
          ) : visible.length === 0 ? (
            <Empty>{cands.length === 0 ? "範囲に含まれる区間はありません。" : "絞り込みに合う区間はありません。"}</Empty>
          ) : (
            <ul class="plain corr-list">
              {visible.map((c) => (
                <li key={c.id} class={c.checked ? "on" : ""}>
                  <label class="check">
                    <input
                      type="checkbox"
                      checked={c.checked}
                      onChange={() => setCands(toggleChecked(cands, c.id))}
                    />
                    <span class="corr-time">
                      {fmtTime(c.start_at)}–{fmtTime(c.end_at)}
                    </span>
                    <span class="corr-dur">{fmtDur(c.duration_ms)}</span>
                    <span class="corr-what">{candidateTitle(c)}</span>
                  </label>
                  <span class="corr-tags">
                    <span class="tag">{CATEGORY_LABEL[c.category]}</span>
                    {c.becomes_off_pc && <span class="tag">PC 外</span>}
                    {c.full_duration_ms > c.duration_ms && (
                      <span class="meta">（範囲の内側 {fmtDur(c.duration_ms)} / 全体 {fmtDur(c.full_duration_ms)}）</span>
                    )}
                  </span>
                </li>
              ))}
            </ul>
          )}

          <p class="corr-summary" role="status">
            {needTask ? "割り当て先を選んでください。" : describeTotals(label, totals)}
          </p>
          <div class="row">
            <button type="button" class="primary" disabled={act.busy || needTask || totals.selected === 0} onClick={confirm}>
              付け替える
            </button>
            <button
              type="button"
              class="ghost"
              disabled={act.busy || ignorableIds(visible).length === 0}
              onClick={ignoreChecked}
              title="チェックした区間を、タスクに付け替えず数えない時間にします（区間全体）"
            >
              チェックした区間を無視
            </button>
            <button type="button" class="ghost" onClick={p.onClose}>
              やめる
            </button>
          </div>
          {(ignored.data?.segments ?? []).filter((s) => s.kind === "ignored").length > 0 && (
            <details>
              <summary>無視した区間（元に戻せます）</summary>
              <ul class="plain">
                {(ignored.data?.segments ?? [])
                  .filter((s) => s.kind === "ignored")
                  .map((s) => (
                    <li key={s.id} class="row">
                      <span class="corr-time">
                        {fmtTime(s.start_at)}–{fmtTime(s.end_at)}
                      </span>
                      <span class="corr-dur">{fmtDur(s.duration_ms)}</span>
                      <span class="corr-what">{s.exe_name ?? ""}</span>
                      {s.decided_by === "confirmation" ? (
                        <button type="button" class="ghost" disabled={act.busy} onClick={() => restore(s.id)}>
                          元に戻す
                        </button>
                      ) : (
                        <span class="meta">無視リストのアプリ（設定で外せます）</span>
                      )}
                    </li>
                  ))}
              </ul>
            </details>
          )}
        </div>
      )}
    </Card>
  );
}
