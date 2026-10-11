# CLI リファレンス

`hitoiki` コマンドの一覧です。内容は `hitoiki --help` と各サブコマンドの `--help` から取っています。最新の引数は `--help` で確認してください。

## 共通の仕様

- **`--json`**: 参照系コマンドを機械可読な JSON で出力します（全コマンド共通のオプション）。
- **対話しない**: プロンプトは出しません。不足している引数はエラーになります。
- **終了コード**: 0 成功、1 一般エラー、2 引数エラー、3 対象なし、4 曖昧（ID の前方一致が複数あるなど）。
- **常駐していなくても動く**: CLI は DB を直接読み書きします。変更したときは、常駐処理が動いていれば名前付きパイプで通知します（失敗しても無視します）。
- **日付・期間の指定**: `--date` / `--period` は `today` / `yesterday` / `this-week` / `last-week` / `YYYY-MM-DD`。`--from` / `--to` は ISO 8601・ローカル日時・日付（`--to` が日付のみのときはその日の終わりまで）。
- **`HITOIKI_HOME`**: 設定と DB の置き場所を一括で差し替える環境変数です（テスト用）。

## コマンド一覧

### 状況・記録の確認

| コマンド | 内容 |
|---|---|
| `hitoiki status` | 現在の doing タスク、直近の区間、今日の集計、未応答のプッシュ、（`general.update_check = true` で新版が見つかっていれば）新しいバージョンの案内。ネットワークには出ず、保存済みの確認結果を使う |
| `hitoiki log [--date D] [--from F] [--to T] [--raw]` | 区間一覧。収集が止まっていた時間帯は「記録なし」の行で表示（JSON は `gaps`）。`--raw` で生データ（window_span / idle_span / agent_event）を時系列に混ぜて表示 |
| `hitoiki report [--period P] [--from F] [--to T] [--by 軸] [--include-idle]` | 期間内の集計。`--by` は `task`（既定）/ `activity` / `quadrant` / `okr` / `skill` / `agent` / `kind`。`--include-idle` で無操作・休憩も合計に含める |

### タスク

| コマンド | 内容 |
|---|---|
| `hitoiki task add <TITLE> [...]` | タスクを追加。`--body` / `--body-file`（本文。`--body -` で標準入力）、`--urgent` / `--not-urgent`、`--important` / `--not-important`、`--estimate`（`90m` / `1h30m` / 分）、`--kr`（KR の ID）、`--skill`、`--url`、`--plan-start` / `--plan-end`、`--interrupt`（割り込みタスク） |
| `hitoiki task list [--status S] [--kind K]` | 一覧。`--status` はカンマ区切りの `todo,doing,done,dropped,all`（既定 `todo,doing`） |
| `hitoiki task show <ID>` | 詳細 |
| `hitoiki task edit <ID> [...]` | 編集。`add` と同じオプションと `--title` |
| `hitoiki task start <ID>` | doing にする（`task.single_doing` が true なら他の doing を todo に戻す） |
| `hitoiki task done <ID>` / `hitoiki task drop <ID>` | 完了にする / やめる |
| `hitoiki task rule add <ID> --field F --pattern P [--type T] [--priority N]` | 帰属ルールを追加。`--field` は `exe` / `title` / `cwd` / `repo` / `url` / `git_branch`、`--type` は `glob`（既定）/ `regex` / `contains` |
| `hitoiki task rule list [ID]` | 帰属ルールの一覧 |

### 仕分け・再計算

| コマンド | 内容 |
|---|---|
| `hitoiki review list [--date D] [--from F] [--to T] [--all]` | 要確認（needs_review）の区間。`review.min_sec`（既定 60 秒）未満の短い区間は既定で隠し、末尾に件数を示す。`--all` で全件。`--json` には隠した件数 `hidden_short` が入る |
| `hitoiki review set <SEGMENT> [--task T] [--kind K] [--activity A] [--rule]` | 区間を確定。`--rule` で区間から帰属ルールを作る |
| `hitoiki rebuild [--date D] [--from F] [--to T]` | 派生層（区間）を再計算 |

### 計画・記録

| コマンド | 内容 |
|---|---|
| `hitoiki block add <START> <END> [--task T] [--label L] [--focus]` | 計画ブロックを追加（`--focus` は集中ブロック） |
| `hitoiki block list [--date D] [--from F] [--to T]` | 計画ブロックの一覧 |
| `hitoiki check [-e 1-5] [-m 1-5] [-s 1-5] [--erg TAG] [--note TEXT]` | コンディション（エネルギー・気分・ストレス）の記録 |
| `hitoiki hassle <TEXT> [-i 1-5]` | デイリーハッスルの記録（強度の既定は 3） |
| `hitoiki abc add [--a A] [--b B] [--c-emotion E] [--c-intensity 1-5] [--from-hassle ID]` | ABC 理論の記録 |
| `hitoiki abc dispute <ID>` | AI で受け止め方を掘り下げる問いを生成 |

### OKR・スキル

| コマンド | 内容 |
|---|---|
| `hitoiki okr add-objective <TITLE> [--from F] [--to T]` | Objective を追加 |
| `hitoiki okr add-kr <OBJECTIVE> <TITLE> [--target N]` | KR を追加 |
| `hitoiki okr list` | 一覧 |
| `hitoiki skill add <NAME> [--parent P]` | スキルを追加 |
| `hitoiki skill import-preset [NAME]` | 同梱プリセットを取り込む（省略で全部） |
| `hitoiki skill list` | 一覧 |

### 振り返り（AI）

| コマンド | 内容 |
|---|---|
| `hitoiki reflect day_start [--date D]` / `day_end` / `weekly` | AI で問いを生成して保存 |
| `hitoiki reflect answer <ID> [--file F]` | 回答を保存（`--file -` で標準入力） |
| `hitoiki reflect list [--kind K] [--limit N]` | 保存済みの振り返り一覧（新しい順。`--kind` は `day_start` / `day_end` / `weekly`） |
| `hitoiki reflect show <ID>` | 問いと回答の詳細 |
| `hitoiki context --kind K [--date D]` | AI 用のコンテキストを JSON で出力 |

### エージェントのセッション

| コマンド | 内容 |
|---|---|
| `hitoiki agent sessions [--date D]` | エージェントセッションの一覧 |
| `hitoiki agent import [--agent claude-code\|codex] [--since S]` | セッションログを手動で取り込む |

### 常駐・画面・診断・設定

| コマンド | 内容 |
|---|---|
| `hitoiki daemon` | ヘッドレス常駐（UI とトースト通知なし。Ctrl+C で停止） |
| `hitoiki ui [画面]` | 常駐アプリのウインドウを開く。未起動なら同じフォルダの `hitoiki-app.exe` を起動 |
| `hitoiki doctor [--network]` | 収集状態（直近 24 時間の記録停止を含む）、エージェントログ（ウインドウに対応付けできなかったプロンプトを含む）、カレンダー、通知、判定器などの診断。`--network` でカレンダー取得・判定器への疎通も試す（ネットワークを使う） |
| `hitoiki config path` | 設定ファイルのパス |
| `hitoiki config get <KEY>` / `hitoiki config set <KEY> <VALUE>` | ドット区切りのキーで読み書き。値は TOML として解釈し、書き換え後に全体を検証 |
| `hitoiki version [--check]` | バージョン表示。`--check` で GitHub Releases の最新版を確認（手動で実行したときのほか、`general.update_check = true` の常駐が 1 日 1 回行う確認と同じ処理） |
| `hitoiki ingest context` | 補助コレクタ用の取り込み |
| `hitoiki native-host [install\|uninstall] [--extension-id ID] [--firefox-extension-id ID]` | ブラウザ拡張のネイティブメッセージングホスト。`install` / `uninstall` で登録・解除 |

## `hitoiki ui` の画面名

`today`、`review`、`tasks`、`plan`、`reflect`、`report`、`records`、`okr`、`settings`、`onboarding`。それぞれ画面の名前は「今日」「仕分け」「タスク」「計画」「振り返り」「レポート」「記録」「OKR・スキル」「設定」「はじめに」です（[日々の使い方](./daily-use.md)）。`reflection` も `reflect` として受け付け、`reflect/day_start` のように種別まで指定できます。未知の名前は `today` になります。

## エージェントからの利用

Claude Code などのコーディングエージェントからも、同じコマンドを使えます。

- 参照系は `--json` を付けると解析しやすい出力になります。
- 対話的なプロンプトを出さないので、不足した引数はエラー（終了コード 2）になり、処理が止まりません。
- 終了コードで結果を判断できます（3 は対象が無い、4 は ID が曖昧）。
- `hitoiki context --kind day_end` のように、AI に渡す文脈を JSON で取り出せます。
- 例: タスクを作って開始する。

```powershell
hitoiki task add "不具合の調査" --important --json
hitoiki task start <id>
```

エージェント自身の稼働時間は、エージェントのセッションログから自動で取り込まれます（フックの設定は不要です）。`claude -p` など非対話の実行は作業時間に混ざりません。
