# 自己管理ツール 設計書（実装インプット用）

名称: **Hitoiki（一息）**（決定）。CLIコマンド名・クレート名・パス名は `hitoiki` を使う。

由来: 「一息つきませんか」と寄り添って声をかける存在。プッシュによる寄り添いを名前で表し、記録の側面は機能名（タイムライン画面の呼称「あしあと」等）で表す。

---

## 0. この文書の位置づけ

- 実装担当（コーディングエージェント）へのインプット。決定事項・未確定事項・実装時に検証が必要な事項を区別して記載する。
- 「決定」と書かれた項目は変更しない。「推奨」は理由があれば変更してよい。「要検証」は実装前または実装中に実際の挙動を確認すること。

---

## 1. 目的

書籍「エンジニアのための自己管理入門」の考え方を日常業務に組み込むための個人用ツール。以下4領域を1つのタイムライン上で統合し、記録・可視化・内省を支援する。

1. タスク・タイムマネジメント
2. モチベーションマネジメント
3. アンガー・ストレスマネジメント
4. スキルマネジメント

特に、コーディングエージェントとの対話時間のような「名前の無い工数」を自動で捕捉し、タスクに紐付けて可視化する。

### 1.1 設計原則（決定）

- **P1 存在を意識させない**: 情報は可能な限り自動収集する。手入力はプッシュ通知への1アクション回答を基本とする。
- **P2 判断はユーザーに委ねる**: 自動判定で確信が持てないものは捨てずに「要確認」として溜め、振り返り時にユーザーが確定する。
- **P3 何にどれだけ時間を使ったかの可視化**: 「無駄の検出」ではなく時間の使い方全体の可視化が目的。良し悪しの評価をツールが押し付けない。
- **P4 ローカル完結**: データは完全にローカル保存。同期・テレメトリなし。
- **P5 CLIファースト**: すべての機能はCLIから操作可能。GUIはCLI/コアと同じロジックの上に乗る。エージェントから扱えることを重視。
- **P6 拡張点を残す**: OS、エージェント、カレンダー、判定器はアダプタ/プラグインで差し替え可能にする。

### 1.2 スコープ外（決定）

- エージェント自体の稼働時間・並列実行の管理（ユーザーが消費した時間のみ対象）
- チーム向け機能、他者への共有・報告
- データの同期、クラウド保存
- 高機能なタスク管理（カンバン、依存関係、サブタスク等）。チームのタスクはGitHub/GitLab Issue、Jiraで管理されている前提
- 予実管理の専用機能（実績は自動算出、見積もりは任意の1フィールドのみ）
- キー入力内容の記録（絶対に行わない）

---

## 2. 前提環境（決定）

| 項目 | 内容 |
|---|---|
| 対象OS | 初期はWindowsのみ。OS依存部分はトレイト/モジュールで分離し、macOS/Linuxを追加可能にする |
| 利用環境 | 会社支給PCでの日常利用を含む。EDR/ウイルス対策ソフトに検知されにくい実装を選ぶ |
| 対応エージェント | Claude Code、Codex CLI。追加可能なアダプタ構造にする |
| カレンダー | Outlook（社内）、Googleカレンダー（自宅）の両方 |
| LLM | 社内ではLiteLLMプロキシ（AIGW）経由のOpenAI互換APIのみ利用可能。接続先は設定で切り替え |
| 配布 | GitHub Releasesからのバイナリダウンロード。コード署名なし |
| 利用者 | 個人。社内での配布・利用も想定（各自がローカルで使う） |

---

## 3. 全体アーキテクチャ

### 3.1 技術選定（推奨）

- 言語: **Rust**（理由: Tauriとの親和性、`windows` クレートでWin32 APIを直接扱える、単一バイナリ配布が容易）
- デスクトップ常駐: **Tauri v2**（タスクトレイ常駐）
- DB: **SQLite**（WALモード）。`rusqlite` または `sqlx`
- UI: TypeScript + Vite（フレームワークは任意）。**Tauri WebView内でのみ表示する。ブラウザ向けの配信・ローカルHTTPサーバは持たない**（決定）
- プロセス間通信: Windowsの名前付きパイプ（CLI→常駐アプリ）、Tauriのコマンド/イベント（UI↔コア）
- 設定: TOML

### 3.2 構成

```
crates/
  hitoiki-core        ドメインモデル、ストレージ、集計、帰属判定パイプライン、判定器IF、プッシュ判定ロジック
  hitoiki-collect     収集器のトレイト定義
  hitoiki-collect-win Windows実装（前面ウインドウ、無操作、ロック/スリープ）
  hitoiki-agents      エージェントアダプタ（claude-code, codex）
  hitoiki-calendar    カレンダーアダプタ（ics。将来 graph, google）
  hitoiki-judge       判定器（rule, openai-compatible, external-command）
  hitoiki-ipc         CLI↔常駐アプリの通知（名前付きパイプ。OS抽象化して将来Unixドメインソケット）
  hitoiki-cli         CLIバイナリ `hitoiki`（ネイティブメッセージングホストも兼ねる、6.4）
  hitoiki-app         Tauriアプリ（トレイ常駐。収集・プッシュ・UIを内包）
ui/               フロントエンドソース（Tauri WebView用）
```

### 3.3 プロセスモデル

- **常駐アプリ（hitoiki-app）**: 収集器、エージェントログ監視、カレンダー取得、帰属判定の定期実行、プッシュ通知、UIを担当。OSログイン時に自動起動（設定で切替）。
- **ヘッドレスモード**: `hitoiki daemon` で同等の常駐処理（UIとトースト以外）をTauriなしで起動可能にする（デバッグ・UI不要な環境用）。ヘッドレス時の操作はすべてCLIで行える。
- **CLI（hitoiki）**: DBを直接読み書きする。常駐アプリが起動していなくても動作すること。常駐アプリへの即時反映が必要な操作（タスク開始など）は名前付きパイプで通知を試み、失敗しても処理自体は成功させる（常駐側は次回の定期処理でDBから拾う）。
- 同時書き込みはSQLite WAL + busy_timeoutで扱う。収集データの書き込みは常駐側のみ。
- **UI↔コア**: Tauriのコマンド（呼び出し）とイベント（区間更新・プッシュ・状態変化の通知）で行う。ネットワークポートは一切開かない（決定）。
- **名前付きパイプ**: `\\.\pipe\hitoiki-<ユーザーSID>`。作成者ユーザーのみアクセス可能なセキュリティ記述子を付ける。メッセージは改行区切りJSON。
- フロントエンドからコアの呼び出しは薄いクライアント層（`ui/src/api/`）に集約し、将来トランスポートを変えても画面側を変更せずに済むようにする（推奨）。

### 3.4 ファイル配置（Windows）

| 用途 | パス |
|---|---|
| 設定 | `%APPDATA%\hitoiki\config.toml` |
| DB | `%LOCALAPPDATA%\hitoiki\hitoiki.db` |
| ログ | `%LOCALAPPDATA%\hitoiki\logs\` |
| 判定器・プラグイン | `%APPDATA%\hitoiki\plugins\` |
| 名前付きパイプ | `\\.\pipe\hitoiki-<ユーザーSID>` |

---

## 4. データの層構造

データは3層で扱う。**生データは改変しない。派生データは生データ＋ユーザー確定情報から再計算可能にする。**

1. **生データ層（raw）**: 収集器が書く事実。前面ウインドウ区間、無操作区間、エージェントイベント、カレンダー予定。
2. **派生層（derived）**: 帰属判定パイプラインが生成するアクティビティ区間（何をしていたか、どのタスクか）。再計算可能。
3. **ユーザー層（user）**: タスク、OKR、スキル、チェックイン、記録、振り返り、ユーザーによる確定ラベル・ルール。

ユーザーが派生区間のラベルを修正した場合、その修正は「上書き（override）」としてユーザー層に保存し、再計算時にも優先適用する。修正を一般化できる場合（例: このタイトルパターンは常にタスクX）はルール化を提案する。

---

## 5. データモデル

SQLiteスキーマの論理定義。時刻はすべてUTCのUnixミリ秒（INTEGER）で保存し、表示時にローカルタイムゾーンへ変換する。IDはULID（TEXT）を推奨。

### 5.1 生データ層

```
window_span
  id, start_at, end_at,
  hwnd TEXT,            -- OS依存のウインドウ識別子（文字列化）
  pid INTEGER, exe_path TEXT, exe_name TEXT,
  title TEXT,           -- 全文保存（除外ルール該当時は保存しない、6.5参照）
  excluded INTEGER      -- 除外ルールでマスクされたか

idle_span
  id, start_at, end_at,
  reason TEXT           -- 'no_input' | 'locked' | 'sleep'

agent_event
  id, at,
  agent TEXT            -- 'claude-code' | 'codex' | ...
  session_id TEXT,
  kind TEXT             -- 'session_start' | 'prompt_submit' | 'response_complete' | 'session_end'
  cwd TEXT, git_branch TEXT NULL,
  prompt_excerpt TEXT NULL,  -- プロンプト先頭N文字（設定で長さ変更・無効化可）
  source_file TEXT, source_offset INTEGER  -- 重複取り込み防止

agent_session_window          -- セッションとウインドウの対応（7.3参照）
  session_id, agent, hwnd, title_at_bind, bound_at

context_hint                  -- 補助コレクタからの詳細コンテキスト（6.4）
  id, at, source TEXT ('browser'|'vscode'|...), hwnd TEXT NULL,
  kind TEXT ('url'|'workspace'|'file'|...), value TEXT

calendar_event
  id, source TEXT, external_id TEXT,
  start_at, end_at, all_day INTEGER,
  title TEXT, location TEXT NULL, is_busy INTEGER,
  fetched_at
```

### 5.2 派生層

```
activity_segment
  id, start_at, end_at,
  kind TEXT
    -- 'work'         通常の作業
    -- 'agent_dialog' エージェント対話（応答確認＋プロンプト作成）
    -- 'agent_watch'  プロンプト送信直後の見守り
    -- 'meeting'      会議
    -- 'idle'         無操作（未解決）
    -- 'break'        休憩（ユーザー確定または予定から）
    -- 'interrupt'    割り込み対応
  task_id NULL,
  activity_label TEXT NULL     -- 開発/レビュー/調査/コミュニケーション/... （設定で定義）
  confidence REAL NULL,
  decided_by TEXT              -- 'rule' | 'judge:<name>' | 'calendar' | 'user'
  status TEXT                  -- 'auto' | 'needs_review' | 'confirmed'
  window_span_ids TEXT         -- 根拠となる生データ（JSON配列）
  agent_session_id NULL
```

### 5.3 ユーザー層

```
task
  id, title TEXT, body_md TEXT,
  status TEXT        -- 'todo' | 'doing' | 'done' | 'dropped'
  kind TEXT          -- 'normal' | 'interrupt'
  urgent INTEGER, important INTEGER   -- アイゼンハワー（NULL許容=未設定）
  estimate_min INTEGER NULL           -- 見積もり（任意）
  planned_start_at NULL, planned_end_at NULL  -- 任意
  key_result_id NULL,
  external_url TEXT NULL              -- Issue/Jira等へのリンク（参照のみ。同期しない）
  created_at, updated_at, done_at NULL

task_match_rule          -- 行動ログ→タスクの帰属ルール
  id, task_id, field TEXT ('exe'|'title'|'cwd'|'repo'|'url'|'git_branch'),
  pattern TEXT, pattern_type TEXT ('glob'|'regex'|'contains'),
  priority INTEGER, enabled INTEGER

activity_rule            -- 行動ログ→活動ラベルの汎用ルール（タスク非依存）
  id, field, pattern, pattern_type, activity_label, priority, enabled

segment_override
  id, start_at, end_at, kind NULL, task_id NULL, activity_label NULL, created_at

time_block               -- タイムブロッキング（計画）
  id, start_at, end_at, task_id NULL, label TEXT NULL, focus INTEGER

objective
  id, title, description_md, period_start, period_end, status
key_result
  id, objective_id, title, target TEXT NULL, current TEXT NULL, status

skill
  id, name, parent_id NULL, description, preset INTEGER
task_skill
  task_id, skill_id
activity_skill_rule      -- 任意: 活動ラベルやリポジトリからスキルを自動付与
  id, field, pattern, pattern_type, skill_id

checkin
  id, at,
  energy INTEGER NULL (1-5), mood INTEGER NULL (1-5), stress INTEGER NULL (1-5),
  erg_tag TEXT NULL ('existence'|'relatedness'|'growth')
  note TEXT NULL,
  push_id NULL

hassle                   -- デイリーハッスル
  id, at, text, intensity INTEGER (1-5), segment_id NULL, push_id NULL

abc_record               -- ABC理論
  id, at,
  activating_event TEXT,       -- A: 出来事
  belief TEXT NULL,            -- B: 受け止め方
  consequence_emotion TEXT NULL, consequence_intensity INTEGER NULL,  -- C
  dispute TEXT NULL,           -- 反論・別の捉え方
  ai_dialog_md TEXT NULL,
  hassle_id NULL

reflection
  id, at, kind TEXT ('day_start'|'day_end'|'weekly'),
  context_json TEXT,           -- AIに渡した集計
  questions_md TEXT,           -- AIが生成した問い
  answers_md TEXT NULL,        -- ユーザーの回答
  summary_md TEXT NULL

push_log
  id, at, trigger TEXT, payload_json, delivered INTEGER,
  responded_at NULL, response_json NULL, snoozed_until NULL, suppressed_reason NULL

setting (key TEXT PRIMARY KEY, value_json)
schema_migration (version)
```

---

## 6. 収集器（Windows実装）

### 6.1 前面ウインドウ

- `SetWinEventHook(EVENT_SYSTEM_FOREGROUND, ..., WINEVENT_OUTOFCONTEXT)` で前面ウインドウの切り替えを検知。
- 前面ウインドウのタイトル変更（ブラウザのタブ切り替え、ターミナルのタブ切り替え等）を拾うため、`EVENT_OBJECT_NAMECHANGE` もフックし、前面ウインドウのhwndに一致するもののみ処理。
- プロセス情報は `GetWindowThreadProcessId` → `OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION)` → `QueryFullProcessImageNameW`。
- 区間化: 前面hwndまたはタイトルが変化したら現区間を閉じて新区間を開く。同一内容の短い往復は統合しない（生データは忠実に）。
- フックはメッセージループを持つ専用スレッドで動かす。
- フォールバックとして数秒間隔のポーリング（`GetForegroundWindow`）でも整合を取る（フック取りこぼし対策）。

### 6.2 無操作

- `GetLastInputInfo` を1秒間隔程度でポーリング。最終入力からの経過が閾値（既定120秒、設定可）を超えたら無操作区間開始とし、開始時刻は「最終入力時刻」に遡る。
- 画面ロック/アンロック: `WTSRegisterSessionNotification`（`WTS_SESSION_LOCK/UNLOCK`）。
- スリープ/復帰: 電源ブロードキャスト（`WM_POWERBROADCAST`）。
- 動画視聴・会議視聴など入力のない作業は無操作として記録される。帰属判定でカレンダーやアプリ種別により補正する（7.2）。

### 6.3 EDR対策の実装制約（決定）

- **低レベルキーボード/マウスフック（`WH_KEYBOARD_LL` 等）は使わない。** キー入力の内容も回数も取らない。
- DLLインジェクション、他プロセスのメモリ読み取りは行わない。
- 使用するAPIは上記の公開・低権限APIに限定する。管理者権限を要求しない。

### 6.4 タブ・ペイン単位の識別（ベストエフォート）

- 初期はウインドウタイトルで区別する（ブラウザもWindows Terminalもアクティブタブ名がタイトルに反映される）。
- 拡張点として「補助コレクタ」を定義し、後からブラウザ拡張・VS Code拡張が詳細コンテキスト（URL、ワークスペース、ファイル等）を送れるようにする。受け口はHTTPではなく以下とする:
  - 共通: `hitoiki ingest context`（標準入力でJSONを受け取り、DBの `context_hint` に保存）
  - ブラウザ拡張: ネイティブメッセージング。`hitoiki native-host` をホストとして動かし、受け取ったメッセージを `ingest context` と同じ処理に渡す。ホストの登録は `hitoiki native-host install` でHKCU配下のレジストリとマニフェストJSONを書く（管理者権限不要）
  - VS Code拡張等: CLIを直接呼ぶ
- 初期実装では `ingest context` コマンドと `context_hint` テーブルのみ用意し、帰属判定での利用は後のフェーズとする。

### 6.5 除外ルール

- 設定で除外対象を定義（exe名、タイトルパターン）。既定でパスワードマネージャ類、ブラウザのプライベートウインドウ（タイトル末尾パターン）を含める。
- 該当区間は `title` を保存せず `excluded=1`、`exe_name` のみ保持（設定で区間ごと破棄も可）。

### 6.6 OS抽象化

`hitoiki-collect` に以下のトレイトを定義し、Windows実装を `hitoiki-collect-win` に置く。

```rust
trait ForegroundCollector { fn run(&self, tx: Sender<ForegroundChange>) -> Result<()>; }
trait IdleCollector       { fn run(&self, tx: Sender<IdleChange>) -> Result<()>; }
trait SessionCollector    { fn run(&self, tx: Sender<SessionChange>) -> Result<()>; } // lock/sleep
```

---

## 7. 時間の帰属判定パイプライン

生データから `activity_segment` を生成する。常駐側で定期実行（既定1分ごとに直近区間を再計算）し、`hitoiki rebuild --from --to` で任意期間を再計算可能にする。

### 7.1 処理順

1. タイムラインを `window_span` と `idle_span` から構築（無操作区間はウインドウ区間より優先）。
2. **エージェント対話の抽出**（7.3）
3. **会議・予定の適用**（7.2）
4. **override の適用**（ユーザー確定は最優先・確定状態）
5. **ルール判定**: `task_match_rule` → `activity_rule` を priority 順に適用。
6. **判定器**（設定で有効な場合）: ルールで決まらなかった区間、または確信度の低い区間について判定器に問い合わせる（8章）。
7. **確信度による状態付け**: 確信度 ≥ 閾値 → `auto`、未満または未判定 → `needs_review`。
8. 隣接する同一内容の区間を統合（短い中断の吸収は設定値、既定30秒）。
9. 現在 `doing` のタスクがあり、区間がどのルールにも該当しない場合は、その `doing` タスクを低確信度の候補として付与（`needs_review`）。

### 7.2 無操作と予定の突き合わせ

- 無操作区間がカレンダーの予定（busy）と重なる → `meeting`（`decided_by='calendar'`, `auto`）。
- 会議アプリ（Teams/Zoom等、設定で定義）が前面で予定と重なる → `meeting`。
- 予定外の無操作 → `idle`（`needs_review`）。振り返り時にユーザーが「休憩/会議/思考/その他」等を付与。
- 予定のタイトルにタスクのマッチルールが当たれば `task_id` も付与。

### 7.3 エージェント対話時間の算出（重要）

**前提**: ユーザーはエージェントに作業させている間、別の作業をしていることが多い。応答完了から次のプロンプト送信までには大きなラグがあり得る。その区間を丸ごと計上してはならない。**計上するのはユーザーが実際にそのエージェントのウインドウにいた時間のみ。**

#### セッションとウインドウの対応付け

- `prompt_submit` の時刻は「ユーザーがエンターを押した瞬間」であり、その時点の前面ウインドウがそのセッションのウインドウである。
- `prompt_submit` 発生時刻の前面 `window_span` の hwnd とタイトルを `agent_session_window` に記録する。
- 同一hwndに複数セッションがある場合（Windows Terminalのタブ等）は、submit時点のタイトルで区別する（ベストエフォート）。
- `prompt_submit` 時点でターミナル/IDE以外が前面だった場合（時刻ずれ等）は、直前N秒（既定5秒）以内にあったターミナル系exeの区間を採用。見つからなければ対応付け不可として扱う。

#### 計上ルール

セッション内の各 `prompt_submit` について:

- 対象期間 = (直前の `response_complete`、または直前の `prompt_submit`、またはセッション開始) 〜 `prompt_submit`
- **agent_dialog** = 対象期間のうち「そのセッションのウインドウ（hwnd＋タイトル一致）が前面」かつ「無操作でない」時間の合計。期間内で複数回戻ってきた場合もすべて合算（複数区間として記録）。
- **agent_watch** = `prompt_submit` 後、そのウインドウが前面のまま続いた時間。無操作閾値に達するか、ウインドウが切り替わった時点で打ち切る。さらに上限（既定10分）を設ける。
- 対象期間中に他のウインドウにいた時間は、そのウインドウの作業として通常の帰属判定に回す（＝ラグは自然に除外される）。
- 応答確認とプロンプト作成はキー入力を見ずに区別できないため、`agent_dialog` として1種類で扱う。

#### タスクへの帰属

- セッションの `cwd` / `git_branch` に `task_match_rule` を適用。
- 該当しなければ、セッション開始時点の `doing` タスク → それもなければ `needs_review`。
- `prompt_excerpt` は振り返り時の判断材料として表示する。

### 7.4 割り込みの検出

- 候補条件（設定で調整可）: 
  - `doing` タスクがある、またはフォーカス用 `time_block` の最中に、
  - コミュニケーション系アプリ（Teams/Slack/Outlook等、設定で定義）が前面になり、
  - その後、元のタスクに該当しない作業が一定時間（既定3分）以上続いた。
- 候補区間は `kind='interrupt'`、`needs_review`。
- 内容の自動解決: 割り込み後の作業区間のタイトル・cwdから既存タスクに帰属できればそれを付与。できなければ、振り返り時に「割り込みタスク」として新規タスク化（`kind='interrupt'`）を提案する。タイトル候補はウインドウタイトルから生成。
- 判定器が有効なら `is_interrupt`（真偽）の問いで補強。

---

## 8. 判定器（Judge）

### 8.1 方針

- 標準はルールベース（7章）のみで動作する。判定器は任意の追加。
- 判定器はすべて同じ「型付き質問セット → 型付き回答＋確信度」のインターフェースで扱い、差し替え可能にする。
- 問いの定義は判定器から独立させる（同じ問いをルール/LLM/Jev等で比較できるように）。
- 想定される実装: 
  - `openai-compatible`: OpenAI互換のChat Completions APIに構造化出力で問い合わせる。社内ではLiteLLMプロキシ（AIGW）を指す。
  - `external-command`: 任意の外部コマンド（標準入出力でJSON）。
  - `jev`（将来）: TypeSafe AIのJev（判断特化モデル。Choice/Score/Noulの3種の型付き問いに確率付きで回答）。現時点では社内で利用できないため実装しないが、インターフェースはJevの形に合わせておき、後からアダプタを追加できるようにする。

### 8.2 質問セット定義

`%APPDATA%\hitoiki\questions\*.toml`

```toml
[set]
name = "segment_classification"
applies_to = "activity_segment"

[[question]]
name = "task"
type = "choice"            # choice | score | bool
description = "この作業区間はどのタスクに該当するか"
options_from = "open_tasks" # 動的選択肢（open_tasks | activity_labels | skills）
allow_none = true

[[question]]
name = "activity"
type = "choice"
options_from = "activity_labels"

[[question]]
name = "is_interrupt"
type = "bool"
description = "計画外の割り込み対応か"

[[question]]
name = "urgency"
type = "score"
scale = [1, 5]
```

### 8.3 外部コマンドのプロトコル

標準入力:

```json
{
  "version": 1,
  "state": {
    "segment": {"start": "...", "end": "...", "exe": "...", "title": "...", "cwd": "..."},
    "context": {"doing_task": {...}, "recent_segments": [...], "calendar": [...]}
  },
  "questions": [
    {"name": "task", "type": "choice", "description": "...", "options": [{"id": "01J...", "label": "..."}], "allow_none": true},
    {"name": "is_interrupt", "type": "bool", "description": "..."},
    {"name": "urgency", "type": "score", "scale": [1, 5]}
  ]
}
```

標準出力:

```json
{
  "version": 1,
  "answers": {
    "task": {"value": "01J...", "confidence": 0.82, "probs": {"01J...": 0.82, "none": 0.1}},
    "is_interrupt": {"value": false, "confidence": 0.9},
    "urgency": {"value": 2, "confidence": 0.6}
  }
}
```

- タイムアウト（既定10秒）、非0終了、JSON不正はすべて「未判定」として扱い、ルール判定結果を維持する。
- バッチ処理: 複数区間をまとめて送れる `batch` モード（`states` 配列）を任意対応にする。

### 8.4 設定

```toml
[judge]
enabled = false
default = "aigw"
auto_confidence_threshold = 0.8   # これ以上は auto、未満は needs_review

[judge.backends.aigw]
type = "openai-compatible"
base_url = "https://aigw.example.internal/v1"
model = "..."
api_key_env = "HITOIKI_AIGW_API_KEY"

[judge.backends.custom]
type = "external-command"
command = ["python", "C:/tools/myjudge.py"]
timeout_sec = 10
```

---

## 9. プッシュ通知（ツールの中核）

手入力の唯一の主要経路。**「1アクションで答えられる」「邪魔をしない」の両立**が要件。

### 9.1 トリガー

| ID | 条件 | 内容 |
|---|---|---|
| `day_start` | その日最初の操作を検知（ロック解除・無操作明け。設定時刻以降） | チェックイン（エネルギー・気分）、始業の振り返りを開く導線 |
| `day_end` | 設定時刻以降、一定時間無操作 or 設定時刻ちょうど | 終業の振り返りへの導線（要確認区間の件数を表示） |
| `weekly` | 設定曜日・時刻 | 週次振り返りへの導線 |
| `long_focus` | 同一タスク/活動が連続N分（既定90分） | 休憩の提案、エネルギーのチェックイン |
| `switch_spike` | 直近M分のコンテキストスイッチ回数が閾値超え | ストレスのチェックイン、デイリーハッスルの記録 |
| `after_interrupt` | 割り込み候補の終了後 | 割り込みの一言記録（ハッスル）、ストレス |
| `meeting_overrun` | 予定終了時刻を超えて会議アプリが前面 | ハッスルの記録 |
| `late_work` | 設定時刻以降の作業継続 | ストレス・エネルギーのチェックイン |

各トリガーは設定で有効/無効・閾値を変更可能。

### 9.2 抑制ルール

- 会議中（`meeting` 判定中）・フォーカス用 `time_block` 中は保留し、終了後に（まだ有効なら）出す。
- 1日あたりの上限（既定6回）、同一トリガーの最小間隔（既定60分）。
- 無応答が続く場合は自動で頻度を下げる（直近の応答率に応じて間隔を伸ばす）。
- スヌーズ（15分/1時間/今日はもういい）。
- 抑制・スキップした場合も `push_log` に理由を残す（振り返りで通知設定を調整する材料）。

### 9.3 通知UI

- Windowsのトースト通知にアクションボタンを付ける（例: エネルギー `1`〜`5`）。テキスト入力欄（一言メモ）も任意で付ける。
- 通知クリックでアプリの該当画面（チェックイン詳細、振り返り）を開く。
- **要検証**: Tauri公式の通知プラグインがアクションボタン・入力欄・アクティベーションのコールバックに対応しているか。対応していない場合は `windows` クレートのWinRTトーストAPI（`ToastNotification`、AUMID登録）を直接使う。未署名・非パッケージアプリでのAUMID登録とアクティベーション受信方法を確認すること。
- フォールバック: トレイアイコンのバッジ＋小さなポップアップウインドウ。

---

## 10. 書籍の手法の組み込み

| 手法 | データ | 入力 | 可視化・活用 |
|---|---|---|---|
| タイムブロッキング | `time_block` | UI/CLIで計画。カレンダー予定も表示 | 計画ブロックと実タイムラインの重ね合わせ＝予実。フォーカスブロック中は通知抑制 |
| アイゼンハワーマトリクス | `task.urgent/important` | タスク作成時（任意）。割り込みは自動で urgent=1 | 4象限ごとの時間配分（日/週） |
| OKR | `objective`, `key_result`, `task.key_result_id` | UI/CLI | OKRごとの週次投下時間。どのKRにも紐付かない時間の割合 |
| デイリーハッスル | `hassle` | プッシュ（9.1の摩擦系トリガー）、CLI | 頻度・強度の推移、活動/時間帯との相関 |
| ABC理論 | `abc_record` | ハッスルから昇格、またはCLI/UI | AIがB（受け止め方）を掘り下げる問いを生成（11章）。ユーザーが別の捉え方を記入 |
| ERG理論 | `checkin.erg_tag`、記録全般 | 任意タグ | 週次振り返りでAIが記録から満たされていない欲求の仮説を立て、問いとして提示（断定しない） |

予実について（決定）: 実績入力フィールドは作らない。実績は常に `activity_segment` から算出。見積もりは `task.estimate_min`（任意）のみ。計画は `time_block` で表現する。

スキル: ユーザーが定義（プリセットを同梱し、取り込みは任意）。`task_skill` と `activity_skill_rule` を通じて区間にスキルを帰属させ、スキルごとの累積時間を集計する。

---

## 11. AI連携（振り返りと内省）

### 11.1 方針

- ツール本体にLLMの推論を埋め込まない。振り返りの問いや要約は、設定されたコマンド（`claude -p`、`codex exec` 等）を呼び出して生成する。
- ツールの役割は「集計してコンテキストを作る」「問いを受け取って保存・表示する」「回答を保存する」。
- AIの出力は**助言ではなく内省を促す問い**を主とする（プロンプトテンプレートで指示する）。

### 11.2 設定

```toml
[ai]
command = ["claude", "-p"]     # 標準入力でプロンプトを渡し、標準出力を受け取る
# command = ["codex", "exec", "-"]
timeout_sec = 180
templates_dir = "%APPDATA%/hitoiki/templates"
```

- **要検証**: `claude -p` / `codex exec` に標準入力でプロンプトを渡す正確な方法と、非対話実行時の出力形式。

### 11.3 振り返りの種類

| 種類 | タイミング | コンテキスト | 出力 |
|---|---|---|---|
| `day_start` | 始業 | 前日の要約、今日の予定・time_block、doingタスク、直近のチェックイン | 今日の焦点を考える問い（2〜3個） |
| `day_end` | 終業 | 当日の時間配分（タスク/活動/象限/OKR/スキル）、エージェント対話時間、割り込み、ハッスル、チェックイン推移、needs_review件数 | 要約＋問い。needs_review の仕分けUIへの導線 |
| `weekly` | 週次 | 週の集計、OKR進捗、ハッスル・ABC記録、チェックイン推移、前週との差分 | 要約、ERGの観点からの仮説を問いの形で、来週のtime_block検討の問い |

- テンプレートはファイルで差し替え可能（`day_start.md`, `day_end.md`, `weekly.md`, `abc_dispute.md`）。
- 使用したコンテキスト（JSON）、問い、回答を `reflection` に保存。

### 11.4 エージェントからの利用

エージェントが自分で集計・要約できるよう、すべての集計をCLIで `--json` 出力可能にする（12章）。`hitoiki context --kind day_end --date ...` で振り返り用コンテキストをそのまま出力できるようにする。

---

## 12. CLI

### 12.1 共通仕様

- すべての参照系コマンドは `--json` で機械可読出力（安定したスキーマ。`"version"` フィールドを含める）。
- 日時指定: `--date 2026-10-10`, `--from/--to`（ISO 8601またはローカル日付）、`today`/`yesterday`/`this-week`/`last-week` のエイリアス。
- IDは前方一致での短縮指定を許可（一意な場合）。
- 終了コード: 0 成功、1 一般エラー、2 引数エラー、3 対象なし、4 曖昧（候補複数）。
- 対話的プロンプトは出さない（エージェントから使うため）。不足情報はエラーにする。

### 12.2 コマンド一覧

```
hitoiki status                        現在のdoingタスク、直近の区間、今日の集計の要約、未応答のプッシュ

# タスク
hitoiki task add <title> [--body-file F|--body -] [--urgent] [--important] [--estimate 90m]
                     [--kr ID] [--skill NAME...] [--url URL] [--plan-start T --plan-end T] [--interrupt]
hitoiki task list [--status todo,doing] [--kind interrupt]
hitoiki task show <id>
hitoiki task edit <id> [...addと同じオプション]
hitoiki task start <id>               doingにする（他のdoingは todo に戻すか設定に従う）
hitoiki task done <id>
hitoiki task drop <id>
hitoiki task rule add <id> --field cwd --pattern "C:/src/foo/**" [--type glob]
hitoiki task rule list [<id>]

# タイムライン・帰属
hitoiki log [--date D] [--raw]        区間一覧（--rawで生データ）
hitoiki review list [--date D]        needs_review の区間
hitoiki review set <segment-id> [--task ID] [--kind K] [--activity L] [--rule]   確定（--rule でルール化）
hitoiki rebuild --from T --to T       派生層の再計算

# 計画
hitoiki block add <start> <end> [--task ID] [--label L] [--focus]
hitoiki block list [--date D]

# 記録（手入力）
hitoiki check [-e 1-5] [-m 1-5] [-s 1-5] [--erg existence|relatedness|growth] [--note TEXT]
hitoiki hassle <text> [-i 1-5]
hitoiki abc add --a TEXT [--b TEXT] [--c-emotion TEXT] [--c-intensity N] [--from-hassle ID]
hitoiki abc dispute <id>              AIで受け止め方を掘り下げる問いを生成

# OKR・スキル
hitoiki okr add-objective <title> --from D --to D
hitoiki okr add-kr <objective-id> <title> [--target TEXT]
hitoiki okr list
hitoiki skill add <name> [--parent ID]
hitoiki skill import-preset [<name>]
hitoiki skill list

# 集計・振り返り
hitoiki report [--period today|this-week|...] [--by task|activity|quadrant|okr|skill|agent] [--json]
hitoiki context --kind day_start|day_end|weekly [--date D]     AI用コンテキストをJSONで出力
hitoiki reflect day_start|day_end|weekly [--date D]            AIで問いを生成し保存
hitoiki reflect answer <reflection-id> [--file F|-]

# エージェント
hitoiki agent sessions [--date D]
hitoiki agent import [--agent claude-code|codex] [--since T]   ログの手動再取り込み

# 常駐・UI・設定
hitoiki daemon                        ヘッドレス常駐
hitoiki ui [<screen>]                 常駐アプリのウインドウを開く（未起動なら起動）。screen: today|review|tasks|...
hitoiki ingest context                補助コレクタ用（標準入力JSON）
hitoiki native-host [install|uninstall]  ブラウザ拡張のネイティブメッセージングホスト
hitoiki config path|get <key>|set <key> <value>
hitoiki doctor                        収集状態、エージェントログの検出、カレンダー取得、通知、判定器の疎通確認
```

---

## 13. エージェントアダプタ

### 13.1 方針

- **フックの設定を必須にしない**。エージェントが残すセッションログ（JSONL）をファイル監視で取り込む。
- アダプタトレイト:

```rust
trait AgentAdapter {
    fn name(&self) -> &str;
    fn log_roots(&self) -> Vec<PathBuf>;                 // 監視対象ディレクトリ
    fn parse_incremental(&self, file: &Path, from_offset: u64)
        -> Result<(Vec<AgentEvent>, u64)>;               // 追記分のみ解析
}
```

- ファイル監視は `notify` クレート等。取り込み位置（ファイル＋オフセット）を保存し、再起動時も重複しないようにする。

### 13.2 Claude Code（要検証）

- ログの場所: `%USERPROFILE%\.claude\projects\<エンコードされたパス>\<session-id>.jsonl`
- 各行にタイムスタンプ、セッションID、cwd、メッセージ種別が含まれる想定。
- **注意**: ツール実行結果も `user` 種別の行として記録される場合がある。ユーザーが入力したプロンプトとツール結果を区別するロジックが必要。
- `response_complete` はアシスタントの最終応答（ツール呼び出しを含まない応答）の時刻、またはStopフック相当の情報から判定。
- 実装前に実際のファイルを確認し、形式をテストフィクスチャとして保存すること。
- 任意の精度向上策として、Claude Codeのフック（`UserPromptSubmit`, `Stop`, `SessionStart`, `SessionEnd`）から `hitoiki agent hook ...` を呼ぶ方式もサポートしてよい（`hitoiki agent install-hooks` で設定ファイルへの追記を支援）。ログ取り込みと重複した場合はセッションID＋時刻で重複除去。

### 13.3 Codex CLI（要検証）

- ログの場所: `%USERPROFILE%\.codex\sessions\YYYY\MM\DD\rollout-*.jsonl`（想定）
- 同様にユーザー入力・応答完了の時刻、cwdを抽出。
- 形式はバージョンにより変わる可能性があるため、パーサはバージョン差分に寛容にし、解析不能行はスキップしてログに残す。

---

## 14. カレンダーアダプタ

- 初期実装: **ICS URL**（Outlook/Googleカレンダーとも公開URLを発行可能）。複数URLを登録可能。
- 取得間隔（既定15分）、前後の取得範囲（既定 前7日〜後14日）。
- 繰り返し予定の展開（RRULE、EXDATE、タイムゾーン）に対応したライブラリを使う。
- 将来アダプタ: Microsoft Graph（テナントのアプリ登録・同意が必要な場合あり）、Google Calendar API、Outlookローカル操作（従来版Outlookのみ）。
- 公開URLは秘密情報として扱い、設定ファイルに保存（ログに出さない）。

```toml
[[calendar.sources]]
name = "work"
type = "ics"
url = "https://outlook.office365.com/owa/calendar/.../calendar.ics"

[[calendar.sources]]
name = "home"
type = "ics"
url = "https://calendar.google.com/calendar/ical/.../basic.ics"
```

---

## 15. GUI

### 15.1 画面

- **今日**: タイムライン（計画ブロックと実区間の重ね合わせ）、doingタスク、今日の集計、未確認件数
- **仕分け（Review）**: needs_review の区間を一覧し、タスク/種別/活動を割り当て。「この条件を常にこのタスクに」でルール化。割り込みのタスク化
- **タスク**: 一覧・作成・編集（タイトル、Markdown本文、任意フィールド）
- **計画**: タイムブロックの作成（ドラッグ）、カレンダー予定の表示
- **振り返り**: 始業/終業/週次。AIの問いと回答欄、集計
- **レポート**: タスク/活動/象限/OKR/スキル/エージェント対話時間の時間配分、チェックイン・ハッスルの推移
- **記録**: チェックイン、ハッスル、ABCの一覧と入力
- **OKR / スキル**: 管理画面
- **設定**: 収集・除外ルール、通知、判定器、AIコマンド、カレンダー

### 15.2 トレイ

- 現在のdoingタスクと今日の作業時間を表示
- クイック操作: タスク切り替え、チェックイン、ハッスル記録、UIを開く、通知の一時停止

### 15.3 UIとコアの接続

- Tauriコマンド: タスク、区間、仕分け、ブロック、チェックイン、ハッスル、ABC、OKR、スキル、レポート、振り返り、設定のCRUD・参照
- Tauriイベント: 区間更新、プッシュ発生、doingタスク変化、取り込み完了
- CLIとTauriコマンドは同じcore関数を呼ぶこと（ロジックの重複禁止）
- 通知のアクティベーション（トーストのボタン押下等）は常駐アプリが受け、必要に応じて該当画面を開く

---

## 16. 設定ファイル全体像（抜粋）

```toml
[general]
timezone = "Asia/Tokyo"
autostart = true

[collect]
idle_threshold_sec = 120
merge_gap_sec = 30
terminal_exes = ["WindowsTerminal.exe", "pwsh.exe", "Code.exe"]
communication_exes = ["ms-teams.exe", "Slack.exe", "olk.exe", "OUTLOOK.EXE"]
meeting_exes = ["ms-teams.exe", "Zoom.exe"]

[[collect.exclude]]
exe = "KeePassXC.exe"
[[collect.exclude]]
title_regex = "InPrivate|シークレット"

[agent]
enabled = ["claude-code", "codex"]
prompt_excerpt_chars = 200
watch_max_min = 10

[push]
daily_limit = 6
day_start_after = "08:00"
day_end_at = "18:30"
weekly = { weekday = "fri", at = "17:00" }
long_focus_min = 90
switch_spike = { window_min = 15, count = 20 }
late_work_after = "20:00"

[activity]
labels = ["開発", "レビュー", "調査", "設計", "コミュニケーション", "会議", "事務", "学習"]
```

---

## 17. 配布

- リポジトリ: GitHubのパブリックリポジトリ（決定）。個人の就業時間外の開発。
- ライセンス: **MIT OR Apache-2.0 のデュアルライセンス**（決定）。リポジトリ直下に `LICENSE-MIT` と `LICENSE-APACHE` を置き、各クレートの `Cargo.toml` に `license = "MIT OR Apache-2.0"` を設定する。依存クレートのライセンスは `cargo-deny` 等でCI上でチェックし、コピーレフト系（GPL等）の混入を防ぐ。
- GitHub Actionsでビルドし、GitHub Releasesに以下を添付:
  - `hitoiki-<ver>-windows-x64-setup.exe`（推奨。Tauri の NSIS インストーラー。MSI は作らない）
  - `hitoiki-<ver>-windows-x64.zip`（ポータブル版。`hitoiki.exe` CLI、`hitoiki-app.exe` 常駐アプリ。展開のみで動作すること）
- インストーラーの仕様（`crates/hitoiki-app/tauri.conf.json`、`tauri.bundle.conf.json`、`windows/`）:
  - `installMode: currentUser`（管理者権限不要）。既定のインストール先は `%LOCALAPPDATA%\HitoikiApp`。データ用の `%LOCALAPPDATA%\hitoiki` とは大文字小文字を除いて同名になるため、製品名（`productName`）を `HitoikiApp` にして衝突を避ける。実行ファイル名は `mainBinaryName` で `hitoiki-app.exe` に固定する（収集器が自己除外に使う）。
  - CLI は `externalBin`（サイドカー）で同梱し、インストール先に `hitoiki.exe` として `hitoiki-app.exe` と並べる（CLI の `ui` 起動と daemon が同じフォルダの `hitoiki-app.exe` を探すため）。`externalBin` はビルド時にファイルの存在を検査されるので、通常の `cargo build` / `cargo test` を壊さないよう、bundle 専用の `tauri.bundle.conf.json` に分け、`tauri build --config` で合成する。ビルド前に `binaries/hitoiki-x86_64-pc-windows-msvc.exe` へ CLI をコピーする（`.github/workflows/release.yml` 参照）。
  - NSIS フック（`windows/hooks.nsh`）が PowerShell（`windows/hitoiki-path.ps1`）を呼び、インストール先をユーザー PATH（HKCU）へ重複なしで追加し、アンインストールで除去する。NSIS の文字列長制限を避けるため PATH の編集は PowerShell 側で行い、`REG_EXPAND_SZ` を保つためレジストリを展開なしで直接読み書きする。変更後に `WM_SETTINGCHANGE` を通知する。自動起動の Run エントリ（値名 `Hitoiki`）もアンインストール時（更新を除く）に削除する。
  - アンインストールでユーザーデータ（`%APPDATA%\hitoiki`、`%LOCALAPPDATA%\hitoiki`）は消さない。標準の「アプリデータを削除」は `dev.hitoiki.app` フォルダのみが対象。
  - updater プラグイン（自動更新）は入れない。
- コード署名なし。READMEに、ダウンロードしたファイルのプロパティで「ブロックの解除」を行う手順を記載（SmartScreen対策）。
- ネットワークポートを開かないことをREADMEに明記する（会社PCでの導入判断の材料）。
- 自動更新は初期実装しない。`hitoiki version --check` でGitHub Releasesの最新版を確認できる程度に留める（ネットワークアクセスはこのコマンド実行時のみ）。
- DBスキーマのマイグレーションは起動時に自動適用。適用前にDBファイルをバックアップ。

---

## 18. 実装フェーズ

各フェーズ終了時点で単体として使える状態にする。

### Phase 1: 自分の時間が見える
- core、SQLite、マイグレーション
- Windows収集器（前面ウインドウ、無操作、ロック/スリープ）、除外ルール
- 帰属判定パイプライン（ルールのみ、override）
- タスクCRUD、task_match_rule
- CLI: `status`, `task *`, `log`, `review *`, `rebuild`, `report`, `daemon`, `doctor`
- 受け入れ基準: 1日動かして `hitoiki report --by task` が妥当な時間配分を出す。needs_review を CLI で確定できる

### Phase 2: エージェント対話時間
- Claude Code / Codex アダプタ（ログ取り込み）
- セッションとウインドウの対応付け、agent_dialog / agent_watch 算出（7.3）
- `hitoiki agent *`
- 受け入れ基準: エージェントを裏で動かしつつ別作業をした場合に、別作業の時間が agent_dialog に混入しない

### Phase 3: GUIと常駐
- Tauri常駐アプリ、トレイ、UI（今日・仕分け・タスク・レポート）、Tauriコマンド/イベント
- 名前付きパイプによるCLIからの通知、`hitoiki ui`

### Phase 4: プッシュと記録
- プッシュ通知（トリガー、抑制、トースト）
- チェックイン、ハッスル、ABC、カレンダー（ICS）、会議判定、割り込み検出
- タイムブロッキング

### Phase 5: 内省とフレームワーク
- OKR、スキル、アイゼンハワー集計
- AI振り返り（`context`, `reflect`, `abc dispute`）、テンプレート

### Phase 6: 拡張
- 判定器（openai-compatible、external-command）、質問セット
- 補助コレクタの活用（`ingest context`、ネイティブメッセージングホスト、ブラウザ拡張・VS Code拡張）
- エージェントフック方式
- （将来）Jevアダプタ、Graph/Googleカレンダー直接連携、他OS

---

## 19. テスト方針

- 帰属判定パイプラインは純粋関数として実装し、生データのフィクスチャ（JSON）→期待される区間のテーブルテストを書く。特に7.3のケース:
  - 応答完了後すぐ戻ってきてプロンプト送信
  - 応答完了後に別作業30分、戻ってきて2分でプロンプト送信（agent_dialogは2分のみ）
  - 区間中に2回戻ってきた
  - 同一ターミナルの別タブに別セッション
  - 送信後に見守り→無操作で打ち切り
- エージェントログのパーサは実ファイルから作った匿名化フィクスチャでテスト。
- 収集器はOS依存部をトレイトで差し替え、モックで帰属判定まで通すE2Eテストを用意。
- 時刻はテスト可能な時計（Clockトレイト）経由で取得する。

---

## 20. 未確定事項・要検証事項

| # | 項目 | 種別 |
|---|---|---|
| 1 | Claude Code / Codex のセッションログ形式（ユーザー入力とツール結果の区別、応答完了の判定方法） | 要検証 |
| 2 | Windows Terminal で複数タブのセッションをタイトルで区別できるか | 要検証 |
| 3 | 未署名・非パッケージのTauriアプリでアクション付きトースト通知とその応答受信が可能か | 要検証 |
| 4 | `claude -p` / `codex exec` の非対話実行で標準入力からプロンプトを渡す方法 | 要検証 |
| 5 | 社内環境でOutlookのICS公開が許可されているか（不可ならGraph等の代替） | 要確認（ユーザー） |
| 6 | スキルのプリセット内容 | 未確定（初期は少数の汎用セットで可） |
| 7 | 未署名exeからのネイティブメッセージングホスト登録が社内環境のブラウザポリシーで許可されるか | 要検証（Phase 6） |
