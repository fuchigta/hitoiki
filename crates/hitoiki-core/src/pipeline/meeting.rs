//! docs/architecture.md「帰属判定パイプライン」の無操作と予定の突き合わせ。

use super::piece::{relabel, Piece};
use super::PipelineInput;
use crate::model::*;
use crate::rules::{MatchTarget, TaskRuleSet};

pub(crate) fn apply(input: &PipelineInput, task_rules: &TaskRuleSet, pieces: &mut Vec<Piece>) {
    let mut events: Vec<&CalendarEvent> = input
        .calendar
        .iter()
        .filter(|e| e.is_busy && !e.all_day && e.start_at < e.end_at)
        .collect();
    events.sort_by_key(|e| (e.start_at, &e.id));

    let is_meeting_exe = |p: &Piece| {
        p.exe
            .as_deref()
            .is_some_and(|x| input.meeting_exes.iter().any(|m| m.eq_ignore_ascii_case(x)))
    };

    for ev in events {
        let task = task_rules
            .find(&MatchTarget::title_only(&ev.title))
            .map(String::from);
        relabel(
            pieces,
            ev.start_at,
            ev.end_at,
            |p| p.kind == SegmentKind::Idle || (p.kind == SegmentKind::Work && !p.fixed && is_meeting_exe(p)),
            |p| {
                p.kind = SegmentKind::Meeting;
                p.task_id = task.clone();
                p.decided_by = Some("calendar".into());
                p.confidence = Some(1.0);
                p.status = Some(SegmentStatus::Auto);
                p.fixed = true;
            },
        );
    }

    // 予定外の無操作は要確認。
    for p in pieces.iter_mut() {
        if p.kind == SegmentKind::Idle && p.status.is_none() {
            p.status = Some(SegmentStatus::NeedsReview);
            p.fixed = true;
        }
    }
}
