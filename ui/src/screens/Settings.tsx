import { useState } from "preact/hooks";
import { api } from "../api";
import { Card, ErrorBox, Field } from "../components";
import { loadTimeZone, useAction, useAsync } from "../lib";

type Cfg = { general?: { timezone?: string; autostart?: boolean }; push?: { enabled?: boolean } };

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
          <p class="muted">タイムゾーン: {c.general?.timezone ?? "-"}</p>
        </div>
      </Card>
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
      <Card title="設定ファイル">
        <p class="muted">
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
        {msg && <p class="muted">{msg}</p>}
        <details>
          <summary>現在の設定</summary>
          <pre>{JSON.stringify(cfg.data, null, 2)}</pre>
        </details>
      </Card>
    </div>
  );
}
