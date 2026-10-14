import { Fragment } from "preact";
import { api, type KeyResult, type ObjectiveView, type SkillView } from "../api";
import { EmptyState, ErrorBox, InlineNew } from "../components";
import { splitMeasures } from "../kr-util";
import { hrefOf, navigate, useAction, useAsync, useLiveTick, useRoute } from "../lib";
import { periodInputs } from "../manage-util";
import { KrPanel, ObjectiveDetail, OKR_STATUS_LABEL, SkillDetail, useObjectiveDelete } from "../okr-edit";
import { krMetaText, okrSelection, okrTab, skillTree, type OkrTab } from "../okr-util";
import { Help, ListDetail, ObjectList, ObjectRow, Tabs } from "../ui/parts";

const TABS = [
  { id: "okr", label: "目標" },
  { id: "skills", label: "スキル" },
];

export function Okr() {
  const route = useRoute();
  const sel = okrSelection(route.path);
  const tab = okrTab(route.params.get("tab"), sel);
  const okr = useAsync(() => api.okrList(), []);
  const skills = useAsync(() => api.skillList(), []);
  const act = useAction();
  const objectives = okr.data?.objectives ?? [];
  const skillList = skills.data?.skills ?? [];

  const goTab = (t: string) => navigate(hrefOf("okr", [], t === "skills" ? { tab: "skills" } : {}));
  const home = (t: OkrTab) => hrefOf("okr", [], t === "skills" ? { tab: "skills" } : {});

  const list = (
    <div class="stack">
      <Tabs tabs={TABS} active={tab} onChange={goTab} idBase="okr" label="目標の一覧" panels={false} />
      {tab === "okr" ? (
        <>
          <div class="list-head">
            <Help>
              目標と、その達成を測る成果の指標（時間・回数・完了数で数える）を書く場所です。OKR（Objectives and Key Results）という手法の形で、目標の下に成果の指標を並べます。
            </Help>
            <span class="spacer" />
            <InlineNew
              label="＋ 目標"
              inputLabel="新しい目標"
              onCreate={async (title) => {
                const o = await api.okrAddObjective(title);
                okr.reload();
                navigate(hrefOf("okr", ["o", o.id]));
                return o;
              }}
            />
          </div>
          <ErrorBox error={okr.error} />
          {okr.data && objectives.length === 0 && <EmptyState>目標はまだありません。</EmptyState>}
          <ObjectList label="目標と成果の指標">
            {objectives.map((o) => (
              <Fragment key={o.id}>
                <ObjectiveRow o={o} selected={sel?.kind === "o" && sel.id === o.id} onDeleted={() => deleted(okr.reload, home("okr"))} />
                {o.key_results.map((k) => (
                  <KrRow key={k.id} k={k} selected={sel?.kind === "kr" && sel.id === k.id} />
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
                navigate(hrefOf("okr", ["skill", s.id], { tab: "skills" }));
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
                onSelect={() => navigate(hrefOf("okr", ["skill", s.id], { tab: "skills" }))}
              />
            ))}
          </ObjectList>
        </>
      )}
    </div>
  );

  return <ListDetail list={list} detail={<Detail sel={sel} tab={tab} route={route.params} objectives={objectives} skills={skillList} reloadOkr={okr.reload} reloadSkills={skills.reload} loaded={!!okr.data && !!skills.data} />} />;
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
  objectives,
  skills,
  reloadOkr,
  reloadSkills,
  loaded,
}: {
  sel: ReturnType<typeof okrSelection>;
  tab: OkrTab;
  route: URLSearchParams;
  objectives: ObjectiveView[];
  skills: SkillView[];
  reloadOkr: () => void;
  reloadSkills: () => void;
  loaded: boolean;
}) {
  const home = hrefOf("okr", [], tab === "skills" ? { tab: "skills" } : {});
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
  if (sel.kind === "o") {
    const o = objectives.find((x) => x.id === sel.id);
    if (!o) return <EmptyState>目標が見つかりません。</EmptyState>;
    return (
      <ObjectiveDetail
        key={`${o.id}?${route.toString()}`}
        o={o}
        initialEditing={route.get("edit") === "1"}
        initialAdding={route.get("new") === "kr"}
        onChanged={reloadOkr}
        onDeleted={() => deleted(reloadOkr, home)}
      />
    );
  }
  if (sel.kind === "kr") {
    const k = objectives.flatMap((o) => o.key_results).find((x) => x.id === sel.id);
    if (!k) return <EmptyState>成果の指標が見つかりません。</EmptyState>;
    return <KrPanel key={k.id} k={k} objectives={objectives} onChanged={reloadOkr} onDeleted={() => deleted(reloadOkr, home)} />;
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
        reloadOkr();
      }}
      onDeleted={() => deleted(reloadSkills, home)}
    />
  );
}

/** 目標の見出し行。タイトル・期間・状態。「…」に成果の指標の追加・編集・削除。 */
function ObjectiveRow({ o, selected, onDeleted }: { o: ObjectiveView; selected: boolean; onDeleted: () => void }) {
  const del = useObjectiveDelete(o.id, onDeleted);
  const period = o.period_start != null || o.period_end != null ? periodText(o) : undefined;
  return (
    <ObjectRow
      group
      selected={selected}
      title={o.title}
      tags={o.status !== "active" ? [OKR_STATUS_LABEL[o.status] ?? o.status] : undefined}
      meta={period}
      onSelect={() => navigate(hrefOf("okr", ["o", o.id]))}
      menuLabel={`目標「${o.title}」の操作`}
      menu={[
        { label: "成果の指標を追加", onSelect: () => navigate(hrefOf("okr", ["o", o.id], { new: "kr" })) },
        { label: "編集", onSelect: () => navigate(hrefOf("okr", ["o", o.id], { edit: "1" })) },
        { label: "削除…", onSelect: del.start, disabled: del.busy },
      ]}
      trailing={del.dialog}
    />
  );
}

function periodText(o: ObjectiveView): string {
  const p = periodInputs(o);
  return `${p.from || "…"} 〜 ${p.to || "…"}`;
}

/** 成果の指標の行。主軸の値を 1 つ、書いてある目標値と並べるだけ（達成率や評価は出さない）。 */
function KrRow({ k, selected }: { k: KeyResult; selected: boolean }) {
  return (
    <ObjectRow
      indent={1}
      selected={selected}
      title={k.title}
      tags={k.status !== "active" ? [OKR_STATUS_LABEL[k.status] ?? k.status] : undefined}
      meta={<KrMeta k={k} />}
      onSelect={() => navigate(hrefOf("okr", ["kr", k.id]))}
    />
  );
}

function KrMeta({ k }: { k: KeyResult }) {
  const tick = useLiveTick(["segments-updated"]);
  const sum = useAsync(() => api.okrShow(k.id), [k.id, k.measure, k.day_min, tick]);
  const main = sum.data ? splitMeasures(sum.data).main.text : undefined;
  if (!main) return k.target ? `目標値 ${k.target}` : null;
  return krMetaText(main, k.target);
}
