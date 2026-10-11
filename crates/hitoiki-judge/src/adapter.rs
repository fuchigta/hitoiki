//! 帰属判定パイプラインとの接続。区間 → 問い合わせ状態、回答 → `JudgeAnswer`。

use crate::openai::value_valid;
use crate::protocol::{Answers, Judge, OptionSpec, QuestionSpec, SegmentState, State, StateContext};
use crate::questions::{OptionSources, QuestionSet};
use chrono_tz::Tz;
use hitoiki_core::model::{ActivitySegment, CalendarEvent, Skill, Task, TaskStatus};
use hitoiki_core::pipeline::{JudgeAnswer, SegmentJudge};
use hitoiki_core::timefmt::fmt_rfc3339;
use serde_json::json;

/// 直前の区間として文脈に含める件数。
const RECENT_SEGMENTS: usize = 3;

/// 選択肢の解決と文脈の材料。構築時に渡す。
#[derive(Debug, Clone)]
pub struct JudgeContext {
    pub tz: Tz,
    /// 未完了タスク（Done / Dropped は内部で除外する）。
    pub open_tasks: Vec<Task>,
    /// 空なら設定の `activity.labels` を使う（`from_config`）。
    pub activity_labels: Vec<String>,
    pub skills: Vec<Skill>,
    pub doing_task: Option<Task>,
    pub calendar: Vec<CalendarEvent>,
    /// None なら `paths::questions_dir()` から読み、無ければ組み込み。
    pub question_set: Option<QuestionSet>,
}

impl JudgeContext {
    pub fn new(tz: Tz) -> Self {
        Self {
            tz,
            open_tasks: vec![],
            activity_labels: vec![],
            skills: vec![],
            doing_task: None,
            calendar: vec![],
            question_set: None,
        }
    }
}

pub struct JudgeAdapter {
    judge: Box<dyn Judge>,
    ctx: JudgeContext,
    questions: Vec<QuestionSpec>,
    task_ids: Vec<String>,
    labels: Vec<String>,
}

impl JudgeAdapter {
    pub fn new(judge: Box<dyn Judge>, mut ctx: JudgeContext) -> Self {
        ctx.open_tasks
            .retain(|t| matches!(t.status, TaskStatus::Todo | TaskStatus::Doing));
        let set = ctx
            .question_set
            .clone()
            .unwrap_or_else(|| QuestionSet::load_dir(&hitoiki_core::paths::questions_dir()));
        let src = OptionSources {
            open_tasks: ctx
                .open_tasks
                .iter()
                .map(|t| OptionSpec {
                    id: t.id.clone(),
                    label: t.title.clone(),
                })
                .collect(),
            activity_labels: ctx
                .activity_labels
                .iter()
                .map(|l| OptionSpec {
                    id: l.clone(),
                    label: l.clone(),
                })
                .collect(),
            skills: ctx
                .skills
                .iter()
                .map(|s| OptionSpec {
                    id: s.id.clone(),
                    label: s.name.clone(),
                })
                .collect(),
        };
        let questions = set.resolve(&src);
        Self {
            task_ids: src.open_tasks.iter().map(|o| o.id.clone()).collect(),
            labels: ctx.activity_labels.clone(),
            judge,
            ctx,
            questions,
        }
    }

    /// 送信する問い（テスト・診断用）。
    pub fn questions(&self) -> &[QuestionSpec] {
        &self.questions
    }

    /// 区間 → 問い合わせ状態。`recent` は直前の区間。
    pub fn state_for(&self, seg: &ActivitySegment, recent: &[ActivitySegment]) -> State {
        let tz = self.ctx.tz;
        let calendar = self
            .ctx
            .calendar
            .iter()
            .filter(|e| e.start_at < seg.end_at && e.end_at > seg.start_at)
            .map(|e| {
                json!({
                    "start": fmt_rfc3339(tz, e.start_at),
                    "end": fmt_rfc3339(tz, e.end_at),
                    "title": e.title,
                    "busy": e.is_busy,
                })
            })
            .collect();
        let recent_segments = recent
            .iter()
            .map(|r| {
                json!({
                    "start": fmt_rfc3339(tz, r.start_at),
                    "end": fmt_rfc3339(tz, r.end_at),
                    "exe": r.exe_name,
                    "title": r.title,
                    "task_id": r.task_id,
                    "activity": r.activity_label,
                })
            })
            .collect();
        State {
            segment: SegmentState {
                start: fmt_rfc3339(tz, seg.start_at),
                end: fmt_rfc3339(tz, seg.end_at),
                exe: seg.exe_name.clone(),
                title: seg.title.clone(),
                // 区間にはまだ cwd が無い（エージェント対話の cwd は将来対応）。
                cwd: None,
            },
            context: StateContext {
                doing_task: self
                    .ctx
                    .doing_task
                    .as_ref()
                    .map(|t| json!({"id": t.id, "title": t.title})),
                recent_segments,
                calendar,
            },
        }
    }

    /// 回答 → `JudgeAnswer`。範囲外の値は無視。確信度は採用した回答の最小値。
    /// 何も採用できなければ None。
    pub fn to_answer(&self, answers: &Answers) -> Option<JudgeAnswer> {
        let mut confs = vec![];
        let valid = |name: &str| -> Option<&crate::protocol::Answer> {
            let a = answers.get(name)?;
            let q = self.questions.iter().find(|q| q.name == name)?;
            value_valid(q, &a.value).then_some(a)
        };

        let mut task_id = None;
        if let Some(a) = valid("task") {
            confs.push(a.confidence);
            task_id = a
                .value
                .as_str()
                .filter(|s| *s != "none" && self.task_ids.iter().any(|t| t == s))
                .map(String::from);
        }
        let mut activity_label = None;
        if let Some(a) = valid("activity") {
            confs.push(a.confidence);
            activity_label = a
                .value
                .as_str()
                .filter(|s| self.labels.iter().any(|l| l == s))
                .map(String::from);
        }
        let mut is_interrupt = None;
        if let Some(a) = valid("is_interrupt") {
            confs.push(a.confidence);
            is_interrupt = a.value.as_bool();
        }
        if task_id.is_none() && activity_label.is_none() && is_interrupt.is_none() {
            return None;
        }
        let confidence = confs
            .into_iter()
            .map(crate::protocol::clamp01)
            .fold(1.0, f64::min);
        Some(JudgeAnswer {
            task_id,
            activity_label,
            is_interrupt,
            confidence,
            judge_name: self.judge.name().to_string(),
        })
    }
}

impl SegmentJudge for JudgeAdapter {
    fn judge(&self, segs: &[ActivitySegment]) -> Vec<Option<JudgeAnswer>> {
        if self.questions.is_empty() {
            return vec![None; segs.len()];
        }
        let states: Vec<State> = segs
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let from = i.saturating_sub(RECENT_SEGMENTS);
                self.state_for(s, &segs[from..i])
            })
            .collect();
        // 失敗は judge 側で warn 済み（送信内容は出さない）。ここでは未判定として扱う。
        self.judge
            .ask_batch(&states, &self.questions)
            .into_iter()
            .map(|a| a.and_then(|a| self.to_answer(&a)))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{Answer, JudgeRequest, JudgeResponse};
    use hitoiki_core::model::{SegmentKind, SegmentStatus, TaskKind};
    use serde_json::Value;

    fn task(id: &str, st: TaskStatus) -> Task {
        Task {
            id: id.into(),
            title: format!("title-{id}"),
            body_md: String::new(),
            status: st,
            kind: TaskKind::Normal,
            urgent: None,
            important: None,
            estimate_min: None,
            planned_start_at: None,
            planned_end_at: None,
            key_result_id: None,
            external_url: None,
            created_at: 0,
            updated_at: 0,
            done_at: None,
        }
    }

    fn seg(s: i64, e: i64) -> ActivitySegment {
        ActivitySegment {
            id: "x".into(),
            start_at: s,
            end_at: e,
            kind: SegmentKind::Work,
            task_id: None,
            activity_label: None,
            confidence: None,
            decided_by: None,
            status: SegmentStatus::NeedsReview,
            window_span_ids: vec![],
            agent_session_id: None,
            exe_name: Some("a.exe".into()),
            title: Some("t".into()),
            offline: false,
        }
    }

    struct Fixed(Option<Answers>);
    impl Judge for Fixed {
        fn name(&self) -> &str {
            "fixed"
        }
        fn ask(&self, _: &JudgeRequest) -> anyhow::Result<JudgeResponse> {
            unreachable!()
        }
        fn ask_batch(&self, states: &[State], _: &[QuestionSpec]) -> Vec<Option<Answers>> {
            states.iter().map(|_| self.0.clone()).collect()
        }
    }

    fn ans(v: Value, c: f64) -> Answer {
        Answer {
            value: v,
            confidence: c,
            probs: None,
        }
    }

    fn adapter(a: Option<Answers>) -> JudgeAdapter {
        let mut ctx = JudgeContext::new(chrono_tz::UTC);
        ctx.open_tasks = vec![task("T1", TaskStatus::Todo), task("T2", TaskStatus::Done)];
        ctx.activity_labels = vec!["開発".into(), "会議".into()];
        ctx.question_set = Some(QuestionSet::builtin());
        JudgeAdapter::new(Box::new(Fixed(a)), ctx)
    }

    #[test]
    fn closed_tasks_excluded_from_options() {
        let a = adapter(None);
        let t = a.questions().iter().find(|q| q.name == "task").unwrap();
        assert_eq!(t.options.as_ref().unwrap().len(), 1);
    }

    #[test]
    fn convert_answers() {
        let mut m = Answers::new();
        m.insert("task".into(), ans(json!("T1"), 0.9));
        m.insert("activity".into(), ans(json!("開発"), 0.7));
        m.insert("is_interrupt".into(), ans(json!(false), 0.95));
        let a = adapter(Some(m.clone()));
        let r = a.judge(&[seg(0, 1000)]);
        let r = r[0].clone().unwrap();
        assert_eq!(r.task_id.as_deref(), Some("T1"));
        assert_eq!(r.activity_label.as_deref(), Some("開発"));
        assert_eq!(r.is_interrupt, Some(false));
        assert!((r.confidence - 0.7).abs() < 1e-9);
        assert_eq!(r.judge_name, "fixed");

        // none と範囲外 id
        m.insert("task".into(), ans(json!("none"), 0.9));
        m.insert("activity".into(), ans(json!("存在しない"), 0.9));
        m.remove("is_interrupt");
        assert!(a.to_answer(&m).is_none());
        m.insert("task".into(), ans(json!("T2"), 0.9)); // 完了済みは選択肢外
        assert!(a.to_answer(&m).is_none());
    }

    #[test]
    fn failure_is_none() {
        let a = adapter(None);
        assert_eq!(a.judge(&[seg(0, 1), seg(1, 2)]), vec![None, None]);
    }

    #[test]
    fn state_shape() {
        let mut a = adapter(None);
        a.ctx.calendar = vec![CalendarEvent {
            id: "c".into(),
            source: "s".into(),
            external_id: "e".into(),
            start_at: 500,
            end_at: 5000,
            all_day: false,
            title: "定例".into(),
            location: None,
            is_busy: true,
            fetched_at: 0,
        }];
        let st = a.state_for(&seg(0, 1000), &[seg(-10, 0)]);
        assert_eq!(st.segment.start, "1970-01-01T00:00:00+00:00");
        assert_eq!(st.segment.cwd, None);
        assert_eq!(st.context.recent_segments.len(), 1);
        assert_eq!(st.context.calendar.len(), 1);
        let v = serde_json::to_value(&st).unwrap();
        assert!(v["segment"]["cwd"].is_null());
    }
}
