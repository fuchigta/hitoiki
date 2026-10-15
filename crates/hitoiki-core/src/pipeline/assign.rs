//! docs/dev/architecture.md「割り当てパイプライン」: 確定・ルール・自動判定・割り込み・doing フォールバック・状態付け。

use super::piece::{relabel, split_at, Piece};
use super::{doing_at, JudgeAnswer, PipelineInput, SegmentJudge};
use crate::id::new_id;
use crate::model::*;
use crate::rules::{ActivityRuleSet, MatchTarget, TaskRuleSet};
use std::collections::HashMap;

/// ユーザー確定（確定）。最優先で、確定状態にする。None の項目は元の値を保つ。
/// `task_id` が空文字列なら「タスクなし」（元のタスクを外す）。
/// `off_pc` の確定は、範囲のうち区間が無い部分（記録なし）にも区間を作る（PC 外で過ごした時間）。
pub(crate) fn apply_confirmations(input: &PipelineInput, pieces: &mut Vec<Piece>) {
    let mut ovs: Vec<&SegmentConfirmation> = input.confirmations.iter().collect();
    ovs.sort_by(|a, b| a.created_at.cmp(&b.created_at).then_with(|| a.id.cmp(&b.id)));
    let apply = |o: &SegmentConfirmation, p: &mut Piece| {
        if let Some(k) = o.kind {
            p.kind = k;
        }
        match o.task_id.as_deref() {
            Some("") => p.task_id = None,
            Some(t) => p.task_id = Some(t.to_string()),
            None => {}
        }
        if let Some(l) = &o.activity {
            p.label = Some(l.clone());
        }
        if o.off_pc {
            p.off_pc = true;
        }
        // 無視の区間はタスクにも活動にも属さない。
        if p.kind == SegmentKind::Ignored {
            p.task_id = None;
            p.label = None;
        }
        p.status = Some(SegmentStatus::Confirmed);
        p.decided_by = Some("confirmation".into());
        p.confidence = Some(1.0);
        p.fixed = true;
    };
    for o in ovs {
        relabel(pieces, o.start_at, o.end_at, |_| true, |p| apply(o, p));
        if o.off_pc {
            fill_uncovered(input, pieces, o, |p| apply(o, p));
        }
    }
}

/// `off_pc` の確定の範囲（出力範囲と現在時刻に切る）のうち、区間が無い部分に区間を作る。
/// 収集器が動いていなかった時間（記録なし）を、PC 外の作業として計上するため。種別は確定に従い、無ければ作業。
fn fill_uncovered(
    input: &PipelineInput,
    pieces: &mut Vec<Piece>,
    o: &SegmentConfirmation,
    apply: impl Fn(&mut Piece),
) {
    let lo = o.start_at.max(input.range.start);
    let hi = o.end_at.min(input.range.end).min(input.now);
    if lo >= hi {
        return;
    }
    let mut covered: Vec<(crate::Millis, crate::Millis)> = pieces
        .iter()
        .filter(|p| p.start < hi && p.end > lo)
        .map(|p| (p.start, p.end))
        .collect();
    covered.sort_unstable();
    let mut cur = lo;
    let mut added = Vec::new();
    for (s, e) in covered {
        if s > cur {
            added.push((cur, s.min(hi)));
        }
        cur = cur.max(e);
        if cur >= hi {
            break;
        }
    }
    if cur < hi {
        added.push((cur, hi));
    }
    for (s, e) in added {
        let mut p = Piece::idle(s, e);
        p.kind = SegmentKind::Work;
        apply(&mut p);
        pieces.push(p);
    }
    pieces.sort_by_key(|p| (p.start, p.end));
}

/// task_assign_rule → activity_rule。
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
        if p.label.is_none()
            && !matches!(
                p.kind,
                SegmentKind::Idle | SegmentKind::Break | SegmentKind::Ignored
            )
        {
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
        activity: p.label.clone(),
        confidence: p.confidence,
        decided_by: p.decided_by.clone(),
        status: SegmentStatus::NeedsReview,
        window_span_ids: p.ids.clone(),
        agent_session_id: p.session.clone(),
        exe_name: p.exe.clone(),
        title: p.title.clone(),
        off_pc: p.off_pc,
    }
}

/// ルールで task が決まらなかった作業区間を自動判定に問い合わせる。
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
            activity,
            is_interrupt,
            confidence,
            judge_name,
        }) = a
        else {
            continue;
        };
        if task_id.is_none() && activity.is_none() && is_interrupt.is_none() {
            continue;
        }
        let p = &mut pieces[i];
        if task_id.is_some() {
            p.task_id = task_id;
        }
        if activity.is_some() {
            p.label = activity;
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

/// 割り込みの検出（docs/dev/architecture.md「割り当てパイプライン」）。
/// コミュニケーション系アプリが前面になり、その後「元のタスクに割り当てられない作業」が
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
                // `actual.trust_doing` なら、始めたタスクを信頼して要確認にしない。
                p.status = Some(if input.trust_doing {
                    SegmentStatus::Auto
                } else {
                    SegmentStatus::NeedsReview
                });
            }
        }
    }
}

/// 画面ロック／スリープによる無操作が `actual.auto_break_locked_min` 以上なら、その無操作区間を
/// 自動で休憩（break、decided_by `rule:locked`、auto）にする。ユーザー確定済みの区間（decided_by あり）は対象外。
/// 隣り合う無操作の記録（無操作 → ロック）は 1 つの区間にまとまっているので、区間に重なる
/// ロック／スリープの記録の合計時間（区間への重なりのみ）で判定する。
pub(crate) fn apply_locked_break(input: &PipelineInput, pieces: &mut [Piece]) {
    if input.auto_break_locked <= 0 {
        return;
    }
    let mut locked: Vec<(crate::Millis, crate::Millis)> = input
        .idles
        .iter()
        .filter(|i| i.reason != IdleReason::NoInput)
        .map(|i| (i.start_at, i.end_at.unwrap_or(input.now)))
        .filter(|(s, e)| s < e)
        .collect();
    locked.sort_unstable();
    for p in pieces.iter_mut() {
        if p.kind != SegmentKind::Idle || p.decided_by.is_some() {
            continue;
        }
        // 区間に重なるロック部分の和集合の長さ。
        let (mut total, mut cur_end) = (0, crate::Millis::MIN);
        for &(s, e) in &locked {
            let (s, e) = (s.max(p.start), e.min(p.end));
            if s >= e {
                continue;
            }
            let s = s.max(cur_end);
            if e > s {
                total += e - s;
                cur_end = e;
            }
        }
        if total >= input.auto_break_locked {
            p.kind = SegmentKind::Break;
            p.decided_by = Some("rule:locked".into());
            p.confidence = Some(1.0);
            p.status = Some(SegmentStatus::Auto);
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
