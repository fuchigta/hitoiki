import { useState } from "preact/hooks";
import { api } from "../api";
import { Card, ErrorBox, Field } from "../components";
import { QuickShortcutCard } from "../quick-shortcut";
import { hasExe, normalizeExe } from "../ignore-util";
import { DataSettings } from "../data-settings";
import { loadTimeZone, useAction, useAsync } from "../lib";
import { OverflowMenu } from "../ui/parts";

type Cfg = {
  general?: { timezone?: string; autostart?: boolean; update_check?: boolean | null }; push?: { enabled?: boolean; idle_return_min?: number; urgent_task_interval_min?: number; important_stale_days?: number; kr_idle_weekday?: number };
  review?: { trust_doing?: boolean; auto_break_locked_min?: number };
};

export function Settings() {
  const cfg = useAsync(() => api.configAll(), []);
  const path = useAsync(() => api.configPath(), []);
  const [key, setKey] = useState("");
  const [value, setValue] = useState("");
  const [msg, setMsg] = useState("");
  const act = useAction();
  const c = (cfg.data ?? {}) as Cfg;

  const set = async (k: string, v: string) => {
    const r = await act.run(() => api.configSet(k, v));
    if (r) {
      setMsg(`${k} を保存しました。`);
      if (k === "general.timezone") void loadTimeZone();
      cfg.reload();
    }
  };

  return (
    <div class="stack">
      <ErrorBox error={cfg.error ?? act.error} />
      <Card title="全般">
        <div class="stack">
          <label class="check">
            <input
              type="checkbox"
              checked={!!c.general?.autostart}
              onChange={(e) => set("general.autostart", String(e.currentTarget.checked))}
            />
            ログイン時に自動で起動する
          </label>
          <label class="check">
            <input
              type="checkbox"
              checked={c.push?.enabled !== false}
              onChange={(e) => set("push.enabled", String(e.currentTarget.checked))}
            />
            問いかけの通知を受け取る
          </label>
          <label class="check">
            <input
              type="checkbox"
              checked={c.general?.update_check === true}
              onChange={(e) => set("general.update_check", String(e.currentTarget.checked))}
            />
            新しいバージョンを確認する（1日1回、GitHub に問い合わせます。送る情報はありません）
          </label>
          <p class="meta">タイムゾーン: {c.general?.timezone ?? "-"}</p>
        </div>
      </Card>
      <QuickShortcutCard />
      <Card title="通知">
        <div class="row">
          <button onClick={() => act.run(() => api.notifyTest())}>テスト通知を出す</button>
          <button class="ghost" onClick={() => act.run(() => api.snooze(60))}>
            1 時間止める
          </button>
          <button class="ghost" onClick={() => act.run(() => api.snooze(0))}>
            止めるのをやめる
          </button>
        </div>
      </Card>
      <Card title="仕分け">
        <div class="stack">
          <label class="check">
            <input
              type="checkbox"
              checked={c.review?.trust_doing === true}
              onChange={(e) => set("review.trust_doing", String(e.currentTarget.checked))}
            />
            始めたタスクを信頼する（進行中のタスクに仮で当てた時間も、確認待ちにしない）
          </label>
          <div class="form-row">
            <Field label="ロック・スリープがこの分数以上なら自動で休憩にする（0 で無効）">
              <input
                type="number"
                min="0"
                value={c.review?.auto_break_locked_min ?? 10}
                onChange={(e) => set("review.auto_break_locked_min", String(Math.max(0, Number(e.currentTarget.value) || 0)))}
              />
            </Field>
            <Field label="無操作がこの分数以上続いて戻ったら「さっきは？」と尋ねる（0 で無効）">
              <input
                type="number"
                min="0"
                value={c.push?.idle_return_min ?? 15}
                onChange={(e) => set("push.idle_return_min", String(Math.max(0, Number(e.currentTarget.value) || 0)))}
              />
            </Field>
          </div>
          <div class="form-row">
            <Field label="緊急のしるしが付いたタスクを、この分数おきに知らせる（0 で無効）">
              <input
                type="number"
                min="0"
                value={c.push?.urgent_task_interval_min ?? 120}
                onChange={(e) => set("push.urgent_task_interval_min", String(Math.max(0, Number(e.currentTarget.value) || 0)))}
              />
            </Field>
            <Field label="重要なタスクに、この日数以上時間が入っていなければ知らせる（0 で無効）">
              <input
                type="number"
                min="0"
                value={c.push?.important_stale_days ?? 3}
                onChange={(e) => set("push.important_stale_days", String(Math.max(0, Number(e.currentTarget.value) || 0)))}
              />
            </Field>
          </div>
          <div class="form-row">
            <Field label="今週まだ時間が入っていない KR を知らせる曜日（1=月 2=火 3=水 4=木 5=金 6=土 7=日、0 で無効）">
              <input
                type="number"
                min="0"
                max="7"
                value={c.push?.kr_idle_weekday ?? 3}
                onChange={(e) =>
                  set("push.kr_idle_weekday", String(Math.min(7, Math.max(0, Number(e.currentTarget.value) || 0))))
                }
              />
            </Field>
          </div>
        </div>
      </Card>
      <IgnoreCard />
      <DataSettings />
      <Card title="はじめに">
        <div class="row">
          <button
            class="ghost"
            onClick={async () => {
              if (await act.run(() => api.onboardingReset())) location.hash = "#/onboarding";
            }}
          >
            オンボーディングをもう一度見る
          </button>
        </div>
      </Card>
      <Card title="設定ファイル">
        <p class="meta">
          {path.data?.path}
          <br />
          DB: {path.data?.db}
        </p>
        <form
          class="form-row"
          onSubmit={(e) => {
            e.preventDefault();
            set(key.trim(), value.trim());
          }}
        >
          <Field label="キー">
            <input value={key} onInput={(e) => setKey(e.currentTarget.value)} placeholder="push.daily_limit" />
          </Field>
          <Field label="値（TOML）">
            <input value={value} onInput={(e) => setValue(e.currentTarget.value)} placeholder='6 / true / "08:00"' />
          </Field>
          <button disabled={act.busy || !key.trim()}>保存</button>
        </form>
        {msg && <p class="meta" role="status">{msg}</p>}
        <details>
          <summary>現在の設定</summary>
          <pre>{JSON.stringify(cfg.data, null, 2)}</pre>
        </details>
      </Card>
    </div>
  );
}

/** 無視するアプリ（collect.ignore_exes）の一覧・追加・削除。変更すると直近 7 日を再計算する。 */
function IgnoreCard() {
  const list = useAsync(() => api.ignoreList(), []);
  const [exe, setExe] = useState("");
  const act = useAction();
  const exes = list.data?.exes ?? [];
  const name = normalizeExe(exe);
  const add = async () => {
    if (await act.run(() => api.ignoreAdd(name))) {
      setExe("");
      list.reload();
    }
  };
  const remove = async (e: string) => {
    if (await act.run(() => api.ignoreRemove(e))) list.reload();
  };
  return (
    <Card
      title="無視するアプリ"
      help="ここのアプリが前面の時間は、短いものは前後の作業に含め、続いたものは集計・仕分け・レポートに数えません。記録は消えず、外せば元に戻ります。追加・削除すると直近 7 日を再計算します。"
    >
      <div class="stack">
        <ErrorBox error={list.error ?? act.error} />
        <ul class="plain">
          {exes.map((e) => (
            <li key={e} class="row">
              <span>{e}</span>
              <span class="spacer" />
              <OverflowMenu label={`${e} の操作`} items={[{ label: "外す", onSelect: () => remove(e), disabled: act.busy }]} />
            </li>
          ))}
        </ul>
        <form
          class="form-row"
          onSubmit={(ev) => {
            ev.preventDefault();
            void add();
          }}
        >
          <Field label="追加する exe 名">
            <input value={exe} onInput={(e) => setExe(e.currentTarget.value)} placeholder="explorer.exe" />
          </Field>
          <button disabled={act.busy || !name || hasExe(exes, name)}>追加</button>
        </form>
      </div>
    </Card>
  );
}
