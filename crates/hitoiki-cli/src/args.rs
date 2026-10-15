//! コマンド体系の定義（docs/dev/architecture.md「CLI」）。

use clap::{Args, Parser, Subcommand};
use hitoiki_core::model::{ErgChoice, ErgTag, MatchField, PatternType, SegmentKind, TaskKind};
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
    /// 通知（プッシュ）の一覧と、あとからの回答・見送り
    #[command(subcommand)]
    Push(PushCmd),
    /// コンディションの記録（`check list|edit|delete` で一覧・編集・削除）
    #[command(args_conflicts_with_subcommands = true)]
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
        /// ERG タグ（ふつうは付けなくてよい。メモの内容から AI が判定する。付けるとユーザーの確定になる）
        #[arg(long)]
        erg: Option<ErgTag>,
        #[arg(long)]
        note: Option<String>,
        #[command(subcommand)]
        cmd: Option<CheckCmd>,
    },
    /// 気になったこと（ハッスル）の記録（`hassle list|edit|delete` で一覧・編集・削除）
    #[command(args_conflicts_with_subcommands = true)]
    Hassle {
        text: Option<String>,
        /// 強度 1-5（既定 3）
        #[arg(short = 'i', long)]
        intensity: Option<i64>,
        #[command(subcommand)]
        cmd: Option<HassleCmd>,
    },
    /// 活動ルール（アプリ・タイトル等のパターンから活動ラベルを当てる）
    #[command(subcommand, name = "activity-rule")]
    ActivityRule(ActivityRuleCmd),
    /// 活動ラベルの候補（自動判定（AI）へ渡す一覧）
    #[command(subcommand, name = "activity-label")]
    ActivityLabel(LabelCmd),
    /// カレンダーソース（ICS）
    #[command(subcommand)]
    Calendar(CalendarCmd),
    /// 捉え直し（ABC）の記録。出来事・受け止め方・感情を分けて書く
    #[command(subcommand)]
    Abc(AbcCmd),
    /// 目標
    #[command(subcommand)]
    Goal(GoalCmd),
    /// 成果の指標
    #[command(subcommand, name = "key-result")]
    KeyResult(KeyResultCmd),
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
        /// task | activity | urgent_important | key_result | skill | agent | kind
        #[arg(long, default_value = "task")]
        by: String,
        /// idle / break も合計に含める
        #[arg(long)]
        include_idle: bool,
    },
    /// 活動量ヒートマップ（日別の濃淡）
    Heatmap {
        /// time | kind:<種別> | task:<ID・タイトル> | skill:<名前> | records[:checkin|hassle|abc|reflection]
        #[arg(long, default_value = "time")]
        by: String,
        /// 開始日（既定: 今日を含む週から数えて 53 週分）
        #[arg(long)]
        from: Option<String>,
        /// 終了日（含む。既定: 今日）
        #[arg(long)]
        to: Option<String>,
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
    /// ERG（存在・関係・成長）。記録の内容から AI が判定し、確認待ちをユーザーが確定する
    #[command(subcommand)]
    Erg(ErgCmd),
    /// エージェントセッション
    #[command(subcommand)]
    Agent(AgentCmd),
    /// ヘッドレス常駐（UI とトースト通知なし。Ctrl+C で停止）
    Daemon,
    /// 常駐アプリのウインドウを開く（未起動なら起動する）。screen: today|today/design|log|tasks|goals|records|report|settings|onboarding（旧名: review・plan・reflect・reflection・checkin・hassle）
    Ui { screen: Option<String> },
    /// 補助コレクタ用の取り込み
    #[command(subcommand)]
    Ingest(IngestCmd),
    /// ブラウザ拡張のネイティブメッセージングホスト（install / uninstall で登録・解除）
    NativeHost(NativeHostArgs),
    /// 無視するアプリ（collect.ignore_exes）の管理と、無視の提案
    #[command(subcommand)]
    Ignore(IgnoreCmd),
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
    /// 成果の指標の ID
    #[arg(long, conflicts_with = "no_key_result")]
    pub key_result: Option<String>,
    /// 成果の指標との紐付けを外す（edit のみ）
    #[arg(long)]
    pub no_key_result: bool,
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
    /// 誤入力のタスクを削除する（やめる=記録として残す / 削除=無かったことにする）
    Delete {
        id: String,
        /// 計上済みの時間・確定済みの割り当てがあっても削除する
        #[arg(long)]
        force: bool,
        /// 削除せず、影響だけを表示する
        #[arg(long)]
        dry_run: bool,
    },
    /// タスクを選んで（または作って）すぐ開始する。割り込みで素早く切り替えるとき
    Switch {
        /// 未完了のタスクのタイトル（完全一致・部分一致）または ID（前方一致）
        query: String,
        /// 既存のタスクを探さず、query をタイトルとして新規作成して開始する
        #[arg(long)]
        new: bool,
        /// 新規作成するタスクを割り込み（urgent=1）にする（--new が必要）
        #[arg(long, requires = "new")]
        interrupt: bool,
    },
    /// 直前まで doing だったタスクに戻る（割り込みが終わったとき）
    Back,
    /// タスクの割り当てルール
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
    /// ルールを編集する（指定した項目だけ変える）
    Edit {
        /// ルールの ID（前方一致）
        rule: String,
        #[arg(long)]
        field: Option<MatchField>,
        #[arg(long)]
        pattern: Option<String>,
        #[arg(long = "type")]
        pattern_type: Option<PatternType>,
        #[arg(long)]
        priority: Option<i64>,
        /// 有効にする
        #[arg(long, conflicts_with = "disable")]
        enable: bool,
        /// 無効にする（ルールは残る）
        #[arg(long)]
        disable: bool,
    },
    /// ルールを削除する
    Delete {
        rule: String,
        /// 削除せず、対象だけ表示する
        #[arg(long)]
        dry_run: bool,
    },
}

/// 削除系コマンドの共通オプション。
#[derive(Debug, Args, Clone, Copy)]
pub struct DeleteOpts {
    /// 影響があっても削除する
    #[arg(long)]
    pub force: bool,
    /// 削除せず、影響だけ表示する
    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Debug, Subcommand)]
pub enum CheckCmd {
    /// 調子（チェックイン）の一覧
    List {
        #[command(flatten)]
        range: RangeOpts,
    },
    /// 調子を編集する（指定した項目だけ変える。-e/-m/-s に 0 を渡すとその項目を消す）
    Edit {
        id: String,
        #[arg(short = 'e', long)]
        energy: Option<i64>,
        #[arg(short = 'm', long)]
        mood: Option<i64>,
        #[arg(short = 's', long)]
        stress: Option<i64>,
        #[arg(long)]
        erg: Option<ErgTag>,
        /// 空文字を渡すとメモを消す
        #[arg(long)]
        note: Option<String>,
    },
    Delete {
        id: String,
        #[command(flatten)]
        opts: DeleteOpts,
    },
}

#[derive(Debug, Subcommand)]
pub enum HassleCmd {
    List {
        #[command(flatten)]
        range: RangeOpts,
    },
    /// 気になったことを編集する
    Edit {
        id: String,
        #[arg(long)]
        text: Option<String>,
        #[arg(short = 'i', long)]
        intensity: Option<i64>,
    },
    /// 気になったことを削除する（元にした捉え直しは残り、紐付けだけ外れる）
    Delete {
        id: String,
        #[command(flatten)]
        opts: DeleteOpts,
    },
}

#[derive(Debug, Subcommand)]
pub enum ActivityRuleCmd {
    List,
    Add {
        #[arg(long)]
        field: MatchField,
        #[arg(long)]
        pattern: String,
        /// glob | regex | contains（既定 glob）
        #[arg(long = "type")]
        pattern_type: Option<PatternType>,
        /// 当てる活動ラベル
        #[arg(long)]
        label: String,
        #[arg(long, default_value_t = 0)]
        priority: i64,
    },
    Edit {
        rule: String,
        #[arg(long)]
        field: Option<MatchField>,
        #[arg(long)]
        pattern: Option<String>,
        #[arg(long = "type")]
        pattern_type: Option<PatternType>,
        #[arg(long)]
        label: Option<String>,
        #[arg(long)]
        priority: Option<i64>,
        #[arg(long, conflicts_with = "disable")]
        enable: bool,
        #[arg(long)]
        disable: bool,
    },
    Delete {
        rule: String,
        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum LabelCmd {
    List,
    Add {
        label: String,
    },
    /// 名前を変える（同じラベルの活動ルールも付け替える）
    Rename {
        from: String,
        to: String,
    },
    /// 候補から外す（過去の区間・ルールに付いたラベルは残る）
    Remove {
        label: String,
    },
}

#[derive(Debug, Subcommand)]
pub enum CalendarCmd {
    /// ソースの一覧（URL は表示しない）
    List,
    /// ICS のソースを追加する
    Add { name: String, url: String },
    Edit {
        name: String,
        #[arg(long)]
        new_name: Option<String>,
        #[arg(long)]
        url: Option<String>,
    },
    /// ソースを削除する（取得済みの予定のコピーも消える）
    Delete {
        name: String,
        #[command(flatten)]
        opts: DeleteOpts,
    },
}

#[derive(Debug, Subcommand)]
pub enum PushCmd {
    /// 配信された通知の一覧（既定は今日。答えたもの・見送ったものも含む）
    List {
        #[command(flatten)]
        range: RangeOpts,
        /// 未回答のものだけ
        #[arg(long)]
        unanswered: bool,
    },
    /// 通知に答える（または --dismiss で見送る）。記録の時刻は通知の時刻になる
    Answer {
        /// 通知の ID（前方一致）
        id: String,
        /// エネルギー 1-5
        #[arg(short = 'e', long)]
        energy: Option<i64>,
        /// 気分 1-5
        #[arg(short = 'm', long)]
        mood: Option<i64>,
        /// ストレス 1-5
        #[arg(short = 's', long)]
        stress: Option<i64>,
        /// 一言（気になったこととして保存）
        #[arg(long)]
        hassle: Option<String>,
        /// 無操作の分類: break / meeting / thinking / away、または task（通知が示したタスクをしていた。PC 外の作業として確定）（無操作からの復帰の通知のみ）
        #[arg(long)]
        idle: Option<String>,
        /// タスク通知への答え: start（始める）/ later（あとで）/ skip_week（今週は見送る）/ skip（計画ブロックの開始を今回はやめる）
        #[arg(long)]
        task: Option<String>,
        /// 答えない（見送り）。一覧からは消えるが、応答率では無応答として数える
        #[arg(long)]
        dismiss: bool,
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
    /// 区間を確定する。複数の ID を渡すとまとめて同じ割り当てをする。
    /// `--from` / `--to` を渡すと、時間帯の範囲に含まれる区間をまとめて付け替える（実績の補正）
    Set {
        /// 区間 ID（前方一致）。複数指定できる（範囲指定のときは不要）
        #[arg(required_unless_present = "from", conflicts_with_all = ["from", "to"])]
        segments: Vec<String>,
        /// 割り当てるタスク
        #[arg(long)]
        task: Option<String>,
        #[arg(long, conflicts_with_all = ["from", "to"])]
        kind: Option<SegmentKind>,
        #[arg(long)]
        activity: Option<String>,
        /// 区間からルールを生成する
        #[arg(long, conflicts_with_all = ["from", "to"])]
        rule: bool,
        /// 範囲の開始（ISO 8601 / ローカル日時 / `HH:MM`）。境界にまたがる区間は範囲の内側だけを対象にする
        #[arg(long, requires = "to")]
        from: Option<String>,
        /// 範囲の終了
        #[arg(long, requires = "from")]
        to: Option<String>,
        /// 範囲指定のとき、タスクなしにする
        #[arg(long, requires = "from", conflicts_with_all = ["task", "rest"])]
        no_task: bool,
        /// 範囲指定のとき、休憩にする
        #[arg(long, requires = "from", conflicts_with_all = ["task", "no_task"])]
        rest: bool,
        /// 範囲指定のとき、ユーザー確定済みの区間も対象にする（`review.correct_include_confirmed`）
        #[arg(long, requires = "from")]
        include_confirmed: bool,
        /// 範囲指定のとき、会議の区間も対象にする（`review.correct_include_meeting`）
        #[arg(long, requires = "from")]
        include_meeting: bool,
        /// 範囲指定のとき、無操作・記録なしも PC 外の作業として対象にする（`review.correct_include_idle`）
        #[arg(long, requires = "from")]
        include_idle: bool,
        /// 範囲指定のとき、対象から外す区間 ID（記録なしは `gap:<開始のミリ秒>`）。複数指定できる
        #[arg(long, requires = "from", num_args = 1.., value_name = "SEGMENT")]
        exclude: Vec<String>,
        /// 範囲指定のとき、確定せずに対象と合計だけを表示する
        #[arg(long, requires = "from")]
        dry_run: bool,
    },
    /// 実績の補正（範囲指定の付け替え）の一覧
    Corrections {
        /// このタスクの補正だけ
        #[arg(long)]
        task: Option<String>,
        /// この日（today / yesterday / YYYY-MM-DD）と重なる補正だけ
        #[arg(long)]
        date: Option<String>,
    },
    /// 実績の補正を取り消す（その補正で作った確定を削除して再計算する）
    UndoCorrection {
        /// 補正の ID（前方一致）
        id: String,
    },
    /// 区間を無視にする（集計・仕分け・レポートに数えない。`review unignore` で取り消せる）
    Ignore {
        /// 区間 ID（前方一致）を 1 つ以上
        #[arg(required = true)]
        segments: Vec<String>,
    },
    /// 無視にした区間を元に戻す（確定を削除して再計算する）
    Unignore {
        /// 区間 ID（前方一致）を 1 つ以上
        #[arg(required = true)]
        segments: Vec<String>,
    },
    /// 無操作の区間に分類をつける: `review idle <区間 ID>... <break|meeting|thinking|away>`
    /// （休憩 = 種別 break、会議 = 種別 meeting、考えごと = 作業 + 活動「考えごと」、離席 = 休憩 + 活動「離席」）
    Idle {
        /// 区間 ID（前方一致）を 1 つ以上、最後に分類
        #[arg(required = true, num_args = 2.., value_name = "SEGMENT... PRESET")]
        args: Vec<String>,
    },
    /// ルール化の提案を表示する。`--accept N` で N 番目をルールにし、`--dismiss N` で断る
    Suggest {
        #[arg(long, value_name = "N", conflicts_with = "dismiss")]
        accept: Option<usize>,
        #[arg(long, value_name = "N")]
        dismiss: Option<usize>,
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
        /// 気になったことの ID（A を補完する）
        #[arg(long)]
        from_hassle: Option<String>,
    },
    /// AI で受け止め方を掘り下げる問いを生成
    Dispute { id: String },
    /// 捉え直しの一覧
    List {
        #[command(flatten)]
        range: RangeOpts,
    },
    /// 捉え直しを編集する（指定した項目だけ変える。文字列に空文字を渡すと消す。--c-intensity 0 で強度を消す）
    Edit {
        id: String,
        #[arg(long = "a")]
        a: Option<String>,
        #[arg(long = "b")]
        b: Option<String>,
        #[arg(long)]
        c_emotion: Option<String>,
        #[arg(long)]
        c_intensity: Option<i64>,
        /// 反論・掘り下げのメモ
        #[arg(long)]
        dispute: Option<String>,
    },
    Delete {
        id: String,
        #[command(flatten)]
        opts: DeleteOpts,
    },
}

#[derive(Debug, Subcommand)]
pub enum GoalCmd {
    /// 目標を追加する
    Add {
        title: String,
        #[arg(long)]
        from: Option<String>,
        #[arg(long)]
        to: Option<String>,
    },
    /// 目標と成果の指標の一覧
    List,
    /// 目標と、その下の成果の指標を表示する
    Show {
        /// 目標の ID（前方一致）
        goal: String,
    },
    /// 目標を編集する（指定した項目だけ変える。--description "" で説明を消す）
    Edit {
        id: String,
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        description: Option<String>,
        #[arg(long)]
        from: Option<String>,
        #[arg(long)]
        to: Option<String>,
        /// 期間を消す
        #[arg(long)]
        clear_period: bool,
        /// active | achieved | dropped
        #[arg(long)]
        status: Option<String>,
    },
    /// 目標を削除する（配下の成果の指標も消え、成果の指標に紐付いたタスクは残って紐付けだけ外れる）
    Delete {
        id: String,
        #[command(flatten)]
        opts: DeleteOpts,
    },
}

#[derive(Debug, Subcommand)]
pub enum KeyResultCmd {
    /// 成果の指標を目標の下に追加する
    Add {
        /// 目標の ID（前方一致）
        goal: String,
        title: String,
        #[arg(long)]
        target: Option<String>,
        /// 成果の指標のスキル（スキル名。複数指定可、`,` 区切りも可）。成果の指標の時間はこのスキルが付いたタスク・活動の時間が主になる
        #[arg(long, value_delimiter = ',')]
        skill: Vec<String>,
    },
    /// 成果の指標を編集する（指定した項目だけ変える。--target "" で目標値を消す）
    Edit {
        id: String,
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        target: Option<String>,
        /// 現在値
        #[arg(long)]
        current: Option<String>,
        /// --current で値を変えるときに履歴へ残すメモ
        #[arg(long, requires = "current")]
        note: Option<String>,
        /// active | achieved | dropped
        #[arg(long)]
        status: Option<String>,
        /// 別の目標へ付け替える
        #[arg(long)]
        goal: Option<String>,
        /// 数え方: time（時間）| active_days（取り組んだ日数）| done_tasks（完了したタスク数）
        #[arg(long)]
        measure: Option<String>,
        /// 数え方が active_days のとき「取り組んだ日」とみなす 1 日の最小分数（既定 15）
        #[arg(long)]
        active_day_min: Option<i64>,
        /// 成果の指標のスキル（スキル名。複数指定可、`,` 区切りも可。指定すると今のスキルを置き換える）。成果の指標の時間はスキルが主で、このスキル（子スキルを含む）が付いたタスク・活動の時間を数える。紐付けたタスクの時間も重ならないように合算する
        #[arg(long, value_delimiter = ',')]
        skill: Vec<String>,
        /// 成果の指標のスキルを全て外す
        #[arg(long)]
        no_skill: bool,
    },
    /// 成果の指標にタスクを紐付ける（別の成果の指標に紐付いていたものは付け替える）
    Link {
        /// 成果の指標の ID（前方一致）またはタイトル
        key_result: String,
        /// タスクの ID（前方一致。複数可）
        #[arg(required = true)]
        tasks: Vec<String>,
    },
    /// タスクの成果の指標との紐付けを外す
    Unlink {
        /// タスクの ID（前方一致。複数可）
        #[arg(required = true)]
        tasks: Vec<String>,
    },
    /// 成果の指標の集計（数え方の値、取り組んだ日、完了数、週ごとの時間、スキル別、見積と実績）
    Show {
        /// 成果の指標の ID（前方一致）またはタイトル
        key_result: String,
        /// 集計の期間（省略時は目標の期間、無ければ直近 8 週）
        #[command(flatten)]
        range: RangeOpts,
    },
    /// スキルも紐付けタスクも無い成果の指標に、紐付けるとよさそうなタスク・スキルの候補を出す（自動では紐付けない）。
    /// 名前の近さで探し、AI が設定されていれば AI にも聞いて合わせる（送るのは成果の指標・目標の名前とタスク・スキルの名前だけ）
    Suggest {
        /// 成果の指標の ID（前方一致）またはタイトル
        key_result: String,
        /// AI に聞かず、名前の近い候補だけにする
        #[arg(long)]
        no_ai: bool,
    },
    /// 提案の候補を「付ける」（task:<ID> は成果の指標に紐付け、skill:<ID または名前> は成果の指標のスキルに加える。今のスキルは残る）
    SuggestAccept {
        /// 成果の指標の ID（前方一致）またはタイトル
        key_result: String,
        /// 候補（`key-result suggest` が出した task:<ID> / skill:<ID>。複数可）
        #[arg(required = true)]
        candidates: Vec<String>,
    },
    /// 提案の候補を「付けない」（以後この成果の指標には提案しない）
    SuggestReject {
        /// 成果の指標の ID（前方一致）またはタイトル
        key_result: String,
        /// 候補（`key-result suggest` が出した task:<ID> / skill:<ID>。複数可）
        #[arg(required = true)]
        candidates: Vec<String>,
    },
    /// 成果の指標の現在値を記録する（現在値を更新し、履歴に 1 行残す。同じ値でも残る）
    Progress {
        /// 成果の指標の ID（前方一致）またはタイトル
        key_result: String,
        /// 現在値（例: 12、3 回）
        value: String,
        /// 履歴に添えるメモ
        #[arg(long)]
        note: Option<String>,
    },
    /// 成果の指標の現在値の履歴を、週ごとの時間と並べて表示する
    History {
        /// 成果の指標の ID（前方一致）またはタイトル
        key_result: String,
        /// 期間（省略時は目標の期間、無ければ直近 8 週）
        #[command(flatten)]
        range: RangeOpts,
    },
    /// 成果の指標を削除する（紐付いたタスクは残って紐付けだけ外れる）
    Delete {
        id: String,
        #[command(flatten)]
        opts: DeleteOpts,
    },
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
    /// スキルを編集する（名前または ID。取り込み済みのプリセットも編集できる）
    Edit {
        skill: String,
        #[arg(long)]
        name: Option<String>,
        /// 親スキル（名前または ID）
        #[arg(long, conflicts_with = "no_parent")]
        parent: Option<String>,
        /// 親を外してトップレベルにする
        #[arg(long)]
        no_parent: bool,
        /// 説明（空文字で消す）
        #[arg(long)]
        description: Option<String>,
    },
    /// スキルを削除する（タスクへの付与は外れる）
    Delete {
        skill: String,
        /// 子スキルの扱い: reparent（親を付け替える・既定）/ delete（一緒に削除）
        #[arg(long, default_value = "reparent")]
        children: String,
        #[command(flatten)]
        opts: DeleteOpts,
    },
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
    /// 回答を保存する。構造化された問いには `{"answers":[{"index":0,"energy":3}]}` の JSON を、従来の問いには Markdown を渡す
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
pub enum ErgCmd {
    /// 未判定の記録を AI で判定する。確信度が `ai.erg_auto_threshold` 以上なら採用、未満は確認待ち。AI が未設定なら何もしない
    Classify {
        #[command(flatten)]
        range: RangeOpts,
        /// 保存せず、判定の結果だけ表示する
        #[arg(long)]
        dry_run: bool,
    },
    /// 確認待ち（AI の提案で未確定）の一覧
    Review,
    /// タグを付ける・直す（ユーザーの確定。AI は上書きしない）。`none` はどれにも関わらない
    Set {
        /// 記録の ID（前方一致。調子・気になったこと・捉え直し・振り返り）
        record: String,
        /// existence | relatedness | growth | none
        tag: ErgChoice,
    },
    /// 確認待ちの提案をそのまま確定する
    Confirm {
        /// 記録の ID（前方一致）
        record: String,
    },
    /// 期間内の ERG の分布（件数）と、記録ごとのラベル
    Show {
        #[command(flatten)]
        range: RangeOpts,
    },
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
pub enum IgnoreCmd {
    /// 無視リストの一覧
    List,
    /// アプリを無視リストに追加し、直近 7 日を再計算する
    Add { exe: String },
    /// アプリを無視リストから外し、直近 7 日を再計算する
    Remove { exe: String },
    /// 無視の提案を表示する。`--accept <exe>` で追加、`--dismiss <exe>` で 30 日間断る
    Suggest {
        #[arg(long, value_name = "EXE", conflicts_with = "dismiss")]
        accept: Option<String>,
        #[arg(long, value_name = "EXE")]
        dismiss: Option<String>,
        /// 遡る日数（既定 7）
        #[arg(long)]
        days: Option<i64>,
    },
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
