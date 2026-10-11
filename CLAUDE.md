# Hitoiki 開発メモ

設計: `docs/architecture.md`（現在の実装に基づく設計ドキュメント。挙動を変えたら併せて更新する。コードと食い違う場合はコードが正）。

## 守るべき制約（変更しない）

- ローカル完結。データの同期・クラウド保存・テレメトリは持たない。ネットワークに出てよいのは、設定で明示した ICS 取得と OpenAI 互換判定器、`hitoiki version --check` の実行時だけ。
- ネットワークポートを開かない。UI は Tauri の WebView 内でのみ表示し、ブラウザ向け配信やローカル HTTP サーバは持たない。プロセス間通信は名前付きパイプ（作成者ユーザーのみアクセス可）。
- キー入力（内容も回数も）を記録しない。低レベルフック（`WH_KEYBOARD_LL` 等）、DLL インジェクション、他プロセスのメモリ読み取りは使わない。管理者権限を要求しない。
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

## インストーラー（NSIS）のビルド

- `ui` をビルド → `cargo build --release -p hitoiki-cli` → `target/release/hitoiki.exe` を `crates/hitoiki-app/binaries/hitoiki-x86_64-pc-windows-msvc.exe` にコピー → `cd crates/hitoiki-app && ../../ui/node_modules/.bin/tauri build --bundles nsis --config tauri.bundle.conf.json`（生成物は `target/release/bundle/nsis/`）。
- `externalBin` は `tauri.bundle.conf.json` にだけ書く（`tauri.conf.json` に書くと、サイドカー未配置の `cargo build` / `cargo test` が失敗する）。
- 製品名（`productName`）は `HitoikiApp`。`Hitoiki` にすると既定のインストール先が `%LOCALAPPDATA%\hitoiki`（データ置き場）と衝突する。
- 自動起動エントリ名は `lib.rs` で `Hitoiki` に固定している。インストーラーのフックもこの名前を削除する。
