//! UI から受け取る入力の型。core のサービス入力型へ変換するだけで、ロジックは持たない。

use hitoiki_core::model::{MatchField, PatternType, SegmentKind, TaskKind, TaskStatus};
use hitoiki_core::service::okr::{KrInput, ObjectiveInput};
use hitoiki_core::service::plan::BlockInput;
use hitoiki_core::service::review::ReviewSetInput;
use hitoiki_core::service::skill::SkillInput;
use hitoiki_core::service::task::{RuleInput, TaskFilter};
use hitoiki_core::Millis;
use serde::Deserialize;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct TaskFilterDto {
    pub statuses: Vec<TaskStatus>,
    pub kind: Option<TaskKind>,
}

impl From<TaskFilterDto> for TaskFilter {
    fn from(f: TaskFilterDto) -> Self {
        TaskFilter {
            statuses: f.statuses,
            kind: f.kind,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct RuleInputDto {
    pub task: String,
    pub field: MatchField,
    pub pattern: String,
    #[serde(default)]
    pub pattern_type: Option<PatternType>,
    #[serde(default)]
    pub priority: i64,
}

impl From<RuleInputDto> for RuleInput {
    fn from(r: RuleInputDto) -> Self {
        RuleInput {
            task: r.task,
            field: r.field,
            pattern: r.pattern,
            pattern_type: r.pattern_type,
            priority: r.priority,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ReviewSetDto {
    pub segment: String,
    #[serde(default)]
    pub task: Option<String>,
    #[serde(default)]
    pub kind: Option<SegmentKind>,
    #[serde(default)]
    pub activity: Option<String>,
    #[serde(default)]
    pub rule: bool,
}

impl From<ReviewSetDto> for ReviewSetInput {
    fn from(r: ReviewSetDto) -> Self {
        ReviewSetInput {
            segment: r.segment,
            task: r.task,
            kind: r.kind,
            activity: r.activity,
            rule: r.rule,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct BlockInputDto {
    pub start: Millis,
    pub end: Millis,
    #[serde(default)]
    pub task: Option<String>,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub focus: bool,
}

impl From<BlockInputDto> for BlockInput {
    fn from(b: BlockInputDto) -> Self {
        BlockInput {
            start: b.start,
            end: b.end,
            task: b.task,
            label: b.label,
            focus: b.focus,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ObjectiveInputDto {
    pub title: String,
    #[serde(default)]
    pub from: Option<Millis>,
    #[serde(default)]
    pub to: Option<Millis>,
}

impl From<ObjectiveInputDto> for ObjectiveInput {
    fn from(o: ObjectiveInputDto) -> Self {
        ObjectiveInput {
            title: o.title,
            from: o.from,
            to: o.to,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct KrInputDto {
    pub objective: String,
    pub title: String,
    #[serde(default)]
    pub target: Option<String>,
}

impl From<KrInputDto> for KrInput {
    fn from(k: KrInputDto) -> Self {
        KrInput {
            objective: k.objective,
            title: k.title,
            target: k.target,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct SkillInputDto {
    pub name: String,
    #[serde(default)]
    pub parent: Option<String>,
}

impl From<SkillInputDto> for SkillInput {
    fn from(s: SkillInputDto) -> Self {
        SkillInput {
            name: s.name,
            parent: s.parent,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hitoiki_core::service::task::TaskInput;

    #[test]
    fn タスク入力は省略可能() {
        let t: TaskInput = serde_json::from_str(r#"{"title":"a","urgent":true}"#).unwrap();
        assert_eq!(t.title.as_deref(), Some("a"));
        assert_eq!(t.urgent, Some(true));
        assert!(t.skills.is_empty());
        assert!(!t.clear_skills);
    }

    #[test]
    fn 列挙はsnake_caseで受ける() {
        let r: ReviewSetDto =
            serde_json::from_str(r#"{"segment":"01","kind":"agent_dialog","rule":true}"#).unwrap();
        assert_eq!(r.kind, Some(SegmentKind::AgentDialog));
    }
}
