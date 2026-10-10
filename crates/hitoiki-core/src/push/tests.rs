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
    assert_eq!(p.open_screen.as_deref(), Some("reflection/day_start"));
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
    ctx.recent_segments = vec![seg(SegmentKind::Interrupt, now - 10 * MIN, now - 2 * MIN)];
    assert_eq!(
        delivered(&evaluate(&ctx, &cfg())),
        vec![PushTrigger::AfterInterrupt]
    );
    // 6 分前に終了 → 対象外。
    ctx.recent_segments = vec![seg(SegmentKind::Interrupt, now - 10 * MIN, now - 6 * MIN)];
    assert!(evaluate(&ctx, &cfg()).is_empty());
    // 配信済み。
    ctx.recent_segments = vec![seg(SegmentKind::Interrupt, now - 10 * MIN, now - 2 * MIN)];
    ctx.push_log.push(log("after_interrupt", now - MIN, true));
    assert!(delivered(&evaluate(&ctx, &cfg())).is_empty());
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
        (Some(4), None, id.as_str(), 5000)
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
