//! 問いかけの判定ロジック（docs/dev/architecture.md「問いかけ」、OS 非依存）。
//!
//! - [`evaluate`]: 現在の状況 [`NudgeContext`] と設定から、出すべき通知 / 抑制 / 保留を純粋関数で決める。
//! - [`store`]: `nudge_log` の読み書きと [`store::apply_response`]（応答の保存、checkin / hassle の作成）。
//! - トースト表示（OS 依存）は呼び出し側の責務。
//!
//! 時刻はすべて引数（`ctx.now` など）で与える。この関数群は時計を直接読まない。

mod gate;
pub mod payload;
pub mod store;
mod triggers;

#[cfg(test)]
mod tests;

use crate::model::{ActivitySegment, CalendarEvent, NudgeLog, PlanBlock};
use crate::{config::Nudge, Millis};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};

pub use payload::{NudgeAction, NudgeInput, NudgePayload};
pub use store::{apply_response, NudgeResponse, ResponseOutcome};
pub use triggers::key_result_idle_due;

/// 通知トリガー（docs/dev/architecture.md「問いかけ」）。`ALL` は同時に条件を満たした場合の優先順。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NudgeTrigger {
    DayStart,
    IdleReturn,
    BlockStart,
    DayEnd,
    Weekly,
    UrgentTask,
    AfterInterrupt,
    MeetingOverrun,
    SwitchSpike,
    LongWork,
    ImportantTask,
    KeyResultIdle,
    LateWork,
}

impl NudgeTrigger {
    pub const ALL: &'static [NudgeTrigger] = &[
        NudgeTrigger::DayStart,
        NudgeTrigger::IdleReturn,
        NudgeTrigger::BlockStart,
        NudgeTrigger::DayEnd,
        NudgeTrigger::Weekly,
        NudgeTrigger::UrgentTask,
        NudgeTrigger::AfterInterrupt,
        NudgeTrigger::MeetingOverrun,
        NudgeTrigger::SwitchSpike,
        NudgeTrigger::LongWork,
        NudgeTrigger::ImportantTask,
        NudgeTrigger::KeyResultIdle,
        NudgeTrigger::LateWork,
    ];

    /// `nudge_log.trigger` や `Nudge::disabled_triggers` で使う ID。
    pub fn as_str(&self) -> &'static str {
        match self {
            NudgeTrigger::DayStart => "day_start",
            NudgeTrigger::IdleReturn => "idle_return",
            NudgeTrigger::BlockStart => "block_start",
            NudgeTrigger::DayEnd => "day_end",
            NudgeTrigger::Weekly => "weekly",
            NudgeTrigger::UrgentTask => "urgent_task",
            NudgeTrigger::AfterInterrupt => "after_interrupt",
            NudgeTrigger::MeetingOverrun => "meeting_overrun",
            NudgeTrigger::SwitchSpike => "switch_spike",
            NudgeTrigger::LongWork => "long_work",
            NudgeTrigger::ImportantTask => "important_task",
            NudgeTrigger::KeyResultIdle => "key_result_idle",
            NudgeTrigger::LateWork => "late_work",
        }
    }

    /// 画面・CLI に出す日本語の表示名。
    pub fn label(&self) -> &'static str {
        match self {
            NudgeTrigger::DayStart => "始業",
            NudgeTrigger::IdleReturn => "無操作からの復帰",
            NudgeTrigger::BlockStart => "計画ブロックの開始",
            NudgeTrigger::DayEnd => "終業",
            NudgeTrigger::Weekly => "週次の振り返り",
            NudgeTrigger::UrgentTask => "緊急のタスク",
            NudgeTrigger::AfterInterrupt => "割り込みのあと",
            NudgeTrigger::MeetingOverrun => "会議の延長",
            NudgeTrigger::SwitchSpike => "切り替えの増加",
            NudgeTrigger::LongWork => "長時間の作業",
            NudgeTrigger::ImportantTask => "重要なタスク",
            NudgeTrigger::KeyResultIdle => "時間が入っていない成果の指標",
            NudgeTrigger::LateWork => "遅い時間の作業",
        }
    }

    /// `nudge_log.trigger` の文字列から表示名を引く（未知の値はそのまま返す）。
    pub fn label_of(id: &str) -> String {
        Self::parse(id).map_or_else(|| id.to_string(), |t| t.label().to_string())
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|t| t.as_str() == s)
    }
}

impl std::fmt::Display for NudgeTrigger {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// 現在の無操作状態。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct IdleState {
    /// 無操作と判定中か。
    pub is_idle: bool,
    /// 最終入力からの経過秒。
    pub since_last_input_sec: u64,
}

/// 終わったばかりの無操作（ロック・スリープを含み、続いた記録は 1 つにまとめたもの）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClosedIdle {
    pub start_at: Millis,
    pub end_at: Millis,
}

/// 通知の判定に使うタスクの最小限の情報（未完了で、緊急か重要のしるしが付いたものだけ）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskNudge {
    pub id: String,
    pub title: String,
    pub urgent: bool,
    pub important: bool,
    /// doing（着手を宣言している。doing ログが開いている）。
    pub doing: bool,
    /// 作業系の区間（時間が入った区間）に割り当てられた最後の終了時刻。一度も無ければ None。
    pub last_worked_at: Option<Millis>,
    pub created_at: Millis,
}

/// key_result_idle の判定に使う成果の指標の最小限の情報（今週まだ時間が入っていない成果の指標だけ）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyResultIdleNudge {
    pub id: String,
    pub title: String,
}

/// 通知の文言と判定に使うタスクの参照（今日の plan_block や doing ログが指すタスク。状態は問わない）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskRef {
    pub id: String,
    pub title: String,
    pub status: crate::model::TaskStatus,
}

/// `task_doing_log` の期間（`end_at` が None なら進行中）。idle_return の候補タスクの材料。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DoingSpan {
    pub task_id: String,
    pub start_at: Millis,
    pub end_at: Option<Millis>,
}

/// 設定に出していない内部しきい値。既定値で十分な想定。
#[derive(Debug, Clone)]
pub struct NudgeTuning {
    /// day_end: 終業時刻以降にこの分数以上無操作なら発火。
    pub day_end_idle_min: u64,
    /// day_end: 終業時刻ちょうどとみなす許容幅（秒、±）。
    pub day_end_exact_tolerance_sec: i64,
    /// after_interrupt: 割り込み区間の終了からこの分数以内なら発火。
    pub interrupt_recent_min: u64,
    /// meeting_overrun: 予定終了からこの分数以上、会議アプリが前面なら発火。
    pub meeting_overrun_min: u64,
    /// meeting_overrun: 予定終了からこの分数を過ぎたら対象外。
    pub meeting_overrun_max_min: u64,
    /// long_work: 区間同士の隙間がこの秒数を超えたら連続とみなさない（`collect.merge_gap_sec` 相当）。
    pub focus_gap_sec: u64,
    /// 「いま作業中」とみなす、最終区間の終了から now までの許容秒数。
    pub active_slack_sec: u64,
    /// idle_return: 無操作の終了からこの分数以内なら発火する。
    pub idle_return_recent_min: u64,
    /// block_start: ブロックの開始からこの分数以内なら発火する。
    pub block_start_window_min: u64,
    /// 会議の終了時刻が不明なときの保留時間（分）。
    pub meeting_defer_min: u64,
    /// どのトリガーでも、直前の通知からこの分数は空ける。
    pub min_gap_any_min: u64,
    /// 応答率の算出対象にする直近の通知件数。
    pub response_window: usize,
    /// 応答率の算出に必要な最小件数（未満なら延長しない）。
    pub response_min_samples: usize,
    /// 通知から経過がこの分数未満のものは「まだ応答待ち」として応答率から除く。
    pub response_grace_min: u64,
}

impl Default for NudgeTuning {
    fn default() -> Self {
        Self {
            day_end_idle_min: 10,
            day_end_exact_tolerance_sec: 60,
            interrupt_recent_min: 5,
            meeting_overrun_min: 5,
            meeting_overrun_max_min: 180,
            focus_gap_sec: 30,
            active_slack_sec: 60,
            idle_return_recent_min: 10,
            block_start_window_min: 10,
            meeting_defer_min: 5,
            min_gap_any_min: 10,
            response_window: 5,
            response_min_samples: 3,
            response_grace_min: 30,
        }
    }
}

/// 判定の入力。呼び出し側が DB / 収集器から組み立てる。
#[derive(Debug, Clone)]
pub struct NudgeContext {
    pub now: Millis,
    pub tz: Tz,
    /// 直近の activity_segment（派生層）。目安として直近 3 時間以上。順不同でよい。
    pub recent_segments: Vec<ActivitySegment>,
    pub idle: IdleState,
    /// 直近のロック解除 / 無操作明けイベントの時刻（なければ None）。day_start の検知に使う。
    pub resumed_at: Option<Millis>,
    /// 直近で終わった無操作（なければ None）。idle_return に使う。
    pub last_idle: Option<ClosedIdle>,
    /// 今日の plan_block。
    pub plan_blocks: Vec<PlanBlock>,
    /// 今日前後のカレンダー予定。
    pub calendar_events: Vec<CalendarEvent>,
    /// 直近の nudge_log（抑制ログを含む）。今週分と、応答率算出のための直近数件以上を含めること
    /// （`store::load_recent` が返す内容で足りる）。
    pub nudge_log: Vec<NudgeLog>,
    pub needs_review_count: u32,
    /// 緊急・重要のしるしが付いた未完了のタスク（urgent_task / important_task の材料）。
    pub tasks: Vec<TaskNudge>,
    /// 今週まだ時間が入っていない成果の指標（key_result_idle の材料。`store::load_key_result_idle` が返す内容。曜日に達する前は空でよい）。
    pub key_result_idle: Vec<KeyResultIdleNudge>,
    /// 今日の plan_block と直近の doing ログが指すタスク（block_start の文言と idle_return の候補に使う）。
    pub task_refs: Vec<TaskRef>,
    /// 直近の doing ログ（進行中を含む）。
    pub doing_spans: Vec<DoingSpan>,
    /// 前面ウインドウの exe 名。
    pub foreground_exe: Option<String>,
    /// 会議アプリの exe 名（`collect.meeting_exes`）。
    pub meeting_exes: Vec<String>,
    /// 直近 `switch_spike.window_min` 分の前面区間切替回数。None なら `recent_segments` から数える。
    pub recent_switches: Option<u32>,
    pub tuning: NudgeTuning,
}

impl NudgeContext {
    /// 空の状況（何も起きていない）。テストや呼び出し側の組み立ての出発点。
    pub fn new(now: Millis, tz: Tz) -> Self {
        Self {
            now,
            tz,
            recent_segments: vec![],
            idle: IdleState::default(),
            resumed_at: None,
            last_idle: None,
            plan_blocks: vec![],
            calendar_events: vec![],
            nudge_log: vec![],
            needs_review_count: 0,
            tasks: vec![],
            key_result_idle: vec![],
            task_refs: vec![],
            doing_spans: vec![],
            foreground_exe: None,
            meeting_exes: vec![],
            recent_switches: None,
            tuning: NudgeTuning::default(),
        }
    }
}

/// 判定結果。
#[derive(Debug, Clone, PartialEq)]
pub enum NudgeDecision {
    /// 通知を出す。呼び出し側は表示し、`store::record_decision` で nudge_log に残す。
    Deliver(NudgePayload),
    /// 出さない。`reason` を `suppressed_reason` として nudge_log に残す。
    /// 同じ理由の連続抑制は返さない（ログが膨らまないように）。
    Suppress { trigger: NudgeTrigger, reason: String },
    /// `until` まで保留。条件が残っていれば後の評価で再び Deliver になる。ログ不要。
    Defer { trigger: NudgeTrigger, until: Millis },
}

/// 通知すべきものを判定する（純粋関数）。
///
/// ## 流れ
/// 1. `cfg.enabled == false` なら空。`disabled_triggers` のトリガーは判定しない（ログも残さない）。
/// 2. トリガー条件を満たしたものを [`NudgeTrigger::ALL`] の順に並べる。
/// 3. 各候補に抑制ルール（docs/dev/architecture.md「問いかけ」、`gate.rs` の doc 参照）を適用する。
/// 4. 1 回の評価で `Deliver` になるのは最大 1 件（先に通ったものだけ。残りは次回の評価に回る）。
pub fn evaluate(ctx: &NudgeContext, cfg: &Nudge) -> Vec<NudgeDecision> {
    if !cfg.enabled {
        return vec![];
    }
    let mut out = Vec::new();
    let mut delivered = false;
    for (trigger, payload) in triggers::candidates(ctx, cfg) {
        match gate::check(ctx, cfg, trigger, &payload) {
            gate::Gate::Pass => {
                if !delivered {
                    delivered = true;
                    out.push(NudgeDecision::Deliver(payload));
                }
            }
            gate::Gate::Suppress(reason) => {
                if !gate::is_repeat_suppression(ctx, trigger, &reason) {
                    out.push(NudgeDecision::Suppress { trigger, reason });
                }
            }
            gate::Gate::Defer(until) => out.push(NudgeDecision::Defer { trigger, until }),
        }
    }
    out
}
