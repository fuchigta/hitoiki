import { useState } from "preact/hooks";
import { api } from "../api";
import { Card, Empty, ErrorBox } from "../components";
import { useAction, useAsync } from "../lib";
import { SkillPicker } from "../skill-picker";
import { Help } from "../ui/parts";
import { KrItem, ObjectiveItem, SkillItem } from "../okr-edit";

export function Okr() {
  const okr = useAsync(() => api.okrList(), []);
  const skills = useAsync(() => api.skillList(), []);
  const [obj, setObj] = useState("");
  const [skill, setSkill] = useState("");
  const act = useAction();

  return (
    <div class="grid2">
      <div class="stack">
        <Card title="OKR">
          <ErrorBox error={okr.error} />
          {okr.data?.objectives.length === 0 && <Empty>Objective はまだありません。</Empty>}
          {okr.data?.objectives.map((o) => (
            <ObjectiveItem key={o.id} o={o} onChanged={okr.reload}>
              <ul class="plain">
                {o.key_results.map((k) => (
                  <KrItem key={k.id} k={k} objectives={okr.data?.objectives ?? []} onChanged={okr.reload} />
                ))}
              </ul>
              <KrForm objective={o.id} onAdded={okr.reload} />
            </ObjectiveItem>
          ))}
          <form
            class="row"
            onSubmit={async (e) => {
              e.preventDefault();
              const r = await act.run(() => api.okrAddObjective(obj.trim()));
              if (r) {
                setObj("");
                okr.reload();
              }
            }}
          >
            <input class="grow" value={obj} onInput={(e) => setObj(e.currentTarget.value)} placeholder="新しい Objective" />
            <button disabled={act.busy || !obj.trim()}>追加</button>
          </form>
        </Card>
      </div>
      <div class="stack">
        <Card title="スキル">
          <ErrorBox error={skills.error} />
          {skills.data?.skills.length === 0 && <Empty>スキルはまだありません。</Empty>}
          <ul class="plain">
            {skills.data?.skills.map((s) => (
              <SkillItem key={s.id} s={s} all={skills.data?.skills ?? []} onChanged={skills.reload} />
            ))}
          </ul>
          <form
            class="row"
            onSubmit={async (e) => {
              e.preventDefault();
              const r = await act.run(() => api.skillAdd(skill.trim()));
              if (r) {
                setSkill("");
                skills.reload();
              }
            }}
          >
            <input class="grow" value={skill} onInput={(e) => setSkill(e.currentTarget.value)} placeholder="新しいスキル" />
            <button disabled={act.busy || !skill.trim()}>追加</button>
          </form>
          <div class="row">
            <button
              class="ghost"
              disabled={act.busy}
              onClick={async () => {
                const r = await act.run(() => api.skillImportPreset());
                if (r) skills.reload();
              }}
            >
              プリセットを取り込む
            </button>
          </div>
        </Card>
        <ErrorBox error={act.error} />
      </div>
    </div>
  );
}

function KrForm({ objective, onAdded }: { objective: string; onAdded: () => void }) {
  const [title, setTitle] = useState("");
  const [target, setTarget] = useState("");
  const [skillNames, setSkillNames] = useState<string[]>([]);
  const known = useAsync(() => api.skillList(), []);
  const act = useAction();
  return (
    <form
      class="stack"
      onSubmit={async (e) => {
        e.preventDefault();
        const r = await act.run(() => api.okrAddKr(objective, title.trim(), target.trim() || undefined, skillNames));
        if (r) {
          setTitle("");
          setTarget("");
          setSkillNames([]);
          onAdded();
        }
      }}
    >
      <div class="row">
        <input class="grow" value={title} onInput={(e) => setTitle(e.currentTarget.value)} placeholder="Key Result" />
        <input class="narrow" value={target} onInput={(e) => setTarget(e.currentTarget.value)} placeholder="目標" />
        <button disabled={act.busy || !title.trim()}>追加</button>
      </div>
      <div class="field">
        <span>
          スキル{" "}
          <Help>このスキルが付いたタスク・活動の時間をこの KR の時間として数えます。タスクの紐付けは作成後に補助として足せます。</Help>
        </span>
        <SkillPicker
          value={skillNames}
          skills={known.data?.skills ?? []}
          onChange={setSkillNames}
          ariaLabel="KR のスキルを追加"
          disabled={act.busy}
        />
      </div>
      <ErrorBox error={act.error ?? known.error} />
    </form>
  );
}
