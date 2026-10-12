use super::store::{self, PushResponse};
use super::*;
use crate::db;
use crate::model::{ActivitySegment, CalendarEvent, PushLog, SegmentKind, SegmentStatus, TimeBlock};
use chrono::TimeZone;

const TZ: Tz = chrono_tz::Asia::Tokyo;
const MIN: Millis = 60_000;

/// 2026-10-<day> <h>:<m>（Asia/Tokyo）。12 日が月曜、16 日が金曜。
fn t(day: u32, h: u32, m: u32) -> Millis {
    TZ.with_ymd_and_hms(2026, 10, day, h, m, 0)
        .unwrap()
        .timestamp_millis()
}

fn cfg() -> Push {
    Push::default()
}

fn seg(kind: SegmentKind, start: Millis, end: Millis) -> ActivitySegment {
    ActivitySegment {
        id: format!("seg{start}"),
        start_at: start,
        end_at: end,
        kind,
        task_id: None,
        activity_label: None,
        confidence: None,
        decided_by: None,
        status: SegmentStatus::Auto,
        window_span_ids: vec![],
        agent_session_id: None,
        exe_name: None,
        title: None,
        offline: false,
    }
}

fn work(task: &str, start: Millis, end: Millis) -> ActivitySegment {
    let mut s = seg(SegmentKind::Work, start, end);
    s.task_id = Some(task.into());
    s
}

fn log(trigger: &str, at: Millis, delivered: bool) -> PushLog {
    PushLog {
        id: format!("log{at}{trigger}"),
        at,
        trigger: trigger.into(),
        payload_json: "{}".into(),
        delivered,
        responded_at: None,
        response_json: None,
        snoozed_until: None,
        suppressed_reason: (!delivered).then(|| "x".into()),
    }
}

fn event(start: Millis, end: Millis) -> CalendarEvent {
    CalendarEvent {
        id: "e".into(),
        source: "ics".into(),
        external_id: "e".into(),
        start_at: start,
        end_at: end,
        all_day: false,
        title: "定例".into(),
        location: None,
        is_busy: true,
        fetched_at: 0,
    }
}

fn delivered(ds: &[PushDecision]) -> Vec<PushTrigger> {
    ds.iter()
        .filter_map(|d| match d {
            PushDecision::Deliver(p) => Some(p.trigger),
            _ => None,
        })
        .collect()
}

#[test]
fn day_start_fires_after_resume() {
    let mut ctx = PushContext::new(t(12, 9, 0), TZ);
    ctx.resumed_at = Some(t(12, 8, 55));
    let ds = evaluate(&ctx, &cfg());
    assert_eq!(delivered(&ds), vec![PushTrigger::DayStart]);
    let PushDecision::Deliver(p) = &ds[0] else {
        panic!()
    };
    assert_eq!(p.actions.len(), 5);
    assert_eq!(p.open_screen.as_deref(), Some("today/design"));
}

#[test]
fn day_start_not_before_threshold_or_twice_or_without_resume() {
    let mut ctx = PushContext::new(t(12, 7, 30), TZ);
    ctx.resumed_at = Some(t(12, 7, 20));
    assert!(evaluate(&ctx, &cfg()).is_empty());

    let mut ctx = PushContext::new(t(12, 9, 0), TZ);
    assert!(evaluate(&ctx, &cfg()).is_empty());
    // 昨日の解除は対象外。
    ctx.resumed_at = Some(t(11, 8, 30));
    assert!(evaluate(&ctx, &cfg()).is_empty());
    // 今日すでに配信済み。
    ctx.resumed_at = Some(t(12, 8, 30));
    ctx.push_log.push(log("day_start", t(12, 8, 31), true));
    assert!(delivered(&evaluate(&ctx, &cfg())).is_empty());
}

#[test]
fn disabled_trigger_and_global_off() {
    let mut ctx = PushContext::new(t(12, 9, 0), TZ);
    ctx.resumed_at = Some(t(12, 8, 55));
    let mut c = cfg();
    c.disabled_triggers = vec!["day_start".into()];
    assert!(evaluate(&ctx, &c).is_empty());
    let mut c = cfg();
    c.enabled = false;
    assert!(evaluate(&ctx, &c).is_empty());
}

#[test]
fn day_end_by_idle_or_exact_once() {
    // 18:35、10 分無操作 → 発火。
    let mut ctx = PushContext::new(t(12, 18, 45), TZ);
    ctx.idle = IdleState {
        is_idle: true,
        since_last_input_sec: 11 * 60,
    };
    ctx.needs_review_count = 3;
    let ds = evaluate(&ctx, &cfg());
    let PushDecision::Deliver(p) = &ds[0] else {
        panic!("{ds:?}")
    };
    assert_eq!(p.trigger, PushTrigger::DayEnd);
    assert!(p.body.contains("3 件"));

    // 無操作が短ければ出ない。
    ctx.idle.since_last_input_sec = 60;
    ctx.idle.is_idle = false;
    assert!(evaluate(&ctx, &cfg()).is_empty());

    // ちょうど（18:30:30 など ±1 分）。
    let mut ctx = PushContext::new(t(12, 18, 30) + 30_000, TZ);
    assert_eq!(delivered(&evaluate(&ctx, &cfg())), vec![PushTrigger::DayEnd]);
    // 1 日 1 回。
    ctx.push_log.push(log("day_end", t(12, 18, 30), true));
    ctx.now = t(12, 19, 30);
    ctx.idle.since_last_input_sec = 3600;
    assert!(evaluate(&ctx, &cfg()).is_empty());
    // 終業時刻前は出ない。
    let mut ctx = PushContext::new(t(12, 17, 0), TZ);
    ctx.idle.since_last_input_sec = 3600;
    assert!(evaluate(&ctx, &cfg()).is_empty());
}

#[test]
fn weekly_fires_from_configured_time_once_per_week() {
    // 金曜 17:00（既定）。
    let ctx = PushContext::new(t(16, 17, 5), TZ);
    assert_eq!(delivered(&evaluate(&ctx, &cfg())), vec![PushTrigger::Weekly]);
    // 木曜は出ない。
    let ctx = PushContext::new(t(15, 17, 5), TZ);
    assert!(evaluate(&ctx, &cfg()).is_empty());
    // 金曜 16:59 は出ない。
    let ctx = PushContext::new(t(16, 16, 59), TZ);
    assert!(evaluate(&ctx, &cfg()).is_empty());
    // 週末に持ち越し（今週まだ出ていない）。
    let ctx = PushContext::new(t(17, 10, 0), TZ);
    assert_eq!(delivered(&evaluate(&ctx, &cfg())), vec![PushTrigger::Weekly]);
    // 今週配信済みなら出ない。
    let mut ctx = PushContext::new(t(17, 10, 0), TZ);
    ctx.push_log.push(log("weekly", t(16, 17, 5), true));
    assert!(evaluate(&ctx, &cfg()).is_empty());
}

#[test]
fn long_focus_fires_on_continuous_same_task() {
    let now = t(12, 11, 0);
    let mut ctx = PushContext::new(now, TZ);
    ctx.recent_segments = vec![
        work("T1", t(12, 9, 0), t(12, 10, 0)),
        work("T1", t(12, 10, 0), now),
    ];
    let ds = evaluate(&ctx, &cfg());
    assert_eq!(delivered(&ds), vec![PushTrigger::LongFocus]);
}

#[test]
fn long_focus_not_fired_when_short_broken_or_other_task() {
    let now = t(12, 11, 0);
    // 60 分だけ。
    let mut ctx = PushContext::new(now, TZ);
    ctx.recent_segments = vec![work("T1", t(12, 10, 0), now)];
    assert!(evaluate(&ctx, &cfg()).is_empty());
    // idle で途切れている。
    ctx.recent_segments = vec![
        work("T1", t(12, 9, 0), t(12, 10, 0)),
        seg(SegmentKind::Idle, t(12, 10, 0), t(12, 10, 10)),
        work("T1", t(12, 10, 10), now),
    ];
    assert!(evaluate(&ctx, &cfg()).is_empty());
    // 隙間が大きい。
    ctx.recent_segments = vec![
        work("T1", t(12, 9, 0), t(12, 10, 0)),
        work("T1", t(12, 10, 5), now),
    ];
    assert!(evaluate(&ctx, &cfg()).is_empty());
    // タスクが変わった。
    ctx.recent_segments = vec![
        work("T1", t(12, 9, 0), t(12, 10, 0)),
        work("T2", t(12, 10, 0), now),
    ];
    assert!(evaluate(&ctx, &cfg()).is_empty());
    // もう作業が終わっている（最終区間が古い）。
    ctx.recent_segments = vec![work("T1", t(12, 9, 0), t(12, 10, 30))];
    assert!(evaluate(&ctx, &cfg()).is_empty());
}

#[test]
fn long_focus_uses_agent_session_and_label() {
    let now = t(12, 11, 0);
    let mut ctx = PushContext::new(now, TZ);
    let mut a = seg(SegmentKind::AgentDialog, t(12, 9, 0), t(12, 10, 0));
    a.agent_session_id = Some("S".into());
    let mut b = seg(SegmentKind::AgentWatch, t(12, 10, 0), now);
    b.agent_session_id = Some("S".into());
    ctx.recent_segments = vec![a, b];
    assert_eq!(delivered(&evaluate(&ctx, &cfg())), vec![PushTrigger::LongFocus]);

    let mut a = seg(SegmentKind::Work, t(12, 9, 0), t(12, 10, 0));
    a.activity_label = Some("開発".into());
    let mut b = seg(SegmentKind::Work, t(12, 10, 0), now);
    b.activity_label = Some("開発".into());
    ctx.recent_segments = vec![a, b];
    assert_eq!(delivered(&evaluate(&ctx, &cfg())), vec![PushTrigger::LongFocus]);
}

#[test]
fn long_focus_renotify_needs_another_period() {
    let now = t(12, 11, 0);
    let mut ctx = PushContext::new(now, TZ);
    ctx.recent_segments = vec![work("T1", t(12, 9, 0), now)];
    ctx.push_log.push(log("long_focus", t(12, 10, 30), true));
    // 90 分経っていない。
    assert!(evaluate(&ctx, &cfg()).is_empty());
}

#[test]
fn switch_spike_threshold() {
    let mut ctx = PushContext::new(t(12, 11, 0), TZ);
    ctx.recent_switches = Some(20);
    assert!(evaluate(&ctx, &cfg()).is_empty());
    ctx.recent_switches = Some(21);
    let ds = evaluate(&ctx, &cfg());
    let PushDecision::Deliver(p) = &ds[0] else {
        panic!()
    };
    assert_eq!(p.trigger, PushTrigger::SwitchSpike);
    assert!(p.input.is_some());
    assert_eq!(p.actions[0].id, "stress");

    // 閾値は設定で変えられる。
    let mut c = cfg();
    c.switch_spike.count = 30;
    assert!(evaluate(&ctx, &c).is_empty());
}

#[test]
fn switch_spike_counted_from_segments() {
    let now = t(12, 11, 0);
    let mut ctx = PushContext::new(now, TZ);
    // 直近 15 分に 1 分ずつ 15 区間 → 境界 14。
    ctx.recent_segments = (0..15)
        .map(|i| work(&format!("T{}", i % 2), now - (15 - i) * MIN, now - (14 - i) * MIN))
        .collect();
    let mut c = cfg();
    c.switch_spike.count = 13;
    assert_eq!(delivered(&evaluate(&ctx, &c)), vec![PushTrigger::SwitchSpike]);
    c.switch_spike.count = 14;
    assert!(evaluate(&ctx, &c).is_empty());
}

#[test]
fn after_interrupt_fires_within_five_minutes_once() {
    let now = t(12, 11, 0);
    let mut ctx = PushContext::new(now, TZ);
    let after = |from: Millis| work("T1", from, now);
    ctx.recent_segments = vec![
        seg(SegmentKind::Interrupt, now - 10 * MIN, now - 2 * MIN),
        after(now - 2 * MIN),
    ];
    assert_eq!(
        delivered(&evaluate(&ctx, &cfg())),
        vec![PushTrigger::AfterInterrupt]
    );
    // 6 分前に終了 → 対象外。
    ctx.recent_segments = vec![
        seg(SegmentKind::Interrupt, now - 10 * MIN, now - 6 * MIN),
        after(now - 6 * MIN),
    ];
    assert!(evaluate(&ctx, &cfg()).is_empty());
    // 配信済み。
    ctx.recent_segments = vec![
        seg(SegmentKind::Interrupt, now - 10 * MIN, now - 2 * MIN),
        after(now - 2 * MIN),
    ];
    ctx.push_log.push(log("after_interrupt", now - MIN, true));
    assert!(delivered(&evaluate(&ctx, &cfg())).is_empty());
}

#[test]
fn after_interrupt_not_while_ongoing_and_only_once() {
    let now = t(12, 11, 0);
    let mut ctx = PushContext::new(now, TZ);
    // 進行中の割り込み（終了が now に追従している）では出ない。
    ctx.recent_segments = vec![seg(SegmentKind::Interrupt, now - 10 * MIN, now)];
    assert!(evaluate(&ctx, &cfg()).is_empty());
    // 終了後（別区間が始まった）に 1 回出る。
    ctx.recent_segments = vec![
        seg(SegmentKind::Interrupt, now - 10 * MIN, now - MIN),
        work("T1", now - MIN, now),
    ];
    assert_eq!(
        delivered(&evaluate(&ctx, &cfg())),
        vec![PushTrigger::AfterInterrupt]
    );
    ctx.push_log.push(log("after_interrupt", now, true));
    assert!(evaluate(&ctx, &cfg()).is_empty());
    // 61 分後も同じ割り込みなら出ない（5 分の窓を過ぎ、配信済みでもある）。
    ctx.now = now + 61 * MIN;
    ctx.recent_segments[1].end_at = ctx.now;
    assert!(evaluate(&ctx, &cfg()).is_empty());
}

#[test]
fn late_work_spans_midnight() {
    // 12 日 20:00 以降の遅い時間帯が 13 日の朝まで続く。
    let now = t(13, 0, 30);
    let mut ctx = PushContext::new(now, TZ);
    ctx.recent_segments = vec![work("T1", t(13, 0, 0), now)];
    assert_eq!(delivered(&evaluate(&ctx, &cfg())), vec![PushTrigger::LateWork]);
    // 同じ時間帯（12 日 21:00 に配信済み）なら出ない。
    ctx.push_log.push(log("late_work", t(12, 21, 0), true));
    assert!(evaluate(&ctx, &cfg()).is_empty());
    // 朝（08:00 以降、20:00 前）は対象外。
    ctx.push_log.clear();
    ctx.now = t(13, 9, 0);
    ctx.recent_segments = vec![work("T1", t(13, 8, 30), ctx.now)];
    assert!(evaluate(&ctx, &cfg()).is_empty());
}

#[test]
fn meeting_overrun_fires_even_during_meeting() {
    let now = t(12, 14, 10);
    let mut ctx = PushContext::new(now, TZ);
    ctx.calendar_events = vec![event(t(12, 13, 0), t(12, 14, 0))];
    ctx.meeting_exes = vec!["Zoom.exe".into()];
    ctx.foreground_exe = Some("zoom.exe".into());
    ctx.recent_segments = vec![seg(SegmentKind::Meeting, t(12, 13, 0), now)];
    assert_eq!(
        delivered(&evaluate(&ctx, &cfg())),
        vec![PushTrigger::MeetingOverrun]
    );
    // 3 分超過ではまだ。
    ctx.now = t(12, 14, 3);
    assert!(evaluate(&ctx, &cfg()).is_empty());
    // 会議アプリが前面でなければ出ない。
    ctx.now = now;
    ctx.foreground_exe = Some("code.exe".into());
    assert!(evaluate(&ctx, &cfg()).is_empty());
    // 次の予定が進行中なら延長ではない。
    ctx.foreground_exe = Some("Zoom.exe".into());
    ctx.calendar_events.push(event(t(12, 14, 0), t(12, 15, 0)));
    assert!(evaluate(&ctx, &cfg()).is_empty());
}

#[test]
fn late_work_once_a_day() {
    let now = t(12, 20, 30);
    let mut ctx = PushContext::new(now, TZ);
    ctx.recent_segments = vec![work("T1", t(12, 20, 0), now)];
    assert_eq!(delivered(&evaluate(&ctx, &cfg())), vec![PushTrigger::LateWork]);
    // 20:00 前は出ない。
    ctx.now = t(12, 19, 0);
    ctx.recent_segments = vec![work("T1", t(12, 18, 50), t(12, 19, 0))];
    assert!(evaluate(&ctx, &cfg()).is_empty());
    // 配信済み。
    ctx.now = now;
    ctx.recent_segments = vec![work("T1", t(12, 20, 0), now)];
    ctx.push_log.push(log("late_work", t(12, 20, 5), true));
    assert!(evaluate(&ctx, &cfg()).is_empty());
    // 作業が続いていなければ出ない。
    ctx.push_log.clear();
    ctx.recent_segments = vec![work("T1", t(12, 20, 0), now - 10 * MIN)];
    assert!(evaluate(&ctx, &cfg()).is_empty());
}

fn spike_ctx(now: Millis) -> PushContext {
    let mut ctx = PushContext::new(now, TZ);
    ctx.recent_switches = Some(99);
    ctx
}

#[test]
fn defer_during_calendar_meeting_until_end() {
    let now = t(12, 11, 0);
    let mut ctx = spike_ctx(now);
    ctx.calendar_events = vec![event(t(12, 10, 30), t(12, 11, 30))];
    assert_eq!(
        evaluate(&ctx, &cfg()),
        vec![PushDecision::Defer {
            trigger: PushTrigger::SwitchSpike,
            until: t(12, 11, 30)
        }]
    );
}

#[test]
fn defer_when_meeting_segment_or_app_without_event() {
    let now = t(12, 11, 0);
    let mut ctx = spike_ctx(now);
    ctx.recent_segments = vec![seg(SegmentKind::Meeting, now - 10 * MIN, now)];
    assert_eq!(
        evaluate(&ctx, &cfg()),
        vec![PushDecision::Defer {
            trigger: PushTrigger::SwitchSpike,
            until: now + 5 * MIN
        }]
    );
}

#[test]
fn defer_during_focus_block_until_end() {
    let now = t(12, 11, 0);
    let mut ctx = spike_ctx(now);
    let block = |focus| TimeBlock {
        id: "b".into(),
        start_at: t(12, 10, 0),
        end_at: t(12, 12, 0),
        task_id: None,
        label: None,
        focus,
    };
    ctx.time_blocks = vec![block(true)];
    assert_eq!(
        evaluate(&ctx, &cfg()),
        vec![PushDecision::Defer {
            trigger: PushTrigger::SwitchSpike,
            until: t(12, 12, 0)
        }]
    );
    // フォーカスでないブロックは抑制しない。
    ctx.time_blocks = vec![block(false)];
    assert_eq!(delivered(&evaluate(&ctx, &cfg())), vec![PushTrigger::SwitchSpike]);
}

#[test]
fn daily_limit_suppresses_with_reason_once() {
    let now = t(12, 15, 0);
    let mut ctx = spike_ctx(now);
    for i in 0..6 {
        ctx.push_log
            .push(log("long_focus", t(12, 9, 0) + i * 20 * MIN, true));
    }
    let ds = evaluate(&ctx, &cfg());
    assert_eq!(
        ds,
        vec![PushDecision::Suppress {
            trigger: PushTrigger::SwitchSpike,
            reason: "daily_limit".into()
        }]
    );
    // 記録後は、同じ理由の連続抑制は返さない。
    let mut logged = log("switch_spike", now, false);
    logged.suppressed_reason = Some("daily_limit".into());
    ctx.push_log.push(logged);
    ctx.now = now + 5 * MIN;
    assert!(evaluate(&ctx, &cfg()).is_empty());
    // 昨日の配信は数えない。
    let mut ctx = spike_ctx(now);
    for i in 0..6 {
        ctx.push_log
            .push(log("long_focus", t(11, 9, 0) + i * 20 * MIN, true));
    }
    assert_eq!(delivered(&evaluate(&ctx, &cfg())), vec![PushTrigger::SwitchSpike]);
}

#[test]
fn min_interval_per_trigger() {
    let now = t(12, 15, 0);
    let mut ctx = spike_ctx(now);
    ctx.push_log.push(log("switch_spike", now - 30 * MIN, true));
    assert_eq!(
        evaluate(&ctx, &cfg()),
        vec![PushDecision::Suppress {
            trigger: PushTrigger::SwitchSpike,
            reason: "min_interval".into()
        }]
    );
    // 61 分前なら通る。
    ctx.push_log.clear();
    ctx.push_log.push(log("switch_spike", now - 61 * MIN, true));
    assert_eq!(delivered(&evaluate(&ctx, &cfg())), vec![PushTrigger::SwitchSpike]);
}

#[test]
fn recent_push_of_other_trigger_suppresses_briefly() {
    let now = t(12, 15, 0);
    let mut ctx = spike_ctx(now);
    ctx.push_log.push(log("long_focus", now - 3 * MIN, true));
    assert_eq!(
        evaluate(&ctx, &cfg()),
        vec![PushDecision::Suppress {
            trigger: PushTrigger::SwitchSpike,
            reason: "recent_push".into()
        }]
    );
}

#[test]
fn snooze_suppresses_all_until_expiry() {
    let now = t(12, 15, 0);
    let mut ctx = spike_ctx(now);
    let mut l = log("long_focus", now - 30 * MIN, true);
    l.snoozed_until = Some(now + 10 * MIN);
    ctx.push_log.push(l);
    assert_eq!(
        evaluate(&ctx, &cfg()),
        vec![PushDecision::Suppress {
            trigger: PushTrigger::SwitchSpike,
            reason: "snoozed".into()
        }]
    );
    ctx.now = now + 11 * MIN;
    assert_eq!(delivered(&evaluate(&ctx, &cfg())), vec![PushTrigger::SwitchSpike]);
}

#[test]
fn unresponsive_history_extends_interval_up_to_4x() {
    let now = t(12, 20, 0);
    let mut ctx = spike_ctx(now);
    // 無応答の配信が 5 件（いずれも 30 分以上前）。
    for i in 0..5 {
        ctx.push_log
            .push(log("long_focus", t(12, 9, 0) + i * 70 * MIN, true));
    }
    assert!((gate::interval_factor(&ctx) - 4.0).abs() < 1e-9);
    // 直近の switch_spike は 2 時間前（基本間隔 60 分は超えているが 240 分未満）。
    ctx.push_log.push(log("switch_spike", now - 120 * MIN, true));
    // 件数が増えると窓は直近 5 件: long_focus 4 件 + switch_spike 1 件、すべて無応答。
    assert_eq!(
        evaluate(&ctx, &big_cfg()),
        vec![PushDecision::Suppress {
            trigger: PushTrigger::SwitchSpike,
            reason: "min_interval_backoff".into()
        }]
    );
    // 4 時間以上空けば通る。
    ctx.push_log.retain(|l| l.trigger != "switch_spike");
    ctx.push_log.push(log("switch_spike", now - 250 * MIN, true));
    assert_eq!(
        delivered(&evaluate(&ctx, &big_cfg())),
        vec![PushTrigger::SwitchSpike]
    );
}

#[test]
fn responsive_history_does_not_extend() {
    let now = t(12, 20, 0);
    let mut ctx = spike_ctx(now);
    for i in 0..5 {
        let mut l = log("long_focus", t(12, 9, 0) + i * 70 * MIN, true);
        l.responded_at = Some(l.at + MIN);
        ctx.push_log.push(l);
    }
    assert!((gate::interval_factor(&ctx) - 1.0).abs() < 1e-9);
    // 少ない件数では延長しない。
    let mut ctx = spike_ctx(now);
    ctx.push_log.push(log("long_focus", t(12, 9, 0), true));
    assert!((gate::interval_factor(&ctx) - 1.0).abs() < 1e-9);
    // 応答率 25%（4 件中 1 件）→ 係数 2.5。
    let mut ctx = spike_ctx(now);
    for i in 0..4 {
        let mut l = log("long_focus", t(12, 9, 0) + i * 70 * MIN, true);
        if i == 0 {
            l.responded_at = Some(l.at + MIN);
        }
        ctx.push_log.push(l);
    }
    assert!((gate::interval_factor(&ctx) - 2.5).abs() < 1e-9);
}

#[test]
fn dismissed_pushes_count_as_unresponded_for_backoff() {
    // 「答えない」で見送った通知は応答とは数えず、無応答が続くときと同じく頻度を下げる。
    let now = t(12, 20, 0);
    let mut ctx = spike_ctx(now);
    for i in 0..4 {
        let mut l = log("long_focus", t(12, 9, 0) + i * 70 * MIN, true);
        if i > 0 {
            l.response_json = Some(r#"{"action":"dismiss","value":null,"text":null}"#.into());
        }
        ctx.push_log.push(l);
    }
    assert!(gate::interval_factor(&ctx) > 1.0);
}

#[test]
fn only_one_deliver_per_evaluation() {
    let now = t(16, 17, 5);
    let mut ctx = PushContext::new(now, TZ);
    ctx.recent_switches = Some(99);
    ctx.resumed_at = Some(t(16, 9, 0));
    ctx.idle.since_last_input_sec = 0;
    let ds = evaluate(&ctx, &cfg());
    assert_eq!(delivered(&ds), vec![PushTrigger::DayStart]);
}

#[test]
fn idle_blocks_non_day_end_triggers() {
    let mut ctx = spike_ctx(t(12, 11, 0));
    ctx.idle.is_idle = true;
    assert!(evaluate(&ctx, &cfg()).is_empty());
}

#[test]
fn payloads_are_gentle_and_actionable() {
    for p in [
        payload::day_start(),
        payload::day_end(0),
        payload::weekly(),
        payload::long_focus(95, None),
        payload::switch_spike(15, 21),
        payload::after_interrupt(),
        payload::meeting_overrun(),
        payload::late_work(),
    ] {
        assert!(!p.title.is_empty() && !p.body.is_empty());
        assert!(!p.actions.is_empty() || p.input.is_some());
    }
}

// ===== store / apply_response =====

fn deliver_log(conn: &rusqlite::Connection, trigger: PushTrigger, at: Millis) -> String {
    let payload = match trigger {
        PushTrigger::SwitchSpike => payload::switch_spike(15, 21),
        _ => payload::day_start(),
    };
    store::record_decision(conn, &PushDecision::Deliver(payload), at)
        .unwrap()
        .unwrap()
}

#[test]
fn record_decision_deliver_suppress_defer() {
    let conn = db::open_in_memory().unwrap();
    let id = deliver_log(&conn, PushTrigger::DayStart, 1000);
    let l = store::get_log(&conn, &id).unwrap();
    assert!(l.delivered && l.trigger == "day_start" && l.suppressed_reason.is_none());

    let sid = store::record_decision(
        &conn,
        &PushDecision::Suppress {
            trigger: PushTrigger::LongFocus,
            reason: "daily_limit".into(),
        },
        2000,
    )
    .unwrap()
    .unwrap();
    let l = store::get_log(&conn, &sid).unwrap();
    assert!(!l.delivered);
    assert_eq!(l.suppressed_reason.as_deref(), Some("daily_limit"));

    assert!(store::record_decision(
        &conn,
        &PushDecision::Defer {
            trigger: PushTrigger::LongFocus,
            until: 5
        },
        3000
    )
    .unwrap()
    .is_none());
    assert_eq!(store::load_recent(&conn, 0, 0).unwrap().len(), 2);
    assert_eq!(store::load_recent(&conn, 1500, 0).unwrap().len(), 1);
    assert_eq!(store::load_recent(&conn, 1500, 5).unwrap().len(), 2);
}

#[test]
fn apply_energy_creates_checkin() {
    let conn = db::open_in_memory().unwrap();
    let id = deliver_log(&conn, PushTrigger::DayStart, 1000);
    let out = store::apply_response(
        &conn,
        &id,
        PushResponse {
            action: Some("energy".into()),
            value: Some(4),
            text: None,
        },
        5000,
        TZ,
    )
    .unwrap();
    let cid = out.checkin_id.unwrap();
    let (energy, stress, push_id, at): (Option<i64>, Option<i64>, String, i64) = conn
        .query_row(
            "SELECT energy, stress, push_id, at FROM checkin WHERE id = ?1",
            [&cid],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap();
    assert_eq!(
        (energy, stress, push_id.as_str(), at),
        (Some(4), None, id.as_str(), 1000)
    );
    let l = store::get_log(&conn, &id).unwrap();
    assert_eq!(l.responded_at, Some(5000));
    assert!(l.response_json.unwrap().contains("energy"));
}

#[test]
fn apply_stress_with_text_creates_checkin_and_hassle() {
    let conn = db::open_in_memory().unwrap();
    let id = deliver_log(&conn, PushTrigger::SwitchSpike, 1000);
    let out = store::apply_response(
        &conn,
        &id,
        PushResponse {
            action: Some("stress".into()),
            value: Some(5),
            text: Some("  チャットが多すぎる ".into()),
        },
        5000,
        TZ,
    )
    .unwrap();
    assert!(out.checkin_id.is_some());
    let (text, intensity, push_id): (String, i64, String) = conn
        .query_row(
            "SELECT text, intensity, push_id FROM hassle WHERE id = ?1",
            [out.hassle_id.unwrap()],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(
        (text.as_str(), intensity, push_id.as_str()),
        ("チャットが多すぎる", 5, id.as_str())
    );

    // テキストだけなら hassle のみ（強度 3）。空文字は無視。
    let id2 = deliver_log(&conn, PushTrigger::SwitchSpike, 6000);
    let out = store::apply_response(
        &conn,
        &id2,
        PushResponse {
            text: Some("割り込み".into()),
            ..Default::default()
        },
        7000,
        TZ,
    )
    .unwrap();
    assert!(out.checkin_id.is_none() && out.hassle_id.is_some());
    let id3 = deliver_log(&conn, PushTrigger::SwitchSpike, 8000);
    let out = store::apply_response(
        &conn,
        &id3,
        PushResponse {
            text: Some("   ".into()),
            ..Default::default()
        },
        9000,
        TZ,
    )
    .unwrap();
    assert!(out.hassle_id.is_none());
}

#[test]
fn apply_snooze_sets_until_and_suppresses() {
    let conn = db::open_in_memory().unwrap();
    let now = t(12, 15, 0);
    let id = deliver_log(&conn, PushTrigger::DayStart, now - 30 * MIN);
    let out = store::apply_response(
        &conn,
        &id,
        PushResponse {
            action: Some("snooze_60".into()),
            ..Default::default()
        },
        now,
        TZ,
    )
    .unwrap();
    assert_eq!(out.snoozed_until, Some(now + 60 * MIN));
    assert_eq!(
        store::get_log(&conn, &id).unwrap().snoozed_until,
        Some(now + 60 * MIN)
    );

    let id = deliver_log(&conn, PushTrigger::DayStart, now);
    let out = store::apply_response(
        &conn,
        &id,
        PushResponse {
            action: Some("snooze_today".into()),
            ..Default::default()
        },
        now,
        TZ,
    )
    .unwrap();
    assert_eq!(out.snoozed_until, Some(t(13, 0, 0)));

    // evaluate がスヌーズを読む。
    let mut ctx = spike_ctx(now + 3 * 3600 * 1000);
    ctx.push_log = store::load_recent(&conn, 0, 0).unwrap();
    assert!(matches!(
        evaluate(&ctx, &cfg()).as_slice(),
        [PushDecision::Suppress { reason, .. }] if reason == "snoozed"
    ));
}

#[test]
fn apply_dismiss_does_not_count_as_response_and_errors() {
    let conn = db::open_in_memory().unwrap();
    let id = deliver_log(&conn, PushTrigger::DayStart, 1000);
    store::apply_response(
        &conn,
        &id,
        PushResponse {
            action: Some("dismiss".into()),
            ..Default::default()
        },
        2000,
        TZ,
    )
    .unwrap();
    let l = store::get_log(&conn, &id).unwrap();
    assert_eq!(l.responded_at, None);
    assert!(l.response_json.is_some());

    let bad = |a: &str, v| PushResponse {
        action: Some(a.into()),
        value: v,
        text: None,
    };
    assert!(store::apply_response(&conn, &id, bad("energy", Some(6)), 3000, TZ).is_err());
    assert!(store::apply_response(&conn, &id, bad("energy", None), 3000, TZ).is_err());
    assert!(store::apply_response(&conn, &id, bad("nope", None), 3000, TZ).is_err());
    assert!(matches!(
        store::apply_response(&conn, "missing", bad("open", None), 3000, TZ),
        Err(crate::Error::NotFound(_))
    ));
    let n: i64 = conn
        .query_row("SELECT COUNT(*) FROM checkin", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 0);
}

fn big_cfg() -> Push {
    Push {
        daily_limit: 50,
        ..Push::default()
    }
}

// ===== idle_return =====

fn idle_ctx() -> PushContext {
    // 10:00-10:30 の無操作から 10:32 に復帰した状況。
    let mut ctx = PushContext::new(t(12, 10, 32), TZ);
    ctx.last_idle = Some(ClosedIdle {
        start_at: t(12, 10, 0),
        end_at: t(12, 10, 30),
    });
    ctx
}

#[test]
fn idle_return_fires_after_long_unplanned_idle() {
    let ds = evaluate(&idle_ctx(), &cfg());
    assert_eq!(delivered(&ds), vec![PushTrigger::IdleReturn]);
    let PushDecision::Deliver(p) = &ds[0] else {
        panic!()
    };
    assert!(p.body.contains("30 分"));
    assert_eq!(p.idle_range, Some((t(12, 10, 0), t(12, 10, 30))));
    let ids: Vec<&str> = p.actions.iter().map(|a| a.id.as_str()).collect();
    assert_eq!(ids, ["idle_break", "idle_meeting", "idle_thinking", "idle_away"]);
}

#[test]
fn idle_return_is_not_fired_when_short_old_planned_or_classified() {
    // 短い（14 分 < 15 分）
    let mut ctx = idle_ctx();
    ctx.last_idle.as_mut().unwrap().start_at = t(12, 10, 16);
    assert!(evaluate(&ctx, &cfg()).is_empty());
    // ちょうど 15 分は発火
    ctx.last_idle.as_mut().unwrap().start_at = t(12, 10, 15);
    assert_eq!(delivered(&evaluate(&ctx, &cfg())), vec![PushTrigger::IdleReturn]);
    // 復帰から時間が経っている
    let mut ctx = idle_ctx();
    ctx.now = t(12, 10, 45);
    assert!(evaluate(&ctx, &cfg()).is_empty());
    // まだ無操作中
    let mut ctx = idle_ctx();
    ctx.idle.is_idle = true;
    assert!(evaluate(&ctx, &cfg()).is_empty());
    // 予定（会議）と重なっている
    let mut ctx = idle_ctx();
    ctx.calendar_events = vec![event(t(12, 10, 5), t(12, 10, 25))];
    assert!(evaluate(&ctx, &cfg()).is_empty());
    // すでに休憩として分類済み（長いロックの自動休憩など）
    let mut ctx = idle_ctx();
    ctx.recent_segments = vec![seg(SegmentKind::Break, t(12, 10, 0), t(12, 10, 30))];
    assert!(evaluate(&ctx, &cfg()).is_empty());
    // ユーザー確定済み
    let mut ctx = idle_ctx();
    let mut s = seg(SegmentKind::Work, t(12, 10, 0), t(12, 10, 30));
    s.status = SegmentStatus::Confirmed;
    ctx.recent_segments = vec![s];
    assert!(evaluate(&ctx, &cfg()).is_empty());
    // 要確認の無操作区間なら尋ねる
    let mut ctx = idle_ctx();
    let mut s = seg(SegmentKind::Idle, t(12, 10, 0), t(12, 10, 30));
    s.status = SegmentStatus::NeedsReview;
    ctx.recent_segments = vec![s];
    assert_eq!(delivered(&evaluate(&ctx, &cfg())), vec![PushTrigger::IdleReturn]);
}

#[test]
fn idle_return_can_be_disabled_and_is_not_repeated() {
    let mut c = cfg();
    c.idle_return_min = 0;
    assert!(evaluate(&idle_ctx(), &c).is_empty());
    let mut c = cfg();
    c.disabled_triggers = vec!["idle_return".into()];
    assert!(evaluate(&idle_ctx(), &c).is_empty());
    // 同じ無操作について配信済みなら出さない。
    let mut ctx = idle_ctx();
    ctx.push_log.push(log("idle_return", t(12, 10, 31), true));
    assert!(evaluate(&ctx, &cfg()).is_empty());
    // 通知の一時停止中は抑制される（ログには抑制として残る）。
    let mut ctx = idle_ctx();
    let mut l = log("long_focus", t(12, 9, 0), true);
    l.snoozed_until = Some(t(12, 11, 0));
    ctx.push_log.push(l);
    let ds = evaluate(&ctx, &cfg());
    assert!(delivered(&ds).is_empty());
    assert!(matches!(&ds[0], PushDecision::Suppress { reason, .. } if reason == "snoozed"));
}

#[test]
fn idle_return_button_saves_override_and_labels_segment() {
    let conn = db::open_in_memory().unwrap();
    let (s, e) = (t(12, 10, 0), t(12, 10, 30));
    conn.execute(
        "INSERT INTO activity_segment (id, start_at, end_at, kind, status, window_span_ids) \
         VALUES ('IDLE1', ?1, ?2, 'idle', 'needs_review', '[]')",
        [s, e],
    )
    .unwrap();
    let id = store::record_decision(
        &conn,
        &PushDecision::Deliver(payload::idle_return(s, e, None)),
        t(12, 10, 31),
    )
    .unwrap()
    .unwrap();
    let out = store::apply_response(
        &conn,
        &id,
        PushResponse {
            action: Some("idle_thinking".into()),
            value: None,
            text: None,
        },
        t(12, 10, 32),
        TZ,
    )
    .unwrap();
    assert_eq!(out.idle_labeled, 1);
    let (kind, label, status): (String, Option<String>, String) = conn
        .query_row(
            "SELECT kind, activity_label, status FROM activity_segment WHERE id = 'IDLE1'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(
        (kind.as_str(), label.as_deref(), status.as_str()),
        ("work", Some("考えごと"), "confirmed")
    );
    // 再計算でも残る override が期間全体に保存されている。
    let ovs = crate::repo::overrides::list(&conn, crate::timefmt::Range::new(s, e)).unwrap();
    assert_eq!(ovs.len(), 1);
    assert_eq!((ovs[0].start_at, ovs[0].end_at), (s, e));
    assert!(store::get_log(&conn, &id).unwrap().responded_at.is_some());

    // 期間を持たない通知への idle_* は不正。
    let other = deliver_log(&conn, PushTrigger::DayStart, 1000);
    let r = store::apply_response(
        &conn,
        &other,
        PushResponse {
            action: Some("idle_break".into()),
            ..Default::default()
        },
        2000,
        TZ,
    );
    assert!(r.is_err());
}

#[test]
fn ignored_segments_do_not_count_as_work_for_push() {
    let now = t(12, 11, 0);
    // long_focus: 無視の区間は作業として数えない（最後が無視なら発火しない、間にあれば途切れる）
    let mut ctx = PushContext::new(now, TZ);
    let mut ign = seg(SegmentKind::Ignored, t(12, 10, 0), now);
    ign.task_id = Some("T1".into());
    ctx.recent_segments = vec![work("T1", t(12, 8, 0), t(12, 10, 0)), ign.clone()];
    assert!(evaluate(&ctx, &cfg()).is_empty());
    // late_work: 遅い時間帯でも無視の区間だけなら発火しない
    let now = t(12, 21, 0);
    let mut ctx = PushContext::new(now, TZ);
    ctx.recent_segments = vec![seg(SegmentKind::Ignored, t(12, 20, 30), now)];
    assert!(evaluate(&ctx, &cfg()).is_empty());
    // switch_spike: 無視の区間は切替に数えない
    let now = t(12, 11, 0);
    let mut ctx = PushContext::new(now, TZ);
    ctx.recent_segments = (0..15)
        .map(|i| seg(SegmentKind::Ignored, now - (15 - i) * MIN, now - (14 - i) * MIN))
        .collect();
    let mut c = cfg();
    c.switch_spike.count = 3;
    assert!(evaluate(&ctx, &c).is_empty());
}

// ===== urgent_task / important_task =====

const DAY: Millis = 24 * 60 * MIN;

fn nudge(id: &str, urgent: bool, important: bool, created_at: Millis) -> TaskNudge {
    TaskNudge {
        id: id.into(),
        title: format!("タスク{id}"),
        urgent,
        important,
        doing: false,
        last_worked_at: None,
        created_at,
    }
}

fn block(task: Option<&str>, start: Millis, end: Millis) -> TimeBlock {
    TimeBlock {
        id: format!("b{start}"),
        start_at: start,
        end_at: end,
        task_id: task.map(Into::into),
        label: None,
        focus: false,
    }
}

/// 配信済みのタスク通知のログ。ペイロードに task_id を持つ。
fn task_log(payload: PushPayload, at: Millis, response: Option<&str>) -> PushLog {
    PushLog {
        id: format!("tl{at}"),
        at,
        trigger: payload.trigger.as_str().into(),
        payload_json: serde_json::to_string(&payload).unwrap(),
        delivered: true,
        responded_at: response.map(|_| at + MIN),
        response_json: response.map(|a| format!(r#"{{"action":"{a}"}}"#)),
        snoozed_until: None,
        suppressed_reason: None,
    }
}

fn urgent_ctx(now: Millis) -> PushContext {
    let mut ctx = PushContext::new(now, TZ);
    ctx.tasks = vec![nudge("A", true, false, t(1, 9, 0))];
    ctx
}

fn urgent_delivered(ctx: &PushContext, c: &Push) -> Vec<PushTrigger> {
    delivered(&evaluate(ctx, c))
}

#[test]
fn urgent_task_fires_with_payload() {
    let ctx = urgent_ctx(t(12, 10, 0));
    let ds = evaluate(&ctx, &cfg());
    assert_eq!(delivered(&ds), vec![PushTrigger::UrgentTask]);
    let PushDecision::Deliver(p) = &ds[0] else {
        panic!()
    };
    assert_eq!(p.title, "緊急のタスクがあります");
    assert!(p.body.contains("「タスクA」") && !p.body.contains("ほかに"));
    assert_eq!(p.task_id.as_deref(), Some("A"));
    assert_eq!(
        p.actions.iter().map(|a| a.id.as_str()).collect::<Vec<_>>(),
        vec!["task_start", "task_later"]
    );
    assert_eq!(p.open_screen.as_deref(), Some("tasks"));
    // 複数なら「ほかに N 件」。
    let mut ctx = urgent_ctx(t(12, 10, 0));
    ctx.tasks.push(nudge("B", true, false, t(2, 9, 0)));
    ctx.tasks.push(nudge("C", true, true, t(3, 9, 0)));
    let PushDecision::Deliver(p) = &evaluate(&ctx, &cfg())[0] else {
        panic!()
    };
    assert!(p.body.contains("（ほかに 2 件）"));
}

#[test]
fn urgent_task_not_without_target_or_outside_hours_or_idle() {
    let mut ctx = PushContext::new(t(12, 10, 0), TZ);
    assert!(evaluate(&ctx, &cfg()).is_empty());
    // 重要だけ・緊急でないものは対象外。
    ctx.tasks = vec![nudge("A", false, true, t(11, 9, 0))];
    assert!(urgent_delivered(&ctx, &cfg()).is_empty());
    // 勤務時間外。
    for (h, m) in [(7, 59), (18, 30), (21, 0)] {
        let c = urgent_ctx(t(12, h, m));
        assert!(
            !urgent_delivered(&c, &cfg()).contains(&PushTrigger::UrgentTask),
            "{h}:{m}"
        );
    }
    // 無操作中。
    let mut c = urgent_ctx(t(12, 10, 0));
    c.idle = IdleState {
        is_idle: true,
        since_last_input_sec: 600,
    };
    assert!(evaluate(&c, &cfg()).is_empty());
}

#[test]
fn urgent_task_skipped_while_working_on_a_target() {
    let now = t(12, 10, 0);
    // 作業系の最終区間がその緊急タスクに帰属している。
    let mut ctx = urgent_ctx(now);
    ctx.recent_segments = vec![work("A", t(12, 9, 30), now)];
    assert!(evaluate(&ctx, &cfg()).is_empty());
    // doing（着手を宣言している）。
    let mut ctx = urgent_ctx(now);
    ctx.tasks[0].doing = true;
    assert!(evaluate(&ctx, &cfg()).is_empty());
    // 対象のどれかに取り組み中なら、ほかの緊急タスクについても出さない。
    let mut ctx = urgent_ctx(now);
    ctx.tasks.push(nudge("B", true, false, t(2, 9, 0)));
    ctx.tasks[1].doing = true;
    assert!(evaluate(&ctx, &cfg()).is_empty());
    // 緊急でない別のタスクをしているだけなら出る。
    let mut ctx = urgent_ctx(now);
    ctx.recent_segments = vec![work("Z", t(12, 9, 30), now)];
    assert_eq!(urgent_delivered(&ctx, &cfg()), vec![PushTrigger::UrgentTask]);
}

#[test]
fn urgent_task_skips_tasks_with_upcoming_block() {
    let now = t(12, 10, 0);
    let mut ctx = urgent_ctx(now);
    ctx.tasks.push(nudge("B", true, false, t(2, 9, 0)));
    // A は午後に予定済み、B は未予定。
    ctx.time_blocks = vec![block(Some("A"), t(12, 14, 0), t(12, 15, 0))];
    let PushDecision::Deliver(p) = &evaluate(&ctx, &cfg())[0] else {
        panic!()
    };
    assert_eq!(p.task_id.as_deref(), Some("B"));
    assert!(!p.body.contains("ほかに"));
    // 進行中のブロックも対象外。全部予定済みなら出ない。
    ctx.time_blocks = vec![
        block(Some("A"), t(12, 9, 30), t(12, 10, 30)),
        block(Some("B"), t(12, 16, 0), t(12, 17, 0)),
    ];
    assert!(evaluate(&ctx, &cfg()).is_empty());
    // 終わったブロックは予定済みと数えない。
    ctx.time_blocks = vec![block(Some("A"), t(12, 8, 30), t(12, 9, 30))];
    ctx.tasks.truncate(1);
    assert_eq!(urgent_delivered(&ctx, &cfg()), vec![PushTrigger::UrgentTask]);
    // タスクのないブロックは無関係。
    ctx.time_blocks = vec![block(None, t(12, 11, 0), t(12, 12, 0))];
    assert_eq!(urgent_delivered(&ctx, &cfg()), vec![PushTrigger::UrgentTask]);
}

#[test]
fn urgent_task_interval_and_disable() {
    let now = t(12, 14, 0);
    let mut ctx = urgent_ctx(now);
    // 100 分前に配信 → 既定 120 分未満なので出ない。
    ctx.push_log = vec![task_log(payload::urgent_task("A", "x", 0), now - 100 * MIN, None)];
    assert!(evaluate(&ctx, &cfg()).is_empty());
    // 125 分前 → 出る。
    ctx.push_log = vec![task_log(payload::urgent_task("A", "x", 0), now - 125 * MIN, None)];
    assert_eq!(urgent_delivered(&ctx, &cfg()), vec![PushTrigger::UrgentTask]);
    // 間隔の設定を伸ばせば出ない。
    let mut c = cfg();
    c.urgent_task_interval_min = 180;
    assert!(evaluate(&ctx, &c).is_empty());
    // 0 で無効。
    let mut c = cfg();
    c.urgent_task_interval_min = 0;
    assert!(evaluate(&urgent_ctx(now), &c).is_empty());
    // disabled_triggers。
    let mut c = cfg();
    c.disabled_triggers = vec!["urgent_task".into()];
    assert!(evaluate(&urgent_ctx(now), &c).is_empty());
}

#[test]
fn urgent_task_is_also_gated_by_common_rules() {
    let now = t(12, 14, 0);
    // 直前（5 分前）に別トリガーを配信している。
    let mut ctx = urgent_ctx(now);
    ctx.push_log = vec![log("weekly", now - 5 * MIN, true)];
    let ds = evaluate(&ctx, &cfg());
    assert!(delivered(&ds).is_empty());
    assert!(ds.iter().any(|d| matches!(
        d,
        PushDecision::Suppress { trigger: PushTrigger::UrgentTask, reason } if reason == "recent_push"
    )));
    // 会議中は保留。
    let mut ctx = urgent_ctx(now);
    ctx.calendar_events = vec![event(t(12, 13, 30), t(12, 14, 30))];
    assert!(matches!(
        evaluate(&ctx, &cfg())[0],
        PushDecision::Defer {
            trigger: PushTrigger::UrgentTask,
            ..
        }
    ));
}

#[test]
fn urgent_task_picks_longest_unnotified_then_oldest() {
    let now = t(12, 14, 0);
    let pick = |ctx: &PushContext| {
        let ds = evaluate(ctx, &cfg());
        let PushDecision::Deliver(p) = ds.into_iter().next().unwrap() else {
            panic!()
        };
        p.task_id.unwrap()
    };
    let mut ctx = PushContext::new(now, TZ);
    ctx.tasks = vec![
        nudge("A", true, false, t(5, 9, 0)),
        nudge("B", true, false, t(3, 9, 0)),
        nudge("C", true, false, t(4, 9, 0)),
    ];
    // 同順（まだ誰にも通知していない）なら作成が古い B。
    assert_eq!(pick(&ctx), "B");
    // B に通知済み → 次に作成が古い C。
    ctx.push_log = vec![task_log(
        payload::urgent_task("B", "x", 2),
        now - 5 * 60 * MIN,
        Some("task_later"),
    )];
    assert_eq!(pick(&ctx), "C");
    // B, C に通知済み → A。
    ctx.push_log.push(task_log(
        payload::urgent_task("C", "x", 2),
        now - 4 * 60 * MIN - 10,
        Some("task_later"),
    ));
    assert_eq!(pick(&ctx), "A");
    // 全員に通知済みなら、通知が最も古い B。
    ctx.push_log.push(task_log(
        payload::urgent_task("A", "x", 2),
        now - 3 * 60 * MIN,
        Some("task_later"),
    ));
    assert_eq!(pick(&ctx), "B");
}

fn important_ctx(now: Millis) -> PushContext {
    let mut ctx = PushContext::new(now, TZ);
    // 5 日前に作られ、一度も時間が入っていない。
    ctx.tasks = vec![nudge("I1", false, true, now - 5 * DAY)];
    ctx
}

#[test]
fn important_task_fires_when_stale() {
    let now = t(12, 10, 0);
    let ctx = important_ctx(now);
    let ds = evaluate(&ctx, &cfg());
    assert_eq!(delivered(&ds), vec![PushTrigger::ImportantTask]);
    let PushDecision::Deliver(p) = &ds[0] else {
        panic!()
    };
    assert_eq!(p.title, "重要なタスクに少し時間を取りませんか");
    assert!(p.body.contains("「タスクI1」") && p.body.contains("まだ時間が入っていません"));
    assert_eq!(p.task_id.as_deref(), Some("I1"));
    assert_eq!(
        p.actions.iter().map(|a| a.id.as_str()).collect::<Vec<_>>(),
        vec!["task_start", "open", "task_skip_week"]
    );
    assert_eq!(p.open_screen.as_deref(), Some("today/design"));

    // 時間が入っていれば、最後に手をつけてからの日数を本文に出す。
    let mut ctx = important_ctx(now);
    ctx.tasks[0].last_worked_at = Some(now - 4 * DAY - 3 * 60 * MIN);
    let PushDecision::Deliver(p) = &evaluate(&ctx, &cfg())[0] else {
        panic!()
    };
    assert!(p.body.contains("ここ 4 日ほど時間が入っていません"));
}

#[test]
fn important_task_threshold_uses_latest_of_work_and_creation() {
    let now = t(12, 10, 0);
    // 作業が最近ある（2 日前）→ 3 日未満なので出ない。
    let mut ctx = important_ctx(now);
    ctx.tasks[0].last_worked_at = Some(now - 2 * DAY);
    assert!(evaluate(&ctx, &cfg()).is_empty());
    // 作業は古いが作成が新しい → 作成からの経過で判定。
    let mut ctx = important_ctx(now);
    ctx.tasks[0].created_at = now - 2 * DAY;
    ctx.tasks[0].last_worked_at = Some(now - 10 * DAY);
    assert!(evaluate(&ctx, &cfg()).is_empty());
    // ちょうど 3 日で対象。
    let mut ctx = important_ctx(now);
    ctx.tasks[0].created_at = now - 3 * DAY;
    assert_eq!(
        delivered(&evaluate(&ctx, &cfg())),
        vec![PushTrigger::ImportantTask]
    );
    // 日数の設定を変える・0 で無効。
    let mut c = cfg();
    c.important_stale_days = 7;
    assert!(evaluate(&important_ctx(now), &c).is_empty());
    c.important_stale_days = 0;
    assert!(evaluate(&important_ctx(now), &c).is_empty());
    // disabled_triggers。
    let mut c = cfg();
    c.disabled_triggers = vec!["important_task".into()];
    assert!(evaluate(&important_ctx(now), &c).is_empty());
}

#[test]
fn important_task_excludes_urgent_blocked_and_active() {
    let now = t(12, 10, 0);
    // 緊急でもあるものは urgent_task の担当で、important_task の対象外。
    let mut ctx = important_ctx(now);
    ctx.tasks[0].urgent = true;
    ctx.tasks[0].doing = true; // urgent_task も出さない
    assert!(evaluate(&ctx, &cfg()).is_empty());
    // 今日の time_block に割り当て済み（終わったブロックでも）。
    let mut ctx = important_ctx(now);
    ctx.time_blocks = vec![block(Some("I1"), t(12, 8, 30), t(12, 9, 0))];
    assert!(evaluate(&ctx, &cfg()).is_empty());
    ctx.time_blocks = vec![block(Some("I1"), t(12, 15, 0), t(12, 16, 0))];
    assert!(evaluate(&ctx, &cfg()).is_empty());
    // いま取り組み中（作業の帰属 / doing）。
    let mut ctx = important_ctx(now);
    ctx.recent_segments = vec![work("I1", t(12, 9, 30), now)];
    assert!(evaluate(&ctx, &cfg()).is_empty());
    let mut ctx = important_ctx(now);
    ctx.tasks[0].doing = true;
    assert!(evaluate(&ctx, &cfg()).is_empty());
}

#[test]
fn important_task_work_hours_idle_and_once_a_day() {
    for (h, m) in [(7, 0), (18, 30), (22, 0)] {
        let ctx = important_ctx(t(12, h, m));
        assert!(
            !delivered(&evaluate(&ctx, &cfg())).contains(&PushTrigger::ImportantTask),
            "{h}:{m}"
        );
    }
    let mut ctx = important_ctx(t(12, 10, 0));
    ctx.idle = IdleState {
        is_idle: true,
        since_last_input_sec: 900,
    };
    assert!(evaluate(&ctx, &cfg()).is_empty());

    // 今日すでに配信済みなら出ない（別タスクでも）。
    let now = t(12, 15, 0);
    let mut ctx = important_ctx(now);
    ctx.tasks.push(nudge("I2", false, true, now - 9 * DAY));
    ctx.push_log = vec![task_log(
        payload::important_task("I2", "x", None),
        t(12, 9, 0),
        None,
    )];
    assert!(delivered(&evaluate(&ctx, &cfg())).is_empty());
    // 昨日の配信なら、1 日 1 回の制限にはかからない。
    ctx.push_log = vec![task_log(
        payload::important_task("I2", "x", None),
        t(11, 9, 0),
        None,
    )];
    assert_eq!(
        delivered(&evaluate(&ctx, &cfg())),
        vec![PushTrigger::ImportantTask]
    );
}

#[test]
fn important_task_skip_week_and_selection_order() {
    let now = t(14, 10, 0); // 水曜
    let mut ctx = PushContext::new(now, TZ);
    ctx.tasks = vec![
        nudge("I1", false, true, now - 6 * DAY),
        nudge("I2", false, true, now - 9 * DAY),
        nudge("I3", false, true, now - 20 * DAY),
    ];
    ctx.tasks[0].last_worked_at = Some(now - 4 * DAY);
    let pick = |ctx: &PushContext| {
        let ds = evaluate(ctx, &cfg());
        let Some(PushDecision::Deliver(p)) = ds.into_iter().find(|d| matches!(d, PushDecision::Deliver(_)))
        else {
            return None;
        };
        p.task_id
    };
    // 最後に手をつけた時刻が最も古いもの（I3）。
    assert_eq!(pick(&ctx).as_deref(), Some("I3"));
    // 今週（月曜 = 12 日以降）に I3 を「今週は見送る」→ 次は I2。
    ctx.push_log = vec![task_log(
        payload::important_task("I3", "x", None),
        t(12, 10, 0),
        Some("task_skip_week"),
    )];
    assert_eq!(pick(&ctx).as_deref(), Some("I2"));
    // 見送り以外の応答は除外しない。
    ctx.push_log = vec![task_log(
        payload::important_task("I3", "x", None),
        t(12, 10, 0),
        Some("open"),
    )];
    assert_eq!(pick(&ctx).as_deref(), Some("I3"));
    // 先週の見送りは今週に持ち越さない。
    ctx.push_log = vec![task_log(
        payload::important_task("I3", "x", None),
        t(9, 10, 0),
        Some("task_skip_week"),
    )];
    assert_eq!(pick(&ctx).as_deref(), Some("I3"));
    // 全部見送り済みなら出ない。
    ctx.push_log = ["I1", "I2", "I3"]
        .iter()
        .enumerate()
        .map(|(i, id)| {
            task_log(
                payload::important_task(id, "x", None),
                t(12, 10, i as u32),
                Some("task_skip_week"),
            )
        })
        .collect();
    assert_eq!(pick(&ctx), None);
}

#[test]
fn task_triggers_priority_order() {
    let ids: Vec<&str> = PushTrigger::ALL.iter().map(|t| t.as_str()).collect();
    let pos = |s: &str| ids.iter().position(|x| *x == s).unwrap();
    assert!(pos("urgent_task") < pos("after_interrupt"));
    assert!(pos("long_focus") < pos("important_task"));
    assert_eq!(PushTrigger::label_of("urgent_task"), "緊急のタスク");
    assert_eq!(PushTrigger::label_of("important_task"), "重要なタスク");
    assert_eq!(
        PushTrigger::parse("important_task"),
        Some(PushTrigger::ImportantTask)
    );
}

// ----- 応答 -----

fn deliver_payload(conn: &rusqlite::Connection, p: PushPayload, at: Millis) -> String {
    store::record_decision(conn, &PushDecision::Deliver(p), at)
        .unwrap()
        .unwrap()
}

fn act(action: &str) -> PushResponse {
    PushResponse {
        action: Some(action.into()),
        ..Default::default()
    }
}

#[test]
fn task_start_returns_task_id_and_others_only_record() {
    let conn = db::open_in_memory().unwrap();
    let id = deliver_payload(&conn, payload::urgent_task("TASK1", "x", 0), 1000);
    let out = store::apply_response(&conn, &id, act("task_start"), 5000, TZ).unwrap();
    assert_eq!(out.start_task.as_deref(), Some("TASK1"));
    assert_eq!(out.snoozed_until, None);
    let l = store::get_log(&conn, &id).unwrap();
    assert_eq!(l.responded_at, Some(5000));

    for a in ["task_later", "task_skip_week"] {
        let id = deliver_payload(&conn, payload::important_task("TASK2", "x", Some(4)), 2000);
        let out = store::apply_response(&conn, &id, act(a), 6000, TZ).unwrap();
        assert_eq!(out, store::ResponseOutcome::default(), "{a}");
        let l = store::get_log(&conn, &id).unwrap();
        assert_eq!(l.responded_at, Some(6000));
        assert!(l.response_json.unwrap().contains(a));
    }

    // 対象のタスクを持たない通知への task_start はエラー。
    let id = deliver_payload(&conn, payload::day_start(), 3000);
    let e = store::apply_response(&conn, &id, act("task_start"), 7000, TZ).unwrap_err();
    assert_eq!(e.exit_code(), 2);
}

#[test]
fn payload_task_id_serde_is_optional() {
    // task_id の無い従来のペイロードも読める。出さないときは JSON にも出ない。
    let s = serde_json::to_string(&payload::day_start()).unwrap();
    assert!(!s.contains("task_id"));
    let p: PushPayload = serde_json::from_str(&s).unwrap();
    assert_eq!(p.task_id, None);
    let s = serde_json::to_string(&payload::urgent_task("T", "x", 0)).unwrap();
    assert!(s.contains(r#""task_id":"T""#));
}

// ===== block_start =====

fn task_ref(id: &str, status: crate::model::TaskStatus) -> TaskRef {
    TaskRef {
        id: id.into(),
        title: format!("タスク{id}"),
        status,
    }
}

/// 10:00-11:00 に A のブロックがあり、指定時刻の状況。
fn block_ctx(now: Millis) -> PushContext {
    let mut ctx = PushContext::new(now, TZ);
    ctx.time_blocks = vec![block(Some("A"), t(12, 10, 0), t(12, 11, 0))];
    ctx.task_refs = vec![task_ref("A", crate::model::TaskStatus::Todo)];
    ctx
}

fn block_delivered(ctx: &PushContext, c: &Push) -> Vec<PushTrigger> {
    delivered(&evaluate(ctx, c))
}

#[test]
fn block_start_fires_within_window_with_payload() {
    let ds = evaluate(&block_ctx(t(12, 10, 3)), &cfg());
    assert_eq!(delivered(&ds), vec![PushTrigger::BlockStart]);
    let PushDecision::Deliver(p) = &ds[0] else {
        panic!()
    };
    assert_eq!(p.title, "計画の時間です");
    assert_eq!(p.body, "今から「タスクA」の時間（10:00–11:00）です。始めますか？");
    assert_eq!(p.task_id.as_deref(), Some("A"));
    assert_eq!(p.block_id.as_deref(), Some(format!("b{}", t(12, 10, 0)).as_str()));
    let ids: Vec<&str> = p.actions.iter().map(|a| a.id.as_str()).collect();
    assert_eq!(ids, ["task_start", "block_skip"]);
    assert_eq!(p.actions[0].label, "始める");
    assert_eq!(p.actions[1].label, "今回はやめる");
    assert_eq!(p.open_screen.as_deref(), Some("today"));
    // 開始ちょうどと、窓の終わり（10 分）ちょうどは出る。
    assert_eq!(
        block_delivered(&block_ctx(t(12, 10, 0)), &cfg()),
        vec![PushTrigger::BlockStart]
    );
    assert_eq!(
        block_delivered(&block_ctx(t(12, 10, 10)), &cfg()),
        vec![PushTrigger::BlockStart]
    );
}

#[test]
fn block_start_not_outside_window_or_for_other_blocks() {
    // 開始前・窓の外。
    assert!(evaluate(&block_ctx(t(12, 9, 59)), &cfg()).is_empty());
    assert!(evaluate(&block_ctx(t(12, 10, 11)), &cfg()).is_empty());
    // ブロックがすでに終わっている。
    let mut ctx = block_ctx(t(12, 10, 3));
    ctx.time_blocks = vec![block(Some("A"), t(12, 9, 58), t(12, 10, 2))];
    assert!(evaluate(&ctx, &cfg()).is_empty());
    // タスクの無いブロック、タスク情報の無いブロック。
    let mut ctx = block_ctx(t(12, 10, 3));
    ctx.time_blocks = vec![block(None, t(12, 10, 0), t(12, 11, 0))];
    assert!(evaluate(&ctx, &cfg()).is_empty());
    let mut ctx = block_ctx(t(12, 10, 3));
    ctx.task_refs.clear();
    assert!(evaluate(&ctx, &cfg()).is_empty());
}

#[test]
fn block_start_not_when_doing_working_or_closed() {
    use crate::model::TaskStatus::{Doing, Done, Dropped};
    // すでに doing。
    let mut ctx = block_ctx(t(12, 10, 3));
    ctx.task_refs = vec![task_ref("A", Doing)];
    assert!(evaluate(&ctx, &cfg()).is_empty());
    // 完了・中止。
    for st in [Done, Dropped] {
        let mut ctx = block_ctx(t(12, 10, 3));
        ctx.task_refs = vec![task_ref("A", st)];
        assert!(evaluate(&ctx, &cfg()).is_empty(), "{st:?}");
    }
    // いまの作業区間がそのタスク。別のタスクなら出る。
    let mut ctx = block_ctx(t(12, 10, 3));
    ctx.recent_segments = vec![work("A", t(12, 9, 50), t(12, 10, 3))];
    assert!(evaluate(&ctx, &cfg()).is_empty());
    let mut ctx = block_ctx(t(12, 10, 3));
    ctx.recent_segments = vec![work("B", t(12, 9, 50), t(12, 10, 3))];
    assert_eq!(block_delivered(&ctx, &cfg()), vec![PushTrigger::BlockStart]);
}

#[test]
fn block_start_not_when_already_delivered_for_that_block() {
    let mut ctx = block_ctx(t(12, 10, 3));
    let p = payload::block_start(&format!("b{}", t(12, 10, 0)), "A", "タスクA", "10:00", "11:00");
    ctx.push_log.push(task_log(p, t(12, 10, 1), Some("block_skip")));
    assert!(evaluate(&ctx, &cfg()).is_empty());
    // 別のブロックの配信済みは影響しない。
    let mut ctx = block_ctx(t(12, 10, 3));
    let p = payload::block_start("other", "A", "タスクA", "09:00", "09:30");
    ctx.push_log.push(task_log(p, t(12, 9, 1), None));
    assert_eq!(block_delivered(&ctx, &cfg()), vec![PushTrigger::BlockStart]);
}

#[test]
fn block_start_not_while_idle_and_can_be_disabled() {
    let mut ctx = block_ctx(t(12, 10, 3));
    ctx.idle.is_idle = true;
    assert!(evaluate(&ctx, &cfg()).is_empty());
    let mut c = cfg();
    c.disabled_triggers = vec!["block_start".into()];
    assert!(evaluate(&block_ctx(t(12, 10, 3)), &c).is_empty());
    assert_eq!(PushTrigger::label_of("block_start"), "計画ブロックの開始");
}

#[test]
fn block_start_ignores_interval_rules() {
    // 直前（2 分前）に別の通知が配信され、同じトリガーも 30 分前に配信済み（別ブロック）でも出る。
    let mut ctx = block_ctx(t(12, 10, 3));
    ctx.push_log.push(log("long_focus", t(12, 10, 1), true));
    let p = payload::block_start("older", "A", "タスクA", "09:30", "09:40");
    ctx.push_log.push(task_log(p, t(12, 9, 33), None));
    assert_eq!(block_delivered(&ctx, &cfg()), vec![PushTrigger::BlockStart]);
    // ほかのトリガーは同じ状況で recent_push に抑制される（比較）。
    let mut other = PushContext::new(t(12, 10, 3), TZ);
    other.resumed_at = Some(t(12, 9, 0));
    other.push_log.push(log("long_focus", t(12, 10, 1), true));
    let ds = evaluate(&other, &cfg());
    assert!(matches!(&ds[0], PushDecision::Suppress { reason, .. } if reason == "recent_push"));
}

#[test]
fn block_start_still_obeys_snooze_daily_limit_and_meeting() {
    let mut ctx = block_ctx(t(12, 10, 3));
    let mut l = log("long_focus", t(12, 9, 0), true);
    l.snoozed_until = Some(t(12, 11, 0));
    ctx.push_log.push(l);
    let ds = evaluate(&ctx, &cfg());
    assert!(
        matches!(&ds[0], PushDecision::Suppress { trigger: PushTrigger::BlockStart, reason } if reason == "snoozed")
    );

    let mut ctx = block_ctx(t(12, 10, 3));
    for i in 0..6 {
        ctx.push_log.push(log("long_focus", t(12, 8, i), true));
    }
    let ds = evaluate(&ctx, &cfg());
    assert!(matches!(&ds[0], PushDecision::Suppress { reason, .. } if reason == "daily_limit"));

    // 会議中は保留される。
    let mut ctx = block_ctx(t(12, 10, 3));
    ctx.calendar_events = vec![event(t(12, 10, 0), t(12, 10, 30))];
    let ds = evaluate(&ctx, &cfg());
    assert!(
        matches!(&ds[0], PushDecision::Defer { trigger: PushTrigger::BlockStart, until } if *until == t(12, 10, 30))
    );
}

#[test]
fn block_start_not_deferred_by_its_own_focus_block_but_by_others() {
    // 自分自身がフォーカスブロックなら保留されない。
    let mut ctx = block_ctx(t(12, 10, 3));
    ctx.time_blocks[0].focus = true;
    assert_eq!(block_delivered(&ctx, &cfg()), vec![PushTrigger::BlockStart]);
    // 別のフォーカスブロックの最中なら保留される。
    let mut ctx = block_ctx(t(12, 10, 3));
    let mut other = block(None, t(12, 9, 30), t(12, 10, 30));
    other.focus = true;
    ctx.time_blocks.push(other);
    let ds = evaluate(&ctx, &cfg());
    assert!(matches!(&ds[0], PushDecision::Defer { until, .. } if *until == t(12, 10, 30)));
}

#[test]
fn block_start_priority_after_idle_return() {
    let ids: Vec<&str> = PushTrigger::ALL.iter().map(|t| t.as_str()).collect();
    let pos = |s: &str| ids.iter().position(|x| *x == s).unwrap();
    assert!(pos("idle_return") < pos("block_start") && pos("block_start") < pos("day_end"));
    // 同時に満たしたら idle_return が先、block_start は次の評価で出る（間隔ルールを受けない）。
    let mut ctx = block_ctx(t(12, 10, 32));
    ctx.time_blocks = vec![block(Some("A"), t(12, 10, 30), t(12, 11, 30))];
    ctx.last_idle = Some(ClosedIdle {
        start_at: t(12, 10, 0),
        end_at: t(12, 10, 30),
    });
    assert_eq!(block_delivered(&ctx, &cfg()), vec![PushTrigger::IdleReturn]);
    ctx.now = t(12, 10, 33);
    ctx.push_log.push(log("idle_return", t(12, 10, 32), true));
    assert_eq!(block_delivered(&ctx, &cfg()), vec![PushTrigger::BlockStart]);
}

#[test]
fn block_start_skip_only_records_and_start_returns_task() {
    let conn = db::open_in_memory().unwrap();
    let mk = || payload::block_start("B1", "TASK1", "x", "10:00", "11:00");
    let id = deliver_payload(&conn, mk(), 1000);
    let out = store::apply_response(&conn, &id, act("block_skip"), 5000, TZ).unwrap();
    assert_eq!(out, store::ResponseOutcome::default());
    let l = store::get_log(&conn, &id).unwrap();
    assert_eq!(l.responded_at, Some(5000));
    assert!(l.response_json.unwrap().contains("block_skip"));
    let id = deliver_payload(&conn, mk(), 2000);
    let out = store::apply_response(&conn, &id, act("task_start"), 5000, TZ).unwrap();
    assert_eq!(out.start_task.as_deref(), Some("TASK1"));
}

#[test]
fn payload_block_id_serde_is_optional() {
    let s = serde_json::to_string(&payload::day_start()).unwrap();
    assert!(!s.contains("block_id"));
    let s = serde_json::to_string(&payload::block_start("B", "T", "x", "10:00", "11:00")).unwrap();
    assert!(s.contains(r#""block_id":"B""#));
}

// ===== idle_return の候補タスク =====

fn doing_span(task: &str, start: Millis, end: Option<Millis>) -> DoingSpan {
    DoingSpan {
        task_id: task.into(),
        start_at: start,
        end_at: end,
    }
}

fn idle_task_label(ds: &[PushDecision]) -> Option<(String, Option<String>)> {
    let PushDecision::Deliver(p) = ds.iter().find(|d| matches!(d, PushDecision::Deliver(_)))? else {
        return None;
    };
    let first = p.actions.first()?;
    (first.id == "idle_task").then(|| (first.label.clone(), p.task_id.clone()))
}

#[test]
fn idle_return_candidate_from_doing_log() {
    use crate::model::TaskStatus::Doing;
    let mut ctx = idle_ctx();
    ctx.task_refs = vec![task_ref("A", Doing)];
    // 無操作の前から開いたまま（進行中）。
    ctx.doing_spans = vec![doing_span("A", t(12, 9, 0), None)];
    let ds = evaluate(&ctx, &cfg());
    assert_eq!(delivered(&ds), vec![PushTrigger::IdleReturn]);
    assert_eq!(
        idle_task_label(&ds),
        Some(("タスクAをしていた".into(), Some("A".into())))
    );
    let PushDecision::Deliver(p) = &ds[0] else {
        panic!()
    };
    let ids: Vec<&str> = p.actions.iter().map(|a| a.id.as_str()).collect();
    assert_eq!(
        ids,
        [
            "idle_task",
            "idle_break",
            "idle_meeting",
            "idle_thinking",
            "idle_away"
        ]
    );
    assert!(p.actions.len() <= 5, "トーストのボタンは最大 5 個");
    assert_eq!(p.idle_range, Some((t(12, 10, 0), t(12, 10, 30))));
    // 無操作の前に閉じたログは候補にならない。
    ctx.doing_spans = vec![doing_span("A", t(12, 9, 0), Some(t(12, 9, 59)))];
    assert_eq!(idle_task_label(&evaluate(&ctx, &cfg())), None);
}

#[test]
fn idle_return_candidate_from_block_and_doing_wins() {
    use crate::model::TaskStatus::{Doing, Todo};
    let mut ctx = idle_ctx();
    ctx.task_refs = vec![task_ref("A", Todo), task_ref("B", Doing), task_ref("C", Todo)];
    // ブロックだけ: 重なりが最も長いもの（A は 10 分、C は 20 分）。
    ctx.time_blocks = vec![
        block(Some("A"), t(12, 9, 50), t(12, 10, 10)),
        block(Some("C"), t(12, 10, 10), t(12, 10, 40)),
        block(None, t(12, 10, 0), t(12, 10, 30)),
    ];
    assert_eq!(
        idle_task_label(&evaluate(&ctx, &cfg())),
        Some(("タスクCをしていた".into(), Some("C".into())))
    );
    // doing があれば、ブロックのほうが長くても doing を優先する（重なり 5 分）。
    ctx.doing_spans = vec![doing_span("B", t(12, 10, 25), None)];
    assert_eq!(
        idle_task_label(&evaluate(&ctx, &cfg())),
        Some(("タスクBをしていた".into(), Some("B".into())))
    );
    // doing が複数なら重なりが長いもの（A は 15 分）。
    ctx.doing_spans = vec![
        doing_span("B", t(12, 10, 25), None),
        doing_span("A", t(12, 10, 5), Some(t(12, 10, 20))),
    ];
    assert_eq!(
        idle_task_label(&evaluate(&ctx, &cfg())).unwrap().1.as_deref(),
        Some("A")
    );
}

#[test]
fn idle_return_without_candidate_is_unchanged() {
    let ds = evaluate(&idle_ctx(), &cfg());
    assert_eq!(idle_task_label(&ds), None);
    // 重ならないブロック、タスク情報の無いブロックは候補にならない。
    let mut ctx = idle_ctx();
    ctx.time_blocks = vec![block(Some("A"), t(12, 10, 30), t(12, 11, 0))];
    ctx.task_refs = vec![task_ref("A", crate::model::TaskStatus::Todo)];
    assert_eq!(idle_task_label(&evaluate(&ctx, &cfg())), None);
    let mut ctx = idle_ctx();
    ctx.time_blocks = vec![block(Some("A"), t(12, 10, 0), t(12, 10, 30))];
    assert_eq!(idle_task_label(&evaluate(&ctx, &cfg())), None);
}

#[test]
fn idle_return_asks_after_locked_auto_break_only_with_candidate() {
    let mut locked = seg(SegmentKind::Break, t(12, 10, 0), t(12, 10, 30));
    locked.decided_by = Some("rule:locked".into());
    locked.confidence = Some(1.0);
    // 候補が無ければ従来どおり尋ねない。
    let mut ctx = idle_ctx();
    ctx.recent_segments = vec![locked.clone()];
    assert!(evaluate(&ctx, &cfg()).is_empty());
    // 候補があれば尋ねる。
    ctx.time_blocks = vec![block(Some("A"), t(12, 9, 30), t(12, 11, 0))];
    ctx.task_refs = vec![task_ref("A", crate::model::TaskStatus::Todo)];
    let ds = evaluate(&ctx, &cfg());
    assert_eq!(delivered(&ds), vec![PushTrigger::IdleReturn]);
    assert_eq!(idle_task_label(&ds).unwrap().1.as_deref(), Some("A"));
    // ユーザーが確定した休憩・会議は、候補があっても尋ねない。
    for mutate in 0..3 {
        let mut c = ctx.clone();
        let mut s = locked.clone();
        match mutate {
            0 => s.status = SegmentStatus::Confirmed,
            1 => s.decided_by = Some("user".into()),
            _ => s.kind = SegmentKind::Meeting,
        }
        c.recent_segments = vec![s];
        assert!(evaluate(&c, &cfg()).is_empty(), "{mutate}");
    }
    // 予定と重なる無操作も尋ねない。
    let mut c = ctx.clone();
    c.calendar_events = vec![event(t(12, 10, 5), t(12, 10, 25))];
    assert!(evaluate(&c, &cfg()).is_empty());
}

#[test]
fn idle_task_answer_confirms_range_as_offline_work_and_survives_rebuild() {
    use crate::config::Config;
    use crate::model::{IdleReason, IdleSpan, Task, TaskKind, TaskStatus};
    use crate::repo::collect as rc;
    use crate::timefmt::Range;
    let conn = db::open_in_memory().unwrap();
    crate::repo::task::insert(
        &conn,
        &Task {
            id: "TASKA".into(),
            title: "読書".into(),
            body_md: String::new(),
            status: TaskStatus::Doing,
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
        },
    )
    .unwrap();
    let (s, e) = (t(12, 10, 0), t(12, 10, 30));
    let now = t(12, 10, 32);
    // 30 分のロック（長いロックは自動で休憩になる）。
    rc::open_idle_span(
        &conn,
        &IdleSpan {
            id: "I1".into(),
            start_at: s,
            end_at: None,
            reason: IdleReason::Locked,
        },
    )
    .unwrap();
    rc::close_idle_span(&conn, "I1", e).unwrap();
    let cfg = Config::default();
    let range = Range::new(t(12, 9, 0), t(12, 11, 0));
    crate::rebuild::rebuild(&conn, &cfg, range, now).unwrap();
    let before = crate::repo::segment::list(&conn, range).unwrap();
    assert_eq!(before.len(), 1);
    assert_eq!(
        (before[0].kind, before[0].decided_by.as_deref()),
        (SegmentKind::Break, Some("rule:locked"))
    );

    let id = deliver_payload(
        &conn,
        payload::idle_return(s, e, Some(("TASKA", "読書"))),
        t(12, 10, 31),
    );
    let out = store::apply_response(&conn, &id, act("idle_task"), now, TZ).unwrap();
    assert_eq!(out.idle_labeled, 1);
    assert!(store::get_log(&conn, &id).unwrap().responded_at.is_some());

    let check = |conn: &rusqlite::Connection| {
        let segs = crate::repo::segment::list(conn, range).unwrap();
        assert_eq!(segs.len(), 1, "{segs:?}");
        let g = &segs[0];
        assert_eq!((g.start_at, g.end_at), (s, e));
        assert_eq!(
            (g.kind, g.offline, g.status),
            (SegmentKind::Work, true, SegmentStatus::Confirmed)
        );
        assert_eq!(g.task_id.as_deref(), Some("TASKA"));
    };
    check(&conn);
    // 再計算でも、そのタスクの実績（PC 外の作業）として残る。
    crate::rebuild::rebuild(&conn, &cfg, range, now).unwrap();
    check(&conn);
    // 範囲指定の補正と同じ形で保存される: offline の override が補正に結び付く。
    let ovs = crate::repo::overrides::list(&conn, range).unwrap();
    assert_eq!(ovs.len(), 1);
    assert!(ovs[0].offline && ovs[0].correction_id.is_some());
    assert_eq!(
        (ovs[0].kind, ovs[0].task_id.as_deref()),
        (Some(SegmentKind::Work), Some("TASKA"))
    );
    assert_eq!(
        crate::repo::correction::list(&conn, None, Some("TASKA"))
            .unwrap()
            .len(),
        1
    );

    // 通知にタスクが無ければエラー。
    let id = deliver_payload(&conn, payload::idle_return(s, e, None), t(12, 10, 33));
    let err = store::apply_response(&conn, &id, act("idle_task"), now, TZ).unwrap_err();
    assert_eq!(err.exit_code(), 2);
}

// ===== kr_idle =====

fn kr_ctx(now: Millis) -> PushContext {
    let mut ctx = PushContext::new(now, TZ);
    ctx.kr_idle = vec![
        KrIdleNudge {
            id: "K1".into(),
            title: "英語の勉強".into(),
        },
        KrIdleNudge {
            id: "K2".into(),
            title: "運動".into(),
        },
    ];
    ctx
}

#[test]
fn kr_idle_fires_on_or_after_weekday_with_facts_only() {
    // 既定は水曜（14 日）。火曜（13 日）はまだ出ない。
    assert!(evaluate(&kr_ctx(t(13, 10, 0)), &cfg()).is_empty());
    let ds = evaluate(&kr_ctx(t(14, 10, 0)), &cfg());
    assert_eq!(delivered(&ds), vec![PushTrigger::KrIdle]);
    let PushDecision::Deliver(p) = &ds[0] else {
        panic!()
    };
    assert_eq!(
        p.body,
        "「英語の勉強」「運動」に、今週はまだ時間が入っていません。"
    );
    let ids: Vec<&str> = p.actions.iter().map(|a| a.id.as_str()).collect();
    assert_eq!(ids, vec!["open", "kr_skip_week"]);
    assert_eq!(p.open_screen.as_deref(), Some("today/design"));
    // 水曜以降なら金曜でも出る。
    assert_eq!(
        delivered(&evaluate(&kr_ctx(t(16, 10, 0)), &cfg())),
        vec![PushTrigger::KrIdle]
    );
}

#[test]
fn kr_idle_summarizes_many_krs() {
    let mut ctx = kr_ctx(t(14, 10, 0));
    for i in 3..=6 {
        ctx.kr_idle.push(KrIdleNudge {
            id: format!("K{i}"),
            title: format!("k{i}"),
        });
    }
    let PushDecision::Deliver(p) = &evaluate(&ctx, &cfg())[0] else {
        panic!()
    };
    assert_eq!(
        p.body,
        "「英語の勉強」「運動」「k3」ほか 3 件に、今週はまだ時間が入っていません。"
    );
}

#[test]
fn kr_idle_once_a_week_and_weekday_setting() {
    let mut ctx = kr_ctx(t(16, 10, 0));
    // 今週すでに配信済み（応答は問わない）。
    ctx.push_log = vec![task_log(
        payload::kr_idle(&["a"]),
        t(14, 10, 0),
        Some("kr_skip_week"),
    )];
    assert!(delivered(&evaluate(&ctx, &cfg())).is_empty());
    // 先週の配信は今週に持ち越さない。
    ctx.push_log = vec![task_log(payload::kr_idle(&["a"]), t(9, 10, 0), None)];
    assert_eq!(delivered(&evaluate(&ctx, &cfg())), vec![PushTrigger::KrIdle]);

    // 0 で無効、1（月曜）なら月曜から、7（日曜）なら日曜だけ。
    let mut c = cfg();
    c.kr_idle_weekday = 0;
    assert!(evaluate(&kr_ctx(t(16, 10, 0)), &c).is_empty());
    c.kr_idle_weekday = 8;
    assert!(evaluate(&kr_ctx(t(16, 10, 0)), &c).is_empty());
    c.kr_idle_weekday = 1;
    assert_eq!(
        delivered(&evaluate(&kr_ctx(t(12, 10, 0)), &c)),
        vec![PushTrigger::KrIdle]
    );
    c.kr_idle_weekday = 7;
    assert!(evaluate(&kr_ctx(t(16, 10, 0)), &c).is_empty());
    // 日曜は週次の振り返りも条件を満たすので、それを無効にして見る。
    c.disabled_triggers = vec!["weekly".into()];
    assert_eq!(
        delivered(&evaluate(&kr_ctx(t(18, 10, 0)), &c)),
        vec![PushTrigger::KrIdle]
    );
}

#[test]
fn kr_idle_needs_krs_work_hours_and_not_disabled() {
    let mut ctx = kr_ctx(t(14, 10, 0));
    ctx.kr_idle.clear();
    assert!(evaluate(&ctx, &cfg()).is_empty());
    for (h, m) in [(7, 0), (18, 30), (22, 0)] {
        // 勤務時間外は kr_idle を出さない（終業の通知など、ほかのトリガーは別）。
        assert!(
            !delivered(&evaluate(&kr_ctx(t(14, h, m)), &cfg())).contains(&PushTrigger::KrIdle),
            "{h}:{m}"
        );
    }
    let mut c = cfg();
    c.disabled_triggers = vec!["kr_idle".into()];
    assert!(evaluate(&kr_ctx(t(14, 10, 0)), &c).is_empty());
    // 無操作中は出ない。
    let mut ctx = kr_ctx(t(14, 10, 0));
    ctx.idle = IdleState {
        is_idle: true,
        since_last_input_sec: 900,
    };
    assert!(evaluate(&ctx, &cfg()).is_empty());
}

#[test]
fn kr_idle_obeys_gate_snooze_daily_limit_and_meeting() {
    let now = t(14, 10, 0);
    // スヌーズ中。
    let mut ctx = kr_ctx(now);
    let mut l = log("day_start", t(14, 8, 30), true);
    l.snoozed_until = Some(now + 30 * MIN);
    ctx.push_log = vec![l];
    assert!(delivered(&evaluate(&ctx, &cfg())).is_empty());
    // 1 日上限。
    let mut c = cfg();
    c.daily_limit = 1;
    let mut ctx = kr_ctx(now);
    ctx.push_log = vec![log("day_start", t(14, 8, 30), true)];
    assert!(delivered(&evaluate(&ctx, &c)).is_empty());
    // 会議中は保留。
    let mut ctx = kr_ctx(now);
    ctx.calendar_events = vec![event(t(14, 9, 30), t(14, 10, 30))];
    let ds = evaluate(&ctx, &cfg());
    assert!(matches!(
        ds[0],
        PushDecision::Defer {
            trigger: PushTrigger::KrIdle,
            ..
        }
    ));
}

#[test]
fn kr_skip_week_only_records() {
    let conn = db::open_in_memory().unwrap();
    let id = deliver_payload(&conn, payload::kr_idle(&["a"]), t(14, 10, 0));
    let out = store::apply_response(&conn, &id, act("kr_skip_week"), t(14, 10, 5), TZ).unwrap();
    assert_eq!(out.start_task, None);
    assert!(store::get_log(&conn, &id).unwrap().responded_at.is_some());
}
