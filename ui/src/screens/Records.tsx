import { useState } from "preact/hooks";
import { api, type AbcRecord, type ErgRecordKind } from "../api";
import { Card, Empty, ErrorBox, Field } from "../components";
import { PeriodSelect, Scale } from "../ui/parts";
import { ErgLine, ErgSummary } from "../erg-label";
import { ergKey, labelMap } from "../erg-util";
import { Markdown } from "../markdown";
import { fmtDateTime, useAction, useAsync } from "../lib";
import { AbcItem, CheckinItem, HassleItem } from "../record-edit";

export function Records() {
  const [period, setPeriod] = useState("today");
  const range = { date: period };
  const checkins = useAsync(() => api.checkinList(range), [period]);
  const hassles = useAsync(() => api.hassleList(range), [period]);
  const abcs = useAsync(() => api.abcList(range), [period]);
  const erg = useAsync(() => api.ergOverview(range), [period]);
  const pending = useAsync(() => api.ergReview(), [period]);
  const reloadErg = () => {
    erg.reload();
    pending.reload();
  };
  const labels = labelMap(erg.data?.labels ?? []);
  // 記録ごとの ERG（AI の判定・確認待ち・ユーザーの確定）。文章の無い記録には出さない（ラベルがあれば出す）。
  const ergLine = (kind: ErgRecordKind, id: string, hasText: boolean) => {
    const label = labels.get(ergKey(kind, id));
    if (!hasText && !label) return undefined;
    return <ErgLine kind={kind} id={id} label={label} onChanged={reloadErg} />;
  };

  return (
    <div class="stack">
      <div class="row">
        <PeriodSelect value={period} onChange={setPeriod} only={["today", "this-week", "last-week"]} />
        <span class="meta">一覧の期間</span>
      </div>
      <div class="grid2">
        <CheckinCard onAdded={checkins.reload} />
        <HassleCard onAdded={hassles.reload} />
      </div>
      <Card title="ERG（存在・関係・成長）の分布">
        <ErrorBox error={erg.error} />
        {erg.data && (
          <ErgSummary
            distribution={erg.data.distribution}
            range={range}
            pendingTotal={erg.data.pending_total}
            onChanged={reloadErg}
          />
        )}
      </Card>
      {(pending.data?.items.length ?? 0) > 0 && (
        <Card
          title="ERG の確認待ち"
          help="AI が確信を持てなかったものです。「これでよい」で提案のまま確定、「変える」で選び直せます。確定するまで集計には入りません。"
        >
          <ul class="plain">
            {pending.data!.items.map((i) => (
              <li key={`${i.record_kind}:${i.record_id}`}>
                <span class="meta">{fmtDateTime(i.record_at)}</span> {i.excerpt}
                <ErgLine kind={i.record_kind} id={i.record_id} label={i} onChanged={reloadErg} />
              </li>
            ))}
          </ul>
        </Card>
      )}
      <div class="grid2">
        <Card title="チェックイン">
          <ErrorBox error={checkins.error} />
          {checkins.data?.items.length === 0 && <Empty>記録はありません。</Empty>}
          <ul class="plain">
            {checkins.data?.items.map((c) => (
              <CheckinItem
                key={c.id}
                c={c}
                onChanged={() => {
                  checkins.reload();
                  reloadErg();
                }}
                erg={ergLine("checkin", c.id, !!c.note?.trim())}
              />
            ))}
          </ul>
        </Card>
        <Card title="ハッスル">
          <ErrorBox error={hassles.error} />
          {hassles.data?.items.length === 0 && <Empty>記録はありません。</Empty>}
          <ul class="plain">
            {hassles.data?.items.map((h) => (
              <HassleItem
                key={h.id}
                h={h}
                onChanged={() => {
                  hassles.reload();
                  abcs.reload();
                  reloadErg();
                }}
                erg={ergLine("hassle", h.id, true)}
              />
            ))}
          </ul>
        </Card>
      </div>
      <AbcCard onAdded={abcs.reload} />
      <Card title="ABC の記録">
        <ErrorBox error={abcs.error} />
        {abcs.data?.items.length === 0 && <Empty>記録はありません。</Empty>}
        <ul class="plain">
          {abcs.data?.items.map((a) => (
            <AbcItem
              key={a.id}
              a={a}
              onChanged={() => {
                abcs.reload();
                reloadErg();
              }}
              erg={ergLine("abc", a.id, true)}
            >
              <AbcDialog abc={a} onChanged={abcs.reload} />
            </AbcItem>
          ))}
        </ul>
      </Card>
    </div>
  );
}

/** B（受け止め方）を掘り下げる AI の問い。生成には時間がかかるので、実行中は表示を変える。 */
function AbcDialog({ abc, onChanged }: { abc: AbcRecord; onChanged: () => void }) {
  const act = useAction();
  return (
    <div class="stack">
      {abc.ai_dialog_md && <Markdown text={abc.ai_dialog_md} />}
      <div class="row">
        <button
          disabled={act.busy}
          onClick={async () => {
            if (await act.run(() => api.abcDispute(abc.id))) onChanged();
          }}
        >
          {act.busy ? "AI が考えています…" : abc.ai_dialog_md ? "問いを追加する" : "AI に問いを聞く"}
        </button>
      </div>
      <ErrorBox error={act.error} />
    </div>
  );
}

function CheckinCard({ onAdded }: { onAdded: () => void }) {
  const [energy, setEnergy] = useState<number>();
  const [mood, setMood] = useState<number>();
  const [stress, setStress] = useState<number>();
  const [note, setNote] = useState("");
  const act = useAction();
  return (
    <Card title="チェックイン">
      <Scale label="エネルギー" value={energy} onChange={setEnergy} />
      <Scale label="気分" value={mood} onChange={setMood} />
      <Scale label="ストレス" value={stress} onChange={setStress} />
      <Field label="一言メモ">
        <input value={note} onInput={(e) => setNote(e.currentTarget.value)} />
      </Field>
      <div class="row">
        <span class="spacer" />
        <button
          class="primary"
          disabled={act.busy || (energy == null && mood == null && stress == null && !note.trim())}
          onClick={async () => {
            const r = await act.run(() => api.checkinAdd({ energy, mood, stress, note: note.trim() || undefined }));
            if (r) {
              setEnergy(undefined);
              setMood(undefined);
              setStress(undefined);
              setNote("");
              onAdded();
            }
          }}
        >
          記録
        </button>
      </div>
      <ErrorBox error={act.error} />
    </Card>
  );
}

function HassleCard({ onAdded }: { onAdded: () => void }) {
  const [text, setText] = useState("");
  const [intensity, setIntensity] = useState<number | undefined>(3);
  const act = useAction();
  return (
    <Card title="ハッスルを記録">
      <Field label="気になったこと">
        <textarea rows={3} value={text} onInput={(e) => setText(e.currentTarget.value)} />
      </Field>
      <Scale label="強さ" value={intensity} onChange={setIntensity} />
      <div class="row">
        <span class="spacer" />
        <button
          class="primary"
          disabled={act.busy || !text.trim()}
          onClick={async () => {
            const r = await act.run(() => api.hassleAdd(text.trim(), intensity));
            if (r) {
              setText("");
              onAdded();
            }
          }}
        >
          記録
        </button>
      </div>
      <ErrorBox error={act.error} />
    </Card>
  );
}

function AbcCard({ onAdded }: { onAdded: () => void }) {
  const [a, setA] = useState("");
  const [b, setB] = useState("");
  const [c, setC] = useState("");
  const [ci, setCi] = useState<number>();
  const act = useAction();
  return (
    <Card title="ABC を書く">
      <div class="grid3">
        <Field label="A: 出来事">
          <textarea rows={3} value={a} onInput={(e) => setA(e.currentTarget.value)} />
        </Field>
        <Field label="B: 受け止め方">
          <textarea rows={3} value={b} onInput={(e) => setB(e.currentTarget.value)} />
        </Field>
        <div class="stack">
          <Field label="C: 感情">
            <input value={c} onInput={(e) => setC(e.currentTarget.value)} />
          </Field>
          <Scale label="強さ" value={ci} onChange={setCi} />
        </div>
      </div>
      <div class="row">
        <span class="spacer" />
        <button
          class="primary"
          disabled={act.busy || !a.trim()}
          onClick={async () => {
            const r = await act.run(() =>
              api.abcAdd({
                activating_event: a.trim(),
                belief: b.trim() || undefined,
                consequence_emotion: c.trim() || undefined,
                consequence_intensity: ci,
              }),
            );
            if (r) {
              setA("");
              setB("");
              setC("");
              setCi(undefined);
              onAdded();
            }
          }}
        >
          記録
        </button>
      </div>
      <ErrorBox error={act.error} />
    </Card>
  );
}
