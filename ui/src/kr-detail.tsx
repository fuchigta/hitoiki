import { useState } from "preact/hooks";
import { api, type KeyResult, type KrSummary } from "./api";
import { ErrorBox } from "./components";
import { Help, OverflowMenu } from "./ui/parts";
import { KrHistory } from "./kr-history";
import { SkillPicker } from "./skill-picker";
import {
  barHeights,
  breakdownText,
  linkCandidates,
  rangeSourceLabel,
  relinkCount,
  splitMeasures,
  weekLabel,
} from "./kr-util";
import { aiNote, candidateRef, kindLabel, sourceLabel, wantsSuggestion } from "./kr-suggest-util";
import { STATUS_LABEL, fmtDur, useAction, useAsync, useLiveTick } from "./lib";

/**
 * KR のカードの中身: 選んだ軸の値（大きく）と補助の値、週ごとの時間、スキル別、見積と実績、紐付いたタスク。
 * 目標・現在値は自由記述のまま KR の行に並べるだけで、達成率の計算や評価はしない。
 */
export function KrDetail({ k, onChanged }: { k: KeyResult; onChanged: () => void }) {
  const tick = useLiveTick(["segments-updated"]);
  const sum = useAsync(() => api.okrShow(k.id), [k.id, k.measure, k.day_min, tick]);
  const s = sum.data;
  const reload = () => {
    sum.reload();
    onChanged();
  };
  const skills = <KrSkills k={k} names={s?.kr_skills ?? []} onChanged={reload} />;
  const tasks = <KrTasks k={k} summary={s ?? null} onChanged={reload} />;
  return (
    <div class="kr-detail">
      <ErrorBox error={sum.error} />
      {s && <SummaryHead s={s} />}
      {/* 要点（主軸の値）だけ常時表示し、週別・履歴・提案・スキル・タスクは開いたときに出す。 */}
      <details class="kr-more">
        <summary>詳細を開く</summary>
        <div class="stack kr-more-body">
          {s && <SummaryBody s={s} />}
          <KrHistory k={k} onChanged={onChanged} />
          {wantsSuggestion(s) && <KrSuggest k={k} onChanged={reload} />}
          {/* スキルが主。完了数の軸のときだけ、完了数の対象である紐付けたタスクを先に出す。 */}
          {k.measure === "done" ? (
            <>
              {tasks}
              {skills}
            </>
          ) : (
            <>
              {skills}
              {tasks}
            </>
          )}
        </div>
      </details>
    </div>
  );
}

/** 常時表示する要点: 選んだ軸の値と、補助の値。 */
function SummaryHead({ s }: { s: KrSummary }) {
  const { main, aux } = splitMeasures(s);
  const breakdown = breakdownText(s);
  return (
    <div class="kr-main-row">
      <div>
        <div class="kr-main num" aria-label={main.label}>
          {main.text}
        </div>
        <div class="meta kr-main-label">{main.label}</div>
        {breakdown && main.measure !== "done" && <div class="meta kr-indirect">{breakdown}</div>}
      </div>
      <div class="kr-aux meta">
        {aux.map((a) => (
          <span key={a.measure}>
            {a.label}: {a.text}
          </span>
        ))}
      </div>
    </div>
  );
}

/** 開いたときに出す内訳: 期間、週ごとの時間、スキル別、見積と実績。 */
function SummaryBody({ s }: { s: KrSummary }) {
  const heights = barHeights(s.weekly);
  const hasTime = s.time_ms > 0;
  return (
    <div class="stack">
      <p class="meta kr-range">
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
            <p class="meta kr-skills">
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
        <p class="meta">
          見積の合計 {fmtDur(s.estimate.estimate_min_total * 60_000)}（{s.estimate.estimated_tasks} 件）・
          そのタスクの期間内の実績 {fmtDur(s.estimate.actual_ms_of_estimated)}
        </p>
      )}
    </div>
  );
}

/**
 * 紐づけの提案: スキルも紐付けたタスクも無い KR に、名前の近いタスク・スキルを候補として出す。
 * 自動では紐付けない。「付ける」を選んだものだけ紐付け、「付けない」を選んだものは以後出さない。
 * 最初は手元の候補だけを出し、「AI にも聞く」を押したときだけ AI に問い合わせる
 * （送るのは KR・Objective の名前とタスク・スキルの名前だけ。AI が設定されているときのみ）。
 */
function KrSuggest({ k, onChanged }: { k: KeyResult; onChanged: () => void }) {
  const [askAi, setAskAi] = useState(false);
  const [hidden, setHidden] = useState<Set<string>>(new Set());
  const sug = useAsync(() => api.okrSuggest(k.id, askAi), [k.id, askAi]);
  const act = useAction();
  const r = sug.data;
  const shown = (r?.candidates ?? []).filter((c) => !hidden.has(candidateRef(c)));

  const accept = async (ref: string) => {
    const d = await act.run(() => api.okrSuggestAccept(k.id, [ref]));
    if (d) onChanged();
  };
  const reject = async (ref: string) => {
    const d = await act.run(() => api.okrSuggestReject(k.id, [ref]));
    if (d) setHidden((h) => new Set(h).add(ref));
  };

  // 対象外（すでにスキルか紐付けたタスクがある）や、まだ読み込み中で何も無いときは何も出さない。
  if (r && !r.eligible) return null;
  return (
    <div class="stack kr-suggest">
      <h3>
        紐づけの候補{" "}
        <Help>名前の近いタスク・スキルです。自動では紐付けません。「付けない」を選ぶと、以後この候補は出しません。</Help>
      </h3>
      {r && shown.length === 0 && <p class="hint">候補は見つかりませんでした。</p>}
      <ul class="plain">
        {shown.map((c) => {
          const ref = candidateRef(c);
          return (
            <li key={ref}>
              <span class="row">
                <span class="grow">
                  {c.name}
                  <span class="meta">
                    {" "}
                    （{kindLabel(c.kind)}・{sourceLabel(c.source)}）
                  </span>
                </span>
                <button
                  type="button"
                  disabled={act.busy}
                  aria-label={`${kindLabel(c.kind)}「${c.name}」を KR に付ける`}
                  onClick={() => accept(ref)}
                >
                  付ける
                </button>
                <button
                  type="button"
                  class="ghost"
                  disabled={act.busy}
                  aria-label={`${kindLabel(c.kind)}「${c.name}」は KR に付けない`}
                  onClick={() => reject(ref)}
                >
                  付けない
                </button>
              </span>
            </li>
          );
        })}
      </ul>
      <div class="row">
        {r && aiNote(r, askAi) && <span class="meta note">{aiNote(r, askAi)}</span>}
        <span class="spacer" />
        {!askAi && (
          <button type="button" class="ghost" onClick={() => setAskAi(true)}>
            AI にも聞く
          </button>
        )}
      </div>
      <ErrorBox error={act.error ?? sug.error} />
    </div>
  );
}

/**
 * KR に付けるスキル（主）。このスキル（子スキルを含む）が付いたタスク・活動の時間が、この KR の時間になる。
 * 変更はその場で保存する。
 */
function KrSkills({ k, names, onChanged }: { k: KeyResult; names: string[]; onChanged: () => void }) {
  const known = useAsync(() => api.skillList(), []);
  const act = useAction();
  const change = async (next: string[]) => {
    const r = await act.run(() => api.okrEditKr(k.id, { skills: next, clear_skills: next.length === 0 }));
    if (r) onChanged();
  };
  return (
    <div class="field kr-skill-field">
      <h3>
        KR のスキル{" "}
        <Help>
          {k.measure === "done"
            ? "完了数の対象は紐付けたタスクです。スキルは時間・日数に使います。"
            : "このスキルが付いたタスク・活動の時間をこの KR の時間として数えます。"}
        </Help>
      </h3>
      <SkillPicker
        value={names}
        skills={known.data?.skills ?? []}
        onChange={change}
        ariaLabel={`KR「${k.title}」のスキルを追加`}
        disabled={act.busy}
      />
      <ErrorBox error={act.error ?? known.error} />
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
      <h3>
        紐付けたタスク{" "}
        <Help>
          {k.measure === "done"
            ? "完了数の対象です。"
            : "補助です。スキルが付いていなくても、この KR の時間に含めます。"}
        </Help>
      </h3>
      {tasks.length > 0 && (
        <ul class="plain">
          {tasks.map((t) => (
            <li key={t.id}>
              <span class="row">
                <span class="grow">
                  {t.title}
                  <span class="meta">
                    {" "}
                    （{STATUS_LABEL[t.status] ?? t.status}
                    {t.tracked_ms > 0 ? `・${fmtDur(t.tracked_ms)}` : ""}）
                  </span>
                </span>
                <OverflowMenu
                  label={`タスク「${t.title}」の操作`}
                  items={[{ label: "KR から外す", onSelect: () => unlink(t.id), disabled: act.busy }]}
                />
              </span>
            </li>
          ))}
        </ul>
      )}
      {summary && tasks.length === 0 && <p class="hint">紐付いたタスクはまだありません。</p>}
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
      {cands.length === 0 && <p class="hint">紐付けられる未完了のタスクがありません。</p>}
      <ul class="plain kr-cands">
        {cands.map((c) => (
          <li key={c.task.id}>
            <label class="check">
              <input type="checkbox" checked={sel.has(c.task.id)} onChange={() => toggle(c.task.id)} />
              {c.task.title}
              {c.fromKr && <span class="meta">（KR「{c.fromKr}」に紐付いています。選ぶと付け替えます）</span>}
            </label>
          </li>
        ))}
      </ul>
      <div class="row">
        {relinks > 0 && <span class="meta note">{relinks} 件は別の KR からの付け替えになります。</span>}
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
