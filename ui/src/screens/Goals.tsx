import { Fragment } from "preact";
import { api, type KeyResult, type GoalView, type SkillView } from "../api";
import { EmptyState, ErrorBox, InlineNew } from "../components";
import { splitMeasures } from "../key-result-util";
import { hrefOf, navigate, useAction, useAsync, useLiveTick, useRoute } from "../lib";
import { periodInputs } from "../manage-util";
import { KeyResultPanel, GoalDetail, GOAL_STATUS_LABEL, SkillDetail, useGoalDelete } from "../goal-edit";
import { keyResultMetaText, goalSelection, goalsTab, skillTree, type GoalsTab } from "../goal-util";
import { Help, ListDetail, ObjectList, ObjectRow, Tabs } from "../ui/parts";

const TABS = [
  { id: "goals", label: "目標" },
  { id: "skills", label: "スキル" },
];

export function Goals() {
  const route = useRoute();
  const sel = goalSelection(route.path);
  const tab = goalsTab(route.params.get("tab"), sel);
  const goalsState = useAsync(() => api.goalList(), []);
  const skills = useAsync(() => api.skillList(), []);
  const act = useAction();
  const goals = goalsState.data?.goals ?? [];
  const skillList = skills.data?.skills ?? [];

  const goTab = (t: string) => navigate(hrefOf("goals", [], t === "skills" ? { tab: "skills" } : {}));
  const home = (t: GoalsTab) => hrefOf("goals", [], t === "skills" ? { tab: "skills" } : {});

  const list = (
    <div class="stack">
      <Tabs tabs={TABS} active={tab} onChange={goTab} idBase="goals" label="目標の一覧" panels={false} />
      {tab === "goals" ? (
        <>
          <div class="list-head">
            <Help>
              目標と、その達成を測る成果の指標（時間・取り組んだ日数・完了したタスク数で数える）を書く場所です。目標の下に成果の指標を並べます。
            </Help>
            <span class="spacer" />
            <InlineNew
              label="＋ 目標"
              inputLabel="新しい目標"
              onCreate={async (title) => {
                const o = await api.goalAdd(title);
                goalsState.reload();
                navigate(hrefOf("goals", ["goal", o.id]));
                return o;
              }}
            />
          </div>
          <ErrorBox error={goalsState.error} />
          {goalsState.data && goals.length === 0 && <EmptyState>目標はまだありません。</EmptyState>}
          <ObjectList label="目標と成果の指標">
            {goals.map((o) => (
              <Fragment key={o.id}>
                <GoalRow o={o} selected={sel?.kind === "goal" && sel.id === o.id} onDeleted={() => deleted(goalsState.reload, home("goals"))} />
                {o.key_results.map((k) => (
                  <KeyResultRow key={k.id} k={k} selected={sel?.kind === "key-result" && sel.id === k.id} />
                ))}
              </Fragment>
            ))}
          </ObjectList>
        </>
      ) : (
        <>
          <div class="list-head">
            <Help>
              タスクや成果の指標に付ける分類です。スキルが付いたタスク・活動の時間をまとめて見られます。
            </Help>
            <button
              type="button"
              class="ghost"
              disabled={act.busy}
              onClick={async () => {
                const r = await act.run(() => api.skillImportPreset());
                if (r) skills.reload();
              }}
            >
              プリセットを取り込む
            </button>
            <span class="spacer" />
            <InlineNew
              label="＋ スキル"
              inputLabel="新しいスキル"
              placeholder="名前（Enter で作成）"
              onCreate={async (name) => {
                const s = await api.skillAdd(name);
                skills.reload();
                navigate(hrefOf("goals", ["skill", s.id], { tab: "skills" }));
                return s;
              }}
            />
          </div>
          <ErrorBox error={skills.error ?? act.error} />
          {skills.data && skillList.length === 0 && <EmptyState>スキルはまだありません。</EmptyState>}
          <ObjectList label="スキル">
            {skillTree(skillList).map(({ skill: s, depth }) => (
              <ObjectRow
                key={s.id}
                indent={depth}
                selected={sel?.kind === "skill" && sel.id === s.id}
                title={s.name}
                tags={s.preset ? ["プリセット"] : undefined}
                onSelect={() => navigate(hrefOf("goals", ["skill", s.id], { tab: "skills" }))}
              />
            ))}
          </ObjectList>
        </>
      )}
    </div>
  );

  return <ListDetail list={list} detail={<Detail sel={sel} tab={tab} route={route.params} goals={goals} skills={skillList} reloadGoals={goalsState.reload} reloadSkills={skills.reload} loaded={!!goalsState.data && !!skills.data} />} />;
}

/** 削除後は一覧を読み直し、選択を外す。 */
function deleted(reload: () => void, home: string) {
  reload();
  navigate(home, { replace: true });
}

function Detail({
  sel,
  tab,
  route,
  goals,
  skills,
  reloadGoals,
  reloadSkills,
  loaded,
}: {
  sel: ReturnType<typeof goalSelection>;
  tab: GoalsTab;
  route: URLSearchParams;
  goals: GoalView[];
  skills: SkillView[];
  reloadGoals: () => void;
  reloadSkills: () => void;
  loaded: boolean;
}) {
  const home = hrefOf("goals", [], tab === "skills" ? { tab: "skills" } : {});
  if (!sel) {
    return (
      <EmptyState>
        {tab === "skills"
          ? "スキルを選ぶと詳細が出ます。"
          : "目標か成果の指標を選ぶと詳細が出ます。"}
      </EmptyState>
    );
  }
  if (!loaded) return <EmptyState>読み込み中…</EmptyState>;
  if (sel.kind === "goal") {
    const o = goals.find((x) => x.id === sel.id);
    if (!o) return <EmptyState>目標が見つかりません。</EmptyState>;
    return (
      <GoalDetail
        key={`${o.id}?${route.toString()}`}
        o={o}
        initialEditing={route.get("edit") === "1"}
        initialAdding={route.get("new") === "key-result"}
        onChanged={reloadGoals}
        onDeleted={() => deleted(reloadGoals, home)}
      />
    );
  }
  if (sel.kind === "key-result") {
    const k = goals.flatMap((o) => o.key_results).find((x) => x.id === sel.id);
    if (!k) return <EmptyState>成果の指標が見つかりません。</EmptyState>;
    return <KeyResultPanel key={k.id} k={k} goals={goals} onChanged={reloadGoals} onDeleted={() => deleted(reloadGoals, home)} />;
  }
  const s = skills.find((x) => x.id === sel.id);
  if (!s) return <EmptyState>スキルが見つかりません。</EmptyState>;
  return (
    <SkillDetail
      key={s.id}
      s={s}
      all={skills}
      onChanged={() => {
        reloadSkills();
        reloadGoals();
      }}
      onDeleted={() => deleted(reloadSkills, home)}
    />
  );
}

/** 目標の見出し行。タイトル・期間・状態。「…」に成果の指標の追加・編集・削除。 */
function GoalRow({ o, selected, onDeleted }: { o: GoalView; selected: boolean; onDeleted: () => void }) {
  const del = useGoalDelete(o.id, onDeleted);
  const period = o.period_start != null || o.period_end != null ? periodText(o) : undefined;
  return (
    <ObjectRow
      group
      selected={selected}
      title={o.title}
      tags={o.status !== "active" ? [GOAL_STATUS_LABEL[o.status] ?? o.status] : undefined}
      meta={period}
      onSelect={() => navigate(hrefOf("goals", ["goal", o.id]))}
      menuLabel={`目標「${o.title}」の操作`}
      menu={[
        { label: "成果の指標を追加", onSelect: () => navigate(hrefOf("goals", ["goal", o.id], { new: "key-result" })) },
        { label: "編集", onSelect: () => navigate(hrefOf("goals", ["goal", o.id], { edit: "1" })) },
        { label: "削除…", onSelect: del.start, disabled: del.busy },
      ]}
      trailing={del.dialog}
    />
  );
}

function periodText(o: GoalView): string {
  const p = periodInputs(o);
  return `${p.from || "…"} 〜 ${p.to || "…"}`;
}

/** 成果の指標の行。選んだ数え方の値を 1 つ、書いてある目標値と並べるだけ（達成率や評価は出さない）。 */
function KeyResultRow({ k, selected }: { k: KeyResult; selected: boolean }) {
  return (
    <ObjectRow
      indent={1}
      selected={selected}
      title={k.title}
      tags={k.status !== "active" ? [GOAL_STATUS_LABEL[k.status] ?? k.status] : undefined}
      meta={<KeyResultMeta k={k} />}
      onSelect={() => navigate(hrefOf("goals", ["key-result", k.id]))}
    />
  );
}

function KeyResultMeta({ k }: { k: KeyResult }) {
  const tick = useLiveTick(["segments-updated"]);
  const sum = useAsync(() => api.keyResultShow(k.id), [k.id, k.measure, k.active_day_min, tick]);
  const main = sum.data ? splitMeasures(sum.data).main.text : undefined;
  if (!main) return k.target ? `目標値 ${k.target}` : null;
  return keyResultMetaText(main, k.target);
}
