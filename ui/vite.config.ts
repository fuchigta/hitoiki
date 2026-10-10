import { defineConfig } from "vite";

// 本番は Tauri が ui/dist を直接読む（ローカルサーバもポートも持たない）。
// 開発時のみ Vite の開発サーバを使う（tauri.conf.json の devUrl）。
export default defineConfig({
  clearScreen: false,
  esbuild: { jsx: "automatic", jsxImportSource: "preact" },
  server: { port: 5173, strictPort: true },
  build: { target: "es2022", outDir: "dist", emptyOutDir: true },
});
