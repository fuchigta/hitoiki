//! 設定（docs/dev/architecture.md「ファイル配置と設定」）。`config.toml` が無い・項目が欠けている場合は既定値で補う。

use crate::{paths, Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Config {
    pub general: General,
    pub collect: Collect,
    pub agent: Agent,
    pub nudge: Nudge,
    pub activity: Activity,
    pub judge: Judge,
    pub ai: Ai,
    pub calendar: Calendar,
    pub task: TaskConfig,
    pub actual: Actual,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct General {
    pub timezone: String,
    pub autostart: bool,
    /// 新しいバージョンの確認（1 日 1 回、GitHub に問い合わせる）。
    /// None（未設定）はまだ尋ねていない状態で、確認しない（オプトイン）。true / false は利用者の選択。
    pub update_check: Option<bool>,
    /// クイック入力を開くグローバルショートカット（GUI のみ。例 `Ctrl+Alt+Space`）。空文字で無効。
    pub quick_entry_shortcut: String,
}

impl Default for General {
    fn default() -> Self {
        Self {
            timezone: "Asia/Tokyo".into(),
            autostart: true,
            update_check: None,
            quick_entry_shortcut: "Ctrl+Alt+Space".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Collect {
    pub idle_threshold_sec: u64,
    pub merge_gap_sec: u64,
    pub poll_interval_sec: u64,
    pub terminal_exes: Vec<String>,
    /// GUI のエージェントクライアント（Orca / Cursor など）の exe 名。`terminal_exes` と同じ扱いで、
    /// エージェントとの対話時間を計上するウインドウとみなす。`terminal_exes` を書き換えている既存の設定でも
    /// 既定値が効くように、別のキーにしている。
    pub agent_host_exes: Vec<String>,
    pub communication_exes: Vec<String>,
    pub meeting_exes: Vec<String>,
    /// 通過系アプリの exe 名（タスクバー・スタートメニュー・Alt+Tab など、作業の合間に一瞬だけ前面になるもの）。
    /// 連続して `transient_max_sec` 未満なら、独立した区間にせず前後の作業に吸収する。
    pub transient_exes: Vec<String>,
    /// 通過系アプリとみなす最大の連続時間（秒）。これ以上続いたら通常の作業区間として扱う
    /// （`ignore_exes` の exe は「無視」の区間になる）。
    pub transient_max_sec: u64,
    /// 無視するアプリの exe 名（大文字小文字を区別しない）。ノイズになるシステム系のアプリ向け。
    /// 前面の時間は、`transient_max_sec` 未満なら通過系と同じく前後の作業に吸収し、それ以上続いたものは
    /// 「無視」の区間（`ignored`）にして、集計・レポート・タスクの実績・通知に数えない。
    /// ここに入れた exe は `transient_exes` にも入っているものとして扱う。生データ（`window_span`）は変えない。
    pub ignore_exes: Vec<String>,
    /// 除外区間を丸ごと破棄する（false なら exe 名のみ保持。docs/dev/architecture.md「収集器（Windows）」）。
    pub drop_excluded: bool,
    pub exclude: Vec<ExcludeRule>,
    /// 割り込み判定: コミュニケーション系の後に元タスク外作業が続く最小時間（docs/dev/architecture.md「割り当てパイプライン」）。
    pub interrupt_min_sec: u64,
    /// 割り当ての定期実行で再計算する直近の範囲（分）。
    pub rebuild_window_min: u64,
    /// Hitoiki 自身のウインドウ（hitoiki-app.exe / 自プロセス）を前面にしている時間も記録する。
    /// false（既定）なら記録の空白になる。
    pub record_self: bool,
}

impl Default for Collect {
    fn default() -> Self {
        Self {
            idle_threshold_sec: 120,
            merge_gap_sec: 30,
            poll_interval_sec: 3,
            terminal_exes: [
                "WindowsTerminal.exe",
                "pwsh.exe",
                "powershell.exe",
                "cmd.exe",
                "Code.exe",
                "Claude.exe",
            ]
            .map(String::from)
            .to_vec(),
            agent_host_exes: ["Orca.exe", "Cursor.exe", "Windsurf.exe", "Code - Insiders.exe"]
                .map(String::from)
                .to_vec(),
            communication_exes: ["ms-teams.exe", "Slack.exe", "olk.exe", "OUTLOOK.EXE"]
                .map(String::from)
                .to_vec(),
            meeting_exes: ["ms-teams.exe", "Zoom.exe"].map(String::from).to_vec(),
            // ApplicationFrameHost.exe は UWP アプリの実体なので含めない。
            transient_exes: [
                "explorer.exe",
                "StartMenuExperienceHost.exe",
                "SearchHost.exe",
                "SearchApp.exe",
                "ShellExperienceHost.exe",
                "ShellHost.exe",
                "LockApp.exe",
                "TextInputHost.exe",
            ]
            .map(String::from)
            .to_vec(),
            transient_max_sec: 60,
            ignore_exes: [
                "explorer.exe",
                "ShellExperienceHost.exe",
                "StartMenuExperienceHost.exe",
                "SearchHost.exe",
                "SearchApp.exe",
                "ShellHost.exe",
                "LockApp.exe",
                "TextInputHost.exe",
            ]
            .map(String::from)
            .to_vec(),
            drop_excluded: false,
            exclude: vec![
                ExcludeRule {
                    exe: Some("KeePassXC.exe".into()),
                    title_regex: None,
                },
                ExcludeRule {
                    exe: Some("KeePass.exe".into()),
                    title_regex: None,
                },
                ExcludeRule {
                    exe: Some("1Password.exe".into()),
                    title_regex: None,
                },
                ExcludeRule {
                    exe: Some("Bitwarden.exe".into()),
                    title_regex: None,
                },
                ExcludeRule {
                    exe: None,
                    title_regex: Some("InPrivate|シークレット|Incognito|Private Browsing".into()),
                },
            ],
            interrupt_min_sec: 180,
            rebuild_window_min: 180,
            record_self: false,
        }
    }
}

/// 除外ルール。`exe` と `title_regex` の両方があれば両方一致で除外。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ExcludeRule {
    pub exe: Option<String>,
    pub title_regex: Option<String>,
}

impl ExcludeRule {
    pub fn matches(&self, exe_name: Option<&str>, title: Option<&str>) -> bool {
        if self.exe.is_none() && self.title_regex.is_none() {
            return false;
        }
        let exe_ok = match &self.exe {
            Some(e) => exe_name.is_some_and(|x| x.eq_ignore_ascii_case(e)),
            None => true,
        };
        let title_ok = match &self.title_regex {
            Some(r) => match regex::Regex::new(r) {
                Ok(re) => title.is_some_and(|t| re.is_match(t)),
                Err(_) => false,
            },
            None => true,
        };
        exe_ok && title_ok
    }
}

impl Collect {
    pub fn is_excluded(&self, exe_name: Option<&str>, title: Option<&str>) -> bool {
        self.exclude.iter().any(|r| r.matches(exe_name, title))
    }
    /// エージェントとの対話を行うウインドウの exe か（`terminal_exes` または `agent_host_exes`）。
    pub fn is_terminal(&self, exe: &str) -> bool {
        self.terminal_exes
            .iter()
            .chain(&self.agent_host_exes)
            .any(|e| e.eq_ignore_ascii_case(exe))
    }
    pub fn is_communication(&self, exe: &str) -> bool {
        self.communication_exes
            .iter()
            .any(|e| e.eq_ignore_ascii_case(exe))
    }
    pub fn is_meeting_app(&self, exe: &str) -> bool {
        self.meeting_exes.iter().any(|e| e.eq_ignore_ascii_case(exe))
    }
    /// 無視するアプリの exe か（`ignore_exes`）。
    pub fn is_ignored(&self, exe: &str) -> bool {
        self.ignore_exes.iter().any(|e| e.eq_ignore_ascii_case(exe))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Agent {
    pub enabled: Vec<String>,
    /// 0 でプロンプト抜粋を保存しない。
    pub prompt_excerpt_chars: usize,
    pub wait_max_min: u64,
    /// prompt_submit 時に前面がターミナル以外だった場合に遡る秒数（docs/dev/architecture.md「割り当てパイプライン」）。
    pub bind_lookback_sec: u64,
    /// true なら、`prompt_submit` 時の前面が一覧（`terminal_exes` / `agent_host_exes`）に無いアプリでも、
    /// 除外・通過系・Hitoiki 自身・ブラウザ・コミュニケーション/会議系でなければそのウインドウにいたとみなして対応付ける。
    pub bind_unlisted: bool,
}

impl Default for Agent {
    fn default() -> Self {
        Self {
            enabled: vec!["claude-code".into(), "codex".into()],
            prompt_excerpt_chars: 200,
            wait_max_min: 10,
            bind_lookback_sec: 5,
            bind_unlisted: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Nudge {
    pub enabled: bool,
    pub daily_limit: u32,
    pub min_interval_min: u64,
    pub day_start_after: String,
    pub day_end_at: String,
    pub weekly: Weekly,
    pub long_work_min: u64,
    pub switch_spike: SwitchSpike,
    pub late_work_after: String,
    /// idle_return: 予定外の無操作がこの分数以上続いたあとの復帰で「さっきの N 分は？」と尋ねる。
    /// 0 で無効（`disabled_triggers` に `idle_return` を入れても止められる）。
    pub idle_return_min: u64,
    /// urgent_task: 緊急のしるしが付いた未完了のタスクがあるとき、前回の通知からこの分数以上空けて通知する。
    /// 0 で無効。
    pub urgent_task_interval_min: u64,
    /// important_task: 重要（緊急でない）な未完了のタスクに、最後に手をつけてからこの日数以上経っているとき、
    /// 1 日 1 回まで通知する。0 で無効。
    pub important_stale_days: u64,
    /// key_result_idle: 今週まだ時間が入っていない成果の指標を知らせる曜日（1=月 2=火 3=水 4=木 5=金 6=土 7=日）。
    /// その曜日以降、週に 1 回まで通知する。0（または 8 以上）で無効。
    pub key_result_idle_weekday: u32,
    /// 無効にするトリガー ID。
    pub disabled_triggers: Vec<String>,
    /// 回答を求めるトースト（ボタンや入力欄のあるもの）を、操作するまで画面に残す（Windows の
    /// `scenario="reminder"`）。false なら一定時間で自動的に消える従来の表示。
    pub toast_persistent: bool,
}

impl Default for Nudge {
    fn default() -> Self {
        Self {
            enabled: true,
            daily_limit: 6,
            min_interval_min: 60,
            day_start_after: "08:00".into(),
            day_end_at: "18:30".into(),
            weekly: Weekly::default(),
            long_work_min: 90,
            switch_spike: SwitchSpike::default(),
            late_work_after: "20:00".into(),
            idle_return_min: 15,
            urgent_task_interval_min: 120,
            important_stale_days: 3,
            key_result_idle_weekday: 3,
            disabled_triggers: vec![],
            toast_persistent: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Weekly {
    pub weekday: String,
    pub at: String,
}

impl Default for Weekly {
    fn default() -> Self {
        Self {
            weekday: "fri".into(),
            at: "17:00".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SwitchSpike {
    pub window_min: u64,
    pub count: u32,
}

impl Default for SwitchSpike {
    fn default() -> Self {
        Self {
            window_min: 15,
            count: 20,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Activity {
    pub labels: Vec<String>,
}

impl Default for Activity {
    fn default() -> Self {
        Self {
            labels: [
                "開発",
                "レビュー",
                "調査",
                "設計",
                "コミュニケーション",
                "会議",
                "事務",
                "学習",
            ]
            .map(String::from)
            .to_vec(),
        }
    }
}

/// 実績（要確認の一覧・補正）の設定。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Actual {
    /// 要確認の一覧で既定表示する最小の長さ（秒）。これ未満の区間は既定で隠す（データは変えない）。
    pub min_sec: u64,
    /// 画面ロック／スリープによる無操作がこの分数以上なら、自動で休憩（break）にする。0 で無効。
    pub auto_break_locked_min: u64,
    /// true なら、進行中（doing）のタスクを仮当てした区間も要確認にせず自動（auto）にする。
    /// false（既定）なら従来どおり要確認にする。
    pub trust_doing: bool,
    /// 同じ exe の区間を同じタスクにこの回数以上確定したら、ルール化を提案する。0 で提案しない。
    pub rule_suggest_min_count: u32,
    /// ルール化の提案を断ったあと、同じ組み合わせを再び提案しない日数。
    pub rule_suggest_snooze_days: u32,
    /// 実績の補正で、範囲の端をこの秒数以内の区間の境目に吸着させる。0 で吸着しない。
    pub snap_sec: u64,
    /// 実績の補正で、ユーザー確定済みの区間を最初からチェックする。
    pub correct_include_confirmed: bool,
    /// 実績の補正で、会議（カレンダー由来）の区間を最初からチェックする。
    pub correct_include_meeting: bool,
    /// 実績の補正で、無操作・記録なしの区間を最初からチェックする（PC 外の作業として計上する）。
    pub correct_include_idle: bool,
}

impl Default for Actual {
    fn default() -> Self {
        Self {
            min_sec: 60,
            auto_break_locked_min: 10,
            trust_doing: false,
            rule_suggest_min_count: 3,
            rule_suggest_snooze_days: 14,
            snap_sec: 120,
            correct_include_confirmed: false,
            correct_include_meeting: false,
            correct_include_idle: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Judge {
    pub enabled: bool,
    pub default: String,
    pub auto_confidence_threshold: f64,
    pub backends: BTreeMap<String, JudgeBackend>,
}

impl Default for Judge {
    fn default() -> Self {
        Self {
            enabled: false,
            default: "aigw".into(),
            auto_confidence_threshold: 0.8,
            backends: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum JudgeBackend {
    OpenaiCompatible {
        base_url: String,
        model: String,
        #[serde(default)]
        api_key_env: Option<String>,
        #[serde(default = "default_judge_timeout")]
        timeout_sec: u64,
    },
    ExternalCommand {
        command: Vec<String>,
        #[serde(default = "default_judge_timeout")]
        timeout_sec: u64,
        /// true なら複数区間を `states` 配列でまとめて送る（docs/dev/architecture.md「自動判定」のバッチモード）。
        #[serde(default)]
        batch: bool,
    },
}

fn default_judge_timeout() -> u64 {
    10
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Ai {
    pub command: Vec<String>,
    /// 使うモデル名。未指定なら `command` の先頭が `claude` のときだけ [`DEFAULT_CLAUDE_MODEL`] を使い、
    /// それ以外のコマンドには何も付けない。空文字（または空白だけ）ならモデルの引数を付けず、AI コマンド側の既定に任せる。
    pub model: Option<String>,
    /// `model` を指定したときに `command` の後ろへ付ける引数。空なら `["--model", "{model}"]`
    /// （`claude` と `codex` はどちらも `--model` を受け付ける）。`{model}` をモデル名に置換する。
    pub model_args: Vec<String>,
    pub timeout_sec: u64,
    /// 未指定なら `paths::templates_dir()`。
    pub templates_dir: Option<String>,
    /// 構造化出力（問いを JSON で受け取り、画面で種類ごとに答える）を使うか。
    pub structured: AiStructured,
    /// 構造化出力のときに `command` の後ろへ付ける引数。空なら `claude` 用の既定
    /// （`--output-format json --json-schema {schema}`）。`{schema}`（JSON Schema 文字列）と
    /// `{schema_file}`（一時ファイルのパス。実行後に削除）を置換する。
    pub structured_args: Vec<String>,
    /// 振り返りを生成する前に、その期間の記録の ERG を AI で判定するか（手動の `hitoiki erg classify` は常に使える）。
    pub erg_classify: bool,
    /// ERG の AI 判定を採用する確信度の下限（0〜1）。未満は「確認待ち」として提案だけ保存する。
    pub erg_auto_threshold: f64,
}

impl Default for Ai {
    fn default() -> Self {
        Self {
            command: vec!["claude".into(), "-p".into()],
            model: None,
            model_args: Vec::new(),
            timeout_sec: 180,
            templates_dir: None,
            structured: AiStructured::Auto,
            structured_args: Vec::new(),
            erg_classify: true,
            erg_auto_threshold: 0.8,
        }
    }
}

/// `ai.model` が未指定で、`ai.command` が `claude` のときに使うモデル。
pub const DEFAULT_CLAUDE_MODEL: &str = "haiku";

/// `command` の先頭が claude か（パス末尾のファイル名、拡張子を除いて判定）。
pub fn is_claude(command: &[String]) -> bool {
    command.first().is_some_and(|c| {
        let name = c.rsplit(['/', '\\']).next().unwrap_or(c);
        let stem = name.split('.').next().unwrap_or(name);
        stem.eq_ignore_ascii_case("claude")
    })
}

impl Ai {
    /// 実際に使うモデル名。未指定なら `claude` のときだけ既定のモデル、空白だけなら None。
    pub fn effective_model(&self) -> Option<&str> {
        match self.model.as_deref() {
            Some(m) => Some(m.trim()).filter(|m| !m.is_empty()),
            None => is_claude(&self.command).then_some(DEFAULT_CLAUDE_MODEL),
        }
    }

    /// AI コマンドの argv（`command` + モデルの引数）。構造化出力の引数はこの後ろに足す。
    /// 使うモデルが無ければ（[`Ai::effective_model`]）`command` のまま。
    pub fn argv(&self) -> Vec<String> {
        let mut argv = self.command.clone();
        let Some(model) = self.effective_model() else {
            return argv;
        };
        if self.model_args.is_empty() {
            argv.push("--model".into());
            argv.push(model.to_string());
        } else {
            argv.extend(self.model_args.iter().map(|a| a.replace("{model}", model)));
        }
        argv
    }
}

/// `ai.structured` の値。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum AiStructured {
    /// `ai.command` の先頭が `claude` のときだけ構造化する。
    #[default]
    Auto,
    /// 常に構造化を試みる（失敗したら Markdown にフォールバック）。
    On,
    /// 構造化しない（従来の Markdown）。
    Off,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Calendar {
    pub fetch_interval_min: u64,
    pub past_days: i64,
    pub future_days: i64,
    pub sources: Vec<CalendarSource>,
}

impl Default for Calendar {
    fn default() -> Self {
        Self {
            fetch_interval_min: 15,
            past_days: 7,
            future_days: 14,
            sources: vec![],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalendarSource {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
    /// 秘密情報。ログに出さないこと（docs/dev/architecture.md「カレンダー（ICS）」）。
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TaskConfig {
    /// `task start` 時に他の doing タスクを todo に戻す（false なら doing のまま残す）。
    pub single_doing: bool,
}

impl Default for TaskConfig {
    fn default() -> Self {
        Self { single_doing: true }
    }
}

impl Config {
    /// 既定の場所から読む。ファイルが無ければ既定値。
    pub fn load() -> Result<Self> {
        Self::load_from(&paths::config_file())
    }

    pub fn load_from(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let s = std::fs::read_to_string(path)?;
        toml::from_str(&s).map_err(|e| {
            Error::InvalidArgument(format!("設定ファイルを解釈できません ({}): {e}", path.display()))
        })
    }

    /// 値の意味の検査（型では防げないもの）。`config set` が書き込む前に呼ぶ。
    pub fn validate(&self) -> Result<()> {
        crate::shortcut::parse(&self.general.quick_entry_shortcut)?;
        Ok(())
    }

    pub fn tz(&self) -> Result<chrono_tz::Tz> {
        crate::timefmt::parse_tz(&self.general.timezone)
    }

    /// ドット区切りのキーで値を取得する（`config get`）。
    pub fn get_value(&self, key: &str) -> Result<toml::Value> {
        let root = toml::Value::try_from(self).map_err(anyhow::Error::from)?;
        let mut cur = &root;
        for part in key.split('.') {
            cur = cur
                .get(part)
                .ok_or_else(|| Error::NotFound(format!("設定キー: {key}")))?;
        }
        Ok(cur.clone())
    }
}

/// 設定ファイルの 1 キーを書き換える（`config set`）。値は TOML として解釈し、
/// 解釈できなければ文字列として扱う。書き換え後に全体を再検証する。
pub fn set_value(path: &Path, key: &str, raw: &str) -> Result<()> {
    let mut doc: toml::Table = if path.exists() {
        toml::from_str(&std::fs::read_to_string(path)?)
            .map_err(|e| Error::InvalidArgument(format!("設定ファイルを解釈できません: {e}")))?
    } else {
        toml::Table::new()
    };
    let value: toml::Value = toml::from_str::<toml::Table>(&format!("v = {raw}"))
        .ok()
        .and_then(|mut t| t.remove("v"))
        .unwrap_or_else(|| toml::Value::String(raw.to_string()));
    let parts: Vec<&str> = key.split('.').collect();
    let (last, parents) = parts
        .split_last()
        .ok_or_else(|| Error::InvalidArgument("空のキー".into()))?;
    let mut cur = &mut doc;
    for p in parents {
        cur = cur
            .entry(p.to_string())
            .or_insert_with(|| toml::Value::Table(toml::Table::new()))
            .as_table_mut()
            .ok_or_else(|| Error::InvalidArgument(format!("{p} はテーブルではありません")))?;
    }
    cur.insert(last.to_string(), value);
    let text = toml::to_string_pretty(&doc).map_err(anyhow::Error::from)?;
    let cfg = toml::from_str::<Config>(&text)
        .map_err(|e| Error::InvalidArgument(format!("設定値が不正です: {e}")))?;
    cfg.validate()?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, text)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_roundtrip() {
        let c = Config::default();
        let s = toml::to_string(&c).unwrap();
        let c2: Config = toml::from_str(&s).unwrap();
        assert_eq!(c2.collect.idle_threshold_sec, 120);
    }

    #[test]
    fn update_check_は未設定_true_false_を区別する() {
        assert_eq!(Config::default().general.update_check, None);
        let c: Config = toml::from_str(
            "[general]
timezone = \"UTC\"
",
        )
        .unwrap();
        assert_eq!(c.general.update_check, None);
        let c: Config = toml::from_str(
            "[general]
update_check = true
",
        )
        .unwrap();
        assert_eq!(c.general.update_check, Some(true));
        let c: Config = toml::from_str(
            "[general]
update_check = false
",
        )
        .unwrap();
        assert_eq!(c.general.update_check, Some(false));
        // 未設定のまま書き出しても読み戻せる（TOML は None を省く）。
        let s = toml::to_string(&Config::default()).unwrap();
        assert_eq!(toml::from_str::<Config>(&s).unwrap().general.update_check, None);
    }

    #[test]
    fn ai_argv_はモデルの引数を_command_の後ろに足す() {
        let mut ai = Ai::default();
        let base: Vec<String> = vec!["claude".into(), "-p".into()];
        // 未指定なら claude には既定のモデルを付ける
        assert_eq!(ai.argv(), ["claude", "-p", "--model", DEFAULT_CLAUDE_MODEL]);
        ai.model = Some("  ".into());
        assert_eq!(ai.argv(), base);
        ai.model = Some(" sonnet ".into());
        assert_eq!(ai.argv(), ["claude", "-p", "--model", "sonnet"]);
        ai.model_args = vec!["-m".into(), "x-{model}".into()];
        assert_eq!(ai.argv(), ["claude", "-p", "-m", "x-sonnet"]);
        // 未指定なら claude には既定のモデルを model_args の形で付ける
        ai.model = None;
        assert_eq!(ai.argv(), ["claude", "-p", "-m", "x-haiku"]);
        // claude 以外のコマンドで未指定なら何も付けない
        ai.command = vec!["codex".into(), "exec".into()];
        assert_eq!(ai.argv(), ["codex", "exec"]);
        // 空文字ならモデルの引数を付けない（claude でも）
        ai.command = base.clone();
        ai.model = Some(String::new());
        assert_eq!(ai.argv(), base);
        // 既存の設定ファイル（model なし）も読める
        let c: Config = toml::from_str("[ai]\ntimeout_sec = 5\n").unwrap();
        assert!(c.ai.model.is_none() && c.ai.model_args.is_empty());
    }

    #[test]
    fn クイック入力のショートカットは既定値があり不正な値は保存できない() {
        assert_eq!(Config::default().general.quick_entry_shortcut, "Ctrl+Alt+Space");
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("config.toml");
        set_value(&p, "general.quick_entry_shortcut", "\"Ctrl+Shift+K\"").unwrap();
        assert_eq!(
            Config::load_from(&p).unwrap().general.quick_entry_shortcut,
            "Ctrl+Shift+K"
        );
        // 空文字は無効化として保存できる。
        set_value(&p, "general.quick_entry_shortcut", "\"\"").unwrap();
        // 解釈できない値はエラーで、ファイルは変わらない。
        for bad in ["\"Space\"", "\"Ctrl+Nope\"", "\"Ctrl+\"", "5"] {
            assert!(
                set_value(&p, "general.quick_entry_shortcut", bad).is_err(),
                "{bad}"
            );
        }
        assert_eq!(Config::load_from(&p).unwrap().general.quick_entry_shortcut, "");
    }

    #[test]
    fn partial_config() {
        let c: Config = toml::from_str(
            r#"
[collect]
idle_threshold_sec = 60
[[collect.exclude]]
exe = "Foo.exe"
[judge.backends.custom]
type = "external-command"
command = ["python", "x.py"]
"#,
        )
        .unwrap();
        assert_eq!(c.collect.idle_threshold_sec, 60);
        assert_eq!(c.collect.merge_gap_sec, 30);
        assert!(c.collect.is_excluded(Some("foo.exe"), None));
        assert!(matches!(
            c.judge.backends["custom"],
            JudgeBackend::ExternalCommand { .. }
        ));
    }

    #[test]
    fn exclude_title() {
        let c = Config::default();
        assert!(c.collect.is_excluded(
            Some("msedge.exe"),
            Some("新しいタブ - [InPrivate] - Microsoft Edge")
        ));
        assert!(!c.collect.is_excluded(Some("msedge.exe"), Some("GitHub")));
    }

    #[test]
    fn set_and_get() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("config.toml");
        set_value(&p, "collect.idle_threshold_sec", "300").unwrap();
        set_value(&p, "general.timezone", "UTC").unwrap();
        let c = Config::load_from(&p).unwrap();
        assert_eq!(c.collect.idle_threshold_sec, 300);
        assert_eq!(c.get_value("general.timezone").unwrap().as_str(), Some("UTC"));
        assert!(set_value(&p, "collect.idle_threshold_sec", "\"abc\"").is_err());
    }

    #[test]
    fn actual_と_nudge_の新しいキーの既定値() {
        let c = Config::default();
        assert_eq!(c.actual.auto_break_locked_min, 10);
        assert!(!c.actual.trust_doing);
        assert_eq!(
            (c.actual.rule_suggest_min_count, c.actual.rule_suggest_snooze_days),
            (3, 14)
        );
        assert_eq!(c.nudge.idle_return_min, 15);
        assert_eq!(c.nudge.urgent_task_interval_min, 120);
        assert_eq!(c.nudge.important_stale_days, 3);
        assert_eq!(c.nudge.key_result_idle_weekday, 3);
        assert_eq!(c.actual.snap_sec, 120);
        assert!(
            !c.actual.correct_include_confirmed
                && !c.actual.correct_include_meeting
                && !c.actual.correct_include_idle
        );
        let c: Config = toml::from_str(
            "[actual]
trust_doing = true
auto_break_locked_min = 0
",
        )
        .unwrap();
        assert!(c.actual.trust_doing);
        assert_eq!((c.actual.auto_break_locked_min, c.actual.min_sec), (0, 60));
    }
}
