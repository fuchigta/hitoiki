//! 時間の帰属判定パイプライン（7章）。生データ → `activity_segment` の純粋関数。
//!
//! 処理順: タイムライン → エージェント対話 → 会議 → override → ルール → 判定器 →
//! 割り込み → doing フォールバック → 状態付け → 統合。
//! （割り込み検出はルール結果を見る必要があるため doing フォールバックより前に行う。）

mod agent;
mod attribute;
mod meeting;
mod merge;
mod piece;
#[cfg(test)]
mod tests;
mod timeline;

use crate::config::Config;
use crate::id::new_id;
use crate::model::*;
use crate::rules::{ActivityRuleSet, TaskRuleSet};
use crate::timefmt::Range;
use crate::Millis;
pub(crate) use agent::norm_title;
use piece::Piece;

/// 判定器の答え（8章）。
#[derive(Debug, Clone, PartialEq)]
pub struct JudgeAnswer {
    pub task_id: Option<String>,
    pub activity_label: Option<String>,
    pub is_interrupt: Option<bool>,
    pub confidence: f64,
    pub judge_name: String,
}

/// ルールで決まらなかった作業区間への問い合わせ口（実装は Phase 6）。
/// 戻り値は `segs` と同じ長さ。答えの無い区間は None。
pub trait SegmentJudge {
    fn judge(&self, segs: &[ActivitySegment]) -> Vec<Option<JudgeAnswer>>;
}

/// パイプラインの入力。時間の単位はすべてミリ秒。
#[derive(Debug, Clone)]
pub struct PipelineInput {
    /// 出力を切り取る範囲。生データは範囲の外（直前）を含んでよい。
    pub range: Range,
    pub now: Millis,
    pub windows: Vec<WindowSpan>,
    pub idles: Vec<IdleSpan>,
    pub events: Vec<AgentEvent>,
    pub calendar: Vec<CalendarEvent>,
    pub overrides: Vec<SegmentOverride>,
    pub task_rules: Vec<TaskMatchRule>,
    pub activity_rules: Vec<ActivityRule>,
    pub doing_logs: Vec<TaskDoingLog>,
    pub time_blocks: Vec<TimeBlock>,
    /// 未完了タスク（現状パイプラインでは参照しない。判定器への文脈用に保持）。
    pub tasks: Vec<Task>,
    pub idle_threshold: Millis,
    pub merge_gap: Millis,
    pub terminal_exes: Vec<String>,
    pub communication_exes: Vec<String>,
    pub meeting_exes: Vec<String>,
    pub watch_max: Millis,
    pub bind_lookback: Millis,
    pub interrupt_min: Millis,
    pub auto_confidence_threshold: f64,
}

impl PipelineInput {
    /// 設定値を取り込み、データは空で作る。
    pub fn new(range: Range, now: Millis, cfg: &Config) -> Self {
        Self {
            range,
            now,
            windows: vec![],
            idles: vec![],
            events: vec![],
            calendar: vec![],
            overrides: vec![],
            task_rules: vec![],
            activity_rules: vec![],
            doing_logs: vec![],
            time_blocks: vec![],
            tasks: vec![],
            idle_threshold: cfg.collect.idle_threshold_sec as Millis * 1000,
            merge_gap: cfg.collect.merge_gap_sec as Millis * 1000,
            terminal_exes: cfg.collect.terminal_exes.clone(),
            communication_exes: cfg.collect.communication_exes.clone(),
            meeting_exes: cfg.collect.meeting_exes.clone(),
            watch_max: cfg.agent.watch_max_min as Millis * 60_000,
            bind_lookback: cfg.agent.bind_lookback_sec as Millis * 1000,
            interrupt_min: cfg.collect.interrupt_min_sec as Millis * 1000,
            auto_confidence_threshold: cfg.judge.auto_confidence_threshold,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct PipelineOutput {
    pub segments: Vec<ActivitySegment>,
    pub bindings: Vec<AgentSessionWindow>,
}

/// その時点で doing のタスク（複数なら最も新しく始まったもの）。
pub(crate) fn doing_at(logs: &[TaskDoingLog], t: Millis) -> Option<&str> {
    logs.iter()
        .filter(|l| l.start_at <= t && l.end_at.unwrap_or(Millis::MAX) > t)
        .max_by_key(|l| (l.start_at, &l.id))
        .map(|l| l.task_id.as_str())
}

/// 生データから区間を組み立てる。`judge` があればルールで決まらなかった作業区間を問い合わせる。
pub fn build(input: &PipelineInput, judge: Option<&dyn SegmentJudge>) -> PipelineOutput {
    let task_rules = TaskRuleSet::new(&input.task_rules);
    let activity_rules = ActivityRuleSet::new(&input.activity_rules);

    // 1. タイムライン
    let wl = timeline::window_layer(&input.windows, input.now);
    let idle = timeline::idle_intervals(&input.idles, input.now);
    let mut pieces = timeline::build_pieces(&input.windows, &wl, &idle);

    // 2. エージェント対話
    let bindings = agent::apply(input, &task_rules, &wl, &mut pieces);

    // 出力範囲に切り取る
    clip(&mut pieces, input.range);

    // 3. 会議
    meeting::apply(input, &task_rules, &mut pieces);
    // 4. override
    attribute::apply_overrides(input, &mut pieces);
    // 5. ルール
    attribute::apply_rules(input, &task_rules, &activity_rules, &mut pieces);
    // 6. 判定器
    if let Some(j) = judge {
        attribute::apply_judge(&mut pieces, j);
    }
    // 10. 割り込み
    attribute::detect_interrupts(input, &mut pieces);
    // 9. doing フォールバック
    attribute::apply_doing_fallback(input, &mut pieces);
    // 7. 状態付け
    attribute::finalize_status(input, &mut pieces);
    // 8. 統合
    let pieces = merge::merge(pieces, input.merge_gap);

    let segments = pieces
        .into_iter()
        .map(|p| ActivitySegment {
            id: new_id(),
            start_at: p.start,
            end_at: p.end,
            kind: p.kind,
            task_id: p.task_id,
            activity_label: p.label,
            confidence: p.confidence,
            decided_by: p.decided_by,
            status: p.status.unwrap_or(SegmentStatus::NeedsReview),
            window_span_ids: p.ids,
            agent_session_id: p.session,
            exe_name: p.exe,
            title: p.title,
        })
        .collect();
    PipelineOutput { segments, bindings }
}

/// 出力範囲内の各 `prompt_submit` について、その対象期間（直前の応答完了 / 送信 / セッション開始、
/// 無ければバックトラック）の開始のうち最も早いもの。範囲内に送信が無ければ None。
/// 再計算範囲をここまで広げると、範囲より前から始まる対話も範囲内と同じ結果になる。
pub fn earliest_period_start(input: &PipelineInput) -> Option<Millis> {
    let task_rules = TaskRuleSet::new(&input.task_rules);
    let wl = timeline::window_layer(&input.windows, input.now);
    agent::earliest_period_start(input, &task_rules, &wl)
}

fn clip(pieces: &mut Vec<Piece>, range: Range) {
    pieces.retain_mut(|p| {
        p.start = p.start.max(range.start);
        p.end = p.end.min(range.end);
        p.rep_len = p.len();
        p.start < p.end
    });
}
