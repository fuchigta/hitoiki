//! トリガー条件の判定（9.1）。ここでは「条件を満たすか」だけを見る。抑制は `gate.rs`。
//!
//! 共通ルール: day_end 以外は、無操作中（`ctx.idle.is_idle`）には発火しない。
//! 「1 日 1 回」系は、今日の push_log に配信済み（delivered）があれば発火しない。

use super::{payload, PushContext, PushPayload, PushTrigger};
use crate::config::Push;
use crate::model::{ActivitySegment, SegmentKind};
use crate::timefmt::{day_range, local_date_of, to_local, week_range};
use crate::Millis;
use chrono::{Datelike, NaiveTime, Weekday};

const MIN_MS: Millis = 60_000;

pub(crate) fn parse_hm(s: &str) -> Option<NaiveTime> {
    NaiveTime::parse_from_str(s.trim(), "%H:%M").ok()
}

fn parse_weekday(s: &str) -> Option<Weekday> {
    let s = s.trim().to_ascii_lowercase();
    match s.get(..3)? {
        "mon" => Some(Weekday::Mon),
        "tue" => Some(Weekday::Tue),
        "wed" => Some(Weekday::Wed),
        "thu" => Some(Weekday::Thu),
        "fri" => Some(Weekday::Fri),
        "sat" => Some(Weekday::Sat),
        "sun" => Some(Weekday::Sun),
        _ => None,
    }
}

fn local_time(ctx: &PushContext, ms: Millis) -> NaiveTime {
    to_local(ctx.tz, ms).time()
}

/// 指定トリガーが `since` 以降に配信済みか。
pub(crate) fn delivered_since(ctx: &PushContext, trigger: PushTrigger, since: Millis) -> bool {
    ctx.push_log
        .iter()
        .any(|l| l.delivered && l.trigger == trigger.as_str() && l.at >= since)
}

fn delivered_today(ctx: &PushContext, trigger: PushTrigger) -> bool {
    let day = day_range(ctx.tz, local_date_of(ctx.tz, ctx.now));
    delivered_since(ctx, trigger, day.start)
}

/// 作業系の区間（時間を使っている区間）か。
fn is_working(s: &ActivitySegment) -> bool {
    matches!(
        s.kind,
        SegmentKind::Work | SegmentKind::AgentDialog | SegmentKind::AgentWatch
    )
}

/// 「同じタスク/活動」を表すキー。task_id > agent セッション > activity_label。
fn focus_key(s: &ActivitySegment) -> Option<String> {
    if let Some(t) = &s.task_id {
        Some(format!("task:{t}"))
    } else if let Some(a) = &s.agent_session_id {
        Some(format!("agent:{a}"))
    } else {
        s.activity_label.as_ref().map(|l| format!("label:{l}"))
    }
}

fn sorted_segments(ctx: &PushContext) -> Vec<&ActivitySegment> {
    let mut v: Vec<&ActivitySegment> = ctx
        .recent_segments
        .iter()
        .filter(|s| s.start_at <= ctx.now)
        .collect();
    v.sort_by_key(|s| (s.start_at, s.end_at));
    v
}

/// 「いま作業中」の最終区間（作業系で、終了が now から slack 以内）。
fn active_work_segment<'a>(ctx: &PushContext, segs: &[&'a ActivitySegment]) -> Option<&'a ActivitySegment> {
    let last = *segs.last()?;
    let slack = ctx.tuning.active_slack_sec as Millis * 1000;
    (is_working(last) && ctx.now - last.end_at <= slack).then_some(last)
}

/// 条件を満たしたトリガーと payload を、優先順（`PushTrigger::ALL`）に返す。
pub(crate) fn candidates(ctx: &PushContext, cfg: &Push) -> Vec<(PushTrigger, PushPayload)> {
    let segs = sorted_segments(ctx);
    let mut out = Vec::new();
    for &t in PushTrigger::ALL {
        if cfg.disabled_triggers.iter().any(|d| d == t.as_str()) {
            continue;
        }
        if let Some(p) = check(ctx, cfg, &segs, t) {
            out.push((t, p));
        }
    }
    out
}

fn check(ctx: &PushContext, cfg: &Push, segs: &[&ActivitySegment], t: PushTrigger) -> Option<PushPayload> {
    if ctx.idle.is_idle && t != PushTrigger::DayEnd {
        return None;
    }
    match t {
        PushTrigger::DayStart => day_start(ctx, cfg),
        PushTrigger::DayEnd => day_end(ctx, cfg),
        PushTrigger::Weekly => weekly(ctx, cfg),
        PushTrigger::LongFocus => long_focus(ctx, cfg, segs),
        PushTrigger::SwitchSpike => switch_spike(ctx, cfg, segs),
        PushTrigger::AfterInterrupt => after_interrupt(ctx, segs),
        PushTrigger::MeetingOverrun => meeting_overrun(ctx),
        PushTrigger::LateWork => late_work(ctx, cfg, segs),
    }
}

/// day_start: 今日のロック解除/無操作明けがあり、`day_start_after` 以降、`day_end_at` 前で、
/// 今日まだ配信していない。
fn day_start(ctx: &PushContext, cfg: &Push) -> Option<PushPayload> {
    let resumed = ctx.resumed_at?;
    let after = parse_hm(&cfg.day_start_after)?;
    let today = day_range(ctx.tz, local_date_of(ctx.tz, ctx.now));
    if resumed < today.start || resumed > ctx.now {
        return None;
    }
    let now_t = local_time(ctx, ctx.now);
    if now_t < after || parse_hm(&cfg.day_end_at).is_some_and(|e| now_t >= e) {
        return None;
    }
    (!delivered_today(ctx, PushTrigger::DayStart)).then(payload::day_start)
}

/// day_end: `day_end_at` 以降で `tuning.day_end_idle_min` 分以上無操作、
/// または `day_end_at` ちょうど（±`day_end_exact_tolerance_sec`）。1 日 1 回。
fn day_end(ctx: &PushContext, cfg: &Push) -> Option<PushPayload> {
    let end = parse_hm(&cfg.day_end_at)?;
    let now_t = local_time(ctx, ctx.now);
    if now_t < end && (end - now_t).num_seconds() > ctx.tuning.day_end_exact_tolerance_sec {
        return None;
    }
    let exact = (now_t - end).num_seconds().abs() <= ctx.tuning.day_end_exact_tolerance_sec;
    let idle_long = ctx.idle.since_last_input_sec >= ctx.tuning.day_end_idle_min * 60;
    if !(exact || idle_long) || delivered_today(ctx, PushTrigger::DayEnd) {
        return None;
    }
    Some(payload::day_end(ctx.needs_review_count))
}

/// weekly: 今週の（設定曜日, 時刻）以降で、今週まだ配信していない（週は月曜始まり）。
fn weekly(ctx: &PushContext, cfg: &Push) -> Option<PushPayload> {
    let wd = parse_weekday(&cfg.weekly.weekday)?;
    let at = parse_hm(&cfg.weekly.at)?;
    let local = to_local(ctx.tz, ctx.now);
    let reached = (local.weekday().num_days_from_monday(), local.time()) >= (wd.num_days_from_monday(), at);
    let week = week_range(ctx.tz, local_date_of(ctx.tz, ctx.now));
    (reached && !delivered_since(ctx, PushTrigger::Weekly, week.start)).then(payload::weekly)
}

/// long_focus: 同一キー（task_id / agent セッション / activity_label）の作業系区間が、
/// 隙間 `focus_gap_sec` 以内でつながったまま `long_focus_min` 分以上続いている。
/// 間に idle 等の別区間が挟まれば途切れる。同じ連続の中で再通知するのは、前回配信から
/// さらに `long_focus_min` 分経ってから。
fn long_focus(ctx: &PushContext, cfg: &Push, segs: &[&ActivitySegment]) -> Option<PushPayload> {
    let last = active_work_segment(ctx, segs)?;
    let key = focus_key(last)?;
    let gap = ctx.tuning.focus_gap_sec as Millis * 1000;
    let mut start = last.start_at;
    for prev in segs.iter().rev().skip(1) {
        if prev.end_at + gap < start || !is_working(prev) || focus_key(prev).as_ref() != Some(&key) {
            break;
        }
        start = start.min(prev.start_at);
    }
    let minutes = (last.end_at.min(ctx.now) - start) / MIN_MS;
    if minutes < cfg.long_focus_min as Millis {
        return None;
    }
    let last_push = ctx
        .push_log
        .iter()
        .filter(|l| l.delivered && l.trigger == PushTrigger::LongFocus.as_str() && l.at >= start)
        .map(|l| l.at)
        .max();
    if last_push.is_some_and(|at| ctx.now - at < cfg.long_focus_min as Millis * MIN_MS) {
        return None;
    }
    Some(payload::long_focus(
        minutes as u64,
        last.activity_label.as_deref(),
    ))
}

/// switch_spike: 直近 `window_min` 分の切替回数が `count` を超えている。
/// `ctx.recent_switches` があればそれを使い、なければ窓内の作業系・会議・割り込み区間の数 - 1
/// （区間の境界数）で数える。
fn switch_spike(ctx: &PushContext, cfg: &Push, segs: &[&ActivitySegment]) -> Option<PushPayload> {
    let window = cfg.switch_spike.window_min;
    let n = ctx.recent_switches.unwrap_or_else(|| {
        let from = ctx.now - window as Millis * MIN_MS;
        let cnt = segs
            .iter()
            .filter(|s| s.end_at > from && !matches!(s.kind, SegmentKind::Idle | SegmentKind::Break))
            .count();
        cnt.saturating_sub(1) as u32
    });
    (n > cfg.switch_spike.count).then(|| payload::switch_spike(window, n))
}

/// after_interrupt: interrupt 区間が `interrupt_recent_min` 分以内に終了していて、
/// その終了後にまだ配信していない。
fn after_interrupt(ctx: &PushContext, segs: &[&ActivitySegment]) -> Option<PushPayload> {
    let recent = ctx.tuning.interrupt_recent_min as Millis * MIN_MS;
    let seg = segs
        .iter()
        .rev()
        .find(|s| s.kind == SegmentKind::Interrupt && s.end_at <= ctx.now && ctx.now - s.end_at <= recent)?;
    (!delivered_since(ctx, PushTrigger::AfterInterrupt, seg.end_at)).then(payload::after_interrupt)
}

/// meeting_overrun: 会議アプリが前面で、予定（終日でない）の終了から `meeting_overrun_min` 分以上
/// 過ぎている（`meeting_overrun_max_min` 以内）。ほかの予定が進行中の場合は延長ではないので対象外。
/// その予定の終了後にまだ配信していない。
fn meeting_overrun(ctx: &PushContext) -> Option<PushPayload> {
    let exe = ctx.foreground_exe.as_deref()?;
    if !ctx.meeting_exes.iter().any(|m| m.eq_ignore_ascii_case(exe)) {
        return None;
    }
    let events = || ctx.calendar_events.iter().filter(|e| !e.all_day);
    if events().any(|e| e.start_at <= ctx.now && ctx.now < e.end_at) {
        return None;
    }
    let min = ctx.tuning.meeting_overrun_min as Millis * MIN_MS;
    let max = ctx.tuning.meeting_overrun_max_min as Millis * MIN_MS;
    let ev = events()
        .filter(|e| ctx.now - e.end_at >= min && ctx.now - e.end_at <= max)
        .max_by_key(|e| e.end_at)?;
    (!delivered_since(ctx, PushTrigger::MeetingOverrun, ev.end_at)).then(payload::meeting_overrun)
}

/// late_work: `late_work_after` 以降（同じ日の時刻として）に、いま作業が続いている。1 日 1 回。
fn late_work(ctx: &PushContext, cfg: &Push, segs: &[&ActivitySegment]) -> Option<PushPayload> {
    let after = parse_hm(&cfg.late_work_after)?;
    if local_time(ctx, ctx.now) < after {
        return None;
    }
    active_work_segment(ctx, segs)?;
    (!delivered_today(ctx, PushTrigger::LateWork)).then(payload::late_work)
}
