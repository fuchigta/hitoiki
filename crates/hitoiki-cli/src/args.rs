//! コマンド体系の定義（docs/dev/architecture.md「CLI」）。

use clap::{Args, Parser, Subcommand};
use hitoiki_core::model::{ErgTag, MatchField, PatternType, SegmentKind, TaskKind};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(
    name = "hitoiki",
    about = "Hitoiki（一息）: 自己管理ツールの CLI",
    disable_version_flag = true
)]
pub struct Cli {
    /// 機械可読な JSON で出力する（参照系コマンド）。
    #[arg(long, global = true)]
    pub json: bool,
    #[command(subcommand)]
    pub cmd: Cmd,
}

/// 範囲指定（`--date` と `--from/--to`）。
#[derive(Debug, Args, Clone, Default)]
pub struct RangeOpts {
    /// today / yesterday / this-week / last-week / YYYY-MM-DD
    #[arg(long)]
    pub date: Option<String>,
    /// 開始（ISO 8601 / ローカル日時 / 日付）
    #[arg(long)]
    pub from: Option<String>,
    /// 終了（日付のみの場合はその日の終わりまで）
    #[arg(long)]
    pub to: Option<String>,
}

#[derive(Debug, Subcommand)]
pub enum Cmd {
    /// 現在の doing タスク、直近の区間、今日の集計、未応答のプッシュ
    Status,
    /// タスクの操作
    #[command(subcommand)]
    Task(TaskCmd),
    /// 区間一覧（--raw で生データも）
    Log {
        #[command(flatten)]
        range: RangeOpts,
        /// window_span / idle_span / agent_event を時系列に混ぜて表示する
        #[arg(long)]
        raw: bool,
    },
    /// 要確認区間の一覧と確定
    #[command(subcommand)]
    Review(ReviewCmd),
    /// 派生層の再計算
    Rebuild {
        /// today / yesterday / this-week / last-week / YYYY-MM-DD
        #[arg(long, conflicts_with_all = ["from", "to"])]
        date: Option<String>,
        #[arg(long)]
        from: Option<String>,
        #[arg(long)]
        to: Option<String>,
    },
    /// タイムブロッキング
    #[command(subcommand)]
    Block(BlockCmd),
    /// コンディションの記録
    Check {
        /// エネルギー 1-5
        #[arg(short = 'e', long)]
        energy: Option<i64>,
        /// 気分 1-5
        #[arg(short = 'm', long)]
        mood: Option<i64>,
        /// ストレス 1-5
        #[arg(short = 's', long)]
        stress: Option<i64>,
        /// ERG タグ
        #[arg(long)]
        erg: Option<ErgTag>,
        #[arg(long)]
        note: Option<String>,
    },
    /// デイリーハッスルの記録
    Hassle {
        text: String,
        /// 強度 1-5（既定 3）
        #[arg(short = 'i', long)]
        intensity: Option<i64>,
    },
    /// ABC 理論の記録
    #[command(subcommand)]
    Abc(AbcCmd),
    /// OKR
    #[command(subcommand)]
    Okr(OkrCmd),
    /// スキル
    #[command(subcommand)]
    Skill(SkillCmd),
    /// 期間内の集計
    Report {
        /// today / yesterday / this-week / last-week / YYYY-MM-DD（既定 today）
        #[arg(long)]
        period: Option<String>,
        #[arg(long)]
        from: Option<String>,
        #[arg(long)]
        to: Option<String>,
        /// task | activity | quadrant | okr | skill | agent | kind
        #[arg(long, default_value = "task")]
        by: String,
        /// idle / break も合計に含める
        #[arg(long)]
        include_idle: bool,
    },
    /// AI 用コンテキストを JSON で出力
    Context {
        #[arg(long)]
        kind: String,
        #[arg(long)]
        date: Option<String>,
    },
    /// AI で問いを生成して保存
    #[command(subcommand)]
    Reflect(ReflectCmd),
    /// エージェントセッション
    #[command(subcommand)]
    Agent(AgentCmd),
    /// ヘッドレス常駐（UI とトースト通知なし。Ctrl+C で停止）
    Daemon,
    /// 常駐アプリのウインドウを開く（未起動なら起動する）。screen: today|today/design|review|tasks|report|records|plan|reflection|okr|settings|onboarding
    Ui { screen: Option<String> },
    /// 補助コレクタ用の取り込み
    #[command(subcommand)]
    Ingest(IngestCmd),
    /// ブラウザ拡張のネイティブメッセージングホスト（install / uninstall で登録・解除）
    NativeHost(NativeHostArgs),
    /// 設定
    #[command(subcommand)]
    Config(ConfigCmd),
    /// 収集状態、エージェントログ、カレンダー、通知、判定器などの診断
    Doctor {
        /// カレンダー取得・判定器への疎通も実際に試す（ネットワークを使う）
        #[arg(long)]
        network: bool,
    },
    /// バージョンを表示する
    Version {
        /// GitHub Releases で最新版を確認する（このときだけネットワークを使う）
        #[arg(long)]
        check: bool,
    },
}

/// task add / edit 共通のオプション。
#[derive(Debug, Args, Clone, Default)]
pub struct TaskOpts {
    /// 本文ファイル
    #[arg(long, conflicts_with = "body")]
    pub body_file: Option<PathBuf>,
    /// 本文（`-` で標準入力）
    #[arg(long)]
    pub body: Option<String>,
    #[arg(long, conflicts_with = "not_urgent")]
    pub urgent: bool,
    #[arg(long)]
    pub not_urgent: bool,
    #[arg(long, conflicts_with = "not_important")]
    pub important: bool,
    #[arg(long)]
    pub not_important: bool,
    /// 見積もり（90m / 1h30m / 分）
    #[arg(long)]
    pub estimate: Option<String>,
    /// KR の ID
    #[arg(long)]
    pub kr: Option<String>,
    /// スキル名（複数指定可。`,` 区切りも可）
    #[arg(long, value_delimiter = ',')]
    pub skill: Vec<String>,
    /// Issue / Jira 等のリンク
    #[arg(long)]
    pub url: Option<String>,
    #[arg(long)]
    pub plan_start: Option<String>,
    #[arg(long)]
    pub plan_end: Option<String>,
    /// 割り込みタスク（urgent=1）
    #[arg(long)]
    pub interrupt: bool,
}

#[derive(Debug, Subcommand)]
pub enum TaskCmd {
    /// タスクを追加する
    Add {
        title: String,
        #[command(flatten)]
        opts: TaskOpts,
    },
    /// タスク一覧
    List {
        /// カンマ区切り（todo,doing,done,dropped,all）。既定は todo,doing
        #[arg(long)]
        status: Option<String>,
        #[arg(long)]
        kind: Option<TaskKind>,
    },
    Show {
        id: String,
    },
    /// タスクを編集する
    Edit {
        id: String,
        #[arg(long)]
        title: Option<String>,
        #[command(flatten)]
        opts: TaskOpts,
    },
    /// doing にする
    Start {
        id: String,
    },
    Done {
        id: String,
    },
    Drop {
        id: String,
    },
    /// 帰属ルール
    #[command(subcommand)]
    Rule(RuleCmd),
}

#[derive(Debug, Subcommand)]
pub enum RuleCmd {
    Add {
        id: String,
        /// exe | title | cwd | repo | url | git_branch
        #[arg(long)]
        field: MatchField,
        #[arg(long)]
        pattern: String,
        /// glob | regex | contains（既定 glob）
        #[arg(long = "type")]
        pattern_type: Option<PatternType>,
        #[arg(long, default_value_t = 0)]
        priority: i64,
    },
    List {
        id: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
pub enum ReviewCmd {
    /// needs_review の区間（既定では短い区間を隠す）
    List {
        #[command(flatten)]
        range: RangeOpts,
        /// `review.min_sec` 未満の短い区間も含めて全件表示する
        #[arg(long)]
        all: bool,
    },
    /// 区間を確定する
    Set {
        segment: String,
        #[arg(long)]
        task: Option<String>,
        #[arg(long)]
        kind: Option<SegmentKind>,
        #[arg(long)]
        activity: Option<String>,
        /// 区間からルールを生成する
        #[arg(long)]
        rule: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum BlockCmd {
    Add {
        start: String,
        end: String,
        #[arg(long)]
        task: Option<String>,
        #[arg(long)]
        label: Option<String>,
        #[arg(long)]
        focus: bool,
    },
    List {
        #[command(flatten)]
        range: RangeOpts,
    },
    /// ブロックを編集する（指定した項目だけ変える）
    Edit {
        /// ブロックの ID（前方一致）
        id: String,
        #[arg(long)]
        start: Option<String>,
        #[arg(long)]
        end: Option<String>,
        #[arg(long)]
        task: Option<String>,
        #[arg(long)]
        label: Option<String>,
        /// 集中の時間にする
        #[arg(long, conflicts_with = "no_focus")]
        focus: bool,
        /// 集中の時間を解除する
        #[arg(long)]
        no_focus: bool,
    },
    /// ブロックを削除する
    Delete {
        /// ブロックの ID（前方一致）
        id: String,
    },
    /// ブロックのテンプレート（働き方のパターン）
    #[command(subcommand)]
    Template(TemplateCmd),
}

#[derive(Debug, Subcommand)]
pub enum TemplateCmd {
    /// テンプレートを作る
    Add {
        name: String,
        #[arg(long)]
        description: Option<String>,
        /// 割り当てる曜日（例: mon,wed / 月,水 / 1,3。1=月 … 7=日）
        #[arg(long)]
        weekdays: Option<String>,
        /// ブロック（複数指定可）: HH:MM-HH:MM[,focus][,task=ID][,label=ラベル]
        #[arg(long = "block")]
        blocks: Vec<String>,
    },
    /// テンプレートの一覧
    List,
    /// テンプレートの中身
    Show { name: String },
    /// テンプレートを編集する（指定した項目だけ変える。--block を指定するとブロックを置き換える）
    Edit {
        name: String,
        #[arg(long)]
        new_name: Option<String>,
        #[arg(long)]
        description: Option<String>,
        /// 空文字を渡すと割り当てを外す
        #[arg(long)]
        weekdays: Option<String>,
        #[arg(long = "block")]
        blocks: Vec<String>,
    },
    /// テンプレートを削除する（作成済みのブロックは消えない）
    Delete { name: String },
    /// 指定日にテンプレートを適用してブロックを作る
    Apply {
        name: String,
        /// today / YYYY-MM-DD（省略は今日）
        #[arg(long)]
        date: Option<String>,
        /// 既存のブロック・予定と重なるときの扱い: skip（作らない）/ overlay（重ねて作る）
        #[arg(long, default_value = "skip")]
        overlap: String,
        /// 何も作らず、作られる内容と重なりを表示する
        #[arg(long)]
        dry_run: bool,
    },
    /// 直前の適用で作ったブロックを削除する
    Undo {
        /// その日に適用したもののうち直近（省略は全体で直近）
        #[arg(long)]
        date: Option<String>,
    },
    /// 指定日のブロックからテンプレートを作る
    SaveFrom {
        /// today / YYYY-MM-DD（省略は今日）
        #[arg(long)]
        date: Option<String>,
        #[arg(long)]
        name: String,
        #[arg(long)]
        description: Option<String>,
        #[arg(long)]
        weekdays: Option<String>,
    },
    /// 指定日の曜日に割り当てたテンプレートを表示する
    Suggest {
        #[arg(long)]
        date: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
pub enum AbcCmd {
    Add {
        /// A: 出来事
        #[arg(long = "a")]
        a: Option<String>,
        /// B: 受け止め方
        #[arg(long = "b")]
        b: Option<String>,
        /// C: 感情
        #[arg(long)]
        c_emotion: Option<String>,
        /// C: 強度 1-5
        #[arg(long)]
        c_intensity: Option<i64>,
        /// ハッスルの ID（A を補完する）
        #[arg(long)]
        from_hassle: Option<String>,
    },
    /// AI で受け止め方を掘り下げる問いを生成
    Dispute { id: String },
}

#[derive(Debug, Subcommand)]
pub enum OkrCmd {
    AddObjective {
        title: String,
        #[arg(long)]
        from: Option<String>,
        #[arg(long)]
        to: Option<String>,
    },
    AddKr {
        objective: String,
        title: String,
        #[arg(long)]
        target: Option<String>,
    },
    List,
}

#[derive(Debug, Subcommand)]
pub enum SkillCmd {
    Add {
        name: String,
        #[arg(long)]
        parent: Option<String>,
    },
    /// 同梱プリセットを取り込む（省略で全部）
    ImportPreset {
        name: Option<String>,
    },
    List,
}

#[derive(Debug, Subcommand)]
pub enum ReflectCmd {
    #[command(name = "day_start")]
    DayStart {
        #[arg(long)]
        date: Option<String>,
    },
    #[command(name = "day_end")]
    DayEnd {
        #[arg(long)]
        date: Option<String>,
    },
    Weekly {
        #[arg(long)]
        date: Option<String>,
    },
    Answer {
        id: String,
        /// 回答ファイル（`-` で標準入力）
        #[arg(long)]
        file: Option<String>,
    },
    /// 保存済みの振り返り一覧（新しい順）
    List {
        /// day_start | day_end | weekly
        #[arg(long)]
        kind: Option<String>,
        /// 最大件数（既定 20）
        #[arg(long, default_value_t = 20)]
        limit: i64,
    },
    /// 振り返りの詳細（問い・回答）
    Show { id: String },
}

#[derive(Debug, Subcommand)]
pub enum AgentCmd {
    Sessions {
        #[arg(long)]
        date: Option<String>,
    },
    Import {
        /// claude-code | codex
        #[arg(long)]
        agent: Option<String>,
        #[arg(long)]
        since: Option<String>,
    },
}

/// `native-host`: 引数なし（またはブラウザが渡す `chrome-extension://...`）でホストとして動作する。
#[derive(Debug, Args)]
pub struct NativeHostArgs {
    /// install | uninstall（省略時はネイティブメッセージングホストとして動作）
    pub action: Option<String>,
    /// install: Chrome / Edge の拡張 ID（複数指定可）
    #[arg(long = "extension-id")]
    pub extension_id: Vec<String>,
    /// install: Firefox の拡張 ID（例: hitoiki@example.com）
    #[arg(long = "firefox-extension-id")]
    pub firefox_extension_id: Vec<String>,
    /// ブラウザがホスト起動時に渡す引数（無視する）
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, hide = true)]
    pub rest: Vec<String>,
}

#[derive(Debug, Subcommand)]
pub enum IngestCmd {
    Context,
}

#[derive(Debug, Subcommand)]
pub enum ConfigCmd {
    /// 設定ファイルのパス
    Path,
    Get {
        key: String,
    },
    Set {
        key: String,
        value: String,
    },
}
