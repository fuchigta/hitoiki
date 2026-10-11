import { useEffect, useState } from "preact/hooks";
import { api, type Reflection, type ReflectionKind } from "../api";
import { Card, Empty, ErrorBox, Field } from "../components";
import { dateInput, fmtFull, useAction, useAsync } from "../lib";
import { Markdown } from "../markdown";
import { StructuredAnswer } from "../reflect-form";
import { parseStructured } from "../reflect-util";

const KINDS: { id: ReflectionKind; label: string; hint: string }[] = [
  { id: "day_start", label: "始業", hint: "昨日までを材料に、今日のはじまりを考えます。" },
  { id: "day_end", label: "終業", hint: "今日を一緒にふりかえります。" },
  { id: "weekly", label: "週次", hint: "今週の時間の使い方をふりかえります。" },
];

function kindFromHash(): ReflectionKind {
  const k = location.hash.replace(/^#\/?/, "").split(/[/?]/)[1];
  return KINDS.some((x) => x.id === k) ? (k as ReflectionKind) : "day_end";
}

/** 振り返り（始業・終業・週次）。AI が問いを作り、答えを書いて保存する。 */
export function Reflect() {
  const [kind, setKind] = useState<ReflectionKind>(kindFromHash());
  const [date, setDate] = useState(dateInput());
  const [current, setCurrent] = useState<Reflection | null>(null);
  const [answer, setAnswer] = useState("");
  const [saved, setSaved] = useState("");
  const gen = useAction();
  const save = useAction();
  const past = useAsync(() => api.reflectList(kind, 30), [kind]);

  // 通知やトレイから別の種別が指定されたとき（`#/reflect/day_end`）に追従する。
  useEffect(() => {
    const h = () => {
      if (!location.hash.startsWith("#/reflect")) return;
      setKind(kindFromHash());
    };
    window.addEventListener("hashchange", h);
    return () => window.removeEventListener("hashchange", h);
  }, []);

  // 種別を切り替えたら、表示中の内容は閉じる。
  useEffect(() => {
    setCurrent(null);
    setSaved("");
  }, [kind]);

  useEffect(() => {
    setAnswer(current?.answers_md ?? "");
  }, [current?.id]);

  const select = (k: ReflectionKind) => {
    setKind(k);
    history.replaceState(null, "", `#/reflect/${k}`);
  };

  const generate = async () => {
    setSaved("");
    const r = await gen.run(() => api.reflectGenerate(kind, date));
    if (r) {
      setCurrent(r);
      past.reload();
    }
  };

  const submit = async (e: Event) => {
    e.preventDefault();
    if (!current) return;
    const r = await save.run(() => api.reflectAnswer(current.id, answer));
    if (r) {
      setCurrent(r);
      setSaved("回答を保存しました。");
      past.reload();
    }
  };

  const info = KINDS.find((k) => k.id === kind)!;
  // 構造化された問いは種類ごとのフォームで答える。構造化されていない（フォールバックした）ものは従来の表示。
  const structured = current ? parseStructured(current) : null;

  return (
    <div class="stack">
      <Card>
        <div class="row" role="tablist" aria-label="振り返りの種別">
          {KINDS.map((k) => (
            <button
              key={k.id}
              role="tab"
              aria-selected={kind === k.id}
              class={`chip${kind === k.id ? " on" : ""}`}
              onClick={() => select(k.id)}
            >
              {k.label}
            </button>
          ))}
        </div>
        <p class="muted">
          {info.hint}
          {info.id === "day_start" && (
            <>
              {" "}
              <a href="#/today/design">今日のデザイン</a>でブロックを並べることもできます。
            </>
          )}
        </p>
        <div class="row">
          <Field label={kind === "weekly" ? "基準日（その週）" : "基準日"}>
            <input type="date" value={date} onInput={(e) => setDate(e.currentTarget.value)} />
          </Field>
          <button class="primary" disabled={gen.busy || !date} onClick={generate}>
            {gen.busy ? "生成中…" : "問いを生成"}
          </button>
        </div>
        {gen.busy && (
          <p class="muted" role="status">
            AI が問いを考えています。数十秒かかることがあります。他の画面を使っていても大丈夫です。
          </p>
        )}
        <ErrorBox error={gen.error} />
      </Card>

      {current && structured && (
        <Card title={`${info.label}の振り返り（${fmtFull(current.at)}）`}>
          <StructuredAnswer
            reflection={current}
            onSaved={(r) => {
              setCurrent(r);
              past.reload();
            }}
          />
        </Card>
      )}

      {current && !structured && (
        <Card title={`${info.label}の振り返り（${fmtFull(current.at)}）`}>
          <Markdown text={current.questions_md} />
          <form class="stack" onSubmit={submit}>
            <Field label="いまの気持ちや考えたこと（Markdown）">
              <textarea
                rows={8}
                value={answer}
                onInput={(e) => {
                  setAnswer(e.currentTarget.value);
                  setSaved("");
                }}
              />
            </Field>
            <div class="row">
              <button class="primary" disabled={save.busy || !answer.trim()}>
                回答を保存
              </button>
              {saved && (
                <span class="muted" role="status">
                  {saved}
                </span>
              )}
            </div>
            <ErrorBox error={save.error} />
          </form>
        </Card>
      )}

      <Card title={`過去の${info.label}`}>
        <ErrorBox error={past.error} />
        {past.data && past.data.length === 0 && <Empty>まだありません。</Empty>}
        <ul class="plain">
          {(past.data ?? []).map((r) => (
            <li key={r.id}>
              <button class={`link${current?.id === r.id ? " on" : ""}`} onClick={() => setCurrent(r)}>
                {fmtFull(r.at)}
              </button>
              {r.answers_md || r.answers_json ? <span class="muted"> ・回答済み</span> : <span class="muted"> ・未回答</span>}
            </li>
          ))}
        </ul>
      </Card>
    </div>
  );
}
