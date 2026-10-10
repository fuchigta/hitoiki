# Hitoiki（一息）

> **開発中です。** まだ機能は揃っておらず、仕様・コマンドは変わる可能性があります。

「一息つきませんか」と寄り添って声をかける、個人用の自己管理ツールです。
書籍「エンジニアのための自己管理入門」の考え方を日常業務に組み込むことを目的にしています。

タスク・タイムマネジメント、モチベーション、アンガー・ストレス、スキルの4領域を1つのタイムライン上で統合し、記録・可視化・内省を支援します。
コーディングエージェント（Claude Code など）との対話時間のような「名前の無い工数」を自動で捉え、タスクに紐づけて可視化することを重視しています。

## 設計原則

- **ローカル完結**: データはすべて自分のPCに保存します。同期・クラウド保存は行いません。
- **テレメトリなし**: 利用状況や内容を外部に送信しません。
- **ネットワークポートを一切開かない**: 待ち受け用のHTTPサーバやローカルポートは使いません。常駐アプリとCLIのやり取りは Windows の名前付きパイプ（作成者ユーザーのみアクセス可能）で行います。会社PCへの導入判断の材料にしてください。
- **キー入力を記録しない**: キーの内容も回数も取得しません。低レベルのキーボード/マウスフック（`WH_KEYBOARD_LL` 等）は使いません。
- **管理者権限不要**: 公開・低権限のWindows APIのみを使い、DLLインジェクションや他プロセスのメモリ読み取りは行いません。
- **判断はユーザーに委ねる**: 自動判定に確信が持てないものは捨てず、「要確認」として溜めて振り返り時に確定します。

## インストール

現時点では GitHub Releases からバイナリを取得します。コード署名は付与していません。

1. [Releases](https://github.com/fuchigta/hitoiki/releases) から `hitoiki-<ver>-windows-x64.zip` をダウンロードします。
2. ダウンロードした zip を右クリックし、「プロパティ」を開きます。
3. 「全般」タブ下部の「セキュリティ」欄にある **「ブロックの解除」** にチェックを入れ、「OK」を押します。
4. zip を任意のフォルダ（例: `C:\Tools\hitoiki`）に展開します。
5. 展開したフォルダを PATH に追加します（「環境変数」→ユーザー環境変数の `Path` に追加）。
6. 新しいターミナルを開き、`hitoiki --help` で動作を確認します。

### SmartScreen の警告が出る場合

コード署名がないため、実行時に「Windows によって PC が保護されました」と表示されることがあります。
その場合は「詳細情報」→「実行」を選んでください。ダウンロード元が上記の公式 Releases であることを確認してから実行してください。

## データの保存場所

| 用途 | パス |
|---|---|
| 設定 | `%APPDATA%\hitoiki\config.toml` |
| DB | `%LOCALAPPDATA%\hitoiki\hitoiki.db` |
| ログ | `%LOCALAPPDATA%\hitoiki\logs\` |
| 判定器・プラグイン | `%APPDATA%\hitoiki\plugins\` |

`HITOIKI_HOME` 環境変数を設定すると、設定・DBの置き場所をまとめて差し替えられます（テスト用）。

## 基本的な使い方

```powershell
# タスクを追加し、作業を開始する
hitoiki task add "設計書のレビュー" --important
hitoiki task start <id>

# 現在の状況を確認する
hitoiki status

# タスク別の時間の使い方を集計する
hitoiki report --by task

# 要確認の区間を一覧し、確定する
hitoiki review list

# 常駐処理をヘッドレス（UIなし）で起動する
hitoiki daemon
```

- 参照系コマンドは `--json` で機械可読な出力に切り替えられます。
- 対話的なプロンプトは出しません。不足している引数はエラーになります。
- 終了コードは 0（成功）、1（一般エラー）、2（引数エラー）、3（対象なし）、4（曖昧）です。

## 開発

前提: Rust（stable）、Windows 環境。

```powershell
# テスト
cargo test --workspace

# Lint
cargo clippy --workspace --all-targets -- -D warnings

# フォーマット
cargo fmt --all

# ビルド（CLI）
cargo build --release -p hitoiki-cli
```

ワークスペース構成や設計の詳細は [`hitoiki-design.md`](./hitoiki-design.md) と [`CLAUDE.md`](./CLAUDE.md) を参照してください。

依存クレートのライセンスは [cargo-deny](https://github.com/EmbarkStudios/cargo-deny) で確認しています（`cargo deny check licenses`、CI でも実行）。

## ライセンス

以下のいずれかのライセンスで利用できます（デュアルライセンス）。

- MIT License（[LICENSE-MIT](./LICENSE-MIT)）
- Apache License, Version 2.0（[LICENSE-APACHE](./LICENSE-APACHE)）

お好きな方を選んでください。
