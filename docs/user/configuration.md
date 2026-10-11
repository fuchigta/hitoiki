# 設定

## 場所

| 用途 | パス |
|---|---|
| 設定ファイル | `%APPDATA%\hitoiki\config.toml` |
| DB | `%LOCALAPPDATA%\hitoiki\hitoiki.db` |
| ログ | `%LOCALAPPDATA%\hitoiki\logs\` |
| 判定器・プラグイン | `%APPDATA%\hitoiki\plugins\` |
| 質問セット | `%APPDATA%\hitoiki\questions\` |
| 振り返りテンプレート | `%APPDATA%\hitoiki\templates\` |

`HITOIKI_HOME` 環境変数を設定すると、設定・DB の置き場所をまとめて差し替えられます（テスト用）。

`config.toml` が無い、または項目が欠けていれば既定値で補います。ファイルを直接編集するほか、`hitoiki config path` / `config get <KEY>` / `config set <KEY> <VALUE>`、または設定画面の「設定ファイル」で読み書きできます。キーはドット区切りで、値は TOML として解釈します（解釈できなければ文字列）。書き換え後は全体を検証し、不正なら保存しません。

```powershell
hitoiki config set collect.idle_threshold_sec 300
hitoiki config set general.timezone '"UTC"'
hitoiki config get push.daily_limit
```

設定の変更は、再計算・カレンダー・通知・判定器には次回から反映されます。**収集器とログ監視の設定の変更には、常駐アプリの再起動が必要**です。

## 主なキー

| セクション | キー（既定値） |
|---|---|
| `general` | `timezone`（`Asia/Tokyo`）、`autostart`（true: ログオン時に自動起動）、`update_check`（未設定: 確認しない）、`quick_entry_shortcut`（`Ctrl+Alt+Space`）。[更新の確認](#更新の確認)、[クイック入力のショートカット](#クイック入力のショートカット) を参照 |
| `collect` | `idle_threshold_sec`（120）、`merge_gap_sec`（30）、`poll_interval_sec`（3）、`terminal_exes`、`agent_host_exes`、`communication_exes`、`meeting_exes`、`exclude`、`drop_excluded`（false）、`record_self`（false）、`interrupt_min_sec`（180）、`rebuild_window_min`（180）、`transient_exes`（`explorer.exe` など）、`transient_max_sec`（60） |
| `review` | `min_sec`（60）、`auto_break_locked_min`（10）、`trust_doing`（false）、`rule_suggest_min_count`（3）、`rule_suggest_snooze_days`（14）、`snap_sec`（120）、`correct_include_confirmed` / `correct_include_meeting` / `correct_include_idle`（いずれも false） |
| `agent` | `enabled`（`claude-code`, `codex`）、`prompt_excerpt_chars`（200）、`watch_max_min`（10）、`bind_lookback_sec`（5）、`bind_unlisted`（true） |
| `push` | `enabled`（true）、`daily_limit`（6）、`min_interval_min`（60）、`day_start_after`（08:00）、`day_end_at`（18:30）、`weekly.weekday` / `weekly.at`（fri / 17:00）、`long_focus_min`（90）、`switch_spike.window_min` / `switch_spike.count`（15 / 20）、`late_work_after`（20:00）、`idle_return_min`（15）、`disabled_triggers` |
| `activity` | `labels`（開発、レビュー、調査、設計、コミュニケーション、会議、事務、学習） |
| `judge` | `enabled`（false）、`default`（`aigw`）、`auto_confidence_threshold`（0.8）、`backends.<名前>` |
| `ai` | `command`（`["claude", "-p"]`）、`timeout_sec`（180）、`templates_dir` |
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
- `review.min_sec`: 仕分けの一覧で既定表示する区間の最小の長さ（秒）。これ未満の短い要確認区間は既定で隠します（削除はしません）。「確認待ち」の件数も同じ基準で数えます。
- `review.auto_break_locked_min`: 画面ロック・スリープによる無操作がこの分数以上なら、自動で休憩（根拠 `rule:locked`）にします。`0` で無効。ユーザーが確定した区間は変えません。
- `review.trust_doing`: true にすると、進行中（doing）のタスクを仮で当てた区間を、確認待ちにせず自動（根拠 `doing`）にします。既定は false（確認待ち）。
- `review.snap_sec`: 今日画面の「実績」の列で補正する範囲をドラッグするとき、端をこの秒数以内の区間の境目に吸着させます（既定 120 秒、`0` で吸着しない）。
- `review.correct_include_confirmed` / `review.correct_include_meeting` / `review.correct_include_idle`: 実績の補正（[日々の使い方](./daily-use.md#実績の補正)）で、ユーザー確定済みの区間・会議・無操作と記録なしを最初からチェックするか（いずれも既定 false）。CLI の `--include-confirmed` / `--include-meeting` / `--include-idle` はこの設定を true に上書きします。
- `review.rule_suggest_min_count` / `review.rule_suggest_snooze_days`: 同じ exe（または作業ディレクトリ）を同じタスクにこの回数以上確定したらルール化を提案します（`0` で提案しない）。断った組み合わせは、この日数のあいだ再提案しません。
- `collect.rebuild_window_min`: 帰属判定を定期的に再計算する直近の範囲（分）。
- `agent.prompt_excerpt_chars`: プロンプトの抜粋として保存する文字数。0 にすると抜粋を保存しません。
- `agent.watch_max_min`: エージェントの応答待ち（見守り）として計上する最大の長さ（分）。

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
timeout_sec = 180
```

- 既定は `claude -p` です。他のコマンドに替えるときは、プロンプトを標準入力から読み、回答を標準出力に書くものを指定します。
- `timeout_sec` を超えると強制終了してエラーにします。終了コードが 0 以外、または出力が空の場合もエラーです。
- `structured` は、問いを JSON（種類つき）で受け取って画面で答えられるようにするかの設定です。`auto`（既定）は `command` の先頭が `claude`（パスの末尾のファイル名で判定）のときだけ構造化し、それ以外は従来の Markdown です。`on` は常に試み、`off` は使いません。構造化に失敗した場合（コマンドが対応していない、JSON が読めない、形式が合わない）は、従来の Markdown の問いに自動で切り替え、エラーはログに残します。
- `structured_args` は、構造化のときに `command` の後ろへ付ける引数です。空（既定）なら `claude` 用の `["--output-format", "json", "--json-schema", "{schema}"]` を使います。`{schema}` は JSON Schema の文字列、`{schema_file}` は一時ファイルのパス（実行後に削除）に置き換わります。出力は、`structured_output` を持つ JSON（claude の形式。複数連結や配列も可）か、スキーマに従う JSON そのものを読みます。
- Codex CLI の例: `command = ["codex", "exec"]`、`structured = "on"`、`structured_args = ["--output-schema", "{schema_file}"]`。**この組み合わせは未検証**です（Codex CLI で動作を確認していません）。うまく動かないときは `structured = "off"` にしてください。

```toml
[ai]
structured = "auto"   # auto | on | off
# structured_args = ["--output-format", "json", "--json-schema", "{schema}"]
```

- 問いのテンプレートは組み込みですが、`%APPDATA%\hitoiki\templates`（`ai.templates_dir` で変更可）に同名の `day_start.md`、`day_end.md`、`weekly.md`、`abc_dispute.md` を置くとそちらが優先されます。構造化出力用には `day_start_structured.md`、`day_end_structured.md`、`weekly_structured.md` があり、同じ場所に置けば優先されます（使う問いの種類と数の指示は、このテンプレートに書いてあります）。

## 判定器

ルールで帰属が決まらなかった作業区間を、外部の判定器に問い合わせて分類させる仕組みです。`judge.enabled`（既定 false）が false の間は、帰属ルールと進行中のタスクだけで判定します。

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
- 確信度が `auto_confidence_threshold` 未満の答えは「要確認」になり、仕分けで確定します。
- 答えは一定期間キャッシュされ、同じ内容の区間を何度も問い合わせません。
- 質問セットは、`%APPDATA%\hitoiki\questions\*.toml` に置くと差し替えられます。

判定器には、区間のアプリ名・ウインドウタイトル・作業ディレクトリ、進行中のタスクなどが渡されます。`openai-compatible` や、外部に通信する `external-command` を使うと、これらが接続先に送られます。会社の PC では、接続先が許可されているか確認してください（[プライバシー](./privacy.md)）。

## 通知

`push` セクションで通知の頻度と時刻を調整します。

- `push.enabled`: false にすると通知を出しません。
- `push.daily_limit`: 1 日に出す通知の上限。`push.min_interval_min`: 同じ種類の通知の最小間隔（分）。
- `push.day_start_after` / `push.day_end_at`: 始業の通知を出し始める時刻と、終業の時刻。
- `push.weekly.weekday` / `push.weekly.at`: 週次の振り返りの曜日（`fri` など）と時刻。
- `push.long_focus_min`: 同じ作業が続いたときに声をかけるまでの時間（分）。
- `push.switch_spike.window_min` / `push.switch_spike.count`: 切り替えが多いと判断する、期間（分）と回数。
- `push.late_work_after`: 遅い時間の作業に声をかけ始める時刻。
- `push.idle_return_min`: 予定外の無操作がこの分数以上続いたあと復帰したときに、「さっきの時間は？」と尋ねます（トリガー `idle_return`）。`0` で無効。`push.disabled_triggers` に `idle_return` を入れても止められます。
- `push.disabled_triggers`: 無効にする通知の種類。`day_start`、`day_end`、`weekly`、`after_interrupt`、`meeting_overrun`、`switch_spike`、`long_focus`、`late_work` のいずれか。

通知の仕組みは [日々の使い方](./daily-use.md#通知) を参照してください。

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

オンボーディングの「自動起動と通知」と、設定画面の「新しいバージョンを確認する」でも切り替えられます。新しい版が見つかると、トースト（初めて見つけたとき 1 回だけ）とトレイメニュー（見つかっている間）でお知らせし、`hitoiki status` にも表示します。ダウンロードやインストールは自動では行いません。送る情報は [プライバシー](./privacy.md#ネットワーク通信が発生する場面) を参照してください。
