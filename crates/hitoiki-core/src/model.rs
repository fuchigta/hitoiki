//! ドメインモデル（docs/dev/architecture.md「データモデル」）。DB の行と 1:1 に対応する構造体と、文字列で保存する列挙。

use crate::{Error, Millis};
use serde::{Deserialize, Serialize};

/// DB には snake_case の文字列で保存する列挙を定義するマクロ。
macro_rules! str_enum {
    ($(#[$m:meta])* $name:ident { $($var:ident => $s:literal),+ $(,)? }) => {
        $(#[$m])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(rename_all = "snake_case")]
        pub enum $name { $($var),+ }

        impl $name {
            pub const ALL: &'static [$name] = &[$($name::$var),+];
            pub fn as_str(&self) -> &'static str {
                match self { $($name::$var => $s),+ }
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl std::str::FromStr for $name {
            type Err = Error;
            fn from_str(s: &str) -> Result<Self, Error> {
                match s {
                    $($s => Ok($name::$var),)+
                    _ => Err(Error::InvalidArgument(format!(
                        concat!(stringify!($name), " の値が不正です: {} (候補: {})"),
                        s,
                        [$($s),+].join(", ")
                    ))),
                }
            }
        }

        impl rusqlite::types::ToSql for $name {
            fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
                Ok(self.as_str().into())
            }
        }

        impl rusqlite::types::FromSql for $name {
            fn column_result(v: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
                let s = v.as_str()?;
                s.parse().map_err(|e: Error| rusqlite::types::FromSqlError::Other(Box::new(e)))
            }
        }
    };
}

str_enum!(
    /// 無操作の理由。
    IdleReason { NoInput => "no_input", Locked => "locked", Sleep => "sleep" }
);

str_enum!(
    AgentEventKind {
        SessionStart => "session_start",
        PromptSubmit => "prompt_submit",
        ResponseComplete => "response_complete",
        SessionEnd => "session_end",
    }
);

str_enum!(
    SegmentKind {
        Work => "work",
        AgentDialog => "agent_dialog",
        AgentWatch => "agent_watch",
        Meeting => "meeting",
        Idle => "idle",
        Break => "break",
        Interrupt => "interrupt",
        Ignored => "ignored",
    }
);

str_enum!(
    SegmentStatus { Auto => "auto", NeedsReview => "needs_review", Confirmed => "confirmed" }
);

str_enum!(
    TaskStatus { Todo => "todo", Doing => "doing", Done => "done", Dropped => "dropped" }
);

str_enum!(
    TaskKind { Normal => "normal", Interrupt => "interrupt" }
);

str_enum!(
    /// ルールが参照する行動ログのフィールド。
    MatchField {
        Exe => "exe",
        Title => "title",
        Cwd => "cwd",
        Repo => "repo",
        Url => "url",
        GitBranch => "git_branch",
    }
);

str_enum!(
    PatternType { Glob => "glob", Regex => "regex", Contains => "contains" }
);

str_enum!(
    ReflectionKind { DayStart => "day_start", DayEnd => "day_end", Weekly => "weekly" }
);

str_enum!(
    ErgTag { Existence => "existence", Relatedness => "relatedness", Growth => "growth" }
);

// ===== 生データ層 =====

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WindowSpan {
    pub id: String,
    pub start_at: Millis,
    /// None = 進行中。
    pub end_at: Option<Millis>,
    pub hwnd: Option<String>,
    pub pid: Option<i64>,
    pub exe_path: Option<String>,
    pub exe_name: Option<String>,
    pub title: Option<String>,
    pub excluded: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IdleSpan {
    pub id: String,
    pub start_at: Millis,
    pub end_at: Option<Millis>,
    pub reason: IdleReason,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentEvent {
    pub id: String,
    pub at: Millis,
    pub agent: String,
    pub session_id: String,
    pub kind: AgentEventKind,
    pub cwd: Option<String>,
    pub git_branch: Option<String>,
    pub prompt_excerpt: Option<String>,
    pub source_file: Option<String>,
    pub source_offset: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentSessionWindow {
    pub session_id: String,
    pub agent: String,
    pub hwnd: String,
    pub title_at_bind: Option<String>,
    pub bound_at: Millis,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContextHint {
    pub id: String,
    pub at: Millis,
    pub source: String,
    pub hwnd: Option<String>,
    pub kind: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CalendarEvent {
    pub id: String,
    pub source: String,
    pub external_id: String,
    pub start_at: Millis,
    pub end_at: Millis,
    pub all_day: bool,
    pub title: String,
    pub location: Option<String>,
    pub is_busy: bool,
    pub fetched_at: Millis,
}

// ===== 派生層 =====

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActivitySegment {
    pub id: String,
    pub start_at: Millis,
    pub end_at: Millis,
    pub kind: SegmentKind,
    pub task_id: Option<String>,
    pub activity_label: Option<String>,
    pub confidence: Option<f64>,
    /// 'rule' | 'judge:<name>' | 'calendar' | 'user' | 'doing' | 'agent'
    pub decided_by: Option<String>,
    pub status: SegmentStatus,
    pub window_span_ids: Vec<String>,
    pub agent_session_id: Option<String>,
    pub exe_name: Option<String>,
    pub title: Option<String>,
    /// PC 外で過ごした時間（実績の補正で無操作・記録なしの範囲を割り当てたもの）。
    #[serde(default)]
    pub offline: bool,
}

// ===== ユーザー層 =====

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub title: String,
    pub body_md: String,
    pub status: TaskStatus,
    pub kind: TaskKind,
    pub urgent: Option<bool>,
    pub important: Option<bool>,
    pub estimate_min: Option<i64>,
    pub planned_start_at: Option<Millis>,
    pub planned_end_at: Option<Millis>,
    pub key_result_id: Option<String>,
    pub external_url: Option<String>,
    pub created_at: Millis,
    pub updated_at: Millis,
    pub done_at: Option<Millis>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskDoingLog {
    pub id: String,
    pub task_id: String,
    pub start_at: Millis,
    pub end_at: Option<Millis>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskMatchRule {
    pub id: String,
    pub task_id: String,
    pub field: MatchField,
    pub pattern: String,
    pub pattern_type: PatternType,
    pub priority: i64,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActivityRule {
    pub id: String,
    pub field: MatchField,
    pub pattern: String,
    pub pattern_type: PatternType,
    pub activity_label: String,
    pub priority: i64,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SegmentOverride {
    pub id: String,
    pub start_at: Millis,
    pub end_at: Millis,
    pub kind: Option<SegmentKind>,
    /// 割り当てるタスク。空文字列は「タスクなし」（元のタスクを外す）。None は元の値を保つ。
    pub task_id: Option<String>,
    pub activity_label: Option<String>,
    pub created_at: Millis,
    /// PC 外の時間の印。true なら、範囲のうち区間が無い部分（記録なし）にも区間を作る。
    #[serde(default)]
    pub offline: bool,
    /// この override を作った補正（`segment_correction.id`）。補正以外は None。
    #[serde(default)]
    pub correction_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TimeBlock {
    pub id: String,
    pub start_at: Millis,
    pub end_at: Millis,
    pub task_id: Option<String>,
    pub label: Option<String>,
    pub focus: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Objective {
    pub id: String,
    pub title: String,
    pub description_md: Option<String>,
    pub period_start: Option<Millis>,
    pub period_end: Option<Millis>,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KeyResult {
    pub id: String,
    pub objective_id: String,
    pub title: String,
    pub target: Option<String>,
    pub current: Option<String>,
    pub status: String,
    /// 何を軸に見るか: `time`（時間）/ `days`（取り組んだ日の回数）/ `done`（完了数）。
    pub measure: String,
    /// 回数の軸で「取り組んだ日」とみなす 1 日の最小分数。
    pub day_min: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Skill {
    pub id: String,
    pub name: String,
    pub parent_id: Option<String>,
    pub description: Option<String>,
    pub preset: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActivitySkillRule {
    pub id: String,
    pub field: MatchField,
    pub pattern: String,
    pub pattern_type: PatternType,
    pub skill_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Checkin {
    pub id: String,
    pub at: Millis,
    pub energy: Option<i64>,
    pub mood: Option<i64>,
    pub stress: Option<i64>,
    pub erg_tag: Option<ErgTag>,
    pub note: Option<String>,
    pub push_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Hassle {
    pub id: String,
    pub at: Millis,
    pub text: String,
    pub intensity: i64,
    pub segment_id: Option<String>,
    pub push_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AbcRecord {
    pub id: String,
    pub at: Millis,
    pub activating_event: String,
    pub belief: Option<String>,
    pub consequence_emotion: Option<String>,
    pub consequence_intensity: Option<i64>,
    pub dispute: Option<String>,
    pub ai_dialog_md: Option<String>,
    pub hassle_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Reflection {
    pub id: String,
    pub at: Millis,
    pub kind: ReflectionKind,
    pub context_json: String,
    pub questions_md: String,
    pub answers_md: Option<String>,
    pub summary_md: Option<String>,
    /// 構造化出力（`service::structured::StructuredOutput` の JSON）。無ければ従来の Markdown のみ。
    #[serde(default)]
    pub structured_json: Option<String>,
    /// 構造化された問いへの回答の対応表（`service::reflect::AnswersRecord` の JSON）。
    #[serde(default)]
    pub answers_json: Option<String>,
}

impl Reflection {
    /// 回答（Markdown でも構造化でも）が 1 つでも保存されているか。
    pub fn answered(&self) -> bool {
        self.answers_md.is_some() || self.answers_json.is_some()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PushLog {
    pub id: String,
    pub at: Millis,
    pub trigger: String,
    pub payload_json: String,
    pub delivered: bool,
    pub responded_at: Option<Millis>,
    pub response_json: Option<String>,
    pub snoozed_until: Option<Millis>,
    pub suppressed_reason: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enum_roundtrip() {
        for k in SegmentKind::ALL {
            assert_eq!(k.as_str().parse::<SegmentKind>().unwrap(), *k);
        }
        assert!("nope".parse::<TaskStatus>().is_err());
        assert_eq!(
            serde_json::to_string(&SegmentKind::AgentDialog).unwrap(),
            "\"agent_dialog\""
        );
    }
}
