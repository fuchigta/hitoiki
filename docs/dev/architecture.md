# Hitoiki アーキテクチャ

この文書は現在の実装（`crates/*` と `ui/`）に基づく設計の説明です。コードと食い違った場合はコードが正です。開発時の規約は [`AGENTS.md`](../../AGENTS.md) を参照してください。

## 1. 目的と設計原則

Hitoiki（一息）は、個人用の自己管理ツールです。タスク・タイムマネジメント、モチベーション、アンガー・ストレス、スキルの 4 領域を 1 つのタイムライン上で統合し、記録・可視化・内省を支援します。コーディングエージェントとの対話時間のような「名前の無い工数」を自動で捉え、タスクに紐づけて可視化することを重視します。

### 1.1 設計原則

- **存在を意識させない**: 情報は可能な限り自動収集する。手入力は通知への 1 アクション回答を基本にする。
- **判断はユーザーに委ねる**: 自動判定に確信が持てない区間は捨てずに `needs_review`（要確認）として溜め、振り返り時にユーザーが確定する。
- **評価を押し付けない**: 目的は時間の使い方の可視化であり、良し悪しの断定ではない。通知文言や AI 振り返りのテンプレートもこの方針に従う。
- **ローカル完結**: データはすべてローカルの SQLite に保存する。同期・クラウド保存・テレメトリは無い。
- **CLI ファースト**: すべての機能は CLI から操作できる。GUI は CLI と同じ `hitoiki-core` のロジックの上に乗る。
- **拡張点を残す**: OS 収集器、エージェントアダプタ、カレンダーアダプタ、判定器は差し替え可能な境界で分離する。

### 1.2 禁止事項（セキュリティ・プライバシー）

- ネットワークポートを開かない（待ち受け HTTP サーバ、ローカルポートを持たない）。プロセス間通信は名前付きパイプのみ。
- キー入力の内容も回数も記録しない。低レベルのキーボード/マウスフック（`WH_KEYBOARD_LL` 等）、DLL インジェクション、他プロセスのメモリ読み取りは使わない。管理者権限を要求しない。
- UI は Tauri の WebView 内でのみ表示する。ブラウザ向け配信やローカル HTTP サーバは持たない（`tauri.conf.json` の CSP は `default-src 'self'` ベース）。
- ウインドウタイトル、プロンプト内容、カレンダー ICS URL はログ・診断メッセージに出さない。
- ネットワークに出るのは、設定で明示した場合の「ICS 取得」「OpenAI 互換判定器」と、`hitoiki version --check` の実行時、`general.update_check = true` のときの 1 日 1 回の最新版確認だけ。AI 振り返りは外部コマンドに委ね、Hitoiki 自身は通信しない。

### 1.3 スコープ外

エージェント自体の稼働管理、チーム向け機能・共有、データ同期、高機能なタスク管理（カンバン・依存関係・サブタスク）、予実管理の専用機能（実績は `activity_segment` から算出し、見積もりは `task.estimate_min` の 1 フィールドのみ）。

## 2. クレート構成とプロセスモデル

### 2.1 ワークスペース

| 場所 | 役割 |
|---|---|
| `crates/hitoiki-core` | ドメインモデル（`model.rs`）、スキーマ（`migrations/`、`db.rs`）、設定（`config.rs`）、時刻（`timefmt.rs`、`clock.rs`）、帰属判定パイプライン（`pipeline/`）、再計算（`rebuild.rs`）、ルール（`rules.rs`）、プッシュ判定（`push/`）、サービス層（`service/`）、リポジトリ（`repo/`）。OS 非依存 |
| `crates/hitoiki-collect` | 収集器のトレイト（前面・無操作・セッション）、イベント型、DB へ書く状態機械 `Recorder`、テスト用 `MockCollector` |
| `crates/hitoiki-collect-win` | Windows 収集器の実装。非 Windows ではビルドは通るが何も収集しない |
| `crates/hitoiki-agents` | エージェント（Claude Code / Codex）のセッションログ取り込み、ファイル監視 |
| `crates/hitoiki-calendar` | ICS カレンダーの取得・解析（RRULE/EXDATE/TZID 対応） |
| `crates/hitoiki-judge` | 判定器（外部コマンド、OpenAI 互換）、質問セット、パイプラインへのアダプタ。`bin/hitoiki-echo-judge` は動作確認用 |
| `crates/hitoiki-ipc` | 名前付きパイプ（Windows）/ Unix ドメインソケット（テスト・開発用）による通知 |
| `crates/hitoiki-daemon` | 常駐ランタイム（収集・取り込み・再計算・カレンダー・プッシュ・IPC の各スレッド）、ヘッドレス実行、`doctor` |
| `crates/hitoiki-cli` | `hitoiki` CLI。引数の解釈と出力整形のみで、ロジックは core の `service` を呼ぶ |
| `crates/hitoiki-app` | Tauri v2 の常駐 GUI（`hitoiki-app.exe`）。トレイ、ウインドウ、トースト、Tauri コマンド |
| `ui/` | TypeScript + Vite + React のフロントエンド。ビルド結果（`ui/dist`）が `hitoiki-app` に埋め込まれる |

依存の向きは、`cli` / `app` → `daemon` → `collect` / `agents` / `calendar` / `judge` / `ipc` → `core`。CLI と Tauri は必ず core の関数を呼び、ロジックを重複させない。

### 2.2 プロセスモデル

- **`hitoiki-app.exe`（常駐アプリ）**: 起動するとトレイに常駐し、`hitoiki-daemon` の `Runtime` をプロセス内で動かす。ウインドウを閉じても常駐し、トレイの「終了」で止まる（進行中の区間は閉じて保存される）。`tauri-plugin-single-instance` で二重起動を防ぎ、`general.autostart` でログオン時に自動起動する。
- **`hitoiki daemon`（ヘッドレス）**: 同じ `Runtime` を UI なしで動かす。トーストは出さず、通知はログのみ。
- **CLI（`hitoiki ...`）**: DB を直接読み書きする。常駐プロセスが動いていなくても動作する。DB を変更したら名前付きパイプで常駐側に通知する（失敗しても無視する）。
- **IPC**: 名前は `\\.\pipe\hitoiki-<ユーザー SID>`。作成者ユーザーのみアクセス可能な DACL（`D:P(A;;GA;;;<SID>)`）で作る。メッセージは改行区切り JSON で 1 リクエスト 1 応答。種類は `task_changed` / `data_changed` / `open_ui` / `ping` / `shutdown`。
- **重複起動の検出**: `Runtime::start` は最初に IPC サーバを立て、パイプ名が使用中なら `AlreadyRunning` で失敗する。したがって常駐処理は同時に 1 つだけ。`hitoiki daemon` が動いている間に `hitoiki-app.exe` を起動すると、アプリは UI だけ（`RuntimeMode::External`）で動き、収集は daemon 側が行う。

### 2.3 Runtime のスレッド

スレッドごとに別の SQLite 接続を持つ（WAL + `busy_timeout` 5 秒）。収集データ（`window_span` / `idle_span`）を書くのは `Recorder` だけ。

| スレッド | 内容 |
|---|---|
| `hitoiki-rebuild` | 帰属判定の再計算。起動直後に 1 回、以降は 60 秒ごと、および要求から 3 秒のデバウンス後。直近 `collect.rebuild_window_min`（既定 180 分）を再計算する |
| `hitoiki-recorder` + 収集スレッド | 前面ウインドウ・無操作・ロック/スリープの収集と DB への書き込み |
| `hitoiki-agent-bridge` | エージェントログの増分取り込み。完了ごとに再計算を要求する |
| `hitoiki-calendar` | `calendar.fetch_interval_min`（既定 15 分）ごとに ICS を同期 |
| `hitoiki-push` | 60 秒ごとにプッシュを評価 |
| `hitoiki-update` | `general.update_check = true` のときだけ、起動の 3 分後と以降 24 時間ごとに最新版を確認（[2.5](#25-更新の確認)） |
| IPC サーバ | メッセージ受信。`task_changed` / `data_changed` は再計算を要求、`open_ui` は画面を開く |

UI への通知は `RuntimeSink`（`segments_updated` / `push` / `doing_changed` / `import_completed` / `open_ui` / `shutdown_requested` / `update_available`）を介して行う。アプリは `TauriSink`、ヘッドレスは `LogSink` を使う。

### 2.4 収集器（Windows）

- **前面ウインドウ**: `SetWinEventHook`（フォアグラウンド変化とタイトル変更）に加えて `collect.poll_interval_sec` ごとのポーリングで補う。取得するのは hwnd、pid、exe パス/名、タイトルのみ。
- **無操作**: `GetLastInputInfo` を 1 秒間隔でポーリングし、`collect.idle_threshold_sec` を超えたら無操作区間を開く（開始は最終入力時刻）。入力の内容は取らない。
- **ロック/スリープ**: `WTSRegisterSessionNotification`（`WM_WTSSESSION_CHANGE`）と `WM_POWERBROADCAST`。`idle_span.reason` は `no_input` / `locked` / `sleep`。
- **除外**: `collect.exclude`（exe 名、タイトル正規表現。両方指定すると両方一致で除外）に一致したウインドウは `excluded = 1` とし、exe パスとタイトルを保存しない。`collect.drop_excluded = true` なら区間自体を作らない。既定でパスワードマネージャ（KeePass 系、1Password、Bitwarden）とプライベートブラウズのタイトルを除外する。
- **Hitoiki 自身**: 前面ウインドウが Hitoiki 自身（exe 名 `hitoiki-app.exe`、または自プロセスの pid）の間は区間を作らない（記録の空白になる）。`collect.record_self = true` で記録する。
- **異常終了への備え**: `Recorder` はハートビートを保存し、起動時に前回の未終了区間（`end_at` が NULL）を最後のハートビートで閉じる。
- **稼働の記録と「記録なし」**: `Recorder` は起動時に `setting` の `collector.runs`（JSON 配列。開始・最終ハートビート・正常停止か。60 日・400 件まで）へ新しい稼働を追加し、10 秒ごとのハートビートで最新の稼働の終了時刻を進め、正常停止時に `clean` を立てる。`recording::gaps` は、稼働と稼働のあいだ（クラッシュ、PC 終了、アプリ未起動）と、最後の稼働が止まってから現在まで（最後のハートビートが 30 秒より古い、または正常停止済み）を「記録なし」の区間として返す。1 分未満の空白と、最初の稼働より前（稼働の記録を持たない古い版の期間）は含めない。無操作・ロック・スリープ（`idle_span`）、Hitoiki 自身の前面（`record_self = false`）、除外の破棄は収集器が動いたうえでの空白なので「記録なし」ではない。スリープ中はプロセスごと止まるが、復帰後の最初のハートビートで稼働が続いたものになり、その間は `idle_span`（sleep）が説明する。前面・無操作・セッションの収集スレッドのどれかが停止要求なしに終了したときは、エラーをログに出して稼働のハートビートを止める（以降が「記録なし」に見える）。
- **見える化**: `hitoiki log` と UI のログ取得（`Log.gaps`）は記録なしの区間を含み、今日画面のタイムラインは斜線で重ねる。`hitoiki doctor` の `collect` は直近 24 時間の記録停止を合計して 10 分以上なら警告する。トレイの先頭（とツールチップ）は、DB のハートビート（30 秒以内）から「記録中」/「記録停止中」を出す（`External` のときは daemon 側で収集中である旨を添える）。

### 2.4.1 ログ

`hitoiki-app` とヘッドレス daemon は、同じ `hitoiki_daemon::init_logging` で `%LOCALAPPDATA%\hitoiki\logs\hitoiki.log.<日付>`（日次ローテーション、14 日分。古いファイルはローテーション時に `tracing-appender` が削除する）と stderr に出す。起動・終了・Recorder の開始/停止・収集スレッドの異常終了・panic（場所のみ）を残す。ウインドウタイトル、プロンプト内容、ICS URL は出さない。

### 2.5 更新の確認

オプトインの機能で、`general.update_check`（`Option<bool>`）が `Some(true)` のときだけ動く。`None`（未設定）と `Some(false)` では通信しない。

- 取得は `hitoiki-daemon` の `update::fetch_latest`（ureq で `https://api.github.com/repos/fuchigta/hitoiki/releases/latest` を GET、User-Agent は `hitoiki/<version>`）。`hitoiki version --check` も同じ関数を使う。識別子・利用状況は送らない。
- 比較・判定・保存の純粋な部分は core の `service::update`（`v` 接頭辞を除いて数値で比較、プレリリース無視。確認の要否 `is_due`、通知の要否 `should_notify`）。時刻は `Clock` 経由で渡す。
- `hitoiki-update` スレッドは、起動の 3 分後（最後の確認から 24 時間未満なら残りの時間）に確認し、以降 24 時間ごとに繰り返す。取得に失敗したときは何も保存せず、通知もせず（ログのみ）、1 時間後に再試行する。
- `setting` テーブルのキー: `update.last_checked_at`（最後に確認した時刻。再起動しても 24 時間以内なら再確認しない）、`update.latest_version`（最後に分かった最新版）、`update.notified_version`（通知済みの版。同じ版は 1 回だけ通知）。
- 通知は `RuntimeSink::update_available`。アプリ（`TauriSink`）はトーストを出し（一時停止中は出さない）、トレイに「新しいバージョン vX.Y.Z をダウンロード…」を出す。ヘッドレスはログのみ。トーストやメニューを押すとリリースページ（`https://github.com/fuchigta/hitoiki/releases/latest`）を既定のブラウザで開く。プッシュ通知の 1 日の上限などの抑制ルールとは別枠で、会議中・フォーカス中の保留は行わない。
- `hitoiki status` の `update_available` は保存済みの `update.latest_version` から求める（status 自体はネットワークに出ない）。
- 自動ダウンロード・自動インストールはしない。

## 3. データモデル

時刻はすべて UTC の Unix ミリ秒（`INTEGER`）、ID は ULID（`TEXT`）。スキーマは `crates/hitoiki-core/migrations/*.sql` にあり、起動時に自動適用される。既存 DB にマイグレーションを適用する前には、`hitoiki.db.bak-<日時>` へ `VACUUM INTO` でバックアップする。マイグレーションは現在 `0001_init.sql`（初期スキーマ）、`0002_judge_cache.sql`（判定器キャッシュ）、`0003_block_template.sql`（ブロックのテンプレート）、`0004_correction.sql`（実績の補正）。

### 3.1 データの 3 層

| 層 | 性質 | 再計算 |
|---|---|---|
| 生データ層（raw） | 収集・取り込みの記録。追記が基本で、後から書き換えない | 対象外 |
| 派生層（derived） | 生データ + ユーザー層のルール/確定から計算した結果。いつでも作り直せる | 範囲単位で全置換 |
| ユーザー層（user） | ユーザーの入力・確定・設定。失ってはならないもの | 対象外 |

派生層が壊れても、生データとユーザー層から再構築できることが設計の前提である。ユーザーによる区間の確定は派生層を直接書き換えず、ユーザー層の `segment_override` に残し、再計算のたびに最優先で適用する。

### 3.2 生データ層

| テーブル | 内容 |
|---|---|
| `window_span` | 前面ウインドウの区間（`hwnd`, `pid`, `exe_path`, `exe_name`, `title`, `excluded`）。`end_at` NULL は進行中 |
| `idle_span` | 無操作の区間。`reason` は `no_input` / `locked` / `sleep` |
| `agent_event` | エージェントのイベント（`agent`, `session_id`, `kind`, `cwd`, `git_branch`, `prompt_excerpt`, `source_file`, `source_offset`）。`kind` は `session_start` / `prompt_submit` / `response_complete` / `session_end`。`(agent, session_id, kind, at)` で一意 |
| `agent_ingest_offset` | ログファイルごとの取り込み済みオフセット（増分取り込み用） |
| `agent_session_window` | セッションと hwnd の結び付け（`bound_at`）。パイプラインの出力で、再計算で作り直される |
| `context_hint` | 補助コレクタ（ブラウザ拡張のネイティブホストなど）が送る詳細コンテキスト（`source`, `hwnd`, `kind`, `value`）。現状は保存のみで、帰属判定パイプラインでは使っていない |
| `calendar_event` | カレンダー予定（`source`, `external_id`, 開始・終了, `all_day`, `title`, `location`, `is_busy`）。`(source, external_id, start_at)` で一意 |

`agent_session_window` はパイプラインが生成するが、位置づけは生データ層に置かれており、再計算の範囲ごとに置き換える。

### 3.3 派生層

`activity_segment`: 帰属判定の結果。`kind`（`work` / `agent_dialog` / `agent_watch` / `meeting` / `idle` / `break` / `interrupt`）、`task_id`、`activity_label`、`confidence`、`decided_by`（`rule` / `judge:<name>` / `calendar` / `user` / `doing`）、`status`（`auto` / `needs_review` / `confirmed`）、根拠の `window_span_ids`（JSON 配列）、`agent_session_id`、表示・判定用に非正規化した `exe_name` / `title`、PC 外の印 `offline` を持つ。

### 3.4 ユーザー層

| テーブル | 内容 |
|---|---|
| `task` | タスク。`status`（`todo` / `doing` / `done` / `dropped`）、`kind`（`normal` / `interrupt`）、`urgent` / `important`、`estimate_min`、計画時刻、`key_result_id`、`external_url` |
| `task_doing_log` | doing の履歴。「その時点の doing タスク」を再計算で復元するために使う |
| `task_match_rule` | 行動ログ → タスクの帰属ルール（`field`: `exe` / `title` / `cwd` / `repo` / `url` / `git_branch`、`pattern_type`: `glob` / `regex` / `contains`、`priority`、`enabled`） |
| `activity_rule` | 行動ログ → 活動ラベルのルール（タスク非依存） |
| `segment_override` | ユーザーによる区間の確定（期間と、`kind` / `task_id` / `activity_label` の上書き。NULL の項目は元の値を保つ。`task_id` が空文字列なら「タスクなし」に外す）。`offline`（PC 外の印）と `correction_id`（作った補正）を持つ（3.6） |
| `segment_correction` | 実績の補正 1 回分（期間、割り当て先の `task_id` / `kind`（休憩は `break`）/ `activity_label`、合計と PC 外の時間、区間数）。取り消しの単位（3.6） |
| `time_block` | 計画（タイムブロッキング）。`focus` が 1 のものは集中ブロック |
| `block_template` / `block_template_item` | ブロックのテンプレート（働き方のパターン）。`block_template` は名前（一意）・説明・割り当てた曜日（`weekdays`、JSON 配列、1=月 … 7=日）、`block_template_item` はローカル時刻の 0 時からの分（`start_min` 0〜1439、`end_min` 1〜1440）・`label`・`focus`・任意の `task_id` |
| `block_template_application` | テンプレートの適用履歴（テンプレート名、適用先のローカル日付、作成した `time_block.id` の JSON 配列、`undone_at`）。直前の適用を取り消すために使う |
| `objective` / `key_result` | OKR |
| `skill` / `task_skill` / `activity_skill_rule` | スキル（階層あり、`preset` フラグ）とタスク・活動との紐づけ |
| `checkin` | コンディション（`energy` / `mood` / `stress` 各 1〜5、`erg_tag`、`note`） |
| `hassle` | デイリーハッスル（`text`, `intensity`, `segment_id`） |
| `abc_record` | ABC 理論の記録（`activating_event`, `belief`, `consequence_*`, `dispute`, `ai_dialog_md`） |
| `reflection` | AI 振り返り（`kind`: `day_start` / `day_end` / `weekly`、`context_json`, `questions_md`, `answers_md`） |
| `push_log` | 通知ログ（トリガー、配信済みか、応答、`snoozed_until`、`suppressed_reason`） |
| `setting` | キー・値の設定（収集器の稼働は `collector.heartbeat` と `collector.runs`） |
| `judge_cache` | 判定器の答えのキャッシュ（`key`, `answer_json`, `judge_name`, `created_at`）。保持期間 30 日 |

予実について、実績入力フィールドは作らない。実績は常に `activity_segment` から算出する。

### 3.5 ブロックのテンプレート

`service::block_template`（CLI の `block template`、Tauri のコマンドはこの薄いラッパー）。

- **データ**: テンプレートは時刻をローカル時刻（0 時からの分）で持ち、日付を持たない。同じ名前は作れない。曜日は複数割り当てられ、同じ曜日に複数のテンプレートがあってもよい（`suggest` は該当するものをすべて返す）。
- **適用**（`apply`）: 指定日（既定は今日）のローカル日付と時刻を `timefmt::local_minute_to_ms` で UTC に変換して `time_block` を作る。DST で存在しない時刻（春の空白）は存在する最初の時刻まで進め、2 回ある時刻（秋）は早い方を採る。開始と終了が同じ時刻に潰れたブロックは作らない。24:00 は翌日 0 時。
- **重なり**: 既存の `time_block` と、`is_busy` で終日でないカレンダー予定を重なりとして数える（`preview` が項目ごとに列挙する）。`overlap = skip`（既定）は重なる項目を作らず `skipped` に返し、`overlay` は重ねて作る。
- **取り消し**（`undo`）: 取り消していない直近の適用（`--date` ならその日に適用したもののうち直近）で作ったブロックを削除し、履歴に `undone_at` を付ける。すでに消されたブロックや、手で足したブロックは巻き込まない。
- **保存**（`save_from`）: 指定日の `time_block` からテンプレートを作る。日をまたぐブロックはその日の範囲に切る。
- 画面に 1 回だけ出す案内（閉じたら出さない）は `service::hint` が `setting` の `hint.<名前>.dismissed` に記録する。

ブロックの編集（`service::plan::update`）は、時間・タスク・ラベル・集中をまるごと置き換える。ID は前方一致で指定できる。

### 3.6 実績の補正（`service/correction.rs`）

「実績入力フィールドは作らない」を保ったまま、時間帯の範囲指定でまとめて付け替える。補正は `segment_override`（ユーザー確定、再計算でも最優先）として保存し、1 回の付け替えを `segment_correction` と `segment_override.correction_id` でひとまとまりにする。CLI の `review set --from/--to`・`review corrections`・`review undo-correction` と Tauri の `correction_*` コマンドはこの `service::correction` の薄いラッパー。

- **候補**（`candidates`）: 範囲 `[from, to)` と重なる `activity_segment` を、範囲で切った部分（境界にまたがる区間は内側だけ）として挙げる。さらに `recording::gaps` のうち区間で覆われていない部分を「記録なし」の候補（ID は `gap:<開始のミリ秒>`）として挙げる。分類（`Category`）は 確定済み（status が confirmed）→ 会議 → 無操作・休憩 → エージェント対話・見守り → 作業・割り込み の順に決まる。
- **初期のチェック**: 作業とエージェント対話は ON。確定済みは `review.correct_include_confirmed`、会議は `correct_include_meeting`、無操作・休憩・記録なしは `correct_include_idle` が true のときだけ ON（確定済みの会議は両方が必要）。CLI の `--include-*` が設定を上書きし、`selected`（UI のチェックリスト）か `exclude`（CLI）で最終的な選択を決める。
- **割り当て先**: `task` / `no_task` / `rest` のどれか 1 つ（活動ラベルだけでもよい）。候補ごとに override を 1 つ作る。タスクは `task_id`、「タスクなし」「休憩」は `task_id = ''`（パイプラインが元のタスクを外す）で表し、休憩は `kind = break`。エージェント対話・見守り・会議などは `kind` を変えず、タスクだけを付け替える。
- **PC 外の作業**: 候補が無操作・休憩・記録なしなら、override は `kind = work`（休憩への補正は `break`）で `offline = 1` にする。パイプラインの `apply_overrides` は、`offline` の override について、既存の区間を書き換えるだけでなく、範囲（出力範囲と現在時刻で切る）のうち区間が無い部分に区間（確定・`decided_by = user`・`offline`）を**生成する**。収集器が動いていなかった「記録なし」の時間帯には `window_span` も `idle_span` も無く、通常の override は何にも当たらないため。再計算（`rebuild`）でも override から同じ区間が作られるので保持される。`offline` は統合（`merge.rs`）の同一内容の条件にも入り、通常の作業とは連結しない。`offline` でない override は区間を生成しない。
- **確定の反映**: `apply` は 1 つのトランザクションで補正と override を保存し、`activity_segment` にも即時に反映する（範囲の端で区間を切り、内側を更新し、`offline` なら覆われていない部分に区間を挿入する）。判定器を呼ぶ再計算を伴わないので、直近 `collect.rebuild_window_min` より前の日にも使える。次の再計算で同じ内容になる。`log` の「記録なし」からは、計上済み（`offline`）の部分を除く。
- **取り消し**（`undo`）: 補正と、その `correction_id` を持つ override を削除し、補正の期間を `rebuild_with_judge` で再計算する（判定器は呼び出し側が渡す。CLI は `judge.enabled` から構築、アプリは `hitoiki_daemon::build_judge`）。派生層の復元は再計算で行うため、その期間の判定は最新のルール・doing で作り直される。
- **プレビュー**（`preview`）: 対象の合計、うち PC 外（無操作・休憩・記録なし）の時間、外した候補の数と、説明文（`summary`）を返す。DB は変更しない。UI は、チェックの切り替えごとに同じ数え方を `ui/src/correct-util.ts` で再計算し、範囲選択の吸着（端を 1 分刻みに丸めて、`review.snap_sec` 以内の区間の境目へ寄せる）もそこにある。

## 4. 帰属判定パイプライン

`crates/hitoiki-core/src/pipeline/` は、生データを入力とし `activity_segment` と `agent_session_window` を出力する純粋関数 `build`（`pipeline/mod.rs`）である。現在時刻は `PipelineInput.now` で受け取る。

### 4.1 処理順

`build` の実行順は次のとおり。

1. **タイムライン構築**（`timeline.rs`）: `window_span` の重なりを「後から始まった方を優先」で解消し（進行中の区間は `now` で閉じる）、`idle_span` を重ねて作業区間と無操作区間の列にする。
2. **エージェント対話**（`agent.rs`）: `agent_dialog` と `agent_watch` を付ける（4.2）。この後、出力範囲に区間を切り取る。
3. **会議**（`meeting.rs`）: カレンダーの予定（busy、終日でない）と重なる無操作区間と、会議アプリ（`collect.meeting_exes`）が前面の作業区間を `meeting` にする（4.3）。
4. **override**: `segment_override` を作成順に適用し、`confirmed` / `decided_by = user` / 確信度 1.0 にする（`attribute.rs`）。`offline` の override は、区間の無い部分（記録なし）にも区間を作る（3.6）。
5. **ルール**: `task_match_rule` でタスクを決め（`decided_by = rule`、確信度 1.0）、`activity_rule` で活動ラベルを付ける。
6. **通過系アプリの吸収**（`transient.rs`）: `collect.transient_exes`（既定は `explorer.exe`、`StartMenuExperienceHost.exe`、`SearchHost.exe` など。`ApplicationFrameHost.exe` は UWP アプリの実体なので含めない。exe 名は大文字小文字を区別しない）の `work` 区間が隙間なく連続し、その合計が `collect.transient_max_sec`（既定 60 秒）未満なら、独立した区間にせず**直前の作業区間に吸収**する。直前が無い（その日の最初、無操作明けなど）なら直後。吸収先は、隙間なく接する確定前の通常の `work` 区間だけで、`agent_dialog` / `agent_watch`（そのウインドウにいた時間だけを計上するため伸ばさない）、会議、無操作、override（`confirmed`）は吸収先にならない。前後とも吸収先にならなければ通常の区間のまま。連続時間は出力範囲に切り取る前の長さで測る。ルールでタスクや活動が決まった区間と override 済みの区間は対象外。吸収する区間は判定器に問い合わせず、吸収先の帰属（種別・タスク・ラベル・確信度・根拠・exe/title）を写して、以降は吸収先と同じ内容として扱う（割り込み検出で通信アプリの一部になり、統合で 1 区間になる）。`window_span` と `window_span_ids`（根拠）は変えない。
7. **判定器**: ルールでタスクが決まらなかった `work` 区間だけを問い合わせる（4.5）。
8. **割り込み検出**: コミュニケーション系アプリ（`collect.communication_exes`）が前面になり、その後に元のタスクに帰属しない作業が `collect.interrupt_min_sec`（既定 180 秒）以上続いたら、その区間から元のタスクへ戻る手前までを `interrupt`（`needs_review`）にする。「元のタスク」は、その時点の doing タスク、無ければ `focus` の `time_block` のタスク。ルール・判定器の結果を見るため、doing フォールバックより前に行う。
9. **doing フォールバック**: どれにも該当しない `work` 区間に、その時点の doing タスクを確信度 0.5・`decided_by = doing` で付ける。状態は `needs_review`。ただし `review.trust_doing = true` なら `auto`（始めたタスクを信頼する）。doing の開始・終了時刻で区間を分割する。
   - **ロック中の自動休憩**（`apply_locked_break`）: 判定の決まっていない（`decided_by` なし）無操作区間のうち、重なる `idle_span` の理由が `locked` / `sleep` のものの合計が `review.auto_break_locked_min`（既定 10 分、0 で無効）以上なら、`break`（`decided_by = rule:locked`、確信度 1.0、`auto`）にする。override と予定との突き合わせ（会議）は先に適用済みなので影響しない。
10. **状態付け**: 状態が未決の区間は、確信度が `judge.auto_confidence_threshold`（既定 0.8）以上なら `auto`、未満または確信度なしなら `needs_review`。
11. **統合**（`merge.rs`）: 隣接する同一内容（種別、タスク、ラベル、状態、セッション）の区間を連結し、同一内容にはさまれた `collect.merge_gap_sec`（既定 30 秒）以下の短い `work` を吸収する。`agent_dialog` / `agent_watch` と `confirmed` 区間は、時間を水増ししないため、前後の区間として短い中断を吸収することはない。

「固定」（`fixed`）フラグが付いた区間（対話・見守り・会議・override・予定外の無操作）は、後続の段階で上書きされない。

### 4.2 エージェント対話時間

計上するのは「ユーザーがそのセッションのウインドウにいた時間」だけである。応答完了から次の送信までのラグの間に別ウインドウにいた時間は、通常の作業として残る。

- **セッションとウインドウの結び付け**: 次の順に最初に見つかったものを使う。(1) `prompt_submit` の時刻に前面だったウインドウが `collect.terminal_exes` または `collect.agent_host_exes`（GUI のエージェントクライアント。既定は `Orca.exe`、`Cursor.exe`、`Windsurf.exe`、`Code - Insiders.exe`）のもの。(2) 直前 `agent.bind_lookback_sec`（既定 5 秒）内の最も新しい (1) の区間。(3) `agent.bind_unlisted = true`（既定）なら、送信時に前面だった一覧外のウインドウ。ただし除外ウインドウ、`transient_exes`、Hitoiki 自身、ブラウザ（chrome / msedge / firefox / brave / vivaldi / opera / arc / iexplore）、`communication_exes`、`meeting_exes` は対象外で、送信時刻が無操作（ロック・スリープを含む）の最中でもない。(3) は、人が打った送信なら直前に入力があるはずで、無操作中の送信（エージェント自身の自動投入など）や、ブラウザ・通信アプリなどの別作業の裏で動いたセッションを誤って対話時間にしないための歯止めである。それでも別の一覧外アプリで作業中の裏で送信されたものは対話時間に混ざりうるので、そのアプリを `communication_exes` などに入れるか `agent.bind_unlisted = false` にして一覧だけで判定する。対応付けできなかった送信は `hitoiki doctor` の `agent` が、直近 24 時間の件数と送信時の前面 exe 名（タイトルは出さない）で知らせる。結び付けは `(hwnd, 正規化タイトル)` で行い、タイトルの先頭の記号（スピナー）と空白は `normalize_title` で除く。
- **agent_dialog**: ユーザーが送信（`prompt_submit`）するまでの「対象期間」のうち、結び付けたウインドウが前面で無操作でない部分。対象期間の開始は、直前の応答完了 / 直前の送信 / セッション開始（最初の送信とほぼ同時の開始は無いものとして扱う。`SESSION_START_GRACE` = 5 秒）。いずれも無ければ、送信直前で同じ `(hwnd, タイトル)` が連続していた区間の開始まで遡る。
- **agent_watch**: 送信後、同じウインドウが前面のまま続いた時間。次の応答完了または次の送信、あるいは `agent.watch_max_min`（既定 10 分）で打ち切る。
- **タスクの決定**: セッションの `cwd` / `git_branch` に対する `task_match_rule` が最優先（`rule`、確信度 1.0、`auto`）。無ければセッション開始時点の doing タスク（`doing`、確信度 0.5、`needs_review`）。どちらも無ければタスクなしで `needs_review`。
- **同一ウインドウの複数セッション**: 同じ `(hwnd, タイトル)` に結び付いた別セッションの対象期間が重なる場合は、その時点で最も近い過去の送信を持つセッションに帰属させる（ベストエフォート）。

### 4.3 会議・予定

busy で終日でない予定の時間帯について、無操作区間と、`meeting_exes` が前面の未固定作業区間を `meeting`（`decided_by = calendar`、確信度 1.0、`auto`）にする。予定タイトルが `task_match_rule`（`title` フィールド）に一致すればタスクも付く。予定と重ならない無操作区間は `needs_review` にする（休憩か離席かをユーザーが決める）。決め方は、ロック・スリープが長ければ自動で休憩（4.1 の「ロック中の自動休憩」）、そうでなければ仕分け画面のワンタップ（4.7）。

### 4.4 ルールの評価

`rules.rs` の `TaskRuleSet` / `ActivityRuleSet` が `priority` 順に評価する。対象フィールドは `exe` / `title` / `cwd` / `repo` / `url` / `git_branch`（ウインドウに対しては exe とタイトル、セッションに対しては cwd・リポジトリ・ブランチ）。

### 4.5 判定器とキャッシュ

判定器の答えは `JudgeAnswer`（`task_id`, `activity_label`, `is_interrupt`, `confidence`, `judge_name`）。答えがあれば `decided_by = judge:<name>` と確信度を付け、`is_interrupt = true` なら `interrupt`（`needs_review`）にする。判定器の呼び出しは `rebuild.rs` の `CachedJudge` が包み、区間の内容（exe、正規化タイトル、根拠ウインドウ ID）と、判定器の設定・活動ラベル・未完了タスク集合のハッシュからキーを作って `judge_cache` を引く。キャッシュに無い区間だけを実際に問い合わせる。未判定（タイムアウトなど）はキャッシュせず、次回再び問い合わせる。キーに開始・終了時刻を含めないのは、範囲の切り取りや進行中区間の伸びで変わらないようにするため。

### 4.6 再計算（rebuild）

`rebuild.rs` は DB から生データを読み（範囲の手前 6 時間まで遡る）、パイプラインを実行して派生層を置き換える。結果が「どの範囲をどの順で再計算してきたか」に依存しないよう、次を守る。

- **範囲の拡張**: 境界をまたぐ既存区間を含むところまで広げる（区間を重ねない・欠けさせない）。範囲内の `prompt_submit` の対象期間の開始まで広げる（要求範囲の開始から最大 24 時間まで）。収束するまで繰り返す（上限 16 回）。
- **排他**: 判定器の呼び出しを含む計算は書き込みトランザクションの外で行い、最後の「範囲の再確定 → 重なる区間の削除 → 挿入」を 1 つの `BEGIN IMMEDIATE` トランザクションで行う。範囲が読み込み時から変わっていたら（他の再計算が書き込んだら）読み直す。3 回失敗したら 4 回目は書き込みロックを取ったまま読み・計算・書き込みを行い、必ず終わらせる。
- **書き込み**: `activity_segment` と `agent_session_window` を範囲内で全置換し、新しく得た判定器の答えを `judge_cache` に保存して古いものを削除する。

### 4.7 仕分けの補助（`service/review.rs`）

- **無操作のワンタップ分類**（`IdlePreset`）: 新しい種別は作らず、既存の種別と活動ラベルの組で表し、`segment_override` に保存する（再計算でも残る）。`break` = 種別 break、`meeting` = 種別 meeting、`thinking` = 種別 work + 活動「考えごと」（タスクなし）、`away` = 種別 break + 活動「離席」。`set_idle` は区間 ID 指定（CLI・UI）、`set_idle_range` は期間指定（通知のボタン。区間がまだ無くても期間全体の override を保存する）。
- **一括割り当て**（`set_many`）: 複数の区間 ID に同じタスク・種別・活動（と `--rule`）を 1 つのトランザクションで適用する。同じ条件のルールは 1 つにまとめる。
- **ルール化の提案**（`suggest_rules` / `accept_suggestion` / `dismiss_suggestion`）: 直近 30 日にユーザーが確定した（`decided_by = user`）作業系の区間を、(条件, タスク) ごとに数える。条件はエージェント区間なら作業ディレクトリ（`cwd` の glob）、それ以外は exe 名（`exe` の contains）。`review.rule_suggest_min_count`（既定 3）回以上で、同じルールが未登録かつ断られていないものを多い順に返す。受け入れは既存の `--rule` と同じ重複確認（`ensure_task_rule`）でルールを追加する。断った組み合わせは `setting` の `review.rule_suggest_dismissed`（キー → 再提案してよい時刻の JSON）に `review.rule_suggest_snooze_days`（既定 14）日分記録する。

## 5. エージェントアダプタ

フックの設定を必須にしない。エージェントが残すセッションログ（JSONL）をファイル監視で増分取り込みし、`agent_event` に保存する。

- **共通**（`hitoiki-agents`）: `AgentAdapter` トレイト（`name` / `log_roots` / `accepts_file` / `parse_incremental`）。`agent.enabled` で有効なアダプタを選ぶ。`notify` クレートでファイルを監視し、2 秒のデバウンスと 30 秒ごとの再走査を行う。取り込み位置は `agent_ingest_offset` に保存し、改行で終わっていない最終行は書き込み途中とみなしてオフセットを進めない。重複は `(agent, session_id, kind, at)` の一意索引で除く。`agent.prompt_excerpt_chars`（既定 200、0 で保存しない）文字だけプロンプトの抜粋を保存する。解析できない行はスキップする。
- **Claude Code**: `$CLAUDE_CONFIG_DIR` または `~/.claude` 配下の `projects/<エンコードされたパス>/<session>.jsonl`。応答完了は `type=assistant` かつ `message.stop_reason == "end_turn"`（同じ `message.id` は 1 件にまとめる）。人間の入力とツール結果は `message.content` の形で区別する。`entrypoint` が `sdk-` で始まる非対話実行（Hitoiki 自身の AI 振り返りなど）、サブエージェント（`subagents/`、`isSidechain`）は対象外。
- **Codex CLI**: `$CODEX_HOME` または `~/.codex` 配下の `sessions/YYYY/MM/DD/rollout-*.jsonl`。新形式は `session_meta` / `event_msg`（`user_message` と `task_complete` / `turn_complete`）を使う。旧形式は行ごとの時刻が無いため、`session_start` 以外は出さない。非対話実行（`codex exec`）のセッションは対象外。形式は未検証（14 章）。
- **手動取り込み**: `hitoiki agent import [--agent ...] [--since ...]`。`hitoiki agent sessions` でセッション一覧を見る。

## 6. カレンダー（ICS）

`hitoiki-calendar` は `calendar.sources`（`name`, `type`, `url`）の ICS URL を `ureq` で取得する（タイムアウト 30 秒）。取得（`IcsAdapter::fetch`）と解析（`parse_ics`）は分離している。

- 取得範囲は `calendar.past_days`（既定 7）〜 `calendar.future_days`（既定 14）。RRULE を展開し、EXDATE を適用する。Windows のタイムゾーン名は `tzmap` で IANA に解決する。
- `STATUS:CANCELLED` は除外。`TRANSP:TRANSPARENT` と `X-MICROSOFT-CDO-BUSYSTATUS:FREE` は `is_busy = 0`。
- 同期は対象範囲の `calendar_event` をソースごとに置き換える。成功したソースがあれば再計算を要求する。
- URL は秘密情報なので、ログとエラーメッセージには `name` だけを出す。

## 7. 判定器

ルールで決まらなかった作業区間の帰属を、型付き質問への回答と確信度で返す差し替え可能な部品。`judge.enabled`（既定 false）が false の間は、判定器を呼ばず、ルールと doing フォールバックだけで帰属する（これが既定の「ルール」判定）。有効にすると `judge.default` が指すバックエンド（`judge.backends.<名前>`）を使う。

- **質問セット**: 既定は `segment_classification`（`task` = 未完了タスクからの選択、`activity` = `activity.labels` からの選択、`is_interrupt` = 真偽）。`%APPDATA%\hitoiki\questions\*.toml`（`paths::questions_dir()`）に質問セットを置くと、`applies_to` が `activity_segment`（または未指定）の最初の有効なセットで差し替えられる（読めない場合は組み込み）。選択肢は `open_tasks` / `activity_labels` / `skills` から動的に解決できる。
- **問い合わせの状態**: 区間（開始・終了、exe、タイトル、cwd）と文脈（doing タスク、直前 3 区間、重なるカレンダー予定）。
- **`external-command`**: 設定の `command` を起動し、標準入力に JSON（`version`, `state`（バッチなら `states`）, `questions`）、標準出力から JSON を受け取る。`timeout_sec`（既定 10）を超えたら未判定。`batch = true` で最大 20 件ずつまとめて送る。プロトコル版は `PROTOCOL_VERSION`（1）。
- **`openai-compatible`**: `{base_url}/chat/completions` に `temperature = 0` で問い合わせる。構造化出力（`json_schema`, strict）を要求し、サーバが 400/422 で拒否したら `json_object` に切り替える。API キーは `api_key_env` が指す環境変数から読み、`Authorization: Bearer` で送る（設定ファイルにキーを書かない）。
- **確信度**: 回答された質問の確信度の最小値を区間の確信度とする。`auto_confidence_threshold` 未満なら `needs_review` として人が確定する。

## 8. プッシュ通知

通知は「1 アクションで答えられる」ことを基本とし、文言は寄り添うトーンで評価を含めない。判定ロジック（`hitoiki-core/src/push/`）は OS 非依存の純粋関数 `evaluate` で、時計を直接読まない。評価は `hitoiki-daemon` の `push_tick` が 60 秒ごとに行う。

### 8.1 トリガー

同時に満たした場合の優先順は次のとおり。1 回の評価で配信されるのは最大 1 件。無操作中は `day_end` 以外発火しない。`push.disabled_triggers` で個別に無効化できる。

| ID | 条件 |
|---|---|
| `day_start` | 今日のロック解除/無操作明けがあり、`push.day_start_after`（既定 08:00）以降 `push.day_end_at` 前で、今日まだ配信していない。通知本体のクリックは今日の画面の「今日のデザイン」モード（`open_screen = today/design`）を開く |
| `day_end` | `push.day_end_at`（既定 18:30）以降に 10 分以上無操作、または終業時刻ちょうど（±60 秒）。1 日 1 回 |
| `weekly` | `push.weekly`（既定 金曜 17:00）以降で、今週（月曜始まり）まだ配信していない |
| `after_interrupt` | 割り込み区間が終わり、終了から 5 分以内で、その割り込み以降に未配信 |
| `meeting_overrun` | 会議アプリが前面で、予定の終了から 5〜180 分経過（ほかの予定が進行中なら対象外） |
| `switch_spike` | 直近 `push.switch_spike.window_min`（15）分の切替回数が `count`（20）を超える |
| `long_focus` | 同一キー（task、エージェントセッション、活動ラベルの順）の作業が `push.long_focus_min`（90）分以上連続 |
| `late_work` | `push.late_work_after`（20:00）〜翌朝の `day_start_after` に作業が続いている。時間帯ごとに 1 回 |
| `idle_return` | 直近で終わった無操作（`PushContext.last_idle`。隣り合う `idle_span` はつなぐ）が `push.idle_return_min`（既定 15 分、0 で無効）以上で、終了から 10 分以内。busy な予定（終日を除く）と重なる、休憩・会議・確定済みの区間が重なる（長いロックの自動休憩を含む）、その無操作の開始以降に配信済み、のいずれかなら発火しない。優先順は `day_start` の次 |

### 8.2 抑制ルール

候補ごとに `gate.rs` の次の順序で判定し、最初に該当したものが結果になる。抑制した場合は `push_log.suppressed_reason` 付きで記録する（同じ理由の連続抑制は記録しない）。

1. スヌーズ中（`snoozed`）。「今日はもういい」はローカル 24:00 までのスヌーズ。
2. 1 日の上限（`push.daily_limit`、既定 6、`daily_limit`）。
3. どのトリガーでも直前の配信から 10 分未満（`recent_push`）。
4. 同一トリガーの最小間隔（`push.min_interval_min`、既定 60 分）。直近 5 件のうち配信から 30 分以上経ったものの応答率が低い（3 件以上で評価）と、間隔を最大 4 倍まで延長する（`min_interval` / `min_interval_backoff`）。
5. 会議中、または `focus` の `time_block` 中は保留（`Defer`。ログは残さず、条件が残っていれば後の評価で出る）。`meeting_overrun` は会議の保留を受けない。

### 8.3 応答と表示

- **応答**: `store::apply_response` が応答を `push_log` に保存し、チェックイン（`checkin`）、ハッスル（`hassle`）、スヌーズに変換する。`idle_return` のボタン（`idle_break` / `idle_meeting` / `idle_thinking` / `idle_away`）は、ペイロードの `idle_range`（対象の期間）に `review::set_idle_range` で分類をつける（override 保存。UI へは `segments_updated` を送り、再計算を要求する）。
- **後からの回答と見送り**（`service::pushlog`）: `list`（配信済みの通知を状態 `unanswered` / `answered` / `dismissed` 付きで返す。トリガーの日本語名 `PushTrigger::label()`、ペイロード、`idle_range` を含む）と `answer`（`apply_response_with` を通す。CLI の `push answer` と同じ）。UI の「未回答の問いかけ」（`ui/src/push-answer.tsx`、ポップアップ窓と共有）は既存の Tauri コマンド `push_respond`（`runtime::handle_activation`）を呼ぶので、トーストの応答と同じ経路になる。
  - **記録の時刻**: checkin / hassle の `at` は応答時刻ではなく通知の時刻（`push_log.at`）。あとから答えても、通知時点の状態として時系列に載る。
  - **見送り**（`dismiss`）: `responded_at` を付けず `response_json` に `{"action":"dismiss"}` を残す（スキーマ変更なし）。未回答の一覧（`repo::push::list_unanswered`）から外れる。応答率（8.2 の 4）は `responded_at` の有無で数えるので、見送りは無応答のまま数える。つまり「答えない通知が続くと間隔を延ばす」ルールは、一覧から外しても効き続ける。見送ったあとでも答え直せる。
  - **対象期間**: `status` と今日画面の一覧は今日の分だけ。過去は `push list --date`。
- **トースト**（`hitoiki-app/src/notify.rs`）: WinRT の `ToastNotificationManager` でボタン付きトーストを出す（ボタンは最大 5 個、入力欄は 1 つ）。回答を求める通知（push_log に紐づき、ボタンか入力欄があるもの）は `scenario="reminder"`（`toast_scenario`）で、操作するまで残る（`push.toast_persistent`、false で従来の短い表示）。更新のお知らせとテスト通知は対象外。AUMID は `Hitoiki.App`。起動時に `HKCU\Software\Classes\AppUserModelId\Hitoiki.App` へ `DisplayName` / `IconUri` / `IconBackgroundColor` を書き、`SetCurrentProcessExplicitAppUserModelID` で設定する。起動時に前回のトーストを履歴から消す（受け手がいないため）。ボタンの引数は `action=<id>&value=<値>&push=<push_id>&open=<画面>`。
- **フォールバック**: トーストを出せない場合は、画面右下の小さなポップアップ窓（ラベル `popup`）に切り替える。UI へは Tauri イベント `push` も送る。
- **一時停止**: アプリ側の一時停止中は表示しない。

## 9. AI 振り返り

Hitoiki は推論を持たない。`ai.command`（既定 `["claude", "-p"]`）を子プロセスとして起動し、標準入力にプロンプトを渡して標準出力を受け取る。`ai.timeout_sec`（既定 180）を超えたら強制終了してエラーにする。非 0 終了と空出力もエラー。AI の呼び出し以外でネットワークに接続しない。

- **種別**: `day_start` / `day_end` / `weekly` と、ABC 記録の反論（`abc_dispute`）。
- **コンテキスト**: `service/context.rs` の `ContextJson`（集計、計画、doing タスク、割り込み、ハッスル、ABC 記録、チェックイン、要確認件数、OKR、前週との差、ERG の分布など。種別ごとに含める項目が変わる）。`hitoiki context` で AI 用に JSON 出力もできる。
- **テンプレート**: `crates/hitoiki-core/templates/{day_start,day_end,weekly,abc_dispute}.md` を組み込みで持つ。`{{context}}` や `{{date}}` などのプレースホルダを展開する。`ai.templates_dir`（未指定なら `%APPDATA%\hitoiki\templates`）に同名の `<name>.md` があればそちらを優先する。
- **保存**: 生成した問いを `reflection.questions_md` に保存し、回答を `answers_md` に書く。
- **エージェントからの利用**: `hitoiki context`、`hitoiki reflect ...` を CLI から呼べる。なお Claude Code が `claude -p` を実行しても、その `entrypoint` が `sdk-` で始まるので作業時間には混ざらない（5 章）。

## 10. CLI

共通仕様: 参照系は `--json` で機械可読な出力。対話的なプロンプトは出さず、不足引数はエラー。終了コードは 0 成功、1 一般、2 引数、3 対象なし、4 曖昧（`Error::exit_code()`）。`HITOIKI_HOME` で設定と DB の置き場所を差し替えられる。

主なコマンド: `status`、`task`（`rule` サブコマンドを含む）、`log`、`review`、`rebuild`、`block`、`check`、`hassle`、`abc`、`okr`、`skill`、`report`、`context`、`reflect`、`agent`、`daemon`、`ui`、`ingest context`、`native-host`（install / uninstall / ホスト動作）、`config`（path / get / set）、`doctor`、`version [--check]`。

`doctor` は config / db / collect / agent / calendar / notify / judge / ai を ok・warn・error で診断する。`collect` は直近 24 時間の記録停止（2.4）、`agent` は直近 24 時間のプロンプトのうちウインドウに対応付けできなかった件数（4.2）も見る。ネットワークへの実アクセス（カレンダー取得・判定器への疎通）は `--network` 指定時だけ。

`native-host` は、ブラウザ拡張からの URL などを `context_hint` に保存するネイティブメッセージングホスト（4 バイト長 + JSON、1 MB 上限）で、Chrome/Edge/Firefox 向けのマニフェストとレジストリ登録を `install` が行う。

## 11. GUI

`ui/` は Preact + Vite + TypeScript の SPA で、画面は `today` / `review` / `tasks` / `plan` / `reflect` / `report` / `records` / `okr` / `settings`、初回起動時の `onboarding`、通知用の `popup`。オンボーディングの完了は `setting` テーブルの `onboarding.completed_at` で判定し（`service::onboarding`）、未完了で `--screen` 指定なしに起動したときだけメインウインドウを開いて表示する。ロジックは持たず、Tauri のコマンド（`hitoiki-app/src/commands.rs`）が core の `service` を呼ぶ。区間更新・プッシュ・doing 変更・取り込み完了は Tauri イベントで UI に通知する。トレイ（`tray.rs`）のメニューは、現在のタスクと今日の合計の表示、タスクの切り替え、チェックイン（エネルギー）、ハッスルの記録、UI を開く、通知の一時停止、終了を持つ。

## 12. ファイル配置と設定

### 12.1 ファイル配置

| 用途 | パス |
|---|---|
| 設定 | `%APPDATA%\hitoiki\config.toml` |
| DB | `%LOCALAPPDATA%\hitoiki\hitoiki.db`（WAL。マイグレーション前のバックアップは同じ場所に `hitoiki.db.bak-<日時>`） |
| ログ | `%LOCALAPPDATA%\hitoiki\logs\hitoiki.log.<日付>`（日次ローテーション、14 日分。app と daemon で共通） |
| 判定器・プラグイン | `%APPDATA%\hitoiki\plugins\` |
| 質問セット | `%APPDATA%\hitoiki\questions\` |
| 振り返りテンプレート | `%APPDATA%\hitoiki\templates\` |
| 通知アイコン | `%LOCALAPPDATA%\hitoiki\app-icon.png`（起動時に書き出す） |

`HITOIKI_HOME` を設定すると、上記の設定系・データ系の両方がその配下にまとまる（テスト用）。

### 12.2 設定の主なキー

`config.toml` が無い、または項目が欠けていれば既定値で補う（`crates/hitoiki-core/src/config.rs`）。`hitoiki config get/set` はドット区切りのキーで読み書きし、書き込み後に全体を再検証する。

| セクション | 主なキー（既定値） |
|---|---|
| `general` | `timezone`（`Asia/Tokyo`）、`autostart`（true）、`update_check`（未設定=確認しない） |
| `collect` | `idle_threshold_sec`（120）、`merge_gap_sec`（30）、`poll_interval_sec`（3）、`terminal_exes`、`agent_host_exes`、`communication_exes`、`meeting_exes`、`exclude`、`drop_excluded`（false）、`record_self`（false）、`interrupt_min_sec`（180）、`rebuild_window_min`（180）、`transient_exes`（`explorer.exe` など）、`transient_max_sec`（60） |
| `review` | `min_sec`（60: 仕分け一覧で既定表示する最小の長さ。未満は既定で隠し、`--all` / 画面のトグルで表示する。件数表示も同じ基準で、短い件数は別に返す）、`auto_break_locked_min`（10）、`trust_doing`（false）、`rule_suggest_min_count`（3）、`rule_suggest_snooze_days`（14）、`snap_sec`（120: 補正の範囲選択の吸着幅）、`correct_include_confirmed` / `correct_include_meeting` / `correct_include_idle`（false: 補正の初期チェック） |
| `agent` | `enabled`（`claude-code`, `codex`）、`prompt_excerpt_chars`（200）、`watch_max_min`（10）、`bind_lookback_sec`（5）、`bind_unlisted`（true） |
| `push` | `enabled`（true）、`daily_limit`（6）、`min_interval_min`（60）、`day_start_after`（08:00）、`day_end_at`（18:30）、`weekly.weekday`/`weekly.at`（fri / 17:00）、`long_focus_min`（90）、`switch_spike.window_min`/`count`（15 / 20）、`late_work_after`（20:00）、`idle_return_min`（15）、`disabled_triggers` |
| `activity` | `labels`（開発、レビュー、調査、設計、コミュニケーション、会議、事務、学習） |
| `judge` | `enabled`（false）、`default`、`auto_confidence_threshold`（0.8）、`backends.<名前>`（`type = "openai-compatible"`: `base_url`, `model`, `api_key_env`, `timeout_sec`／`type = "external-command"`: `command`, `timeout_sec`, `batch`） |
| `ai` | `command`（`["claude", "-p"]`）、`timeout_sec`（180）、`templates_dir` |
| `calendar` | `fetch_interval_min`（15）、`past_days`（7）、`future_days`（14）、`sources`（`name`, `type`, `url`） |
| `task` | `single_doing`（true: `task start` で他の doing を todo に戻す） |

設定の変更は、再計算・カレンダー・プッシュ・判定器には次回から反映される。収集器とログ監視の設定の変更には再起動が必要。

## 13. 配布とライセンス

- GitHub Releases に次の 2 つを添付する（`.github/workflows/release.yml`）。コード署名は無い。
  - `hitoiki-<ver>-windows-x64-setup.exe`（推奨。Tauri の NSIS インストーラー。MSI は作らない）
  - `hitoiki-<ver>-windows-x64.zip`（ポータブル版。`hitoiki.exe`、`hitoiki-app.exe`。展開のみで動作する）
- インストーラーの仕様（`crates/hitoiki-app/tauri.conf.json`、`tauri.bundle.conf.json`、`windows/`）:
  - `installMode: currentUser`（管理者権限不要）。製品名（`productName`）は `Hitoiki` で、既定のインストール先 `%LOCALAPPDATA%\Hitoiki` はデータ置き場 `%LOCALAPPDATA%\hitoiki` と同じフォルダになる（意図した構成。アンインストーラーは自分が置いたファイルだけを消し、フォルダは空のときだけ削除する）。実行ファイル名は `mainBinaryName` で `hitoiki-app.exe` に固定する（収集器の自己除外が exe 名で判定するため）。
  - CLI は `externalBin`（サイドカー）で同梱し、インストール先に `hitoiki.exe` として `hitoiki-app.exe` と並べる（CLI の `ui` と daemon が同じフォルダの `hitoiki-app.exe` を探すため）。`externalBin` はビルド時にファイルの存在を検査されるので、bundle 専用の `tauri.bundle.conf.json` に分けて `tauri build --config` で合成する。
  - NSIS フック（`windows/hooks.nsh`）が PowerShell（`windows/hitoiki-path.ps1`）を呼び、インストール先をユーザー PATH（HKCU）へ重複なしで追加し、アンインストールで除去する。`REG_EXPAND_SZ` を保つためレジストリを展開なしで読み書きし、変更後に `WM_SETTINGCHANGE` を通知する。自動起動の Run エントリ（値名 `Hitoiki`、`lib.rs` で固定）もアンインストール時（更新を除く）に削除する。
  - アンインストールでユーザーデータ（`%APPDATA%\hitoiki`、`%LOCALAPPDATA%\hitoiki`）は消さない。標準の「アプリデータを削除」は `dev.hitoiki.app` フォルダのみが対象。
- ライセンスは MIT OR Apache-2.0 のデュアル。依存クレートのライセンスは `cargo deny check`（`deny.toml`、CI でも実行）で検査し、permissive 以外を拒否する。
- 自動更新は無い。ネットワークを使うのは `hitoiki version --check` の実行時と、`general.update_check = true` のときの 1 日 1 回の最新版確認のみ。
- テストは `cargo test --workspace`。時刻は `Clock` トレイト経由で取得し、パイプライン・プッシュ判定は時刻を引数で受けるので決定的にテストできる。

## 14. 未検証事項

- **トーストのボタン押下と入力欄**: 未署名・非パッケージの Tauri アプリで、アクション付きトーストの押下と入力欄の値を受け取れることは、Windows のバージョン、集中モード、通知設定によって十分には確認できていない。ボタンが効かない場合の逃げ道として、本体クリックで画面を開く、ポップアップ窓、設定画面の「テスト通知を出す」を用意している。
- **Codex のログ形式**: 実ログで確認できていない。公開されている形式に基づくパーサであり、バージョン差分で変わる可能性がある。`codex exec` の判別も同様。
- **Claude Code のログ形式**: 手元の実ファイルで確認した形式に基づくが、将来のバージョンで変わりうる。
- **ロック/スリープの実機記録**: `WTSRegisterSessionNotification` と `WM_POWERBROADCAST` による `locked` / `sleep` の記録は、`Recorder` の単体テストでは検証しているが、実機での長期の挙動（復帰直後のイベント順など）は確認していない。
- **Windows Terminal の複数タブ**: ウインドウタイトルでタブを区別できるかはターミナルの設定に依存する。区別できない場合、同じ `(hwnd, タイトル)` のセッションの帰属はベストエフォート（4.2）。
- **ブラウザ拡張とネイティブホスト**: 拡張自体は本リポジトリに無い。未署名の exe からのホスト登録が社内のブラウザポリシーで許可されるかは未確認。`context_hint` は現状、帰属判定で使っていない。
- **OpenAI 互換判定器の接続先**: 構造化出力（`json_schema`）の対応状況はサーバ依存。非対応なら `json_object` に切り替えるが、実サーバでの網羅的な確認はしていない。
- **Outlook の ICS 公開**: 社内で ICS の公開が許可されているかは利用者の環境による（不可なら別のアダプタが必要）。
