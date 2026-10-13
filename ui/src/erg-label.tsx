import { useState } from "preact/hooks";
import { api, type ErgChoice, type ErgDistribution, type ErgLabel, type ErgRecordKind } from "./api";
import { ErrorBox } from "./components";
import { Help } from "./ui/parts";
import {
  classifyMessage,
  distributionRows,
  distributionSummary,
  ERG_CHOICES,
  ERG_HINTS,
  ERG_LEGEND,
  ERG_NAMES,
  ergState,
  fmtConfidence,
} from "./erg-util";
import { useAction } from "./lib";

/** 記録 1 件の ERG。AI の判定は小さく「AI」と分かる印、確認待ちは提案と理由に「これでよい / 変える」を付ける。 */
export function ErgLine({
  kind,
  id,
  label,
  onChanged,
}: {
  kind: ErgRecordKind;
  id: string;
  label?: ErgLabel;
  onChanged: () => void;
}) {
  const [changing, setChanging] = useState(false);
  const act = useAction();
  const st = ergState(label);

  const set = async (tag: ErgChoice) => {
    const r = await act.run(() => api.ergSet(kind, id, tag));
    if (r) {
      setChanging(false);
      onChanged();
    }
  };
  const confirm = async () => {
    if (await act.run(() => api.ergConfirm(kind, id))) onChanged();
  };

  const name = label ? ERG_NAMES[label.tag] : "";
  const conf = fmtConfidence(label?.confidence);

  return (
    <div class="erg-line">
      {st === "unlabeled" && !changing && (
        <button type="button" class="link" onClick={() => setChanging(true)}>
          ERG を付ける
        </button>
      )}
      {st === "user" && label && (
        <span>
          <span class="meta">ERG: </span>
          <span class="tag on" title={ERG_HINTS[label.tag]}>
            {name}
          </span>
        </span>
      )}
      {st === "ai" && label && (
        <span>
          <span class="meta">ERG: </span>
          <span class="tag" title={ERG_HINTS[label.tag]}>
            {name}
          </span>
          <small class="badge-ai" title={`AI が記録の内容から判定しました${conf ? `（確信度 ${conf}）` : ""}`}>
            {" "}
            AI
          </small>
          {label.reason && <span class="meta"> {label.reason}</span>}
        </span>
      )}
      {st === "pending" && label && (
        <span>
          <span class="meta">ERG の提案: </span>
          <span class="tag" title={ERG_HINTS[label.tag]}>
            {name}
          </span>
          <small class="badge-ai" title="AI の提案です。まだ確定していません"> AI</small>
          {label.reason && <span class="meta"> {label.reason}</span>}
          {conf && <span class="meta">（確信度 {conf}）</span>}{" "}
          {!changing && (
            <button type="button" class="ghost" disabled={act.busy} onClick={confirm}>
              これでよい
            </button>
          )}
        </span>
      )}
      {st !== "unlabeled" && !changing && (
        <>
          {" "}
          <button type="button" class="link" disabled={act.busy} onClick={() => setChanging(true)}>
            変える
          </button>
        </>
      )}
      {changing && (
        <span class="row" role="group" aria-label="ERG を選ぶ">
          {ERG_CHOICES.map((c) => (
            <button
              type="button"
              key={c}
              class={label?.tag === c && st !== "pending" ? "chip on" : "chip"}
              title={ERG_HINTS[c]}
              disabled={act.busy}
              onClick={() => set(c)}
            >
              {ERG_NAMES[c]}
            </button>
          ))}
          <button type="button" class="ghost" onClick={() => setChanging(false)}>
            やめる
          </button>
        </span>
      )}
      <ErrorBox error={act.error} />
    </div>
  );
}

/**
 * 期間内の ERG の分布（件数）と、未判定の記録を AI で判定するボタン。
 * 件数は事実として並べるだけで、評価や助言は添えない。
 */
export function ErgSummary({
  distribution,
  range,
  pendingTotal,
  onChanged,
}: {
  distribution: ErgDistribution;
  range: { date?: string; from?: string; to?: string };
  pendingTotal: number;
  onChanged: () => void;
}) {
  const act = useAction();
  const [msg, setMsg] = useState("");
  const classify = async () => {
    setMsg("");
    const o = await act.run(() => api.ergClassify(range));
    if (o) {
      setMsg(classifyMessage(o));
      onChanged();
    }
  };
  const rows = distributionRows(distribution);
  const canClassify = distribution.unlabeled > 0;
  return (
    <div class="stack">
      <ul class="plain erg-dist">
        {rows.map((r) => (
          <li key={r.key}>
            <strong>{r.name}</strong> <span class="num">{r.count} 件</span> <span class="meta">（{r.hint}）</span>
          </li>
        ))}
      </ul>
      <p class="meta">{distributionSummary(distribution)}</p>
      {pendingTotal > 0 && <p class="hint">確認待ちの提案が全期間で {pendingTotal} 件あります（記録画面で確定できます）。</p>}
      <div class="row">
        <button type="button" disabled={act.busy || !canClassify} onClick={classify}>
          {act.busy ? "AI が判定しています…" : "未判定の記録を AI で判定する"}
        </button>
        <Help>
          ERG は「{ERG_LEGEND}」の 3 つの観点です。記録の内容から AI が判定し、AI の判定は小さく「AI」と表示されます。
          確信が持てないものは確認待ちになり、あなたが確定するまで集計に入りません。
        </Help>
        {msg && (
          <span class="meta" role="status">
            {msg}
          </span>
        )}
      </div>
      <ErrorBox error={act.error} />
    </div>
  );
}
