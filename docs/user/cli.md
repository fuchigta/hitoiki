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
| `hitoiki task delete <ID> [--force] [--dry-run]` | 誤入力のタスクを削除する（やめる=記録として残す、削除=無かったことにする）。計上済みの時間・確定した割り当て・補正があるときは `--force` が無いと終了コード 2 で影響を表示して止まる。`--dry-run` は影響（区間数と時間、確定・補正・ルール・ブロックなどの件数）の表示だけで何も変えない。削除すると帰属ルール・スキルの紐付け・進行中の履歴・確定した割り当てと補正を消し、ブロックとテンプレート項目はタスクの指定だけを外し、影響した期間を再計算して未割り当てに戻す。`--json` で件数を取れる |
| `hitoiki task switch <タイトルまたは ID> [--new] [--interrupt]` | 未完了のタスクを選んですぐ開始する（割り込み用）。タイトルは完全一致、ID は前方一致（4 文字以上）、タイトルの部分一致の順に探す。複数に当たると終了コード 4、無いと 3。`--new` は探さずにタイトルとして新規作成して開始し、`--interrupt` を付けると割り込み（緊急）として作る（`--new` が必要）。`--json` の出力は `task start` と同じ形に `created` が加わる |
| `hitoiki task back` | 直前まで進行中だったタスクに戻る（割り込みが終わったとき）。完了・やめたタスクには戻らない。戻り先が無いと終了コード 3 |
| `hitoiki task rule add <ID> --field F --pattern P [--type T] [--priority N]` | 帰属ルールを追加。`--field` は `exe` / `title` / `cwd` / `repo` / `url` / `git_branch`、`--type` は `glob`（既定）/ `regex` / `contains` |
| `hitoiki task rule list [ID]` | 帰属ルールの一覧 |

### 仕分け・再計算

| コマンド | 内容 |
|---|---|
| `hitoiki review list [--date D] [--from F] [--to T] [--all]` | 要確認（needs_review）の区間。`review.min_sec`（既定 60 秒）未満の短い区間は既定で隠し、末尾に件数を示す。`--all` で全件。`--json` には隠した件数 `hidden_short` が入る |
| `hitoiki review set <SEGMENT>... [--task T] [--kind K] [--activity A] [--rule]` | 区間を確定。`--rule` で区間から帰属ルールを作る。ID を複数渡すと、まとめて同じ割り当てをする（1 つでも解決できなければ何も変更しない）。複数のときの `--json` は `count` / `segments` / `override_ids` / `rules` |
| `hitoiki review set --from F --to T (--task ID \| --no-task \| --rest) [--activity A] [--include-confirmed] [--include-meeting] [--include-idle] [--exclude SEGMENT...] [--dry-run]` | 時間帯の範囲に含まれる区間をまとめて付け替える（実績の補正）。区間 ID とは併用できない。境界にまたがる区間は範囲の内側だけが対象。最初のチェックは通常の作業とエージェント対話が ON で、確定済み・会議・無操作と記録なしは OFF（`--include-*` か設定 `review.correct_include_*` で ON）。`--exclude` には対象から外す区間 ID（記録なしは `gap:<開始のミリ秒>`。`--dry-run` の出力で分かる）。無操作・記録なしは種別 work の「PC 外」として計上される。`--dry-run` は確定せず、候補（`checked`）と合計を表示する。`--json` は適用時 `correction` / `preview` / `override_ids`、`--dry-run` 時は `preview` の内容（`summary` / `total_ms` / `offline_ms` / `excluded_count` / `candidates`） |
| `hitoiki review corrections [--task ID] [--date D]` | 補正の一覧（新しい順）。`--task` でそのタスクへの補正だけ、`--date` でその日と重なるものだけ |
| `hitoiki review undo-correction <ID>` | 補正を取り消す。その補正で作った確定を削除して、範囲を再計算する（ID は前方一致。存在しなければ終了コード 3） |
| `hitoiki review idle <SEGMENT>... <PRESET>` | 無操作の区間に分類をつける。`PRESET` は `break`（休憩）、`meeting`（会議）、`thinking`（作業 + 活動「考えごと」）、`away`（休憩 + 活動「離席」）。確定として保存し、再計算でも残る |
| `hitoiki review suggest [--accept N] [--dismiss N]` | ルール化の提案を一覧する。`--accept N` で N 番目の帰属ルールを追加、`--dismiss N` で断る（`review.rule_suggest_snooze_days` 日は再提案しない） |
| `hitoiki rebuild [--date D] [--from F] [--to T]` | 派生層（区間）を再計算 |

### 計画・記録

| コマンド | 内容 |
|---|---|
| `hitoiki block add <START> <END> [--task T] [--label L] [--focus]` | 計画ブロックを追加（`--focus` は集中ブロック） |
| `hitoiki block list [--date D] [--from F] [--to T]` | 計画ブロックの一覧 |
| `hitoiki block edit <ID> [--start S] [--end E] [--task T] [--label L] [--focus \| --no-focus]` | 計画ブロックを編集（指定した項目だけ変える） |
| `hitoiki block delete <ID>` | 計画ブロックを削除 |
| `hitoiki block template add <NAME> [--description D] [--weekdays W] [--block SPEC]...` | ブロックのテンプレートを作る（下記） |
| `hitoiki block template list` / `show <NAME>` | テンプレートの一覧 / 中身 |
| `hitoiki block template edit <NAME> [--new-name N] [--description D] [--weekdays W] [--block SPEC]...` | 指定した項目だけ変える。`--block` を付けるとブロックを置き換える。`--weekdays ""` で曜日の割り当てを外す |
| `hitoiki block template delete <NAME>` | テンプレートを削除（作成済みのブロックは消えない） |
| `hitoiki block template apply <NAME> [--date D] [--overlap skip\|overlay] [--dry-run]` | 指定日（既定は今日）にテンプレートを適用してブロックを作る |
| `hitoiki block template undo [--date D]` | 直前の適用で作ったブロックを削除 |
| `hitoiki block template save-from --name N [--date D] [--description D] [--weekdays W]` | 指定日のブロックからテンプレートを作る |
| `hitoiki block template suggest [--date D]` | 指定日の曜日に割り当てたテンプレート |
| `hitoiki check [-e 1-5] [-m 1-5] [-s 1-5] [--erg TAG] [--note TEXT]` | コンディション（エネルギー・気分・ストレス）の記録 |
| `hitoiki hassle <TEXT> [-i 1-5]` | デイリーハッスルの記録（強度の既定は 3） |
| `hitoiki abc add [--a A] [--b B] [--c-emotion E] [--c-intensity 1-5] [--from-hassle ID]` | ABC 理論の記録 |
| `hitoiki abc dispute <ID>` | AI で受け止め方を掘り下げる問いを生成 |

#### ブロックのテンプレート

`<NAME>` はテンプレートの名前、または ID の前方一致です。

- **`--block SPEC`**: `HH:MM-HH:MM[,focus][,task=ID][,label=ラベル]`（例: `14:00-15:00,focus,label=内省`）。時刻はローカル時刻で、終了は `24:00` も指定できます。`label=` は最後に書きます（ラベルの中のカンマはそのまま入ります）。タスクかラベルのどちらかが必要です。
- **`--weekdays W`**: `mon,wed` / `月,水` / `1,3`（1=月 … 7=日）のカンマ区切り。
- **`apply`**: 既存のブロックや、busy なカレンダー予定と重なる項目は、既定（`--overlap skip`）では作らずにスキップします。`--overlap overlay` なら重ねて作ります。`--dry-run` は何も作らず、作られる内容と重なりを表示します。夏時間で存在しない時刻は、存在する最初の時刻まで進めます。
- **`undo`**: 取り消していない直近の適用で作ったブロックだけを削除します（手で足したブロックや、すでに消したブロックは対象外）。
- `--json` では、`list` が `templates`、`apply` が `created` / `skipped`（`skipped` は重なりを `conflicts` に持つ）、`apply --dry-run` が `blocks`、`undo` が `removed` を返します。

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
| `hitoiki reflect answer <ID> [--file F]` | 回答を保存（`--file -` で標準入力）。構造化された問いには JSON `{"answers":[{"index":0,"energy":3,"mood":4},{"index":1,"text":"…","intensity":3}]}` を渡す（下記）。Markdown の回答も受け付ける |
| `hitoiki reflect list [--kind K] [--limit N]` | 保存済みの振り返り一覧（新しい順。`--kind` は `day_start` / `day_end` / `weekly`） |
| `hitoiki reflect show <ID>` | 問いと回答の詳細 |
| `hitoiki context --kind K [--date D]` | AI 用のコンテキストを JSON で出力 |

構造化された振り返り（[設定](./configuration.md#ai-コマンド) の `ai.structured`）では、問いに `[0]`、`[1]` … の番号と種類（チェックイン・ハッスル・ABC・自由記述）が付いて表示されます。`--json` では `structured`（問い）と `answers`（回答済みの対応表）がそのまま入ります。回答 JSON の各要素は `index`（問いの番号）と、種類ごとの次の項目です。

| 種類 | 項目 | 保存先 |
|---|---|---|
| チェックイン | `energy` / `mood` / `stress`（1〜5）、`erg`（`existence` / `relatedness` / `growth`）、`note` | `checkin` |
| ハッスル | `text`、`intensity`（1〜5、省略時 3） | `hassle` |
| ABC | `activating_event`（A）、`belief`（B）、`emotion` / `emotion_intensity`（C）、`dispute`（反論） | `abc_record` |
| 自由記述 | `text` | 振り返りの回答（`answers_md`） |

空の項目は読み飛ばし、回答済みの問いには答え直せません（不正な値が 1 件でもあれば何も保存しません）。保存した記録の ID は `answers` に入ります。

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

`today`、`review`、`tasks`、`plan`、`reflect`、`report`、`records`、`okr`、`settings`、`onboarding`。それぞれ画面の名前は「今日」「仕分け」「タスク」「計画」「振り返り」「レポート」「記録」「OKR・スキル」「設定」「はじめに」です（[日々の使い方](./daily-use.md)）。`reflection` も `reflect` として受け付け、`reflect/day_start` のように種別まで指定できます。`today/design` は今日の画面を「今日のデザイン」モードで開きます（始業の通知の本体をクリックしたときも同じ）。未知の名前は `today` になります。

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
