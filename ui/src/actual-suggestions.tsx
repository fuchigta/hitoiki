import { useState } from "preact/hooks";
import { api, type IgnoreSuggestion, type RuleSuggestion } from "./api";
import { ErrorBox } from "./components";
import { suggestionLine } from "./ignore-util";
import { useAction } from "./lib";

type Item = { type: "rule"; sg: RuleSuggestion } | { type: "ignore"; sg: IgnoreSuggestion };

const keyOf = (it: Item) => (it.type === "rule" ? `rule|${it.sg.task_id}|${it.sg.field}|${it.sg.pattern}` : `ignore|${it.sg.exe}`);

/**
 * 一覧の上に出す「提案」の帯。ルール化と無視の提案を 1 枚にまとめ、先頭の 1 件だけを見せる。
 * 「ほか N 件」で残りを開く。操作は各提案ごと（ルールにする／今はしない、無視する／今回はしない）。
 */
export function SuggestionBand({
  rules,
  ignores,
  onDone,
}: {
  rules: RuleSuggestion[];
  ignores: IgnoreSuggestion[];
  onDone: () => void;
}) {
  const [open, setOpen] = useState(false);
  const items: Item[] = [
    ...ignores.map((sg): Item => ({ type: "ignore", sg })),
    ...rules.map((sg): Item => ({ type: "rule", sg })),
  ];
  if (items.length === 0) return null;
  const shown = open ? items : items.slice(0, 1);
  return (
    <section class="suggest-band" aria-label="提案">
      <header class="suggest-head">
        <h2>提案</h2>
        {items.length > 1 && (
          <button type="button" class="link" aria-expanded={open} onClick={() => setOpen(!open)}>
            {open ? "閉じる" : `ほか ${items.length - 1} 件`}
          </button>
        )}
      </header>
      <ul class="plain">
        {shown.map((it) => (
          <li key={keyOf(it)}>{it.type === "rule" ? <RuleLine sg={it.sg} onDone={onDone} /> : <IgnoreLine sg={it.sg} onDone={onDone} />}</li>
        ))}
      </ul>
    </section>
  );
}

/** 「Orca.exe はいつもこのタスクにしますか？」というルール化の提案。 */
function RuleLine({ sg, onDone }: { sg: RuleSuggestion; onDone: () => void }) {
  const act = useAction();
  const ref = { task_id: sg.task_id, field: sg.field, pattern: sg.pattern, pattern_type: sg.pattern_type };
  const accept = async () => {
    if (await act.run(() => api.actualSuggestAccept(ref))) onDone();
  };
  const dismiss = async () => {
    const r = await act.run(async () => {
      await api.actualSuggestDismiss(ref);
      return true;
    });
    if (r) onDone();
  };
  return (
    <>
      <div class="row">
        <span>
          <strong>{sg.target}</strong> はいつも「{sg.task_title ?? "このタスク"}」にしますか？
          <span class="meta"> （最近 {sg.count} 回、同じ割り当てを確定しました）</span>
        </span>
        <span class="spacer" />
        <button class="ghost" disabled={act.busy} onClick={dismiss}>
          今はしない
        </button>
        <button disabled={act.busy} onClick={accept}>
          ルールにする
        </button>
      </div>
      <ErrorBox error={act.error} />
    </>
  );
}

/** 「explorer.exe は短い区間が 214 件（合計 16.5分）。無視しますか？」という無視の提案。 */
function IgnoreLine({ sg, onDone }: { sg: IgnoreSuggestion; onDone: () => void }) {
  const act = useAction();
  const accept = async () => {
    if (await act.run(() => api.ignoreAdd(sg.exe))) onDone();
  };
  const dismiss = async () => {
    const r = await act.run(async () => {
      await api.ignoreSuggestDismiss(sg.exe);
      return true;
    });
    if (r) onDone();
  };
  return (
    <>
      <div class="row">
        <span>
          {suggestionLine(sg)}無視しますか？
          <span class="meta"> （{sg.reasons.map((r) => r.text).join("、")}。無視しても記録は消えず、いつでも戻せます）</span>
        </span>
        <span class="spacer" />
        <button class="ghost" disabled={act.busy} onClick={dismiss}>
          今回はしない
        </button>
        <button disabled={act.busy} onClick={accept}>
          無視する
        </button>
      </div>
      <ErrorBox error={act.error} />
    </>
  );
}
