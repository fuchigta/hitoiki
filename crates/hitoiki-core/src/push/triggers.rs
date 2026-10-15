//! トリガー条件の判定（docs/dev/architecture.md「プッシュ通知」）。ここでは「条件を満たすか」だけを見る。抑制は `gate.rs`。
//!
//! 共通ルール: day_end 以外は、無操作中（`ctx.idle.is_idle`）には発火しない。
//! 「1 日 1 回」系は、今日の push_log に配信済み（delivered）があれば発火しない。

use super::{payload, PushContext, PushPayload, PushTrigger, TaskNudge};
use crate::config::Push;
use crate::model::{ActivitySegment, SegmentKind};
use crate::timefmt::{day_range, local_date_of, to_local, week_range};
use crate::Millis;
use chrono::{Datelike, Duration, NaiveTime, TimeZone, Weekday};

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

/// 作業系の区間（時間を使っている区間）か。無視（`ignored`）・無操作・休憩・割り込みは含めない。
fn is_working(s: &ActivitySegment) -> bool {
    matches!(
        s.kind,
        SegmentKind::Work | SegmentKind::AgentDialog | SegmentKind::AgentWait
    )
}

/// 「同じタスク/活動」を表すキー。task_id > agent セッション > activity。
fn focus_key(s: &ActivitySegment) -> Option<String> {
    if let Some(t) = &s.task_id {
        Some(format!("task:{t}"))
    } else if let Some(a) = &s.agent_session_id {
        Some(format!("agent:{a}"))
    } else {
        s.activity.as_ref().map(|l| format!("label:{l}"))
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
        PushTrigger::IdleReturn => idle_return(ctx, cfg),
        PushTrigger::BlockStart => block_start(ctx, segs),
        PushTrigger::DayEnd => day_end(ctx, cfg),
        PushTrigger::Weekly => weekly(ctx, cfg),
        PushTrigger::UrgentTask => urgent_task(ctx, cfg, segs),
        PushTrigger::ImportantTask => important_task(ctx, cfg, segs),
        PushTrigger::KrIdle => kr_idle(ctx, cfg),
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

/// idle_return: 予定外の無操作が `push.idle_return_min` 分以上続いたあとに戻ってきた（無操作の終了から
/// `tuning.idle_return_recent_min` 分以内）。次の場合は尋ねない。
/// - 無操作の間に busy な予定（終日を除く）が重なっていた（会議など、予定どおり）。
/// - その時間がすでに分類済み（休憩・会議・確定）。長いロックの自動休憩もここに含まれる。
/// - その無操作について配信済み。
///
/// `push.idle_return_min = 0` で無効。
fn idle_return(ctx: &PushContext, cfg: &Push) -> Option<PushPayload> {
    if cfg.idle_return_min == 0 {
        return None;
    }
    let idle = ctx.last_idle?;
    let len = idle.end_at - idle.start_at;
    if len < cfg.idle_return_min as Millis * MIN_MS
        || idle.end_at > ctx.now
        || ctx.now - idle.end_at > ctx.tuning.idle_return_recent_min as Millis * MIN_MS
    {
        return None;
    }
    let overlaps = |s: Millis, e: Millis| s < idle.end_at && e > idle.start_at;
    if ctx
        .calendar_events
        .iter()
        .any(|e| e.is_busy && !e.all_day && overlaps(e.start_at, e.end_at))
    {
        return None;
    }
    let candidate = idle_candidate_task(ctx, idle.start_at, idle.end_at);
    // ロックの自動休憩（decided_by `rule:locked` で未確定）は、候補のタスクがあるときだけ「分類済み」とみなさない。
    let auto_locked = |s: &ActivitySegment| {
        s.kind == SegmentKind::Break
            && s.status != crate::model::SegmentStatus::Confirmed
            && s.decided_by.as_deref() == Some("rule:locked")
    };
    let classified = ctx.recent_segments.iter().any(|s| {
        overlaps(s.start_at, s.end_at)
            && (matches!(s.kind, SegmentKind::Break | SegmentKind::Meeting)
                || s.status == crate::model::SegmentStatus::Confirmed)
            && !(candidate.is_some() && auto_locked(s))
    });
    if classified || delivered_since(ctx, PushTrigger::IdleReturn, idle.start_at) {
        return None;
    }
    Some(payload::idle_return(
        idle.start_at,
        idle.end_at,
        candidate.map(|t| (t.id.as_str(), t.title.as_str())),
    ))
}

/// 無操作の期間 [start, end) に「していた」候補のタスク。
/// (1) doing ログが開いていたタスク、なければ (2) タスク付きの time_block が重なっていたタスクで、
/// それぞれ重なりが最も長い 1 件（同じ長さなら先に始まったもの）。doing を優先する。
fn idle_candidate_task(ctx: &PushContext, start: Millis, end: Millis) -> Option<&super::TaskRef> {
    let overlap = |s: Millis, e: Millis| (e.min(end) - s.max(start)).max(0);
    // 重なりが最長（同じなら開始が早い、さらに同じなら ID 順）の 1 件。
    fn longest(items: Vec<(Millis, Millis, &str)>) -> Option<(Millis, Millis, &str)> {
        items
            .into_iter()
            .filter(|&(len, _, _)| len > 0)
            .min_by(|a, b| (std::cmp::Reverse(a.0), a.1, a.2).cmp(&(std::cmp::Reverse(b.0), b.1, b.2)))
    }
    let doing = longest(
        ctx.doing_spans
            .iter()
            .map(|d| {
                (
                    overlap(d.start_at, d.end_at.unwrap_or(end)),
                    d.start_at,
                    d.task_id.as_str(),
                )
            })
            .collect(),
    );
    let (_, _, id) = doing.or_else(|| {
        longest(
            ctx.time_blocks
                .iter()
                .filter_map(|b| Some((overlap(b.start_at, b.end_at), b.start_at, b.task_id.as_deref()?)))
                .collect(),
        )
    })?;
    ctx.task_refs.iter().find(|t| t.id == id)
}

/// block_start: 今日の time_block のうち、タスクがあり、開始を過ぎて `tuning.block_start_window_min` 分以内
/// （かつ終了前）のもの。次の場合は出さない。
/// - そのタスクがすでに doing、いまの作業区間がそのタスク、または完了・中止済み。
/// - そのブロックについて配信済み（push_log のペイロードの `block_id`）。
///
/// 候補が複数なら、開始が最も新しいブロック。タスクの情報（`ctx.task_refs`）が無いブロックは対象外。
/// 無操作中は共通ルールで出ない。抑制の例外は `gate.rs`。
fn block_start(ctx: &PushContext, segs: &[&ActivitySegment]) -> Option<PushPayload> {
    let window = ctx.tuning.block_start_window_min as Millis * MIN_MS;
    let active = active_task_id(ctx, segs);
    let sent: Vec<String> = ctx
        .push_log
        .iter()
        .filter(|l| l.delivered && l.trigger == PushTrigger::BlockStart.as_str())
        .filter_map(|l| {
            serde_json::from_str::<PushPayload>(&l.payload_json)
                .ok()?
                .block_id
        })
        .collect();
    let (block, task) = ctx
        .time_blocks
        .iter()
        .filter(|b| b.start_at <= ctx.now && ctx.now - b.start_at <= window && ctx.now < b.end_at)
        .filter(|b| !sent.contains(&b.id))
        .filter_map(|b| {
            let task = ctx
                .task_refs
                .iter()
                .find(|t| Some(t.id.as_str()) == b.task_id.as_deref())?;
            (task.status == crate::model::TaskStatus::Todo && active.as_deref() != Some(task.id.as_str()))
                .then_some((b, task))
        })
        .max_by_key(|(b, _)| (b.start_at, b.id.clone()))?;
    Some(payload::block_start(
        &block.id,
        &task.id,
        &task.title,
        &local_time(ctx, block.start_at).format("%H:%M").to_string(),
        &local_time(ctx, block.end_at).format("%H:%M").to_string(),
    ))
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

/// long_focus: 同一キー（task_id / agent セッション / activity）の作業系区間が、
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
    Some(payload::long_focus(minutes as u64, last.activity.as_deref()))
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
            .filter(|s| {
                s.end_at > from
                    && !matches!(
                        s.kind,
                        SegmentKind::Idle | SegmentKind::Break | SegmentKind::Ignored
                    )
            })
            .count();
        cnt.saturating_sub(1) as u32
    });
    (n > cfg.switch_spike.count).then(|| payload::switch_spike(window, n))
}

/// after_interrupt: interrupt 区間が終わっていて（その後に interrupt でない別の区間が始まっている）、
/// 終了から `interrupt_recent_min` 分以内で、その割り込みの開始以降にまだ配信していない。
/// 進行中の割り込み区間は終了時刻が再計算のたびに伸びるため、後続区間の有無で終了を判定する。
fn after_interrupt(ctx: &PushContext, segs: &[&ActivitySegment]) -> Option<PushPayload> {
    let recent = ctx.tuning.interrupt_recent_min as Millis * MIN_MS;
    let seg = segs.iter().rev().find(|s| {
        s.kind == SegmentKind::Interrupt
            && s.end_at <= ctx.now
            && ctx.now - s.end_at <= recent
            && segs
                .iter()
                .any(|n| n.kind != SegmentKind::Interrupt && n.start_at >= s.end_at)
    })?;
    // 配信は割り込みの終了後にしか起きないので、開始時刻を基準にすれば終了時刻が多少動いても重複しない。
    (!delivered_since(ctx, PushTrigger::AfterInterrupt, seg.start_at)).then(payload::after_interrupt)
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

/// late_work: 「遅い時間帯」（`late_work_after` 〜 翌朝の `day_start_after`、日付をまたぐ）に、
/// いま作業が続いている。1 回の遅い時間帯につき 1 回（時間帯の開始日単位）。
fn late_work(ctx: &PushContext, cfg: &Push, segs: &[&ActivitySegment]) -> Option<PushPayload> {
    let after = parse_hm(&cfg.late_work_after)?;
    let morning = parse_hm(&cfg.day_start_after).unwrap_or(NaiveTime::MIN);
    let now_t = local_time(ctx, ctx.now);
    let today = local_date_of(ctx.tz, ctx.now);
    // 遅い時間帯の開始日。
    let start_date = if now_t >= after {
        today
    } else if now_t < morning {
        today - Duration::days(1)
    } else {
        return None;
    };
    active_work_segment(ctx, segs)?;
    let since = ctx
        .tz
        .from_local_datetime(&start_date.and_time(after))
        .earliest()
        .map(|d| d.timestamp_millis())
        .unwrap_or_else(|| day_range(ctx.tz, start_date).start);
    (!delivered_since(ctx, PushTrigger::LateWork, since)).then(payload::late_work)
}

/// 勤務時間内（`day_start_after` ≤ 現在 < `day_end_at`）か。時刻の設定が読めなければ、その側は制限しない。
fn in_work_hours(ctx: &PushContext, cfg: &Push) -> bool {
    let now_t = local_time(ctx, ctx.now);
    parse_hm(&cfg.day_start_after).is_none_or(|s| now_t >= s)
        && parse_hm(&cfg.day_end_at).is_none_or(|e| now_t < e)
}

/// 通知ログのペイロードから、対象のタスク ID を読む。
fn log_task_id(l: &crate::model::PushLog) -> Option<String> {
    serde_json::from_str::<PushPayload>(&l.payload_json).ok()?.task_id
}

/// いま取り組んでいるタスクの ID（作業系の最終区間が割り当てられているタスク）。
fn active_task_id(ctx: &PushContext, segs: &[&ActivitySegment]) -> Option<String> {
    active_work_segment(ctx, segs)?.task_id.clone()
}

/// 今日の time_block が、そのタスクに割り当てられているか。`only_upcoming` なら終了がまだ先のものだけ。
fn has_block(ctx: &PushContext, task_id: &str, only_upcoming: bool) -> bool {
    ctx.time_blocks
        .iter()
        .any(|b| b.task_id.as_deref() == Some(task_id) && (!only_upcoming || b.end_at > ctx.now))
}

/// urgent_task: 勤務時間内に、緊急のしるしが付いた未完了のタスクがあり、それに取り組んでいない。
/// 次のタスクは対象から外す。
/// - いま作業している（作業系の最終区間の割り当て、または doing）。対象のどれかに取り組み中なら、そもそも出さない。
/// - 今日の time_block が割り当てられ、終了がまだ先（予定済み・進行中）。
///
/// 前回の urgent_task の配信から `push.urgent_task_interval_min` 分以上（0 で無効）。
/// 示すのは、最も長く通知していないもの（同順なら作成が古いもの）。通知した履歴は `ctx.push_log`
/// （今週分＋直近）のペイロードのタスク ID で見る。
fn urgent_task(ctx: &PushContext, cfg: &Push, segs: &[&ActivitySegment]) -> Option<PushPayload> {
    if cfg.urgent_task_interval_min == 0 || !in_work_hours(ctx, cfg) {
        return None;
    }
    let urgent: Vec<&TaskNudge> = ctx.tasks.iter().filter(|t| t.urgent).collect();
    if urgent.is_empty() {
        return None;
    }
    let active = active_task_id(ctx, segs);
    if urgent
        .iter()
        .any(|t| t.doing || active.as_deref() == Some(t.id.as_str()))
    {
        return None;
    }
    let sent: Vec<&crate::model::PushLog> = ctx
        .push_log
        .iter()
        .filter(|l| l.delivered && l.trigger == PushTrigger::UrgentTask.as_str())
        .collect();
    let interval = cfg.urgent_task_interval_min as Millis * MIN_MS;
    if sent.iter().any(|l| ctx.now - l.at < interval) {
        return None;
    }
    let mut cands: Vec<&TaskNudge> = urgent
        .into_iter()
        .filter(|t| !has_block(ctx, &t.id, true))
        .collect();
    // 最後に通知した時刻（無ければ 0）が古い順、同じなら作成が古い順、さらに同じなら ID 順。
    let last_notified = |t: &TaskNudge| {
        sent.iter()
            .filter(|l| log_task_id(l).as_deref() == Some(t.id.as_str()))
            .map(|l| l.at)
            .max()
            .unwrap_or(0)
    };
    cands.sort_by(|a, b| {
        (last_notified(a), a.created_at, &a.id).cmp(&(last_notified(b), b.created_at, &b.id))
    });
    let first = cands.first()?;
    Some(payload::urgent_task(&first.id, &first.title, cands.len() - 1))
}

/// kr_idle の材料（今週まだ時間が入っていない成果の指標）を読む必要があるか。無効でなく、設定の曜日に達していて、
/// 今週（月曜始まり）まだ配信していないとき true。毎回の評価で集計し直さないための絞り込みにも使う。
pub fn kr_idle_due(cfg: &Push, tz: chrono_tz::Tz, now: Millis, push_log: &[crate::model::PushLog]) -> bool {
    if !(1..=7).contains(&cfg.kr_idle_weekday) || cfg.disabled_triggers.iter().any(|d| d == "kr_idle") {
        return false;
    }
    let local = to_local(tz, now);
    if local.weekday().num_days_from_monday() + 1 < cfg.kr_idle_weekday {
        return false;
    }
    let week = week_range(tz, local_date_of(tz, now));
    !push_log
        .iter()
        .any(|l| l.delivered && l.trigger == PushTrigger::KrIdle.as_str() && l.at >= week.start)
}

/// kr_idle: 設定の曜日（`push.kr_idle_weekday`。1=月〜7=日、0 で無効）以降の勤務時間内に、今週まだ時間が入っていない
/// 成果の指標（`ctx.kr_idle`）があれば 1 回だけ事実を伝える。週は月曜始まりで、今週すでに配信していれば出さない。
/// 対象の成果の指標の選び方（数え方・目標の期間・スキルまたは紐付けタスクの有無）は `store::load_kr_idle`。
fn kr_idle(ctx: &PushContext, cfg: &Push) -> Option<PushPayload> {
    if ctx.kr_idle.is_empty() || !in_work_hours(ctx, cfg) || !kr_idle_due(cfg, ctx.tz, ctx.now, &ctx.push_log)
    {
        return None;
    }
    let titles: Vec<&str> = ctx.kr_idle.iter().map(|k| k.title.as_str()).collect();
    Some(payload::kr_idle(&titles))
}

/// important_task: 重要（緊急でない）な未完了のタスクのうち、最後に手をつけた時刻（作業系の区間の最終終了と
/// 作成時刻の大きいほう）が `push.important_stale_days` 日以上前のものを、最も古いもの 1 件示す（0 で無効）。
/// 今日の time_block に割り当て済み、いま取り組み中（doing を含む）、今週この通知に「今週は見送る」と
/// 答えたタスクは除く。勤務時間内に 1 日 1 回まで。
fn important_task(ctx: &PushContext, cfg: &Push, segs: &[&ActivitySegment]) -> Option<PushPayload> {
    if cfg.important_stale_days == 0
        || !in_work_hours(ctx, cfg)
        || delivered_today(ctx, PushTrigger::ImportantTask)
    {
        return None;
    }
    let active = active_task_id(ctx, segs);
    let week = week_range(ctx.tz, local_date_of(ctx.tz, ctx.now));
    let skipped: Vec<String> = ctx
        .push_log
        .iter()
        .filter(|l| l.delivered && l.at >= week.start && l.trigger == PushTrigger::ImportantTask.as_str())
        .filter(|l| {
            l.response_json
                .as_deref()
                .and_then(|j| serde_json::from_str::<super::PushResponse>(j).ok())
                .is_some_and(|r| r.action.as_deref() == Some(payload::TASK_SKIP_WEEK))
        })
        .filter_map(log_task_id)
        .collect();
    let stale = cfg.important_stale_days as Millis * 24 * 60 * MIN_MS;
    let last_touch = |t: &TaskNudge| t.last_worked_at.unwrap_or(0).max(t.created_at);
    let first = ctx
        .tasks
        .iter()
        .filter(|t| t.important && !t.urgent)
        .filter(|t| !t.doing && active.as_deref() != Some(t.id.as_str()))
        .filter(|t| !has_block(ctx, &t.id, false) && !skipped.contains(&t.id))
        .filter(|t| ctx.now - last_touch(t) >= stale)
        .min_by(|a, b| (last_touch(a), a.created_at, &a.id).cmp(&(last_touch(b), b.created_at, &b.id)))?;
    let days = first
        .last_worked_at
        .map(|_| ((ctx.now - last_touch(first)) / (24 * 60 * MIN_MS)) as u64);
    Some(payload::important_task(&first.id, &first.title, days))
}
