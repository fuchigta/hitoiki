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
    pub push: Push,
    pub activity: Activity,
    pub judge: Judge,
    pub ai: Ai,
    pub calendar: Calendar,
    pub task: TaskConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct General {
    pub timezone: String,
    pub autostart: bool,
    /// 新しいバージョンの確認（1 日 1 回、GitHub に問い合わせる）。
    /// None（未設定）はまだ尋ねていない状態で、確認しない（オプトイン）。true / false は利用者の選択。
    pub update_check: Option<bool>,
}

impl Default for General {
    fn default() -> Self {
        Self {
            timezone: "Asia/Tokyo".into(),
            autostart: true,
            update_check: None,
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
    pub communication_exes: Vec<String>,
    pub meeting_exes: Vec<String>,
    /// 除外区間を丸ごと破棄する（false なら exe 名のみ保持。docs/dev/architecture.md「収集器（Windows）」）。
    pub drop_excluded: bool,
    pub exclude: Vec<ExcludeRule>,
    /// 割り込み判定: コミュニケーション系の後に元タスク外作業が続く最小時間（docs/dev/architecture.md「帰属判定パイプライン」）。
    pub interrupt_min_sec: u64,
    /// 帰属判定の定期実行で再計算する直近の範囲（分）。
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
            communication_exes: ["ms-teams.exe", "Slack.exe", "olk.exe", "OUTLOOK.EXE"]
                .map(String::from)
                .to_vec(),
            meeting_exes: ["ms-teams.exe", "Zoom.exe"].map(String::from).to_vec(),
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
    pub fn is_terminal(&self, exe: &str) -> bool {
        self.terminal_exes.iter().any(|e| e.eq_ignore_ascii_case(exe))
    }
    pub fn is_communication(&self, exe: &str) -> bool {
        self.communication_exes
            .iter()
            .any(|e| e.eq_ignore_ascii_case(exe))
    }
    pub fn is_meeting_app(&self, exe: &str) -> bool {
        self.meeting_exes.iter().any(|e| e.eq_ignore_ascii_case(exe))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Agent {
    pub enabled: Vec<String>,
    /// 0 でプロンプト抜粋を保存しない。
    pub prompt_excerpt_chars: usize,
    pub watch_max_min: u64,
    /// prompt_submit 時に前面がターミナル以外だった場合に遡る秒数（docs/dev/architecture.md「帰属判定パイプライン」）。
    pub bind_lookback_sec: u64,
}

impl Default for Agent {
    fn default() -> Self {
        Self {
            enabled: vec!["claude-code".into(), "codex".into()],
            prompt_excerpt_chars: 200,
            watch_max_min: 10,
            bind_lookback_sec: 5,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Push {
    pub enabled: bool,
    pub daily_limit: u32,
    pub min_interval_min: u64,
    pub day_start_after: String,
    pub day_end_at: String,
    pub weekly: Weekly,
    pub long_focus_min: u64,
    pub switch_spike: SwitchSpike,
    pub late_work_after: String,
    /// 無効にするトリガー ID。
    pub disabled_triggers: Vec<String>,
}

impl Default for Push {
    fn default() -> Self {
        Self {
            enabled: true,
            daily_limit: 6,
            min_interval_min: 60,
            day_start_after: "08:00".into(),
            day_end_at: "18:30".into(),
            weekly: Weekly::default(),
            long_focus_min: 90,
            switch_spike: SwitchSpike::default(),
            late_work_after: "20:00".into(),
            disabled_triggers: vec![],
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
        /// true なら複数区間を `states` 配列でまとめて送る（docs/dev/architecture.md「判定器」のバッチモード）。
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
    pub timeout_sec: u64,
    /// 未指定なら `paths::templates_dir()`。
    pub templates_dir: Option<String>,
}

impl Default for Ai {
    fn default() -> Self {
        Self {
            command: vec!["claude".into(), "-p".into()],
            timeout_sec: 180,
            templates_dir: None,
        }
    }
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
    toml::from_str::<Config>(&text).map_err(|e| Error::InvalidArgument(format!("設定値が不正です: {e}")))?;
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
}
