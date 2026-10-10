//! 質問セット（8.2）。定義の読み込みと、動的選択肢の解決。

use crate::protocol::{OptionSpec, QuestionSpec, QuestionType};
use anyhow::{bail, Context};
use serde::Deserialize;
use std::path::Path;

pub const DEFAULT_SET: &str = "segment_classification";

/// 動的選択肢の供給元。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OptionsFrom {
    OpenTasks,
    ActivityLabels,
    Skills,
}

/// TOML の選択肢。文字列（id = label）か `{id, label}`。
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum StaticOption {
    Plain(String),
    Full { id: String, label: Option<String> },
}

#[derive(Debug, Clone, Deserialize)]
pub struct QuestionDef {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: QuestionType,
    #[serde(default)]
    pub description: String,
    pub options_from: Option<OptionsFrom>,
    #[serde(default)]
    pub options: Vec<StaticOption>,
    #[serde(default)]
    pub allow_none: bool,
    pub scale: Option<[i64; 2]>,
}

#[derive(Debug, Clone, Deserialize)]
struct SetHeader {
    name: String,
    #[serde(default)]
    applies_to: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct SetFile {
    set: SetHeader,
    #[serde(default)]
    question: Vec<QuestionDef>,
}

#[derive(Debug, Clone)]
pub struct QuestionSet {
    pub name: String,
    pub questions: Vec<QuestionDef>,
}

/// 動的選択肢の実体。
#[derive(Debug, Clone, Default)]
pub struct OptionSources {
    pub open_tasks: Vec<OptionSpec>,
    pub activity_labels: Vec<OptionSpec>,
    pub skills: Vec<OptionSpec>,
}

impl QuestionSet {
    /// 組み込みの既定セット（task / activity / is_interrupt）。
    pub fn builtin() -> Self {
        Self {
            name: DEFAULT_SET.into(),
            questions: vec![
                QuestionDef {
                    name: "task".into(),
                    kind: QuestionType::Choice,
                    description: "この作業区間はどのタスクに該当するか".into(),
                    options_from: Some(OptionsFrom::OpenTasks),
                    options: vec![],
                    allow_none: true,
                    scale: None,
                },
                QuestionDef {
                    name: "activity".into(),
                    kind: QuestionType::Choice,
                    description: "この作業区間の活動の種類".into(),
                    options_from: Some(OptionsFrom::ActivityLabels),
                    options: vec![],
                    allow_none: false,
                    scale: None,
                },
                QuestionDef {
                    name: "is_interrupt".into(),
                    kind: QuestionType::Bool,
                    description: "計画外の割り込み対応か".into(),
                    options_from: None,
                    options: vec![],
                    allow_none: false,
                    scale: None,
                },
            ],
        }
    }

    pub fn parse(text: &str) -> anyhow::Result<Self> {
        let f: SetFile = toml::from_str(text).context("質問セットを解釈できません")?;
        for q in &f.question {
            validate(q)?;
        }
        Ok(Self {
            name: f.set.name,
            questions: f.question,
        })
    }

    /// `dir/*.toml` を読み、`segment_classification` を優先して返す。
    /// ディレクトリが無い・有効なセットが無い場合は組み込み。壊れたファイルは警告して無視。
    pub fn load_dir(dir: &Path) -> Self {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return Self::builtin();
        };
        let mut paths: Vec<_> = rd
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "toml"))
            .collect();
        paths.sort();
        let mut found: Vec<(Self, bool)> = vec![];
        for p in paths {
            let parsed = std::fs::read_to_string(&p)
                .map_err(anyhow::Error::from)
                .and_then(|t| {
                    let f: SetFile = toml::from_str(&t)?;
                    let applies = f
                        .set
                        .applies_to
                        .as_deref()
                        .is_none_or(|a| a == "activity_segment");
                    for q in &f.question {
                        validate(q)?;
                    }
                    Ok((
                        Self {
                            name: f.set.name,
                            questions: f.question,
                        },
                        applies,
                    ))
                });
            match parsed {
                Ok(v) => found.push(v),
                Err(e) => tracing::warn!("質問セットを読めません ({}): {e:#}", p.display()),
            }
        }
        let pick = found
            .iter()
            .position(|(s, a)| *a && s.name == DEFAULT_SET)
            .or_else(|| found.iter().position(|(s, a)| *a && !s.questions.is_empty()));
        match pick {
            Some(i) => found.swap_remove(i).0,
            None => Self::builtin(),
        }
    }

    /// 動的選択肢を解決して送信用の問いにする。選択肢が 0 件の choice は除外する。
    pub fn resolve(&self, src: &OptionSources) -> Vec<QuestionSpec> {
        self.questions
            .iter()
            .filter_map(|q| {
                let options = match q.kind {
                    QuestionType::Choice => {
                        let mut v: Vec<OptionSpec> = match q.options_from {
                            Some(OptionsFrom::OpenTasks) => src.open_tasks.clone(),
                            Some(OptionsFrom::ActivityLabels) => src.activity_labels.clone(),
                            Some(OptionsFrom::Skills) => src.skills.clone(),
                            None => vec![],
                        };
                        for o in &q.options {
                            v.push(match o {
                                StaticOption::Plain(s) => OptionSpec {
                                    id: s.clone(),
                                    label: s.clone(),
                                },
                                StaticOption::Full { id, label } => OptionSpec {
                                    id: id.clone(),
                                    label: label.clone().unwrap_or_else(|| id.clone()),
                                },
                            });
                        }
                        if v.is_empty() {
                            return None;
                        }
                        Some(v)
                    }
                    _ => None,
                };
                Some(QuestionSpec {
                    name: q.name.clone(),
                    kind: q.kind,
                    description: q.description.clone(),
                    options,
                    allow_none: q.allow_none && q.kind == QuestionType::Choice,
                    scale: q.scale,
                })
            })
            .collect()
    }
}

fn validate(q: &QuestionDef) -> anyhow::Result<()> {
    match q.kind {
        QuestionType::Score => match q.scale {
            Some([lo, hi]) if lo < hi => {}
            _ => bail!("score 質問 {} には scale = [min, max] が必要です", q.name),
        },
        QuestionType::Choice if q.options_from.is_none() && q.options.is_empty() => {
            bail!("choice 質問 {} には選択肢がありません", q.name)
        }
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
[set]
name = "segment_classification"
applies_to = "activity_segment"

[[question]]
name = "task"
type = "choice"
description = "d"
options_from = "open_tasks"
allow_none = true

[[question]]
name = "mood"
type = "choice"
options = ["a", { id = "b", label = "B" }]

[[question]]
name = "urgency"
type = "score"
scale = [1, 5]
"#;

    fn src() -> OptionSources {
        OptionSources {
            open_tasks: vec![OptionSpec {
                id: "T1".into(),
                label: "仕事".into(),
            }],
            ..Default::default()
        }
    }

    #[test]
    fn parse_and_resolve() {
        let s = QuestionSet::parse(SAMPLE).unwrap();
        let qs = s.resolve(&src());
        assert_eq!(qs.len(), 3);
        assert!(qs[0].allow_none);
        assert_eq!(qs[0].options.as_ref().unwrap()[0].id, "T1");
        assert_eq!(qs[1].options.as_ref().unwrap()[1].label, "B");
        assert_eq!(qs[2].scale, Some([1, 5]));
    }

    #[test]
    fn empty_options_dropped() {
        let qs = QuestionSet::builtin().resolve(&OptionSources::default());
        // task / activity は選択肢なしで除外され、is_interrupt だけ残る
        assert_eq!(qs.len(), 1);
        assert_eq!(qs[0].name, "is_interrupt");
    }

    #[test]
    fn invalid_rejected() {
        assert!(QuestionSet::parse("[set]\nname='x'\n[[question]]\nname='s'\ntype='score'\n").is_err());
    }

    #[test]
    fn load_dir_fallback_and_pick() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(QuestionSet::load_dir(d.path()).name, DEFAULT_SET);
        std::fs::write(d.path().join("a.toml"), "broken = [").unwrap();
        std::fs::write(
            d.path().join("b.toml"),
            SAMPLE.replace("segment_classification", "mine"),
        )
        .unwrap();
        let s = QuestionSet::load_dir(d.path());
        assert_eq!(s.name, "mine");
        assert_eq!(s.questions.len(), 3);
        let missing = QuestionSet::load_dir(&d.path().join("nope"));
        assert_eq!(missing.name, DEFAULT_SET);
    }
}
