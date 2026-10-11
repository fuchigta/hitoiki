//! プッシュ通知の判定ロジック（docs/dev/architecture.md「プッシュ通知」、OS 非依存）。
//!
//! - [`evaluate`]: 現在の状況 [`PushContext`] と設定から、出すべき通知 / 抑制 / 保留を純粋関数で決める。
//! - [`store`]: `push_log` の読み書きと [`store::apply_response`]（応答の保存、checkin / hassle の作成）。
//! - トースト表示（OS 依存）は呼び出し側の責務。
//!
//! 時刻はすべて引数（`ctx.now` など）で与える。この関数群は時計を直接読まない。

mod gate;
pub mod payload;
pub mod store;
mod triggers;

#[cfg(test)]
mod tests;

use crate::model::{ActivitySegment, CalendarEvent, PushLog, TimeBlock};
use crate::{config::Push, Millis};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};

pub use payload::{PushAction, PushInput, PushPayload};
pub use store::{apply_response, PushResponse, ResponseOutcome};

/// 通知トリガー（docs/dev/architecture.md「プッシュ通知」）。`ALL` は同時に条件を満たした場合の優先順。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PushTrigger {
    DayStart,
    DayEnd,
    Weekly,
    AfterInterrupt,
    MeetingOverrun,
    SwitchSpike,
    LongFocus,
    LateWork,
}

impl PushTrigger {
    pub const ALL: &'static [PushTrigger] = &[
        PushTrigger::DayStart,
        PushTrigger::DayEnd,
        PushTrigger::Weekly,
        PushTrigger::AfterInterrupt,
        PushTrigger::MeetingOverrun,
        PushTrigger::SwitchSpike,
        PushTrigger::LongFocus,
        PushTrigger::LateWork,
    ];

    /// `push_log.trigger` や `Push::disabled_triggers` で使う ID。
    pub fn as_str(&self) -> &'static str {
        match self {
            PushTrigger::DayStart => "day_start",
            PushTrigger::DayEnd => "day_end",
            PushTrigger::Weekly => "weekly",
            PushTrigger::AfterInterrupt => "after_interrupt",
            PushTrigger::MeetingOverrun => "meeting_overrun",
            PushTrigger::SwitchSpike => "switch_spike",
            PushTrigger::LongFocus => "long_focus",
            PushTrigger::LateWork => "late_work",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|t| t.as_str() == s)
    }
}

impl std::fmt::Display for PushTrigger {
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

/// 設定に出していない内部しきい値。既定値で十分な想定。
#[derive(Debug, Clone)]
pub struct PushTuning {
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
    /// long_focus: 区間同士の隙間がこの秒数を超えたら連続とみなさない（`collect.merge_gap_sec` 相当）。
    pub focus_gap_sec: u64,
    /// 「いま作業中」とみなす、最終区間の終了から now までの許容秒数。
    pub active_slack_sec: u64,
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

impl Default for PushTuning {
    fn default() -> Self {
        Self {
            day_end_idle_min: 10,
            day_end_exact_tolerance_sec: 60,
            interrupt_recent_min: 5,
            meeting_overrun_min: 5,
            meeting_overrun_max_min: 180,
            focus_gap_sec: 30,
            active_slack_sec: 60,
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
pub struct PushContext {
    pub now: Millis,
    pub tz: Tz,
    /// 直近の activity_segment（派生層）。目安として直近 3 時間以上。順不同でよい。
    pub recent_segments: Vec<ActivitySegment>,
    pub idle: IdleState,
    /// 直近のロック解除 / 無操作明けイベントの時刻（なければ None）。day_start の検知に使う。
    pub resumed_at: Option<Millis>,
    /// 今日の time_block。
    pub time_blocks: Vec<TimeBlock>,
    /// 今日前後のカレンダー予定。
    pub calendar_events: Vec<CalendarEvent>,
    /// 直近の push_log（抑制ログを含む）。今週分と、応答率算出のための直近数件以上を含めること
    /// （`store::load_recent` が返す内容で足りる）。
    pub push_log: Vec<PushLog>,
    pub needs_review_count: u32,
    /// 前面ウインドウの exe 名。
    pub foreground_exe: Option<String>,
    /// 会議アプリの exe 名（`collect.meeting_exes`）。
    pub meeting_exes: Vec<String>,
    /// 直近 `switch_spike.window_min` 分の前面区間切替回数。None なら `recent_segments` から数える。
    pub recent_switches: Option<u32>,
    pub tuning: PushTuning,
}

impl PushContext {
    /// 空の状況（何も起きていない）。テストや呼び出し側の組み立ての出発点。
    pub fn new(now: Millis, tz: Tz) -> Self {
        Self {
            now,
            tz,
            recent_segments: vec![],
            idle: IdleState::default(),
            resumed_at: None,
            time_blocks: vec![],
            calendar_events: vec![],
            push_log: vec![],
            needs_review_count: 0,
            foreground_exe: None,
            meeting_exes: vec![],
            recent_switches: None,
            tuning: PushTuning::default(),
        }
    }
}

/// 判定結果。
#[derive(Debug, Clone, PartialEq)]
pub enum PushDecision {
    /// 通知を出す。呼び出し側は表示し、`store::record_decision` で push_log に残す。
    Deliver(PushPayload),
    /// 出さない。`reason` を `suppressed_reason` として push_log に残す。
    /// 同じ理由の連続抑制は返さない（ログが膨らまないように）。
    Suppress { trigger: PushTrigger, reason: String },
    /// `until` まで保留。条件が残っていれば後の評価で再び Deliver になる。ログ不要。
    Defer { trigger: PushTrigger, until: Millis },
}

/// 通知すべきものを判定する（純粋関数）。
///
/// ## 流れ
/// 1. `cfg.enabled == false` なら空。`disabled_triggers` のトリガーは判定しない（ログも残さない）。
/// 2. トリガー条件を満たしたものを [`PushTrigger::ALL`] の順に並べる。
/// 3. 各候補に抑制ルール（docs/dev/architecture.md「プッシュ通知」、`gate.rs` の doc 参照）を適用する。
/// 4. 1 回の評価で `Deliver` になるのは最大 1 件（先に通ったものだけ。残りは次回の評価に回る）。
pub fn evaluate(ctx: &PushContext, cfg: &Push) -> Vec<PushDecision> {
    if !cfg.enabled {
        return vec![];
    }
    let mut out = Vec::new();
    let mut delivered = false;
    for (trigger, payload) in triggers::candidates(ctx, cfg) {
        match gate::check(ctx, cfg, trigger) {
            gate::Gate::Pass => {
                if !delivered {
                    delivered = true;
                    out.push(PushDecision::Deliver(payload));
                }
            }
            gate::Gate::Suppress(reason) => {
                if !gate::is_repeat_suppression(ctx, trigger, &reason) {
                    out.push(PushDecision::Suppress { trigger, reason });
                }
            }
            gate::Gate::Defer(until) => out.push(PushDecision::Defer { trigger, until }),
        }
    }
    out
}
