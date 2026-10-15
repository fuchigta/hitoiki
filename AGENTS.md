# Hitoiki 開発メモ

用語: `GLOSSARY.md`（ユビキタス言語。画面・CLI・文書・コードの名前はここに揃える。用語を変えるときは先にここを直す）。決定の記録: `docs/adr/`。

設計: `docs/dev/architecture.md`（現在の実装に基づく設計ドキュメント。コードと食い違う場合はコードが正）。文書の構成は下の「ドキュメント」を参照。

## 守るべき制約（変更しない）

- ローカル完結。データの同期・クラウド保存・テレメトリは持たない。ネットワークに出てよいのは、設定で明示した ICS 取得と OpenAI 互換判定器、`hitoiki version --check` の実行時、`general.update_check = true` のときの 1 日 1 回の最新版確認だけ。
- ネットワークポートを開かない。UI は Tauri の WebView 内でのみ表示し、ブラウザ向け配信やローカル HTTP サーバは持たない。プロセス間通信は名前付きパイプ（作成者ユーザーのみアクセス可）。
- キー入力（内容も回数も）を記録しない。低レベルフック（`WH_KEYBOARD_LL` 等）、DLL インジェクション、他プロセスのメモリ読み取りは使わない（`RegisterHotKey` による、設定で指定したショートカットの登録は可。登録した組み合わせの押下通知だけを受け取るもので、キー入力の記録ではない）。管理者権限を要求しない。
- ウインドウタイトル、プロンプト内容、カレンダー ICS URL をログ・診断メッセージに出さない。
- 実績入力フィールドは作らない。実績は常に `activity_segment` から算出し、見積もりは `task.estimate_min`（任意）のみ、計画は `time_block` で表す。
- 判断はユーザーに委ねる。確信が持てない自動判定は捨てずに `needs_review` にし、ユーザーが確定したものは `segment_override` として最優先で適用する。時間の使い方に良し悪しの評価をつけない（通知・AI 振り返りの文言も同様）。
- すべての機能は CLI から使えるようにする。GUI は CLI と同じ core のロジックの上に載せる。
- ライセンスは MIT OR Apache-2.0 のデュアル。各クレートの `Cargo.toml` に `license = "MIT OR Apache-2.0"` を設定する。

## 構成

- Cargo ワークスペース。`crates/*` が自動でメンバーになる。依存は `[workspace.dependencies]` に追加して `xxx.workspace = true` で参照する。
- `hitoiki-core`: ドメインモデル（`model.rs`）、スキーマ（`migrations/*.sql`、`db.rs` の `MIGRATIONS` に登録）、設定（`config.rs`）、時刻（`timefmt.rs`、`clock.rs`）。CLI と Tauri は必ず core の関数を呼ぶ（ロジックの重複禁止）。
- スキーマ変更は既存のマイグレーションを書き換えず、`0002_xxx.sql` を追加する（初回リリース前は 0001 の修正も可）。

## 規約

- 時刻は UTC の Unix ミリ秒（`Millis = i64`）。現在時刻は `Clock` トレイト経由で取得し、`Utc::now()` を直接呼ばない（テストのため）。
- ID は `id::new_id()`（ULID）。
- エラーは `hitoiki_core::Error`。CLI の終了コードは `Error::exit_code()`（0 成功、1 一般、2 引数、3 対象なし、4 曖昧）。
- `HITOIKI_HOME` 環境変数で設定・DB の置き場所を一括で差し替えられる（テスト用）。
- コメント・ドキュメント・ユーザー向けメッセージは日本語。
- キー入力の記録、低レベルフック（`WH_KEYBOARD_LL` 等）、DLL インジェクション、ネットワークポートを開くことは禁止。
- 依存クレートは MIT / Apache-2.0 / BSD 等の permissive ライセンスのみ（`cargo deny check` で検査。新規クレートには `publish = false` を付ける）。

## コマンド

- テスト: `cargo test --workspace`
- Lint: `cargo clippy --workspace --all-targets -- -D warnings`
- フォーマット: `cargo fmt --all`
- 依存の検査: `cargo deny check`
- コミット前・CI の検査: `spotter check`（v0.13.0 以上。設置と使い方は `docs/dev/development.md`）

## ドキュメント

- 利用者向けは `docs/user/`（install / getting-started / daily-use / cli / configuration / privacy / troubleshooting）。README.md は概要とリンクだけにして薄く保つ。
- 開発者向けは `docs/dev/`（architecture / development / release）。
- 挙動を変えたら、対応する文書も同じコミットで更新する（ソースと文書の対応は `.spotter.yml` の `doc-sync` で検査される）。
- 利用者向けの変更は `CHANGELOG.md` の `[Unreleased]` に反映する（リリース時にまとめて整える。手順は `docs/dev/release.md`）。

## コミット規約

Conventional Commits 形式 `<type>(<scope>): <日本語の要約>`（spotter の `commit-subject` で検査される）。

- type: `feat` / `fix` / `perf` / `refactor` / `docs` / `test` / `build` / `ci` / `chore` / `revert`
- scope は任意。crate 名から `hitoiki-` を除いたもの（`core`, `collect`, `collect-win`, `agents`, `calendar`, `judge`, `ipc`, `cli`, `daemon`, `app`）、または `ui`, `docs`, `release` など。
- 破壊的変更は `!` を付ける（例: `feat(cli)!: ...`）。
- 要約・本文は日本語。マージコミットは検査の対象外。

## リリース

バージョンの上げ方、CHANGELOG の作り方、タグの push、インストーラー（NSIS）のローカルビルドは `docs/dev/release.md` にある。インストーラーの注意点（`externalBin` は `tauri.bundle.conf.json` にだけ書く、`productName` は `Hitoiki`（インストール先がデータ置き場と同じフォルダになる）、自動起動エントリ名は `Hitoiki` 固定）も同文書を参照。
