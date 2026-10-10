//! ルールのパターンマッチ（7.1-5）。glob / regex / contains を、パス区切り（`\` と `/`）と
//! 大文字小文字を無視して評価する。

use crate::model::{ActivityRule, MatchField, PatternType, TaskMatchRule};
use crate::{Error, Result};
use globset::{GlobBuilder, GlobMatcher};
use regex::{Regex, RegexBuilder};
use std::path::Path;

/// ルールが参照する行動ログの値。
#[derive(Debug, Clone, Default)]
pub struct MatchTarget<'a> {
    pub exe: Option<&'a str>,
    pub title: Option<&'a str>,
    pub cwd: Option<&'a str>,
    /// cwd から推定した git リポジトリのルートパス（`/` 区切り）。
    pub repo: Option<String>,
    pub url: Option<&'a str>,
    pub git_branch: Option<&'a str>,
}

impl<'a> MatchTarget<'a> {
    /// 前面ウインドウ由来（exe / title のみ）。
    pub fn window(exe: Option<&'a str>, title: Option<&'a str>) -> Self {
        Self {
            exe,
            title,
            ..Self::default()
        }
    }

    /// エージェントセッション由来（cwd / git_branch。repo は cwd から推定）。
    pub fn session(cwd: Option<&'a str>, git_branch: Option<&'a str>) -> Self {
        Self {
            cwd,
            git_branch,
            repo: cwd.and_then(infer_repo),
            ..Self::default()
        }
    }

    /// 予定タイトル由来。
    pub fn title_only(title: &'a str) -> Self {
        Self::window(None, Some(title))
    }

    fn values(&self, field: MatchField) -> Vec<String> {
        match field {
            MatchField::Exe => self.exe.map(String::from).into_iter().collect(),
            MatchField::Title => self.title.map(String::from).into_iter().collect(),
            MatchField::Cwd => self.cwd.map(String::from).into_iter().collect(),
            MatchField::Url => self.url.map(String::from).into_iter().collect(),
            MatchField::GitBranch => self.git_branch.map(String::from).into_iter().collect(),
            MatchField::Repo => match &self.repo {
                Some(r) => {
                    let name = r.rsplit('/').next().unwrap_or(r).to_string();
                    vec![r.clone(), name]
                }
                None => vec![],
            },
        }
    }
}

/// cwd から上位へたどって `.git` を持つディレクトリを探し、そのパスを `/` 区切りで返す。
/// 見つからなければ None。
pub fn infer_repo(cwd: &str) -> Option<String> {
    let mut p = Path::new(cwd);
    loop {
        if p.join(".git").exists() {
            return Some(norm_sep(&p.to_string_lossy()));
        }
        p = p.parent()?;
    }
}

fn norm_sep(s: &str) -> String {
    s.replace('\\', "/")
}

enum Kind {
    Glob(Vec<GlobMatcher>),
    Regex(Regex),
    Contains(String),
}

/// コンパイル済みの 1 ルール。
pub struct Matcher {
    field: MatchField,
    kind: Kind,
}

impl Matcher {
    pub fn new(field: MatchField, pattern: &str, ptype: PatternType) -> Result<Self> {
        let kind = match ptype {
            PatternType::Glob => {
                let p = norm_sep(pattern);
                let mut pats = vec![p.clone()];
                // `dir/**` はディレクトリ自身にも一致させる。
                if let Some(base) = p.strip_suffix("/**") {
                    pats.push(base.to_string());
                }
                let mut ms = Vec::new();
                for p in pats {
                    let g = GlobBuilder::new(&p)
                        .case_insensitive(true)
                        .literal_separator(false)
                        .build()
                        .map_err(|e| Error::InvalidArgument(format!("glob が不正です: {pattern} ({e})")))?;
                    ms.push(g.compile_matcher());
                }
                Kind::Glob(ms)
            }
            PatternType::Regex => Kind::Regex(
                RegexBuilder::new(pattern)
                    .case_insensitive(true)
                    .build()
                    .map_err(|e| Error::InvalidArgument(format!("正規表現が不正です: {pattern} ({e})")))?,
            ),
            PatternType::Contains => Kind::Contains(norm_sep(pattern).to_lowercase()),
        };
        Ok(Self { field, kind })
    }

    pub fn is_match(&self, t: &MatchTarget<'_>) -> bool {
        t.values(self.field).iter().any(|v| {
            let n = norm_sep(v);
            match &self.kind {
                Kind::Glob(ms) => ms.iter().any(|m| m.is_match(&n)),
                Kind::Regex(re) => re.is_match(v) || re.is_match(&n),
                Kind::Contains(s) => n.to_lowercase().contains(s.as_str()),
            }
        })
    }
}

/// priority 降順（同順位は id 順）に並べた task_match_rule 群。不正なパターンは無視する。
pub struct TaskRuleSet {
    rules: Vec<(Matcher, String)>,
}

impl TaskRuleSet {
    pub fn new(rules: &[TaskMatchRule]) -> Self {
        let mut rs: Vec<&TaskMatchRule> = rules.iter().filter(|r| r.enabled).collect();
        rs.sort_by(|a, b| b.priority.cmp(&a.priority).then_with(|| a.id.cmp(&b.id)));
        let rules = rs
            .into_iter()
            .filter_map(|r| match Matcher::new(r.field, &r.pattern, r.pattern_type) {
                Ok(m) => Some((m, r.task_id.clone())),
                Err(e) => {
                    tracing::warn!("task_match_rule {} を無視します: {e}", r.id);
                    None
                }
            })
            .collect();
        Self { rules }
    }

    /// 最初に一致したルールのタスク ID。
    pub fn find(&self, t: &MatchTarget<'_>) -> Option<&str> {
        self.rules
            .iter()
            .find(|(m, _)| m.is_match(t))
            .map(|(_, id)| id.as_str())
    }
}

/// priority 降順（同順位は id 順）に並べた activity_rule 群。
pub struct ActivityRuleSet {
    rules: Vec<(Matcher, String)>,
}

impl ActivityRuleSet {
    pub fn new(rules: &[ActivityRule]) -> Self {
        let mut rs: Vec<&ActivityRule> = rules.iter().filter(|r| r.enabled).collect();
        rs.sort_by(|a, b| b.priority.cmp(&a.priority).then_with(|| a.id.cmp(&b.id)));
        let rules = rs
            .into_iter()
            .filter_map(|r| match Matcher::new(r.field, &r.pattern, r.pattern_type) {
                Ok(m) => Some((m, r.activity_label.clone())),
                Err(e) => {
                    tracing::warn!("activity_rule {} を無視します: {e}", r.id);
                    None
                }
            })
            .collect();
        Self { rules }
    }

    pub fn find(&self, t: &MatchTarget<'_>) -> Option<&str> {
        self.rules
            .iter()
            .find(|(m, _)| m.is_match(t))
            .map(|(_, l)| l.as_str())
    }
}

/// ウインドウタイトルの比較用正規化。先頭の記号（Claude Code のスピナー ✳ ⠂ ⠐ · * など）と空白を除く。
/// 帰属判定（セッションとウインドウの対応付け）と収集（区間の切り替え判定）で同じ規則を使う。
pub fn normalize_title(t: &str) -> String {
    t.trim_start_matches(|c: char| !c.is_alphanumeric())
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(field: MatchField, p: &str, t: PatternType) -> Matcher {
        Matcher::new(field, p, t).unwrap()
    }

    #[test]
    fn glob_path_separators_and_case() {
        let r = m(MatchField::Cwd, "C:/src/foo/**", PatternType::Glob);
        let t = MatchTarget::session(Some(r"C:\src\foo\bar"), None);
        assert!(r.is_match(&t));
        let t = MatchTarget::session(Some(r"c:\SRC\Foo\bar\baz"), None);
        assert!(r.is_match(&t));
        assert!(r.is_match(&MatchTarget::session(Some(r"C:\src\foo"), None)));
        assert!(!r.is_match(&MatchTarget::session(Some(r"C:\src\other"), None)));
        // パターン側がバックスラッシュでも同じ
        let r = m(MatchField::Cwd, r"C:\src\foo\**", PatternType::Glob);
        assert!(r.is_match(&MatchTarget::session(Some("C:/src/foo/x/y"), None)));
    }

    #[test]
    fn glob_exe() {
        let r = m(MatchField::Exe, "code*.exe", PatternType::Glob);
        assert!(r.is_match(&MatchTarget::window(Some("Code.exe"), None)));
        assert!(!r.is_match(&MatchTarget::window(Some("chrome.exe"), None)));
        assert!(!r.is_match(&MatchTarget::window(None, None)));
    }

    #[test]
    fn regex_and_contains() {
        let r = m(MatchField::Title, r"^issue #\d+", PatternType::Regex);
        assert!(r.is_match(&MatchTarget::window(None, Some("Issue #42 - GitHub"))));
        assert!(!r.is_match(&MatchTarget::window(None, Some("PR #42"))));
        let r = m(MatchField::Title, "Hitoiki", PatternType::Contains);
        assert!(r.is_match(&MatchTarget::window(None, Some("my HITOIKI notes"))));
        assert!(!r.is_match(&MatchTarget::window(None, Some("other"))));
        // 正規表現でパスをバックスラッシュで書いても、スラッシュで書いても一致する
        let r = m(MatchField::Cwd, r"src\\foo", PatternType::Regex);
        assert!(r.is_match(&MatchTarget::session(Some(r"C:\src\foo\x"), None)));
        let r = m(MatchField::Cwd, "src/foo", PatternType::Regex);
        assert!(r.is_match(&MatchTarget::session(Some(r"C:\src\foo\x"), None)));
    }

    #[test]
    fn repo_field_uses_name_or_path() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("myrepo");
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        std::fs::create_dir_all(repo.join("sub/dir")).unwrap();
        let cwd = repo.join("sub/dir");
        let t = MatchTarget::session(Some(cwd.to_str().unwrap()), None);
        assert!(m(MatchField::Repo, "myrepo", PatternType::Contains).is_match(&t));
        assert!(m(MatchField::Repo, "MYREPO", PatternType::Glob).is_match(&t));
        assert!(!m(MatchField::Repo, "other", PatternType::Contains).is_match(&t));
    }

    #[test]
    fn invalid_pattern_is_error() {
        assert!(Matcher::new(MatchField::Title, "(", PatternType::Regex).is_err());
        assert!(Matcher::new(MatchField::Title, "[", PatternType::Glob).is_err());
    }

    #[test]
    fn rule_set_priority_then_id() {
        let mk = |id: &str, task: &str, pri: i64| TaskMatchRule {
            id: id.into(),
            task_id: task.into(),
            field: MatchField::Title,
            pattern: "foo".into(),
            pattern_type: PatternType::Contains,
            priority: pri,
            enabled: true,
        };
        let set = TaskRuleSet::new(&[mk("b", "TB", 1), mk("a", "TA", 1), mk("c", "TC", 0)]);
        let t = MatchTarget::window(None, Some("foo"));
        assert_eq!(set.find(&t), Some("TA"));
        let set = TaskRuleSet::new(&[mk("a", "TA", 0), mk("z", "TZ", 5)]);
        assert_eq!(set.find(&t), Some("TZ"));
    }
}
