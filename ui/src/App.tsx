import { useEffect, useState } from "preact/hooks";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { api, on } from "./api";
import { LeaveConfirm, RuntimeBanner } from "./components";
import { leaveGuard } from "./edit-util";
import { SCREENS, hrefOf, loadTimeZone, navigate, useAsync, useLiveTick, useScreen, useTimeZoneVersion, type ScreenId } from "./lib";
import { Log } from "./screens/Log";
import { Onboarding } from "./screens/Onboarding";
import { Okr } from "./screens/Okr";
import { Popup } from "./screens/Popup";
import { Quick } from "./screens/Quick";
import { Records } from "./screens/Records";
import { Report } from "./screens/Report";
import { Settings } from "./screens/Settings";
import { Tasks } from "./screens/Tasks";
import { Today } from "./screens/Today";

const VIEWS: Record<ScreenId, () => preact.JSX.Element> = {
  today: Today,
  log: Log,
  tasks: Tasks,
  reflect: Records, // 旧名。`legacyRoute` が `records` に写すので、ここは通らない
  report: Report,
  records: Records,
  okr: Okr,
  settings: Settings,
  onboarding: Onboarding,
};

/** 画面を切り替える。未保存の変更があれば確認してから。 */
function goto(hash: string) {
  navigate(hash);
}

function windowLabel(): string {
  try {
    return getCurrentWindow().label;
  } catch {
    return "main"; // Tauri 外（ブラウザ）で開いた場合
  }
}

export function App() {
  const label = windowLabel();
  if (label === "popup") return <Popup />;
  if (label === "quick") return <Quick />;
  return <Main />;
}

function Main() {
  const screen = useScreen();
  const tzVersion = useTimeZoneVersion();
  // 実績の要確認の件数（ナビのバッジ）。
  const tick = useLiveTick();
  const status = useAsync(() => api.status().catch(() => null), [tick]);
  const reviewCount = status.data?.needs_review_count ?? 0;
  // 時刻表示は設定のタイムゾーンに従う。読み込めるまで描画を待つ（ずれた時刻を一瞬出さない）。
  const [tzReady, setTzReady] = useState(false);
  useEffect(() => {
    loadTimeZone().finally(() => setTzReady(true));
  }, []);

  // `hitoiki ui <screen>`・トレイ・通知からの画面切り替え。
  useEffect(() => {
    const off = on<{ screen: string }>("open-screen", (p) => {
      goto(`#/${p.screen}`);
    }).catch(() => () => {});
    return () => {
      off.then((f) => f());
    };
  }, []);

  // 起動引数 `--screen` の指定。
  useEffect(() => {
    api
      .takeInitialScreen()
      .then((s) => {
        if (s) goto(`#/${s}`);
      })
      .catch(() => {});
  }, []);

  const View = VIEWS[screen];
  if (!tzReady) return <div class="shell" aria-busy="true" />;
  return (
    <div class="shell">
      <nav class="nav" aria-label="画面">
        <div class="brand">Hitoiki</div>
        {SCREENS.filter((s) => s.id !== "onboarding" && s.id !== "reflect").map((s) => (
          <a
            key={s.id}
            href={hrefOf(s.id)}
            class={screen === s.id ? "on" : ""}
            aria-current={screen === s.id ? "page" : undefined}
            onClick={(e) => {
              // 未保存の変更があれば、確認を挟んでから移動する。
              if (leaveGuard.isDirty()) {
                e.preventDefault();
                goto(hrefOf(s.id));
              }
            }}
          >
            {s.label}
            {s.id === "log" && reviewCount > 0 && (
              <span class="nav-badge" role="status" aria-label={`要確認 ${reviewCount} 件`}>
                {reviewCount}
              </span>
            )}
          </a>
        ))}
      </nav>
      <main class="content">
        <h1>{SCREENS.find((s) => s.id === screen)?.label}</h1>
        <RuntimeBanner />
        <View key={tzVersion} />
      </main>
      <LeaveConfirm />
    </div>
  );
}
