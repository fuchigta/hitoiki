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

GitHub Releases から取得します。コード署名は付与していません。

### インストーラー（推奨）

1. [Releases](https://github.com/fuchigta/hitoiki/releases) から `hitoiki-<ver>-windows-x64-setup.exe` をダウンロードします。
2. 実行します。SmartScreen の警告が出たら「詳細情報」→「実行」を選びます（下記）。
3. 画面の指示に従ってインストールします。管理者権限は不要で、既定のインストール先は `%LOCALAPPDATA%\HitoikiApp` です（Windows ではフォルダ名の大文字小文字が区別されないため、データ用の `%LOCALAPPDATA%\hitoiki` と衝突しないよう製品名を `HitoikiApp` にしています）。
4. 完了画面の「実行」のチェックを入れたままにすると、そのまま起動して初回のオンボーディングが始まります。

インストーラーが行うこと:

- `hitoiki.exe`（CLI）と `hitoiki-app.exe`（常駐アプリ）を同じフォルダに置きます。
- インストール先を**ユーザー**の環境変数 `Path` に追加します（重複追加はしません）。新しく開いたターミナルで `hitoiki --help` が使えます。
- スタートメニューに登録します（スタートメニュー名は「HitoikiApp」です）。

アンインストールは「設定 > アプリ > インストールされているアプリ」から行います。

- 消えるもの: インストール先のファイル、スタートメニューのショートカット、ユーザー `Path` に追加したエントリ、ログオン時の自動起動登録（`general.autostart`）。
- 残るもの: **Hitoiki のデータ（`%APPDATA%\hitoiki` の設定とプラグイン、`%LOCALAPPDATA%\hitoiki` の DB とログ）は消しません。**
- アンインストール画面の「アプリデータを削除」チェックは、Tauri の識別子（`dev.hitoiki.app`）名のフォルダ（`%APPDATA%\dev.hitoiki.app`、`%LOCALAPPDATA%\dev.hitoiki.app`、WebView2 のキャッシュなど）だけが対象です。Hitoiki の DB・設定は対象外なので、チェックしても消えません。DB ごと消したい場合は上記フォルダを手動で削除してください。
- 更新するときは、新しいインストーラーをそのまま実行して上書きインストールできます（データはそのままです）。

### zip（ポータブル版）

インストーラーを使わず、展開するだけで使うこともできます。

1. [Releases](https://github.com/fuchigta/hitoiki/releases) から `hitoiki-<ver>-windows-x64.zip` をダウンロードします。
2. ダウンロードした zip を右クリックし、「プロパティ」を開きます。
3. 「全般」タブ下部の「セキュリティ」欄にある **「ブロックの解除」** にチェックを入れ、「OK」を押します。
4. zip を任意のフォルダ（例: `C:\Tools\hitoiki`）に展開します。`hitoiki.exe` と `hitoiki-app.exe` は同じフォルダに置いたままにしてください。
5. 展開したフォルダを PATH に追加します（「環境変数」→ユーザー環境変数の `Path` に追加）。
6. 新しいターミナルを開き、`hitoiki --help` で動作を確認します。

### SmartScreen の警告が出る場合

コード署名がないため、実行時に「Windows によって PC が保護されました」と表示されることがあります。
その場合は「詳細情報」→「実行」を選んでください（zip の場合は上記の「ブロックの解除」でも回避できます）。ダウンロード元が上記の公式 Releases であることを確認してから実行してください。

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

## 常駐アプリと UI

インストーラーと zip には CLI（`hitoiki.exe`）に加えて、タスクトレイに常駐する GUI アプリ（`hitoiki-app.exe`）が入っています。

- **`hitoiki-app.exe`**: 起動するとトレイに常駐し、ウインドウ・アプリの利用状況の収集、エージェントログの取り込み、判定の再計算、カレンダー同期、プッシュ通知をまとめて行います（実体は `hitoiki daemon` と同じ常駐処理です）。ウインドウを閉じても常駐を続け、トレイの「終了」で止まります（その時点の区間は閉じて保存されます）。`general.autostart = true` でログオン時に自動起動します。
- **`hitoiki ui [画面]`**: 常駐アプリに画面を開かせます。画面は `today` / `review` / `tasks` / `plan` / `reflect` / `report` / `records` / `okr` / `settings`。アプリが動いていなければ、同じフォルダの `hitoiki-app.exe` を `--screen <画面>` 付きで起動します。
- **`hitoiki daemon`**: UI なしの常駐処理です。トースト通知は出ません（ログのみ）。`hitoiki daemon` が動いている間に `hitoiki-app.exe` を起動すると、アプリは UI だけで動き、収集は daemon 側で行われます（画面とトレイにその旨が表示されます）。
- **`hitoiki doctor`**: 収集・DB・設定・AI コマンドなどの状態を診断します。うまく動かないときに最初に実行してください。

どの経路でも、常駐処理は同時に 1 つだけです（名前付きパイプの重複で判定）。

### 振り返り（AI）

「振り返り」画面で、始業・終業・週次の問いを AI に作らせ、回答を書いて保存できます。AI の呼び出しは設定の `ai.command` で指定したコマンドに標準入力でプロンプトを渡す方式で、Hitoiki 自身はネットワークに接続しません。通知（始業・終業・週次）から開くと、該当する種別の画面が開きます。

### 通知

通知は Windows のトースト（ボタン付き）で出します。トーストが出せない場合は、画面右下の小さなポップアップに切り替えます。通知のボタンや入力欄からの応答は、チェックイン・ハッスル・通知の一時停止として保存されます。

> **要検証**: トーストのボタン押下・入力欄の値の受け取りは、環境（Windows のバージョン、集中モード、通知設定）によって動作を十分には確認できていません。ボタンが効かないときは、設定画面の「テスト通知を出す」で確認し、通知本体をクリックして画面から答えてください。

## 開発

前提: Rust（stable）、Windows 環境、Node.js 20 以上（常駐アプリのフロントエンド用）。

```powershell
# 常駐アプリのフロントエンドを先にビルドする（ui/dist が hitoiki-app に埋め込まれる。
# ui/dist が無いと hitoiki-app はビルドできない）
cd ui; npm ci; npm run build; cd ..

# テスト
cargo test --workspace

# Lint
cargo clippy --workspace --all-targets -- -D warnings

# フォーマット
cargo fmt --all

# ビルド（CLI と常駐アプリ）
cargo build --release -p hitoiki-cli -p hitoiki-app
```

ワークスペース構成や設計の詳細は [`hitoiki-design.md`](./hitoiki-design.md) と [`CLAUDE.md`](./CLAUDE.md) を参照してください。

依存クレートのライセンスは [cargo-deny](https://github.com/EmbarkStudios/cargo-deny) で確認しています（`cargo deny check licenses`、CI でも実行）。

## ライセンス

以下のいずれかのライセンスで利用できます（デュアルライセンス）。

- MIT License（[LICENSE-MIT](./LICENSE-MIT)）
- Apache License, Version 2.0（[LICENSE-APACHE](./LICENSE-APACHE)）

お好きな方を選んでください。
