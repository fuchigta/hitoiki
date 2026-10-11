import { useEffect, useState } from "preact/hooks";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { api, on } from "./api";
import { RuntimeBanner } from "./components";
import { SCREENS, loadTimeZone, useScreen, useTimeZoneVersion, type ScreenId } from "./lib";
import { Onboarding } from "./screens/Onboarding";
import { Okr } from "./screens/Okr";
import { Plan } from "./screens/Plan";
import { Popup } from "./screens/Popup";
import { Records } from "./screens/Records";
import { Reflect } from "./screens/Reflect";
import { Report } from "./screens/Report";
import { Review } from "./screens/Review";
import { Settings } from "./screens/Settings";
import { Tasks } from "./screens/Tasks";
import { Today } from "./screens/Today";

const VIEWS: Record<ScreenId, () => preact.JSX.Element> = {
  today: Today,
  review: Review,
  tasks: Tasks,
  plan: Plan,
  reflect: Reflect,
  report: Report,
  records: Records,
  okr: Okr,
  settings: Settings,
  onboarding: Onboarding,
};

function windowLabel(): string {
  try {
    return getCurrentWindow().label;
  } catch {
    return "main"; // Tauri 外（ブラウザ）で開いた場合
  }
}

export function App() {
  if (windowLabel() === "popup") return <Popup />;
  return <Main />;
}

function Main() {
  const screen = useScreen();
  const tzVersion = useTimeZoneVersion();
  // 時刻表示は設定のタイムゾーンに従う。読み込めるまで描画を待つ（ずれた時刻を一瞬出さない）。
  const [tzReady, setTzReady] = useState(false);
  useEffect(() => {
    loadTimeZone().finally(() => setTzReady(true));
  }, []);

  // `hitoiki ui <screen>`・トレイ・通知からの画面切り替え。
  useEffect(() => {
    const off = on<{ screen: string }>("open-screen", (p) => {
      location.hash = `#/${p.screen}`;
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
        if (s) location.hash = `#/${s}`;
      })
      .catch(() => {});
  }, []);

  const View = VIEWS[screen];
  if (!tzReady) return <div class="shell" aria-busy="true" />;
  return (
    <div class="shell">
      <nav class="nav" aria-label="画面">
        <div class="brand">Hitoiki</div>
        {SCREENS.filter((s) => s.id !== "onboarding").map((s) => (
          <a key={s.id} href={`#/${s.id}`} class={screen === s.id ? "on" : ""} aria-current={screen === s.id ? "page" : undefined}>
            {s.label}
          </a>
        ))}
      </nav>
      <main class="content">
        <h1>{SCREENS.find((s) => s.id === screen)?.label}</h1>
        <RuntimeBanner />
        <View key={tzVersion} />
      </main>
    </div>
  );
}
