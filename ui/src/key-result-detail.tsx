import { useState } from "preact/hooks";
import { api, type KeyResult, type KeyResultSummary } from "./api";
import { ErrorBox, Section } from "./components";
import { OverflowMenu } from "./ui/parts";
import { KeyResultHistory } from "./key-result-history";
import { SkillPicker } from "./skill-picker";
import {
  barHeights,
  breakdownText,
  linkCandidates,
  rangeSourceLabel,
  relinkCount,
  splitMeasures,
  weekLabel,
} from "./key-result-util";
import { aiNote, candidateRef, kindLabel, sourceLabel, wantsSuggestion } from "./key-result-suggest-util";
import { STATUS_LABEL, fmtDur, hrefOf, useAction, useAsync, useLiveTick } from "./lib";

/**
 * 成果の指標のカードの中身: 選んだ数え方の値（大きく）と補助の値、週ごとの時間、スキル別、見積と実績、紐付いたタスク。
 * 目標値・現在値は自由記述のまま成果の指標の行に並べるだけで、達成率の計算や評価はしない。
 */
export function KeyResultDetail({ k, onChanged }: { k: KeyResult; onChanged: () => void }) {
  const tick = useLiveTick(["segments-updated"]);
  const sum = useAsync(() => api.keyResultShow(k.id), [k.id, k.measure, k.active_day_min, tick]);
  const s = sum.data;
  const reload = () => {
    sum.reload();
    onChanged();
  };
  const skills = <KeyResultSkills k={k} names={s?.key_result_skills ?? []} onChanged={reload} />;
  const tasks = <KeyResultTasks k={k} summary={s ?? null} onChanged={reload} />;
  return (
    <div class="stack key-result-detail">
      <ErrorBox error={sum.error} />
      {/* 集計 → 週別 → スキル別 → 値の履歴 → スキル → タスク → 提案。区画ごとに Section で区切る。 */}
      {s && <SummarySections s={s} />}
      <KeyResultHistory k={k} onChanged={onChanged} />
      {/* スキルが主。完了したタスク数の数え方のときだけ、その対象である紐付けたタスクを先に出す。 */}
      {k.measure === "done_tasks" ? (
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
      {wantsSuggestion(s) && <KeyResultSuggest k={k} onChanged={reload} />}
    </div>
  );
}

/** 集計: 選んだ数え方の値（大きく）と、補助の値。期間も添える。 */
function SummaryHead({ s }: { s: KeyResultSummary }) {
  const { main, aux } = splitMeasures(s);
  const breakdown = breakdownText(s);
  return (
    <div class="stack">
      <div class="key-result-main-row">
        <div>
          <div class="key-result-main num" aria-label={main.label}>
            {main.text}
          </div>
          <div class="meta key-result-main-label">{main.label}</div>
          {breakdown && main.measure !== "done_tasks" && <div class="meta key-result-indirect">{breakdown}</div>}
        </div>
        <div class="key-result-aux meta">
          {aux.map((a) => (
            <span key={a.measure}>
              {a.label}: {a.text}
            </span>
          ))}
        </div>
      </div>
      <p class="meta key-result-range">
        期間: {s.range.start.slice(0, 10)} 〜 {s.range.end.slice(0, 10)}（{rangeSourceLabel(s.range_source)}）
      </p>
    </div>
  );
}

/** 集計・週別の時間・スキル別・見積と実績の各区画。 */
function SummarySections({ s }: { s: KeyResultSummary }) {
  const heights = barHeights(s.weekly);
  const hasTime = s.time_ms > 0;
  return (
    <>
      <Section level={3} title="集計">
        <SummaryHead s={s} />
      </Section>
      {hasTime && (
        <Section level={3} title="週別の時間">
          <div class="key-result-bars" role="img" aria-label="週ごとの時間">
            {s.weekly.map((w, i) => (
              <div class="key-result-bar" key={w.week_start} title={`${w.week_start} の週: ${fmtDur(w.duration_ms)}`}>
                <div class="key-result-bar-track">
                  <div class="key-result-bar-fill" style={{ height: `${heights[i]}%` }} />
                </div>
                <span class="key-result-bar-label">{weekLabel(w.week_start)}</span>
              </div>
            ))}
          </div>
        </Section>
      )}
      {hasTime && s.skills.length > 0 && (
        <Section level={3} title="スキル別">
          <p class="meta key-result-skills">
            {s.skills.map((sk, i) => (
              <span key={sk.name}>
                {i > 0 && "、"}
                {sk.name} {fmtDur(sk.duration_ms)}
              </span>
            ))}
            {s.skills.length > 1 && "（複数のスキルが付いたタスクは各スキルに全時間を計上）"}
          </p>
        </Section>
      )}
      {s.estimate.estimated_tasks > 0 && (
        <Section level={3} title="見積と実績">
          <p class="meta">
            見積の合計 {fmtDur(s.estimate.estimate_min_total * 60_000)}（{s.estimate.estimated_tasks} 件）・
            そのタスクの期間内の実績 {fmtDur(s.estimate.actual_ms_of_estimated)}
          </p>
        </Section>
      )}
    </>
  );
}

/**
 * 紐づけの提案: スキルも紐付けたタスクも無い成果の指標に、名前の近いタスク・スキルを候補として出す。
 * 自動では紐付けない。「付ける」を選んだものだけ紐付け、「付けない」を選んだものは以後出さない。
 * 最初は名前の近い候補だけを出し、「AI にも聞く」を押したときだけ AI に問い合わせる
 * （送るのは成果の指標・目標の名前とタスク・スキルの名前だけ。AI が設定されているときのみ）。
 */
function KeyResultSuggest({ k, onChanged }: { k: KeyResult; onChanged: () => void }) {
  const [askAi, setAskAi] = useState(false);
  const [hidden, setHidden] = useState<Set<string>>(new Set());
  const sug = useAsync(() => api.keyResultSuggest(k.id, askAi), [k.id, askAi]);
  const act = useAction();
  const r = sug.data;
  const shown = (r?.candidates ?? []).filter((c) => !hidden.has(candidateRef(c)));

  const accept = async (ref: string) => {
    const d = await act.run(() => api.keyResultSuggestAccept(k.id, [ref]));
    if (d) onChanged();
  };
  const reject = async (ref: string) => {
    const d = await act.run(() => api.keyResultSuggestReject(k.id, [ref]));
    if (d) setHidden((h) => new Set(h).add(ref));
  };

  // 対象外（すでにスキルか紐付けたタスクがある）や、まだ読み込み中で何も無いときは何も出さない。
  if (r && !r.eligible) return null;
  return (
    <Section
      level={3}
      title="紐づけの候補"
      help="名前の近いタスク・スキルです。自動では紐付けません。「付けない」を選ぶと、以後この候補は出しません。"
    >
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
                  aria-label={`${kindLabel(c.kind)}「${c.name}」を成果の指標に付ける`}
                  onClick={() => accept(ref)}
                >
                  付ける
                </button>
                <button
                  type="button"
                  class="ghost"
                  disabled={act.busy}
                  aria-label={`${kindLabel(c.kind)}「${c.name}」は成果の指標に付けない`}
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
    </Section>
  );
}

/**
 * 成果の指標に付けるスキル（主）。このスキル（子スキルを含む）が付いたタスク・活動の時間が、この成果の指標の時間になる。
 * 変更はその場で保存する。
 */
function KeyResultSkills({ k, names, onChanged }: { k: KeyResult; names: string[]; onChanged: () => void }) {
  const known = useAsync(() => api.skillList(), []);
  const act = useAction();
  const change = async (next: string[]) => {
    const r = await act.run(() => api.keyResultEdit(k.id, { skills: next, clear_skills: next.length === 0 }));
    if (r) onChanged();
  };
  return (
    <Section
      level={3}
      title="スキル"
      help={
        k.measure === "done_tasks"
          ? "完了したタスク数の対象は紐付けたタスクです。スキルは時間・取り組んだ日数に使います。"
          : "このスキルが付いたタスク・活動の時間をこの成果の指標の時間として数えます。付け外しはここで行います。"
      }
    >
      <SkillPicker
        value={names}
        skills={known.data?.skills ?? []}
        onChange={change}
        ariaLabel={`成果の指標「${k.title}」のスキルを追加`}
        disabled={act.busy}
      />
      <ErrorBox error={act.error ?? known.error} />
    </Section>
  );
}

/** 紐付いたタスクの一覧（個別に外せる）と、タスクの追加・既存タスクの紐付け。 */
function KeyResultTasks({
  k,
  summary,
  onChanged,
}: {
  k: KeyResult;
  summary: KeyResultSummary | null;
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
    const r = await act.run(() => api.keyResultUnlink([id]));
    if (r) onChanged();
  };

  return (
    <Section
      level={3}
      title="タスク"
      help={
        k.measure === "done_tasks"
          ? "完了したタスク数の対象です。タイトルを押すとタスクの詳細を開きます。"
          : "補助です。スキルが付いていなくても、この成果の指標の時間に含めます。タイトルを押すとタスクの詳細を開きます。"
      }
    >
      {tasks.length > 0 && (
        <ul class="plain">
          {tasks.map((t) => (
            <li key={t.id}>
              <span class="row">
                <span class="grow">
                  <a href={hrefOf("tasks", [t.id])}>{t.title}</a>
                  <span class="meta">
                    {" "}
                    （{STATUS_LABEL[t.status] ?? t.status}
                    {t.tracked_ms > 0 ? `・${fmtDur(t.tracked_ms)}` : ""}）
                  </span>
                </span>
                <OverflowMenu
                  label={`タスク「${t.title}」の操作`}
                  items={[{ label: "成果の指標から外す", onSelect: () => unlink(t.id), disabled: act.busy }]}
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
          placeholder="この成果の指標のタスクを追加（タイトル）"
          aria-label="この成果の指標のタスクを追加"
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
    </Section>
  );
}

/** 未完了のタスクから複数選んで紐付ける。他の成果の指標に紐付いているものは、付け替えになることを示す。 */
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
    const r = await act.run(() => api.keyResultLink(k.id, [...sel]));
    if (r) onDone();
  };

  return (
    <div class="stack key-result-link-picker">
      <ErrorBox error={all.error} />
      <input
        value={query}
        onInput={(e) => setQuery(e.currentTarget.value)}
        placeholder="タスクを絞り込む"
        aria-label="紐付けるタスクを絞り込む"
      />
      {cands.length === 0 && <p class="hint">紐付けられる未完了のタスクがありません。</p>}
      <ul class="plain key-result-cands">
        {cands.map((c) => (
          <li key={c.task.id}>
            <label class="check">
              <input type="checkbox" checked={sel.has(c.task.id)} onChange={() => toggle(c.task.id)} />
              {c.task.title}
              {c.fromKeyResult && <span class="meta">（成果の指標「{c.fromKeyResult}」に紐付いています。選ぶと付け替えます）</span>}
            </label>
          </li>
        ))}
      </ul>
      <div class="row">
        {relinks > 0 && <span class="meta note">{relinks} 件は別の成果の指標からの付け替えになります。</span>}
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
