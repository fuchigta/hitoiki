import { useState } from "preact/hooks";
import { api } from "../api";
import { Card, ErrorBox, Field } from "../components";
import { useAction, useAsync } from "../lib";
import { Help } from "../ui/parts";
import { ONBOARDING_STEPS, clampStep, isLastStep } from "../onboarding-util";

export function Onboarding() {
  const [step, setStep] = useState(0);
  const act = useAction();
  const cur = ONBOARDING_STEPS[clampStep(step)];
  const last = isLastStep(step);

  const finish = async () => {
    const r = await act.run(() => api.onboardingComplete());
    if (r) location.hash = "#/today";
  };

  return (
    <div class="stack onboarding">
      <ol class="steps" aria-label="進み具合">
        {ONBOARDING_STEPS.map((s, i) => (
          <li key={s.id} class={i === step ? "on" : i < step ? "done" : ""} aria-current={i === step ? "step" : undefined}>
            {i + 1}. {s.label}
          </li>
        ))}
      </ol>
      <ErrorBox error={act.error} />

      {cur.id === "welcome" && <Welcome />}
      {cur.id === "task" && <TaskStep />}
      {cur.id === "startup" && <StartupStep />}
      {cur.id === "calendar" && <CalendarStep />}
      {cur.id === "daily" && <DailyStep />}

      <div class="row">
        {step > 0 && (
          <button class="ghost" onClick={() => setStep(clampStep(step - 1))}>
            戻る
          </button>
        )}
        {last ? (
          <button class="primary" disabled={act.busy} onClick={finish}>
            はじめる
          </button>
        ) : (
          <>
            <button class="primary" onClick={() => setStep(clampStep(step + 1))}>
              次へ
            </button>
            <button class="ghost" onClick={() => setStep(clampStep(step + 1))}>
              スキップ
            </button>
          </>
        )}
      </div>
    </div>
  );
}

function Welcome() {
  return (
    <Card title="Hitoiki へようこそ">
      <div class="stack">
        <p>
          Hitoiki は、パソコンでの「ひといき」までの時間を、自動で記録してタイムラインにします。
        </p>
        <ul>
          <li>前面のウインドウ（アプリ名とタイトル）と、無操作の時間を見ています。</li>
          <li>Claude Code / Codex のログから、AI に任せていた時間も拾います。</li>
          <li>記録はもう始まっています。いまは何もしなくて大丈夫です。</li>
        </ul>
        <p class="hint">
          データはこの PC にだけ保存します。ネットワークポートは開かず、キー入力は記録しません。
        </p>
      </div>
    </Card>
  );
}

function TaskStep() {
  const cfg = useAsync(() => api.configAll(), []);
  const trustDoing = ((cfg.data ?? {}) as { review?: { trust_doing?: boolean } }).review?.trust_doing === true;
  const [title, setTitle] = useState("");
  const [started, setStarted] = useState<string | null>(null);
  const act = useAction();

  const submit = async (e: Event) => {
    e.preventDefault();
    const t = title.trim();
    if (!t) return;
    const r = await act.run(async () => {
      const v = await api.taskAdd({ title: t });
      await api.taskStart(v.id);
      return v;
    });
    if (r) {
      setStarted(r.title);
      setTitle("");
    }
  };

  return (
    <Card title="いまやることを 1 つ決める">
      <div class="stack">
        <p>
          進行中（doing）のタスクがあると、ルールに当たらない時間はそのタスクの候補になります。
        </p>
        <label class="check">
          <input
            type="checkbox"
            checked={trustDoing}
            disabled={act.busy}
            onChange={async (e) => {
              const v = e.currentTarget.checked;
              if (await act.run(() => api.configSet("review.trust_doing", String(v)))) cfg.reload();
            }}
          />
          始めたタスクを信頼する（進行中のタスクに仮で当てた時間を、確認待ちにしない）
        </label>
        <ErrorBox error={act.error} />
        <form class="form-row" onSubmit={submit}>
          <Field label="タスク名">
            <input value={title} onInput={(e) => setTitle(e.currentTarget.value)} placeholder="例: 企画書を書く" />
          </Field>
          <button disabled={act.busy || !title.trim()}>作って開始</button>
        </form>
        {started && <p class="hint" role="status">「{started}」を開始しました。終わったら「今日」画面から完了にできます。</p>}
      </div>
    </Card>
  );
}

function StartupStep() {
  const cfg = useAsync(() => api.configAll(), []);
  const act = useAction();
  const [sent, setSent] = useState(false);
  const general = ((cfg.data ?? {}) as { general?: { autostart?: boolean; update_check?: boolean | null } }).general;
  const on = general?.autostart === true;
  // 未設定（null / 欠落）はまだ選んでいない状態。選ばずに進むと確認しない。
  const updateCheck = general?.update_check ?? null;

  return (
    <Card title="自動起動と通知">
      <div class="stack">
        <ErrorBox error={cfg.error ?? act.error} />
        <p>PC を使っている間ずっと記録するには、ログイン時に自動で起動しておくと便利です。</p>
        <label class="check">
          <input
            type="checkbox"
            checked={on}
            disabled={act.busy}
            onChange={async (e) => {
              const v = e.currentTarget.checked;
              if (await act.run(() => api.configSet("general.autostart", String(v)))) cfg.reload();
            }}
          />
          ログイン時に自動で起動する
        </label>
        <fieldset class="stack">
          <legend>
            新しいバージョンを確認する（1日1回、GitHub に問い合わせます。送る情報はありません）{" "}
            <Help>選ばずに進むと確認しません。あとから「設定」画面で変えられます。自動でダウンロードやインストールはしません。</Help>
          </legend>
          <label class="check">
            <input
              type="radio"
              name="update-check"
              checked={updateCheck === true}
              disabled={act.busy}
              onChange={async () => {
                if (await act.run(() => api.configSet("general.update_check", "true"))) cfg.reload();
              }}
            />
            はい
          </label>
          <label class="check">
            <input
              type="radio"
              name="update-check"
              checked={updateCheck === false}
              disabled={act.busy}
              onChange={async () => {
                if (await act.run(() => api.configSet("general.update_check", "false"))) cfg.reload();
              }}
            />
            いいえ
          </label>
        </fieldset>
        <p>Hitoiki は、ときどき通知で「いま何してた？」と聞きます。1 タップで答えられます。</p>
        <div class="row">
          <button
            onClick={async () => {
              if ((await act.run(() => api.notifyTest())) !== undefined || !act.error) setSent(true);
            }}
          >
            テスト通知を出す
          </button>
        </div>
        {sent && <p class="hint" role="status">通知が出ましたか？ 出ないときは Windows の通知設定（集中モードなど）を確認してください。</p>}
      </div>
    </Card>
  );
}

function CalendarStep() {
  return (
    <Card title="カレンダー（任意）">
      <div class="stack">
        <p>カレンダーの ICS URL を登録すると、会議の時間を自動で判定できます。</p>
        <p class="hint">
          「設定」画面の「設定ファイル」で、キー <code>calendar.sources</code> に
          <code>{`[{ name = "仕事", type = "ics", url = "https://…/basic.ics" }]`}</code>
          の形で登録します。URL は秘密情報なので、他の人に見せないでください。あとからでも設定できます。
        </p>
        <div class="row">
          <a href="#/settings">設定を開く</a>
        </div>
      </div>
    </Card>
  );
}

function DailyStep() {
  return (
    <Card title="日々の使い方">
      <ul>
        <li>
          <a href="#/today">今日</a>: タイムラインで一日の流れを確認します。
        </li>
        <li>
          <a href="#/review">仕分け</a>: 要確認の区間を、タスクなどに確定します。
        </li>
        <li>通知に 1 タップで答えると、記録の精度が上がります。</li>
        <li>
          終業時に<a href="#/reflect">振り返り</a>をすると、一日を軽く締められます。
        </li>
      </ul>
      <p class="hint">
        コマンドラインでも同じことができます。たとえば <code>hitoiki status</code> で状況を見られます。
      </p>
    </Card>
  );
}
