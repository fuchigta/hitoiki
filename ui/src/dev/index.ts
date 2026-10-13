// 開発用モック（`vite --mode mock`）の入口。Tauri の IPC を差し替えて、ブラウザだけで UI を表示・操作できるようにする。
// main.tsx から `import.meta.env.MODE === "mock"` のときだけ動的 import される（本番ビルドには含まれない）。
//
// URL クエリ:
//   ?scenario=default|empty|busy   データの状態（既定は default）
//   ?window=main|popup|quick       ウインドウの種別（既定は main）

import { emit } from "@tauri-apps/api/event";
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { createMock } from "./handlers";
import { SCENARIOS, type Scenario } from "./state";

const WINDOWS = ["main", "popup", "quick"] as const;

export function installMock(): void {
  const q = new URLSearchParams(location.search);
  const sc = q.get("scenario") as Scenario | null;
  const scenario: Scenario = sc && SCENARIOS.includes(sc) ? sc : "default";
  const w = q.get("window");
  const label = (WINDOWS as readonly string[]).includes(w ?? "") ? (w as string) : "main";

  const mock = createMock(scenario, { emit: (name, payload) => void emit(name, payload) });
  mockWindows(label);
  mockIPC((cmd, args) => mock.call(cmd, (args ?? {}) as Record<string, unknown>), { shouldMockEvents: true });

  // スクリーンショット・デバッグ用: コンソールから状態を覗けるようにする。
  (window as unknown as { __mock: unknown }).__mock = mock;
  console.info(`[mock] scenario=${scenario} window=${label}`);
}
