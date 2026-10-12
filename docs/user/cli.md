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
| `hitoiki status` | 現在の doing タスク、直近の区間、今日の集計、未応答の問いかけ（時刻・種類・タイトル・ID）、（`general.update_check = true` で新版が見つかっていれば）新しいバージョンの案内。ネットワークには出ず、保存済みの確認結果を使う |
| `hitoiki push list [--unanswered] [--date D] [--from F] [--to T]` | 配信された通知の一覧（既定は今日）。状態（未回答／回答済み／見送り）、種類の日本語名、タイトル、本文、無操作の対象時間帯を出す。`--unanswered` で未回答だけ。JSON は `pushes` |
| `hitoiki push answer <ID> [--energy N] [--mood N] [--stress N] [--hassle TEXT] [--idle break\|meeting\|thinking\|away\|task] [--task start\|later\|skip_week\|skip] [--dismiss]` | 通知に答える。チェックイン・ハッスルの時刻は通知の時刻。`--idle` は「無操作から戻った」通知のみ。`--idle task` は通知に「◯◯をしていた」がある場合に、その無操作の期間をそのタスクの PC 外の作業として確定する（補正の一覧から取り消せる）。`--task` は「緊急のタスク」「重要なタスク」「計画ブロックの開始」の通知のみで、`start` は対象のタスクを進行中にし、`later` は記録だけ、`skip_week` は今週その通知の対象から外す、`skip` は計画ブロックの開始を今回はやめる（記録だけ）。`--dismiss`（答えない）は一覧から外すが応答率では無応答のまま。`--dismiss`・`--idle`・`--task` はほかの回答と同時に指定できない。ID は前方一致。終了コード: 2 引数（値の範囲・組み合わせ・回答なし）、3 通知なし、4 ID が曖昧 |
| `hitoiki log [--date D] [--from F] [--to T] [--raw]` | 区間一覧。収集が止まっていた時間帯は「記録なし」の行で表示（JSON は `gaps`）。`--raw` で生データ（window_span / idle_span / agent_event）を時系列に混ぜて表示 |
| `hitoiki heatmap [--by 指標] [--from D] [--to D] [--json]` | 日別の活動量を、月曜始まりの 7 行 × 週の濃淡（`·░▒▓█`）で表示する。既定は今日を含む直近 53 週。`--by` は `time`（既定。作業系の時間。無操作・休憩・無視を除く）/ `kind:<種別>`（`work` など）/ `task:<ID またはタイトル>` / `skill:<名前または ID>`（子スキルを含む）/ `kr:<ID またはタイトル>`（KR に紐付いたタスクの時間）/ `records`（チェックイン・ハッスル・ABC・振り返りの件数。`records:checkin` / `hassle` / `abc` / `reflection` で種類別）。濃淡は値のある日の分布を 4 等分した段階（0 は値なし）。`--json` は日ごとの `date` / `value`（ミリ秒または件数）/ `level`（0〜4）を返す。曖昧な指定は終了コード 4、無いタスク・スキルは 3 |
| `hitoiki report [--period P] [--from F] [--to T] [--by 軸] [--include-idle]` | 期間内の集計。`--by` は `task`（既定）/ `activity` / `quadrant` / `okr` / `skill` / `agent` / `kind`。`--include-idle` で無操作・休憩も合計に含める |

### タスク

| コマンド | 内容 |
|---|---|
| `hitoiki task add <TITLE> [...]` | タスクを追加。`--body` / `--body-file`（本文。`--body -` で標準入力）、`--urgent` / `--not-urgent`、`--important` / `--not-important`、`--estimate`（`90m` / `1h30m` / 分）、`--kr`（KR の ID）、`--skill`、`--url`、`--plan-start` / `--plan-end`、`--interrupt`（割り込みタスク） |
| `hitoiki task list [--status S] [--kind K]` | 一覧。`--status` はカンマ区切りの `todo,doing,done,dropped,all`（既定 `todo,doing`） |
| `hitoiki task show <ID>` | 詳細 |
| `hitoiki task edit <ID> [...]` | 編集。`add` と同じオプションと `--title`。`--no-kr` で KR との紐付けを外す（`--kr` とは同時に指定できない） |
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
| `hitoiki review ignore <SEGMENT>...` | 区間を「無視」にする（種別 `ignored`・タスクなしの確定として保存。集計・レポート・タスクの実績に数えない。再計算でも残る）。1 つでも解決できなければ何も変更しない（終了コード 3） |
| `hitoiki review unignore <SEGMENT>...` | `review ignore` で無視にした区間を元に戻す（確定を削除して再計算）。無視でない区間、または無視リスト（`collect.ignore_exes`）による無視は終了コード 2 |
| `hitoiki ignore list` | 無視リスト（`collect.ignore_exes`）の一覧。`--json` は `exes` / `default_exes` / `customized` |
| `hitoiki ignore add <EXE>` / `ignore remove <EXE>` | 無視リストに追加／から削除し（設定ファイルの `collect.ignore_exes` だけを書き換える。既定のままなら既定＋追加の形）、直近 7 日を再計算する。すでにある追加は `changed: false`、無い exe の削除は終了コード 3、空の exe は 2 |
| `hitoiki ignore suggest [--accept EXE] [--dismiss EXE] [--days N]` | 無視の提案を一覧する（既定は直近 7 日。区間数・合計・短い区間の割合・要確認件数と理由つき）。`--accept` は `ignore add` と同じ、`--dismiss` はそのアプリを 30 日間提案しない。`--days` が 1 未満なら終了コード 2 |
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
| `hitoiki check list` / `hassle list` / `abc list` `[--date D] [--from F] [--to T]` | 記録の一覧（ID 付き。既定は今日） |
| `hitoiki check edit <ID> [-e N] [-m N] [-s N] [--erg TAG] [--note TEXT]` | チェックインを編集（指定した項目だけ変える。`-e` `-m` `-s` に 0 でその項目を消す、`--note ""` でメモを消す） |
| `hitoiki hassle edit <ID> [--text T] [-i 1-5]` | ハッスルを編集 |
| `hitoiki abc edit <ID> [--a A] [--b B] [--c-emotion E] [--c-intensity N] [--dispute D]` | ABC の記録を編集（文字列は `""` で消す、`--c-intensity 0` で強度を消す） |
| `hitoiki check delete <ID>` / `hassle delete <ID>` / `abc delete <ID>` `[--force] [--dry-run]` | 記録を削除。ハッスルを元にした ABC 記録があると `--force` が必要（ABC は残り、ハッスルとの紐付けだけ外れる） |

`check` / `hassle` の本文が `list` `edit` `delete` と同じ語のときは、サブコマンドとして解釈されます。

### 削除と影響

削除コマンドは共通で、影響（紐付いているタスクの数など）があるときは `--force` を付けないと失敗します（終了コード 2）。`--dry-run` を付けると何も消さず、影響だけを表示します。`--json` では `{"dry_run": ..., "deleted": ..., "impact": {"lines": [{"label": ..., "count": ...}]}}` を返します。

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
| `hitoiki okr edit-objective <ID> [--title T] [--description D] [--from F] [--to T] [--clear-period] [--status active\|done\|dropped]` | Objective を編集（指定した項目だけ変える。`--description ""` で説明を消す） |
| `hitoiki okr edit-kr <ID> [--title T] [--target N] [--current N] [--status S] [--objective O] [--measure time\|days\|done] [--day-min N]` | KR を編集（`--objective` で別の Objective へ付け替え）。`--measure` は何を軸に見るか（時間 / 回数 / 完了数。既定 `time`）、`--day-min` は回数の軸で「取り組んだ日」とみなす 1 日の最小分数（既定 15） |
| `hitoiki okr link <KR> <TASK>...` | タスクを KR に紐付ける（KR は ID の前方一致かタイトル、タスクは ID の前方一致）。別の KR に紐付いていたタスクは付け替える（JSON の `linked[].previous_kr` に元の KR）。1 つでも見つからなければ何も変えず終了コード 3 |
| `hitoiki okr unlink <TASK>...` | タスクの KR との紐付けを外す。KR に紐付いていないタスクは何もしない（JSON は `unlinked`） |
| `hitoiki okr show <KR> [--date D] [--from F] [--to T] [--json]` | KR の集計。選んだ軸の値（時間 / 取り組んだ日 / 完了数）と補助の値、週ごとの時間、スキル別の時間（複数のスキルが付いたタスクは各スキルに全時間）、見積の合計と実績、紐付いたタスクを出す。期間の既定は Objective の期間、無ければ直近 8 週。時間は無操作・休憩・無視を除く。目標・現在値は並べるだけで、達成率や評価は出さない |
| `hitoiki okr delete-objective <ID> [--force] [--dry-run]` | Objective を削除。配下の KR も消え、KR に紐付いたタスクは残って紐付けだけ外れる |
| `hitoiki okr delete-kr <ID> [--force] [--dry-run]` | KR を削除。紐付いたタスクは残って紐付けだけ外れる |
| `hitoiki skill add <NAME> [--parent P]` | スキルを追加（`--parent` は名前または ID） |
| `hitoiki skill import-preset [NAME]` | 同梱プリセットを取り込む（省略で全部） |
| `hitoiki skill list` | 一覧 |
| `hitoiki skill edit <SKILL> [--name N] [--parent P \| --no-parent] [--description D]` | スキルを編集（名前または ID。取り込み済みのプリセットも可。自分や子孫を親にはできない） |
| `hitoiki skill delete <SKILL> [--children reparent\|delete] [--force] [--dry-run]` | スキルを削除。タスクへの付与は外れる。子スキルは `reparent`（既定。親を付け替え）か `delete`（一緒に削除） |
| `hitoiki task rule edit <RULE> [--field F] [--pattern P] [--type T] [--priority N] [--enable \| --disable]` | 帰属ルールを編集・有効/無効の切り替え |
| `hitoiki task rule delete <RULE> [--dry-run]` | 帰属ルールを削除 |
| `hitoiki activity-rule list` / `add --field F --pattern P --label L [--type T] [--priority N]` | 活動ルール（パターンから活動ラベルを当てる）の一覧・追加 |
| `hitoiki activity-rule edit <RULE> [--field F] [--pattern P] [--type T] [--label L] [--priority N] [--enable \| --disable]` / `delete <RULE> [--dry-run]` | 活動ルールの編集・削除 |
| `hitoiki activity-label list` / `add <L>` / `rename <FROM> <TO>` / `remove <L>` | 活動ラベルの候補（`activity.labels`）。`rename` は同じラベルの活動ルールも付け替える。`remove` しても過去の区間のラベルは残る |
| `hitoiki calendar list` / `add <NAME> <URL>` | カレンダーソース（ICS）の一覧・追加。URL は表示しない（ホスト名のみ） |
| `hitoiki calendar edit <NAME> [--new-name N] [--url U]` / `delete <NAME> [--force] [--dry-run]` | 編集・削除。削除すると取得済みの予定のコピーも消える（カレンダー側は変わらない） |

ルールを変えたあとの再計算は、常駐処理が動いていれば今日の分が自動で行われます。過去の日へ反映するには `hitoiki rebuild --date D` を実行してください。

### 振り返り（AI）

| コマンド | 内容 |
|---|---|
| `hitoiki reflect day_start [--date D]` / `day_end` / `weekly` | AI で問いを生成して保存 |
| `hitoiki reflect answer <ID> [--file F]` | 回答を保存（`--file -` で標準入力）。構造化された問いには JSON `{"answers":[{"index":0,"energy":3,"mood":4},{"index":1,"text":"…","intensity":3}]}` を渡す（下記）。Markdown の回答も受け付ける |
| `hitoiki reflect list [--kind K] [--limit N]` | 保存済みの振り返り一覧（新しい順。`--kind` は `day_start` / `day_end` / `weekly`） |
| `hitoiki reflect show <ID>` | 問いと回答の詳細（回答済みの問いには保存したチェックイン・ハッスル・ABC の内容も出す） |
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
