//! UI から受け取る入力の型。core のサービス入力型へ変換するだけで、ロジックは持たない。

use hitoiki_core::model::{ErgTag, MatchField, PatternType, SegmentKind, TaskKind, TaskStatus};
use hitoiki_core::service::okr::{KrInput, ObjectiveInput};
use hitoiki_core::service::plan::BlockInput;
use hitoiki_core::service::record::{AbcInput, CheckInput, HassleInput};
use hitoiki_core::service::review::ReviewSetInput;
use hitoiki_core::service::skill::SkillInput;
use hitoiki_core::service::task::{RuleInput, TaskFilter, TaskInput};
use hitoiki_core::service::RangeArgs;
use hitoiki_core::Millis;
use serde::Deserialize;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct RangeDto {
    /// `today` / `yesterday` / `this-week` / `last-week` / `YYYY-MM-DD`
    pub date: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
}

impl From<RangeDto> for RangeArgs {
    fn from(r: RangeDto) -> Self {
        RangeArgs {
            date: r.date,
            from: r.from,
            to: r.to,
        }
    }
}

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

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct TaskInputDto {
    pub title: Option<String>,
    pub body: Option<String>,
    pub urgent: Option<bool>,
    pub important: Option<bool>,
    pub estimate_min: Option<i64>,
    pub key_result: Option<String>,
    pub skills: Vec<String>,
    pub external_url: Option<String>,
    pub planned_start_at: Option<Millis>,
    pub planned_end_at: Option<Millis>,
    pub interrupt: bool,
}

impl From<TaskInputDto> for TaskInput {
    fn from(t: TaskInputDto) -> Self {
        TaskInput {
            title: t.title,
            body: t.body,
            urgent: t.urgent,
            important: t.important,
            estimate_min: t.estimate_min,
            key_result: t.key_result,
            skills: t.skills,
            external_url: t.external_url,
            planned_start_at: t.planned_start_at,
            planned_end_at: t.planned_end_at,
            interrupt: t.interrupt,
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

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct CheckInputDto {
    pub energy: Option<i64>,
    pub mood: Option<i64>,
    pub stress: Option<i64>,
    pub erg: Option<ErgTag>,
    pub note: Option<String>,
}

impl From<CheckInputDto> for CheckInput {
    fn from(c: CheckInputDto) -> Self {
        CheckInput {
            energy: c.energy,
            mood: c.mood,
            stress: c.stress,
            erg: c.erg,
            note: c.note,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct HassleInputDto {
    pub text: String,
    #[serde(default)]
    pub intensity: Option<i64>,
}

impl From<HassleInputDto> for HassleInput {
    fn from(h: HassleInputDto) -> Self {
        HassleInput {
            text: h.text,
            intensity: h.intensity,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct AbcInputDto {
    pub activating_event: Option<String>,
    pub belief: Option<String>,
    pub consequence_emotion: Option<String>,
    pub consequence_intensity: Option<i64>,
    pub from_hassle: Option<String>,
}

impl From<AbcInputDto> for AbcInput {
    fn from(a: AbcInputDto) -> Self {
        AbcInput {
            activating_event: a.activating_event,
            belief: a.belief,
            consequence_emotion: a.consequence_emotion,
            consequence_intensity: a.consequence_intensity,
            from_hassle: a.from_hassle,
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

    #[test]
    fn タスク入力は省略可能() {
        let t: TaskInputDto = serde_json::from_str(r#"{"title":"a","urgent":true}"#).unwrap();
        let t: TaskInput = t.into();
        assert_eq!(t.title.as_deref(), Some("a"));
        assert_eq!(t.urgent, Some(true));
        assert!(t.skills.is_empty());
    }

    #[test]
    fn 列挙はsnake_caseで受ける() {
        let r: ReviewSetDto =
            serde_json::from_str(r#"{"segment":"01","kind":"agent_dialog","rule":true}"#).unwrap();
        assert_eq!(r.kind, Some(SegmentKind::AgentDialog));
    }
}
