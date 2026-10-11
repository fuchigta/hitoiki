//! docs/dev/architecture.md「帰属判定パイプライン」: override・ルール・判定器・割り込み・doing フォールバック・状態付け。

use super::piece::{relabel, split_at, Piece};
use super::{doing_at, JudgeAnswer, PipelineInput, SegmentJudge};
use crate::id::new_id;
use crate::model::*;
use crate::rules::{ActivityRuleSet, MatchTarget, TaskRuleSet};
use std::collections::HashMap;

/// ユーザー確定（override）。最優先で、確定状態にする。None の項目は元の値を保つ。
pub(crate) fn apply_overrides(input: &PipelineInput, pieces: &mut Vec<Piece>) {
    let mut ovs: Vec<&SegmentOverride> = input.overrides.iter().collect();
    ovs.sort_by(|a, b| a.created_at.cmp(&b.created_at).then_with(|| a.id.cmp(&b.id)));
    for o in ovs {
        relabel(
            pieces,
            o.start_at,
            o.end_at,
            |_| true,
            |p| {
                if let Some(k) = o.kind {
                    p.kind = k;
                }
                if let Some(t) = &o.task_id {
                    p.task_id = Some(t.clone());
                }
                if let Some(l) = &o.activity_label {
                    p.label = Some(l.clone());
                }
                p.status = Some(SegmentStatus::Confirmed);
                p.decided_by = Some("user".into());
                p.confidence = Some(1.0);
                p.fixed = true;
            },
        );
    }
}

/// task_match_rule → activity_rule。
pub(crate) fn apply_rules(
    _input: &PipelineInput,
    task_rules: &TaskRuleSet,
    activity_rules: &ActivityRuleSet,
    pieces: &mut [Piece],
) {
    for p in pieces.iter_mut() {
        if p.win.is_none() {
            continue;
        }
        let target = MatchTarget::window(p.exe.as_deref(), p.title.as_deref());
        if p.kind == SegmentKind::Work && !p.fixed && p.task_id.is_none() {
            if let Some(t) = task_rules.find(&target) {
                p.task_id = Some(t.to_string());
                p.decided_by = Some("rule".into());
                p.confidence = Some(1.0);
            }
        }
        if p.label.is_none() && !matches!(p.kind, SegmentKind::Idle | SegmentKind::Break) {
            if let Some(l) = activity_rules.find(&target) {
                p.label = Some(l.to_string());
            }
        }
    }
}

fn to_segment(p: &Piece) -> ActivitySegment {
    ActivitySegment {
        id: new_id(),
        start_at: p.start,
        end_at: p.end,
        kind: p.kind,
        task_id: p.task_id.clone(),
        activity_label: p.label.clone(),
        confidence: p.confidence,
        decided_by: p.decided_by.clone(),
        status: SegmentStatus::NeedsReview,
        window_span_ids: p.ids.clone(),
        agent_session_id: p.session.clone(),
        exe_name: p.exe.clone(),
        title: p.title.clone(),
    }
}

/// ルールで task が決まらなかった作業区間を判定器に問い合わせる。
pub(crate) fn apply_judge(pieces: &mut [Piece], judge: &dyn SegmentJudge, absorb: &HashMap<usize, usize>) {
    let idx: Vec<usize> = pieces
        .iter()
        .enumerate()
        .filter(|(i, p)| {
            p.kind == SegmentKind::Work
                && !p.fixed
                && p.task_id.is_none()
                && p.win.is_some()
                && !absorb.contains_key(i)
        })
        .map(|(i, _)| i)
        .collect();
    if idx.is_empty() {
        return;
    }
    let segs: Vec<ActivitySegment> = idx.iter().map(|&i| to_segment(&pieces[i])).collect();
    let answers = judge.judge(&segs);
    for (&i, a) in idx.iter().zip(answers) {
        let Some(JudgeAnswer {
            task_id,
            activity_label,
            is_interrupt,
            confidence,
            judge_name,
        }) = a
        else {
            continue;
        };
        if task_id.is_none() && activity_label.is_none() && is_interrupt.is_none() {
            continue;
        }
        let p = &mut pieces[i];
        if task_id.is_some() {
            p.task_id = task_id;
        }
        if activity_label.is_some() {
            p.label = activity_label;
        }
        p.confidence = Some(confidence);
        p.decided_by = Some(format!("judge:{judge_name}"));
        if is_interrupt == Some(true) {
            p.kind = SegmentKind::Interrupt;
            p.status = Some(SegmentStatus::NeedsReview);
        }
    }
}

/// その時点の「元のタスク」。doing タスク、無ければフォーカス time_block のタスク。
/// 外側の None は文脈なし、内側の None は「文脈はあるがタスク未指定」。
fn context(input: &PipelineInput, t: crate::Millis) -> Option<Option<String>> {
    if let Some(tid) = doing_at(&input.doing_logs, t) {
        return Some(Some(tid.to_string()));
    }
    input
        .time_blocks
        .iter()
        .find(|b| b.focus && b.start_at <= t && t < b.end_at)
        .map(|b| b.task_id.clone())
}

/// 割り込みの検出（docs/dev/architecture.md「帰属判定パイプライン」）。
/// コミュニケーション系アプリが前面になり、その後「元のタスクに帰属しない作業」が
/// interrupt_min 以上続いたら、その通信区間から元のタスクに戻る手前までを interrupt にする。
pub(crate) fn detect_interrupts(input: &PipelineInput, pieces: &mut [Piece]) {
    let is_comm = |p: &Piece| {
        p.exe
            .as_deref()
            .is_some_and(|x| input.communication_exes.iter().any(|m| m.eq_ignore_ascii_case(x)))
    };
    let returns =
        |p: &Piece, orig: &Option<String>| p.task_id.is_some() && (orig.is_none() || p.task_id == *orig);
    let n = pieces.len();
    let mut i = 0;
    while i < n {
        let p = &pieces[i];
        let orig = match context(input, p.start) {
            Some(o) if p.kind == SegmentKind::Work && !p.fixed && is_comm(p) => o,
            _ => {
                i += 1;
                continue;
            }
        };
        if returns(p, &orig) {
            i += 1;
            continue;
        }
        let mut j = i + 1;
        let mut off = 0;
        let mut leading = true;
        let mut prev_end = p.end;
        while j < n {
            let q = &pieces[j];
            if q.start != prev_end || q.kind != SegmentKind::Work || q.fixed || returns(q, &orig) {
                break;
            }
            if !(leading && is_comm(q)) {
                leading = false;
                off += q.len();
            }
            prev_end = q.end;
            j += 1;
        }
        if off >= input.interrupt_min {
            for q in &mut pieces[i..j] {
                q.kind = SegmentKind::Interrupt;
                q.status = Some(SegmentStatus::NeedsReview);
            }
        }
        i = j;
    }
}

/// どのルールにも該当しない作業区間に、その時点の doing タスクを低確信度で付与する。
pub(crate) fn apply_doing_fallback(input: &PipelineInput, pieces: &mut Vec<Piece>) {
    for l in &input.doing_logs {
        split_at(pieces, l.start_at);
        if let Some(e) = l.end_at {
            split_at(pieces, e);
        }
    }
    for p in pieces.iter_mut() {
        if p.kind == SegmentKind::Work && !p.fixed && p.task_id.is_none() {
            if let Some(t) = doing_at(&input.doing_logs, p.start) {
                p.task_id = Some(t.to_string());
                p.decided_by = Some("doing".into());
                p.confidence = Some(0.5);
                p.status = Some(SegmentStatus::NeedsReview);
            }
        }
    }
}

/// 確信度による状態付け。
pub(crate) fn finalize_status(input: &PipelineInput, pieces: &mut [Piece]) {
    for p in pieces.iter_mut() {
        if p.status.is_none() {
            p.status = Some(match p.confidence {
                Some(c) if c >= input.auto_confidence_threshold => SegmentStatus::Auto,
                _ => SegmentStatus::NeedsReview,
            });
        }
    }
}
