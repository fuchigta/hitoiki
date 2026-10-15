import { useState } from "preact/hooks";
import { api } from "../api";
import { Card, ErrorBox, Field } from "../components";
import { ActivityCard, CalendarSourcesCard } from "../data-settings";
import { QuickShortcutCard } from "../quick-shortcut";
import { hrefOf, navigate, useAction, useAsync, useRoute } from "../lib";
import { AutostartToggle, NumberSetting, TrustDoingToggle, UpdateCheckToggle, useConfig, type ConfigHandle } from "../settings-parts";
import { RulesTab } from "../settings-rules";
import { SETTINGS_TABS, settingsTab } from "../settings-util";
import { TemplateManager } from "../templates";
import { Tabs } from "../ui/parts";

/** 設定。左のサブナビ（狭いときは上）でタブを切り替える。タブは URL（`#/settings/<tab>`）に載る。 */
export function Settings() {
  const route = useRoute();
  const tab = settingsTab(route.path[0]);
  return (
    <div class="settings">
      <Tabs
        tabs={[...SETTINGS_TABS]}
        active={tab}
        idBase="settings"
        label="設定の項目"
        orientation="vertical"
        panels={false}
        onChange={(id) => navigate(hrefOf("settings", [id]))}
      />
      <div class="settings-body stack" role="tabpanel" id={`settings-panel-${tab}`} aria-labelledby={`settings-tab-${tab}`}>
        {tab === "general" && <GeneralTab />}
        {tab === "notify" && <NotifyTab />}
        {tab === "actual" && <ActualTab />}
        {tab === "rules" && <RulesTab />}
        {tab === "labels" && <ActivityCard />}
        {tab === "calendar" && <CalendarSourcesCard />}
        {tab === "templates" && <TemplatesTab />}
        {tab === "data" && <DataTab />}
      </div>
    </div>
  );
}

function GeneralTab() {
  const cfg = useConfig();
  const act = useAction();
  return (
    <>
      <ErrorBox error={cfg.error} />
      <Card title="全般">
        <div class="stack">
          <AutostartToggle cfg={cfg} />
          <UpdateCheckToggle cfg={cfg} />
          <p class="meta">タイムゾーン（表示）: {cfg.c.general?.timezone ?? "-"}</p>
        </div>
      </Card>
      <QuickShortcutCard />
      <Card title="はじめに">
        <div class="stack">
          <ErrorBox error={act.error} />
          <div class="row">
            <button
              class="ghost"
              onClick={async () => {
                if (await act.run(() => api.onboardingReset())) navigate("#/onboarding");
              }}
            >
              はじめにをもう一度見る
            </button>
          </div>
        </div>
      </Card>
    </>
  );
}

function NotifyTab() {
  const cfg = useConfig();
  const act = useAction();
  const p = cfg.c.push;
  return (
    <>
      <ErrorBox error={cfg.error ?? act.error} />
      <Card title="通知">
        <div class="stack">
          <label class="check">
            <input
              type="checkbox"
              checked={p?.enabled !== false}
              disabled={cfg.busy}
              onChange={(e) => cfg.set("push.enabled", String(e.currentTarget.checked))}
            />
            問いかけの通知を受け取る
          </label>
          <div class="row">
            <button onClick={() => act.run(() => api.notifyTest())}>テスト通知を出す</button>
            <button class="ghost" onClick={() => act.run(() => api.snooze(60))}>
              1 時間止める
            </button>
            <button class="ghost" onClick={() => act.run(() => api.snooze(0))}>
              止めるのをやめる
            </button>
          </div>
        </div>
      </Card>
      <Card title="通知のタイミング">
        <div class="stack">
          <NumberSetting
            cfg={cfg}
            k="push.idle_return_min"
            label="戻ったときの確認"
            help="無操作がこの分数以上続いて戻ったら「さっきは？」と尋ねます。"
            value={p?.idle_return_min ?? 15}
            unit="分"
            zeroOff
          />
          <NumberSetting
            cfg={cfg}
            k="push.urgent_task_interval_min"
            label="緊急タスクの間隔"
            help="緊急のしるしが付いたタスクを、この分数おきに知らせます。"
            value={p?.urgent_task_interval_min ?? 120}
            unit="分おき"
            zeroOff
          />
          <NumberSetting
            cfg={cfg}
            k="push.important_stale_days"
            label="重要タスクの日数"
            help="重要なタスクに、この日数以上時間が入っていなければ知らせます。"
            value={p?.important_stale_days ?? 3}
            unit="日"
            zeroOff
          />
          <NumberSetting
            cfg={cfg}
            k="push.kr_idle_weekday"
            label="成果の指標を知らせる曜日"
            help="今週まだ時間が入っていない成果の指標を知らせる曜日です。1=月 2=火 3=水 4=木 5=金 6=土 7=日。"
            value={p?.kr_idle_weekday ?? 3}
            unit="（1〜7）"
            zeroOff
            max={7}
          />
        </div>
      </Card>
    </>
  );
}

function ActualTab() {
  const cfg = useConfig();
  return (
    <>
      <ErrorBox error={cfg.error} />
      <Card title="判定" help="時間をどのタスク・活動に当てるかの扱いです。確信が持てないものは、捨てずに確認待ちになります。">
        <div class="stack">
          <TrustDoingToggle cfg={cfg} />
          <NumberSetting
            cfg={cfg}
            k="actual.auto_break_locked_min"
            label="ロック・スリープで自動休憩"
            help="ロック・スリープがこの分数以上なら、その時間を自動で休憩にします。"
            value={cfg.c.actual?.auto_break_locked_min ?? 10}
            unit="分以上"
            zeroOff
          />
        </div>
      </Card>
    </>
  );
}

function TemplatesTab() {
  const tasks = useAsync(() => api.taskList(["todo", "doing"]), []);
  return (
    <Card
      title="テンプレート（働き方のパターン）"
      help="よく使う 1 日の配置を保存しておき、曜日の既定にすると、今日の画面の「今日のデザイン」で提案されます。今日の配置からの保存は、今日の画面の「デザイン」から行えます。"
    >
      <TemplateManager tasks={tasks.data?.tasks ?? []} />
    </Card>
  );
}

function DataTab() {
  const cfg: ConfigHandle = useConfig();
  const path = useAsync(() => api.configPath(), []);
  const [key, setKey] = useState("");
  const [value, setValue] = useState("");
  const [msg, setMsg] = useState("");
  return (
    <Card title="設定ファイル">
      <div class="stack">
        <ErrorBox error={cfg.error} />
        <p class="meta">
          {path.data?.path}
          <br />
          DB: {path.data?.db}
        </p>
        <form
          class="form-row"
          onSubmit={async (e) => {
            e.preventDefault();
            const k = key.trim();
            if (await cfg.set(k, value.trim())) setMsg(`${k} を保存しました。`);
          }}
        >
          <Field label="キー">
            <input value={key} onInput={(e) => setKey(e.currentTarget.value)} placeholder="push.daily_limit" />
          </Field>
          <Field label="値（TOML）">
            <input value={value} onInput={(e) => setValue(e.currentTarget.value)} placeholder='6 / true / "08:00"' />
          </Field>
          <button disabled={cfg.busy || !key.trim()}>保存</button>
        </form>
        {msg && <p class="meta" role="status">{msg}</p>}
        <details>
          <summary>現在の設定</summary>
          <pre>{JSON.stringify(cfg.raw, null, 2)}</pre>
        </details>
      </div>
    </Card>
  );
}
