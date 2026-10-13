import { render } from "preact";
import { App } from "./App";
import "./styles.css";

async function start() {
  // `vite --mode mock`（npm run dev:mock）のときだけ、Tauri の IPC をモックに差し替える。
  // 本番ビルドでは条件がコンパイル時に false になり、モックのコードは成果物に入らない。
  if (import.meta.env.MODE === "mock") {
    const { installMock } = await import("./dev");
    installMock();
  }
  render(<App />, document.getElementById("app")!);
}

void start();
