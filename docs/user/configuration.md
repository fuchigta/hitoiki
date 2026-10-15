# 設定

## 場所

| 用途 | パス |
|---|---|
| 設定ファイル | `%APPDATA%\hitoiki\config.toml` |
| DB | `%LOCALAPPDATA%\hitoiki\hitoiki.db` |
| ログ | `%LOCALAPPDATA%\hitoiki\logs\` |
| 自動判定・プラグイン | `%APPDATA%\hitoiki\plugins\` |
| 質問セット | `%APPDATA%\hitoiki\questions\` |
| 振り返りのプロンプト | `%APPDATA%\hitoiki\prompts\` |

`HITOIKI_HOME` 環境変数を設定すると、設定・DB の置き場所をまとめて差し替えられます（テスト用）。

`config.toml` が無い、または項目が欠けていれば既定値で補います。ファイルを直接編集するほか、`hitoiki config path` / `config get <KEY>` / `config set <KEY> <VALUE>`、または設定画面の「設定ファイル」で読み書きできます。キーはドット区切りで、値は TOML として解釈します（解釈できなければ文字列）。書き換え後は全体を検証し、不正なら保存しません。

```powershell
hitoiki config set collect.idle_threshold_sec 300
hitoiki config set general.timezone '"UTC"'
hitoiki config get nudge.daily_limit
```

設定の変更は、再計算・カレンダー・通知・自動判定には次回から反映されます。**収集器とログ監視の設定の変更には、常駐アプリの再起動が必要**です。

## 主なキー

| セクション | キー（既定値） |
|---|---|
| `general` | `timezone`（`Asia/Tokyo`）、`autostart`（true: ログオン時に自動起動）、`update_check`（未設定: 確認しない）、`quick_entry_shortcut`（`Ctrl+Alt+Space`）。[更新の確認](#更新の確認)、[クイック入力のショートカット](#クイック入力のショートカット) を参照 |
| `collect` | `idle_threshold_sec`（120）、`merge_gap_sec`（30）、`poll_interval_sec`（3）、`terminal_exes`、`agent_host_exes`、`communication_exes`、`meeting_exes`、`exclude`、`drop_excluded`（false）、`record_self`（false）、`interrupt_min_sec`（180）、`rebuild_window_min`（180）、`transient_exes`（`explorer.exe` など）、`transient_max_sec`（60）、`ignore_exes`（`explorer.exe`、`ShellExperienceHost.exe` など。無視するアプリ） |
| `actual` | `min_sec`（60）、`auto_break_locked_min`（10）、`trust_doing`（false）、`rule_suggest_min_count`（3）、`rule_suggest_snooze_days`（14）、`snap_sec`（120）、`correct_include_confirmed` / `correct_include_meeting` / `correct_include_idle`（いずれも false） |
| `agent` | `enabled`（`claude-code`, `codex`）、`prompt_excerpt_chars`（200）、`wait_max_min`（10）、`bind_lookback_sec`（5）、`bind_unlisted`（true） |
| `nudge` | `enabled`（true）、`daily_limit`（6）、`min_interval_min`（60）、`day_start_after`（08:00）、`day_end_at`（18:30）、`weekly.weekday` / `weekly.at`（fri / 17:00）、`long_work_min`（90）、`switch_spike.window_min` / `switch_spike.count`（15 / 20）、`late_work_after`（20:00）、`idle_return_min`（15）、`urgent_task_interval_min`（120）、`important_stale_days`（3）、`key_result_idle_weekday`（3）、`toast_persistent`（true）、`disabled_triggers` |
| `activity` | `labels`（開発、レビュー、調査、設計、コミュニケーション、会議、事務、学習） |
| `judge` | `enabled`（false）、`default`（`aigw`）、`auto_confidence_threshold`（0.8）、`backends.<名前>` |
| `ai` | `command`（`["claude", "-p"]`）、`model`（claude なら `haiku`）、`model_args`、`timeout_sec`（180）、`prompts_dir`、`structured`、`structured_args`、`erg_classify`（true）、`erg_auto_threshold`（0.8） |
| `calendar` | `fetch_interval_min`（15）、`past_days`（7）、`future_days`（14）、`sources` |
| `task` | `single_doing`（true: `task start` で他の doing を todo に戻す） |

各キーの意味:

- `collect.idle_threshold_sec`: この秒数、入力が無ければ無操作区間にします（入力の内容は見ません）。
- `collect.merge_gap_sec`: 同じ内容にはさまれた、この秒数以下の短い作業を吸収して 1 つの区間にまとめます。
- `collect.terminal_exes` / `communication_exes` / `meeting_exes`: ターミナル系（エージェントとの対話の判定に使う）、コミュニケーション系（割り込みの判定に使う）、会議アプリの exe 名。
- `collect.agent_host_exes`: GUI のエージェントクライアントの exe 名（既定は `Orca.exe`、`Cursor.exe`、`Windsurf.exe`、`Code - Insiders.exe`）。`terminal_exes` と同じく、そのウインドウが前面にいた時間をエージェントとの対話時間に数えます。`terminal_exes` を書き換えている設定でも既定の一覧が効くよう、別のキーにしています。自分が使うクライアントが無ければ追加してください。
- `agent.bind_unlisted`: true（既定）なら、プロンプトを送った時に前面だったアプリが `terminal_exes` / `agent_host_exes` に無くても、そのウインドウで対話していたものとして数えます。ブラウザ、`explorer.exe` などの通過系、コミュニケーション・会議系、Hitoiki 自身、除外したウインドウ、入力が無い（無操作・ロック中の）時刻の送信は数えません。別のアプリで作業している裏でエージェントが動く使い方で、その作業が対話時間に混ざるときは false にして、一覧のアプリだけにします。
- `collect.interrupt_min_sec`: コミュニケーション系アプリの後に別の作業が続いたとき、割り込みとみなす最小の長さ。
- `collect.transient_exes` / `collect.transient_max_sec`: タスクバー、スタートメニュー、Alt+Tab などを担う「通過系」アプリの exe 名（既定は `explorer.exe`、`StartMenuExperienceHost.exe`、`SearchHost.exe`、`SearchApp.exe`、`ShellExperienceHost.exe`、`ShellHost.exe`、`LockApp.exe`、`TextInputHost.exe`）と、通過とみなす最大の連続時間（秒）。通過系アプリが前面だった時間がこの長さ未満なら、独立した区間にせず直前の作業（無ければ直後の作業）に含めます。この長さ以上続いたとき（ファイラーで作業したときなど）は通常の区間のままです。記録（`window_span`）は変えないので、値を変えて `hitoiki rebuild` すると過去分にも反映できます。
- `collect.ignore_exes`: 「無視する」アプリの exe 名（大文字小文字を区別しない。既定は `explorer.exe`、`ShellExperienceHost.exe`、`StartMenuExperienceHost.exe`、`SearchHost.exe`、`SearchApp.exe`、`ShellHost.exe`、`LockApp.exe`、`TextInputHost.exe`。`ApplicationFrameHost.exe` は UWP アプリの実体なので入れません）。ここに入れたアプリは `transient_exes` にも入っているものとして扱い、前面の時間が `transient_max_sec` 未満なら前後の作業に含めます（時間は保たれます）。それ以上続いたものは「無視」の区間になり、集計・実績の要確認・レポート・タスクの実績・通知（長時間の作業など）に数えません。ルールでタスクや活動が決まった区間と、手で確定た区間は無視しません。`transient_exes` との違いは、長く続いた分を通常の作業として残すか（`transient_exes` だけ）、数えない時間にするか（`ignore_exes`）です。記録（`window_span`）は消えず、リストから外して再計算（`hitoiki rebuild`。`hitoiki ignore remove` は直近 7 日を自動で再計算）すると元に戻ります。設定画面の「ルール」タブ（種類「無視」）、`hitoiki ignore list|add|remove`、実績画面の「今後も無視」から変更できます。既定のままの設定に追加するときは、既定の一覧に追記した形で `config.toml` に書き込みます。
- `actual.min_sec`: 実績の「要確認」の一覧で既定表示する区間の最小の長さ（秒）。これ未満の短い要確認区間は既定で隠します（削除はしません）。「確認待ち」の件数も同じ基準で数えます。
- `actual.auto_break_locked_min`: 画面ロック・スリープによる無操作がこの分数以上なら、自動で休憩（根拠 `rule:locked`）にします。`0` で無効。ユーザーが確定した区間は変えません。
- `actual.trust_doing`: true にすると、進行中（doing）のタスクを仮で当てた区間を、確認待ちにせず自動（根拠 `doing`）にします。既定は false（確認待ち）。
- `actual.snap_sec`: 今日画面の「実績」の列で補正する範囲をドラッグするとき、端をこの秒数以内の区間の境目に吸着させます（既定 120 秒、`0` で吸着しない）。
- `actual.correct_include_confirmed` / `actual.correct_include_meeting` / `actual.correct_include_idle`: 実績の補正（[日々の使い方](./daily-use.md#実績の補正)）で、ユーザー確定済みの区間・会議・無操作と記録なしを最初からチェックするか（いずれも既定 false）。CLI の `--include-confirmed` / `--include-meeting` / `--include-idle` はこの設定を true に上書きします。
- `actual.rule_suggest_min_count` / `actual.rule_suggest_snooze_days`: 同じ exe（または作業ディレクトリ）を同じタスクにこの回数以上確定したらルール化を提案します（`0` で提案しない）。断った組み合わせは、この日数のあいだ再提案しません。
- `collect.rebuild_window_min`: 割り当てを定期的に再計算する直近の範囲（分）。
- `agent.prompt_excerpt_chars`: プロンプトの抜粋として保存する文字数。0 にすると抜粋を保存しません。
- `agent.wait_max_min`: エージェントの作業待ちとして計上する最大の長さ（分）。

## 除外ルール

`collect.exclude` に一致した前面ウインドウは、「除外」として記録し、アプリのパスとウインドウタイトルを保存しません（既定では exe 名だけを残します）。`collect.drop_excluded = true` にすると、区間自体を作りません。

ルールは `exe`（exe 名。大文字小文字は区別しません）と `title_regex`（タイトルの正規表現）のどちらか、または両方を持ち、両方指定したときは両方に一致したときだけ除外します。

既定の除外:

- パスワードマネージャ: `KeePassXC.exe`、`KeePass.exe`、`1Password.exe`、`Bitwarden.exe`
- プライベートブラウズのタイトル: `InPrivate|シークレット|Incognito|Private Browsing`

既定を置き換えるため、`exclude` を `config.toml` に書くと既定の一覧は使われなくなります。既定も残したい場合は、既定の分も書き写してください。

```toml
[[collect.exclude]]
exe = "KeePassXC.exe"

[[collect.exclude]]
title_regex = "InPrivate|シークレット|Incognito|Private Browsing"

[[collect.exclude]]
exe = "msedge.exe"
title_regex = "社外秘"
```

## record_self

`collect.record_self`（既定 false）は、Hitoiki 自身のウインドウ（`hitoiki-app.exe`）が前面にある時間を記録するかどうかです。false の間は、その時間は記録の空白になります。Hitoiki の画面を操作している時間を作業として数えたい場合だけ `true` にします。

## カレンダー

`calendar.sources` に ICS の URL を登録すると、予定を取り込み、会議の時間を自動で判定できます。

```toml
[[calendar.sources]]
name = "仕事"
type = "ics"
url = "https://example.com/path/basic.ics"
```

- URL は秘密情報として扱います。ログやエラーメッセージには `name` だけを出します。他の人に見せないでください。
- `calendar.fetch_interval_min` ごとに取得し、`past_days` 前から `future_days` 先までの予定を対象にします。繰り返し予定（RRULE）の展開と除外日（EXDATE）に対応します。
- 取消された予定と、空き時間扱い（透過）の予定は、忙しい時間として数えません。
- 動作確認は `hitoiki doctor --network` で行えます（この指定のときだけ実際に取得を試します）。

## AI コマンド

振り返りの問いの生成には、`ai.command` で指定した外部コマンドを使います。Hitoiki は推論を持たず、コマンドを子プロセスとして起動して標準入力にプロンプトを渡し、標準出力を受け取ります。

```toml
[ai]
command = ["claude", "-p"]
# model = "sonnet"   # 未指定なら `claude -p --model haiku`、指定すると `claude -p --model sonnet` で起動する
timeout_sec = 180
```

- `erg_classify`（既定 true）は、終業・週次の振り返りを作る前に、その期間の記録の ERG（存在・関係・成長）を AI で判定するかの設定です。**true のとき、ERG が未判定の記録の文章（調子のメモ、気になったこと、捉え直し、振り返りの回答）が `command` に送られます**（`hitoiki erg classify` を実行したときは設定にかかわらず送られます）。`command` が空なら送らず、判定もしません。
- `erg_auto_threshold`（既定 0.8）は、AI の判定を採用する確信度の下限です（0〜1）。未満のものは「未確定」になり、あなたが確定するまで集計に入りません。
- 既定は `claude -p` です。他のコマンドに替えるときは、プロンプトを標準入力から読み、回答を標準出力に書くものを指定します。
- `model` は、AI コマンドに使わせるモデル名です（例: `model = "sonnet"`）。指定すると `command` の後ろに `--model <モデル名>` を付けて起動します（`claude` も `codex` も `--model` を受け付けます）。未指定（既定）のときは、`command` の先頭が `claude` なら `haiku` を使い、それ以外のコマンドには何も付けません。空文字（`model = ""`）にすると何も付けず、コマンド側の既定のモデルになります。`hitoiki config set ai.model sonnet` でも設定できます。
- `model_args` は、`model` を指定したときに `command` の後ろへ付ける引数です。空（既定）なら `["--model", "{model}"]` を使います。`{model}` はモデル名に置き換わります。モデルの指定方法が違うコマンドに替えるときだけ設定します（例: `model_args = ["-m", "{model}"]`）。構造化出力の引数（`structured_args`）は、その後ろに付きます（`command` → モデルの引数 → `structured_args` の順）。
- `timeout_sec` を超えると強制終了してエラーにします。終了コードが 0 以外、または出力が空の場合もエラーです。
- `structured` は、問いを JSON（種類つき）で受け取って画面で答えられるようにするかの設定です。`auto`（既定）は `command` の先頭が `claude`（パスの末尾のファイル名で判定）のときだけ構造化し、それ以外は従来の Markdown です。`on` は常に試み、`off` は使いません。構造化に失敗した場合（コマンドが対応していない、JSON が読めない、形式が合わない）は、従来の Markdown の問いに自動で切り替え、エラーはログに残します。
- `structured_args` は、構造化のときに `command` の後ろへ付ける引数です。空（既定）なら `claude` 用の `["--output-format", "json", "--json-schema", "{schema}"]` を使います。`{schema}` は JSON Schema の文字列、`{schema_file}` は一時ファイルのパス（実行後に削除）に置き換わります。出力は、`structured_output` を持つ JSON（claude の形式。複数連結や配列も可）か、スキーマに従う JSON そのものを読みます。
- Codex CLI の例: `command = ["codex", "exec"]`、`structured = "on"`、`structured_args = ["--output-schema", "{schema_file}"]`（モデルを選ぶなら `model = "..."` を足すと `codex exec --model ...` になります）。**この組み合わせは未検証**です（Codex CLI で動作を確認していません）。うまく動かないときは `structured = "off"` にしてください。

```toml
[ai]
structured = "auto"   # auto | on | off
# structured_args = ["--output-format", "json", "--json-schema", "{schema}"]
```

- 問いのプロンプトは組み込みですが、`%APPDATA%\hitoiki\prompts`（`ai.prompts_dir` で変更可）に同名の `day_start.md`、`day_end.md`、`weekly.md`、`reframe_deepen.md` を置くとそちらが優先されます。構造化出力用には `day_start_structured.md`、`day_end_structured.md`、`weekly_structured.md` があり、同じ場所に置けば優先されます（使う問いの種類と数の指示は、このプロンプトに書いてあります）。

## 自動判定

ルールで割り当てが決まらなかった作業区間を、外部の自動判定に問い合わせて分類させる仕組みです。`judge.enabled`（既定 false）が false の間は、タスクの割り当てルールと進行中のタスクだけで判定します。

```toml
[judge]
enabled = true
default = "my-judge"
auto_confidence_threshold = 0.8

[judge.backends.my-judge]
type = "external-command"
command = ["python", "C:\\path\\to\\judge.py"]
timeout_sec = 10
batch = false

[judge.backends.gateway]
type = "openai-compatible"
base_url = "https://example.com/v1"
model = "model-name"
api_key_env = "MY_API_KEY"
timeout_sec = 10
```

- `external-command`: `command` を起動し、標準入力に JSON を渡して標準出力から JSON を受け取ります。`batch = true` で複数区間をまとめて送ります。
- `openai-compatible`: `base_url` の `/chat/completions` に問い合わせます。API キーは設定ファイルには書かず、`api_key_env` で指した環境変数から読みます。
- 確信度が `auto_confidence_threshold` 未満の答えは「要確認」になり、実績で確定します。
- 答えは一定期間キャッシュされ、同じ内容の区間を何度も問い合わせません。
- 質問セットは、`%APPDATA%\hitoiki\questions\*.toml` に置くと差し替えられます。

自動判定には、区間のアプリ名・ウインドウタイトル・作業ディレクトリ、進行中のタスクなどが渡されます。`openai-compatible` や、外部に通信する `external-command` を使うと、これらが接続先に送られます。会社の PC では、接続先が許可されているか確認してください（[プライバシー](./privacy.md)）。

## 問いかけ

`nudge` セクションで問いかけの頻度と時刻を調整します。

- `nudge.enabled`: false にすると問いかけを出しません。
- `nudge.daily_limit`: 1 日に出す問いかけの上限。`nudge.min_interval_min`: 同じきっかけの問いかけの最小間隔（分）。
- `nudge.day_start_after` / `nudge.day_end_at`: 始業の問いかけを出し始める時刻と、終業の時刻。
- `nudge.weekly.weekday` / `nudge.weekly.at`: 週次の振り返りの曜日（`fri` など）と時刻。
- `nudge.long_work_min`: 同じ作業が続いたときに声をかけるまでの時間（分）。
- `nudge.switch_spike.window_min` / `nudge.switch_spike.count`: 切り替えが多いと判断する、期間（分）と回数。
- `nudge.late_work_after`: 遅い時間の作業に声をかけ始める時刻。
- `nudge.idle_return_min`: 予定外の無操作がこの分数以上続いたあと復帰したときに、「さっきの時間は？」と尋ねます（きっかけ `idle_return`）。`0` で無効。`nudge.disabled_triggers` に `idle_return` を入れても止められます。
- `nudge.urgent_task_interval_min`: 緊急のしるしが付いた未完了のタスクがあるとき、勤務時間内（`nudge.day_start_after`〜`nudge.day_end_at`）に、この分数以上の間隔で「いま取りかかりますか？」と知らせます（きっかけ `urgent_task`、既定 120 分）。`0` で無効。そのタスクに取り組み中のとき、今日の計画でそのタスクの計画ブロックがこれから先にあるときは知らせません。
- `nudge.important_stale_days`: 重要（緊急ではない）な未完了のタスクに、この日数以上時間が入っていないとき、勤務時間内に 1 日 1 回まで「少し時間を取りませんか」と知らせます（きっかけ `important_task`、既定 3 日）。`0` で無効。今日の計画に入っているタスク、取り組み中のタスク、今週「今週は見送る」を押したタスクは対象外です。
- `nudge.toast_persistent`: 回答を求める通知（ボタンや入力欄のあるトースト）を、操作するまで画面に残します（Windows のトーストの `scenario="reminder"`）。`false` にすると一定時間で自動的に消えます。更新のお知らせやテスト通知は常に自動で消えます。見逃した通知は今日画面の「未回答の問いかけ」から答えられます。
- `nudge.key_result_idle_weekday`: 今週まだ時間が入っていない成果の指標を知らせる曜日（きっかけ `key_result_idle`）。`1` = 月、`2` = 火、`3` = 水（既定）、`4` = 木、`5` = 金、`6` = 土、`7` = 日、`0`（または 8 以上）で無効。その曜日以降の勤務時間内に、**週に 1 回まで**「今週、時間が入っていない成果の指標があります」と事実だけを知らせます（週は月曜始まり）。対象は、軸が時間または回数で、目標が有効かつ期間内（期間が無ければ対象）の成果の指標のうち、スキルか紐付けたタスクがあって今週（月曜から今まで）その時間が 1 分も入っていないものです。期間が終わった・まだ始まっていない目標の成果の指標、完了数の軸の成果の指標、スキルも紐付けたタスクも無い成果の指標、完了・やめた成果の指標は対象外です。複数あるときは 1 回の通知にまとめます（3 件まで名前を出し、残りは件数）。ボタンは「今日の予定に入れる」（今日のデザインを開きます）と「今週は見送る」（記録だけ）。スヌーズ・1 日の上限・会議中の保留は他の通知と同じです。
- `nudge.disabled_triggers`: 無効にする問いかけのきっかけ。`day_start`、`idle_return`、`block_start`（計画ブロックの開始。これ専用の設定項目はなく、止めるにはここに入れる）、`day_end`、`weekly`、`urgent_task`、`after_interrupt`、`meeting_overrun`、`switch_spike`、`long_work`、`important_task`、`key_result_idle`、`late_work` のいずれか。

通知の仕組みは [日々の使い方](./daily-use.md#問いかけ) を参照してください。

## 更新の確認

## クイック入力のショートカット

`general.quick_entry_shortcut` は、クイック入力の窓を開くグローバルショートカットです（既定 `Ctrl+Alt+Space`）。空文字（`""`）にすると無効になります。

- 書き方は `Ctrl+Alt+Space` のように、修飾キー（`Ctrl` / `Alt` / `Shift` / `Super`＝Windows キー）を 1 つ以上と、主キー 1 つを `+` でつなぎます（大文字小文字は区別しません）。主キーは英数字、`F1`〜`F24`、`Space` / `Enter` / `Tab` / `Escape` / `Backspace` / `Delete` / `Insert` / `Home` / `End` / `PageUp` / `PageDown` / `Up` / `Down` / `Left` / `Right` です。解釈できない値は `config set` でエラーになり、保存されません。
- 設定画面の「クイック入力」で、「キーを押して記録」を押してから組み合わせを押すか、文字で入力して保存します。他のアプリと衝突して登録できないときはエラーを表示し、それまでの設定を保ちます。変更は再起動なしで反映されます（`hitoiki config set` で書き換えた場合は、アプリを再起動すると反映されます）。
- GUI アプリだけの機能です。ヘッドレスの daemon にはショートカットはありません。

```
hitoiki config set general.quick_entry_shortcut '"Ctrl+Shift+K"'
```

`general.update_check` は、新しいバージョンを確認するかどうかの選択です。

- 未設定（既定）: まだ尋ねていない状態です。**確認しません**。
- `true`: 常駐アプリ（または `hitoiki daemon`）が、起動の数分後と、その後 24 時間ごとに、GitHub Releases の最新版を問い合わせます。
- `false`: 確認しません。

```
hitoiki config set general.update_check true
```

「はじめに」画面の「自動起動と通知」と、設定画面の「新しいバージョンを確認する」でも切り替えられます。新しい版が見つかると、トースト（初めて見つけたとき 1 回だけ）とトレイメニュー（見つかっている間）でお知らせし、`hitoiki status` にも表示します。ダウンロードやインストールは自動では行いません。送る情報は [プライバシー](./privacy.md#ネットワーク通信が発生する場面) を参照してください。
