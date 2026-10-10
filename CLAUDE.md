# Hitoiki 開発メモ

設計書: `hitoiki-design.md`（「決定」は変更しない、「推奨」は理由があれば変更可、「要検証」は実際の挙動を確認する）。

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
