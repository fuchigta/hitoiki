//! 抑制ルール（9.2）。トリガー条件を満たした候補に対して、出す / 抑制 / 保留を決める。
//!
//! 適用順（最初に該当したものが結果になる）:
//! 1. **スヌーズ**: push_log のいずれかの `snoozed_until` が now より先なら `snoozed`。
//!    「今日はもういい」は `apply_response` がその日の終わり（ローカル 24:00）を入れる。
//! 2. **1 日上限**: 今日（ローカル日付）の配信済み件数が `daily_limit` 以上なら `daily_limit`。
//! 3. **全体の最小間隔**: どのトリガーでも直前の配信から `tuning.min_gap_any_min` 分未満なら `recent_push`。
//! 4. **同一トリガーの最小間隔**: 直前の同トリガー配信から `min_interval_min × 係数` 分未満なら
//!    `min_interval`（係数による延長分で止まった場合は `min_interval_backoff`）。
//! 5. **会議中 / フォーカス time_block 中**は `Defer`（会議の予定終了、なければ now + `meeting_defer_min`、
//!    フォーカスブロックの終了まで）。meeting_overrun は会議中に出すためのものなので会議の保留は受けない。
//!
//! ## 応答率による間隔延長
//! 配信済みの直近 `response_window`（5）件のうち、配信から `response_grace_min`（30）分以上経った
//! ものを対象にし、`responded_at` があるものの割合を応答率とする。対象が `response_min_samples`（3）件
//! 未満なら係数 1。応答率 ≥ 0.5 なら係数 1、それ未満は線形に伸ばし、応答率 0 で係数 4（上限）。
//! `係数 = 1 + 3 × (0.5 − 応答率) / 0.5`。

use super::{PushContext, PushTrigger};
use crate::config::Push;
use crate::timefmt::{day_range, local_date_of};
use crate::Millis;

const MIN_MS: Millis = 60_000;

pub(crate) enum Gate {
    Pass,
    Suppress(String),
    Defer(Millis),
}

/// 応答率に応じた最小間隔の係数（1.0〜4.0）。
pub(crate) fn interval_factor(ctx: &PushContext) -> f64 {
    let t = &ctx.tuning;
    let grace = t.response_grace_min as Millis * MIN_MS;
    let mut sent: Vec<_> = ctx
        .push_log
        .iter()
        .filter(|l| l.delivered && ctx.now - l.at >= grace)
        .collect();
    sent.sort_by_key(|l| std::cmp::Reverse(l.at));
    sent.truncate(t.response_window);
    if sent.len() < t.response_min_samples {
        return 1.0;
    }
    let rate = sent.iter().filter(|l| l.responded_at.is_some()).count() as f64 / sent.len() as f64;
    if rate >= 0.5 {
        1.0
    } else {
        (1.0 + 3.0 * (0.5 - rate) / 0.5).clamp(1.0, 4.0)
    }
}

pub(crate) fn check(ctx: &PushContext, cfg: &Push, trigger: PushTrigger) -> Gate {
    let sup = |r: &str| Gate::Suppress(r.to_string());

    if ctx
        .push_log
        .iter()
        .any(|l| l.snoozed_until.is_some_and(|u| u > ctx.now))
    {
        return sup("snoozed");
    }

    let today = day_range(ctx.tz, local_date_of(ctx.tz, ctx.now));
    let delivered_today = ctx
        .push_log
        .iter()
        .filter(|l| l.delivered && l.at >= today.start && l.at < today.end)
        .count();
    if delivered_today >= cfg.daily_limit as usize {
        return sup("daily_limit");
    }

    let last_any = ctx.push_log.iter().filter(|l| l.delivered).map(|l| l.at).max();
    if last_any.is_some_and(|at| ctx.now - at < ctx.tuning.min_gap_any_min as Millis * MIN_MS) {
        return sup("recent_push");
    }

    let last_same = ctx
        .push_log
        .iter()
        .filter(|l| l.delivered && l.trigger == trigger.as_str())
        .map(|l| l.at)
        .max();
    if let Some(at) = last_same {
        let base = cfg.min_interval_min as f64 * MIN_MS as f64;
        let elapsed = (ctx.now - at) as f64;
        let extended = base * interval_factor(ctx);
        if elapsed < base {
            return sup("min_interval");
        }
        if elapsed < extended {
            return sup("min_interval_backoff");
        }
    }

    if trigger != PushTrigger::MeetingOverrun {
        if let Some(until) = meeting_until(ctx) {
            return Gate::Defer(until);
        }
    }
    if let Some(b) = ctx
        .time_blocks
        .iter()
        .filter(|b| b.focus && b.start_at <= ctx.now && ctx.now < b.end_at)
        .max_by_key(|b| b.end_at)
    {
        return Gate::Defer(b.end_at);
    }
    Gate::Pass
}

/// 会議中なら、保留の終了時刻。会議中の判定: 進行中の busy 予定（終日を除く）、
/// 現在の meeting 区間、会議アプリが前面。
fn meeting_until(ctx: &PushContext) -> Option<Millis> {
    let event_end = ctx
        .calendar_events
        .iter()
        .filter(|e| e.is_busy && !e.all_day && e.start_at <= ctx.now && ctx.now < e.end_at)
        .map(|e| e.end_at)
        .max();
    if event_end.is_some() {
        return event_end;
    }
    let slack = ctx.tuning.active_slack_sec as Millis * 1000;
    let in_segment = ctx.recent_segments.iter().any(|s| {
        s.kind == crate::model::SegmentKind::Meeting && s.start_at <= ctx.now && ctx.now - s.end_at <= slack
    });
    let fg_meeting = ctx
        .foreground_exe
        .as_deref()
        .is_some_and(|exe| ctx.meeting_exes.iter().any(|m| m.eq_ignore_ascii_case(exe)));
    (in_segment || fg_meeting).then(|| ctx.now + ctx.tuning.meeting_defer_min as Millis * MIN_MS)
}

/// 同じトリガーの直近の push_log 行が、同じ理由の抑制ログなら true（連続抑制は記録しない）。
/// 間に配信や別理由の抑制が入れば false になり、再び記録される。
pub(crate) fn is_repeat_suppression(ctx: &PushContext, trigger: PushTrigger, reason: &str) -> bool {
    ctx.push_log
        .iter()
        .filter(|l| l.trigger == trigger.as_str())
        .max_by_key(|l| l.at)
        .is_some_and(|l| !l.delivered && l.suppressed_reason.as_deref() == Some(reason))
}
