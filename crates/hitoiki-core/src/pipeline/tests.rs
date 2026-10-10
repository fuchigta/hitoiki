//! パイプラインのテーブルテスト（19章）。時刻は「分」で書き、ID 以外で比較する。

use super::*;
use crate::config::Config;

const M: Millis = 60_000;

fn base() -> PipelineInput {
    let mut cfg = Config::default();
    cfg.agent.watch_max_min = 10;
    let mut i = PipelineInput::new(Range::new(0, 1000 * M), 1000 * M, &cfg);
    i.auto_confidence_threshold = 0.8;
    i
}

fn win(id: &str, s: i64, e: i64, hwnd: &str, exe: &str, title: &str) -> WindowSpan {
    WindowSpan {
        id: id.into(),
        start_at: s * M,
        end_at: Some(e * M),
        hwnd: Some(hwnd.into()),
        pid: None,
        exe_path: None,
        exe_name: Some(exe.into()),
        title: Some(title.into()),
        excluded: false,
    }
}

fn idle(id: &str, s: i64, e: i64) -> IdleSpan {
    IdleSpan {
        id: id.into(),
        start_at: s * M,
        end_at: Some(e * M),
        reason: IdleReason::NoInput,
    }
}

fn ev(id: &str, at: i64, sid: &str, kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        id: id.into(),
        at: at * M,
        agent: "claude-code".into(),
        session_id: sid.into(),
        kind,
        cwd: None,
        git_branch: None,
        prompt_excerpt: None,
        source_file: None,
        source_offset: None,
    }
}

fn rule(id: &str, task: &str, field: MatchField, pat: &str, pri: i64) -> TaskMatchRule {
    TaskMatchRule {
        id: id.into(),
        task_id: task.into(),
        field,
        pattern: pat.into(),
        pattern_type: PatternType::Contains,
        priority: pri,
        enabled: true,
    }
}

fn doing(task: &str, s: i64, e: Option<i64>) -> TaskDoingLog {
    TaskDoingLog {
        id: format!("D{task}{s}"),
        task_id: task.into(),
        start_at: s * M,
        end_at: e.map(|e| e * M),
    }
}

fn cal(s: i64, e: i64, title: &str) -> CalendarEvent {
    CalendarEvent {
        id: format!("C{s}"),
        source: "x".into(),
        external_id: format!("x{s}"),
        start_at: s * M,
        end_at: e * M,
        all_day: false,
        title: title.into(),
        location: None,
        is_busy: true,
        fetched_at: 0,
    }
}

use AgentEventKind::*;
use SegmentKind::*;

/// (開始分, 終了分, 種別) の列。
fn shape(o: &PipelineOutput) -> Vec<(i64, i64, SegmentKind)> {
    o.segments
        .iter()
        .map(|s| (s.start_at / M, s.end_at / M, s.kind))
        .collect()
}

fn find(o: &PipelineOutput, start_min: i64) -> &ActivitySegment {
    o.segments
        .iter()
        .find(|s| s.start_at == start_min * M)
        .unwrap_or_else(|| panic!("{start_min} 分開始の区間なし: {:?}", shape(o)))
}

#[test]
fn timeline_idle_wins_and_later_window_wins() {
    let mut i = base();
    i.windows = vec![
        win("A", 0, 20, "H1", "a.exe", "a"),
        win("B", 10, 30, "H2", "b.exe", "b"), // A と重なる: 後から始まった B を優先
        win("C", 40, 50, "H3", "c.exe", "c"), // 30〜40 はギャップ
    ];
    i.idles = vec![idle("I", 25, 45)];
    let o = build(&i, None);
    assert_eq!(shape(&o), vec![(0, 25, Work), (25, 45, Idle), (45, 50, Work)]);
    // 0〜10 は A、10〜25 は B（統合後の ID は和集合）
    assert_eq!(find(&o, 0).window_span_ids, vec!["A", "B"]);
    assert_eq!(find(&o, 0).exe_name.as_deref(), Some("b.exe")); // 長い方（15 分）
    assert_eq!(find(&o, 25).status, SegmentStatus::NeedsReview);
}

#[test]
fn excluded_window_has_no_title_and_open_span_closes_at_now() {
    let mut i = base();
    let mut w = win("A", 0, 0, "H1", "KeePass.exe", "secret");
    w.end_at = None;
    w.excluded = true;
    i.windows = vec![w];
    i.now = 7 * M;
    let o = build(&i, None);
    assert_eq!(shape(&o), vec![(0, 7, Work)]);
    assert_eq!(o.segments[0].title, None);
    assert_eq!(o.segments[0].exe_name.as_deref(), Some("KeePass.exe"));
}

#[test]
fn output_is_clipped_to_range() {
    let mut i = base();
    i.range = Range::new(10 * M, 20 * M);
    i.windows = vec![win("A", 0, 30, "H1", "a.exe", "a")];
    assert_eq!(shape(&build(&i, None)), vec![(10, 20, Work)]);
}

// ===== 7.3 エージェント対話 =====

fn term(id: &str, s: i64, e: i64, title: &str) -> WindowSpan {
    win(id, s, e, "H1", "WindowsTerminal.exe", title)
}

fn agent_input() -> PipelineInput {
    let mut i = base();
    i.watch_max = 0; // 見守りは個別のテストで確認する
    i
}

fn of_kind(o: &PipelineOutput, k: SegmentKind) -> Vec<(i64, i64)> {
    o.segments
        .iter()
        .filter(|s| s.kind == k)
        .map(|s| (s.start_at / M, s.end_at / M))
        .collect()
}

#[test]
fn dialog_quick_return() {
    let mut i = agent_input();
    i.windows = vec![term("W", 0, 20, "claude")];
    i.events = vec![
        ev("1", 0, "S", SessionStart),
        ev("2", 1, "S", PromptSubmit),
        ev("3", 5, "S", ResponseComplete),
        ev("4", 6, "S", PromptSubmit),
    ];
    let o = build(&i, None);
    assert_eq!(of_kind(&o, AgentDialog), vec![(0, 1), (5, 6)]);
    assert_eq!(o.bindings.len(), 2);
    assert_eq!(o.bindings[0].hwnd, "H1");
    assert_eq!(o.bindings[0].bound_at, M);
    assert_eq!(o.segments[0].agent_session_id.as_deref(), Some("claude-code:S"));
}

#[test]
fn dialog_excludes_lag_while_away() {
    let mut i = agent_input();
    i.windows = vec![
        term("W1", 0, 5, "claude"),
        win("W2", 5, 35, "H2", "chrome.exe", "docs"),
        term("W3", 35, 37, "claude"),
    ];
    i.events = vec![
        ev("1", 0, "S", SessionStart),
        ev("2", 1, "S", PromptSubmit),
        ev("3", 5, "S", ResponseComplete),
        ev("4", 37, "S", PromptSubmit),
    ];
    let o = build(&i, None);
    assert_eq!(of_kind(&o, AgentDialog), vec![(0, 1), (35, 37)]);
    assert_eq!(of_kind(&o, Work), vec![(1, 35)]); // 同一内容なので統合される
}

#[test]
fn dialog_two_returns_make_two_segments() {
    let mut i = agent_input();
    i.windows = vec![
        term("W1", 0, 5, "claude"),
        win("W2", 5, 10, "H2", "chrome.exe", "x"),
        term("W3", 10, 12, "claude"),
        win("W4", 12, 20, "H2", "chrome.exe", "y"),
        term("W5", 20, 22, "claude"),
    ];
    i.events = vec![
        ev("1", 0, "S", SessionStart),
        ev("2", 5, "S", ResponseComplete),
        ev("3", 22, "S", PromptSubmit),
    ];
    let o = build(&i, None);
    assert_eq!(of_kind(&o, AgentDialog), vec![(10, 12), (20, 22)]);
}

#[test]
fn dialog_without_start_backtracks_over_same_title() {
    let mut i = agent_input();
    i.windows = vec![
        win("W0", 0, 5, "H2", "chrome.exe", "x"),
        // 同じ hwnd・同じタイトル（先頭のスピナーは無視）が 5 分から続いていた
        term("W1", 5, 8, "✳ Claude Code"),
        term("W2", 8, 10, "⠂ Claude Code"),
        term("W3", 10, 12, "· Claude Code"),
    ];
    i.events = vec![ev("1", 11, "S", PromptSubmit)];
    let o = build(&i, None);
    assert_eq!(of_kind(&o, AgentDialog), vec![(5, 11)]);
}

#[test]
fn title_spinner_normalization() {
    use super::agent::norm_title;
    assert_eq!(norm_title(Some("✳ Fix bug")), "Fix bug");
    assert_eq!(norm_title(Some("⠐ Fix bug")), "Fix bug");
    assert_eq!(norm_title(Some("* · Fix bug")), "Fix bug");
    assert_eq!(norm_title(Some("作業中")), "作業中");
    assert_eq!(norm_title(None), "");
}

#[test]
fn dialog_separates_tabs_of_same_hwnd() {
    let mut i = agent_input();
    i.windows = vec![
        term("W1", 0, 10, "Tab A"),
        term("W2", 10, 20, "Tab B"),
        term("W3", 20, 30, "Tab A"),
    ];
    i.events = vec![
        ev("1", 0, "SA", SessionStart),
        ev("2", 5, "SA", PromptSubmit),
        ev("3", 6, "SA", ResponseComplete),
        ev("4", 10, "SB", SessionStart),
        ev("5", 15, "SB", PromptSubmit),
        ev("6", 25, "SA", PromptSubmit),
    ];
    let o = build(&i, None);
    let by = |sess: &str| -> Vec<(i64, i64)> {
        o.segments
            .iter()
            .filter(|s| s.kind == AgentDialog && s.agent_session_id.as_deref() == Some(sess))
            .map(|s| (s.start_at / M, s.end_at / M))
            .collect()
    };
    assert_eq!(by("claude-code:SA"), vec![(0, 5), (6, 10), (20, 25)]);
    assert_eq!(by("claude-code:SB"), vec![(10, 15)]);
    assert_eq!(o.bindings.len(), 3);
    assert_eq!(o.bindings[1].title_at_bind.as_deref(), Some("Tab B"));
}

#[test]
fn bind_lookback_to_terminal_when_other_app_in_front() {
    let mut i = agent_input();
    i.bind_lookback = 5_000;
    // 送信時刻には既に chrome が前面（時刻ずれ）。直前 3 秒に端末があった。
    let mut t = term("W1", 0, 0, "claude");
    t.end_at = Some(10 * M - 3_000);
    let mut c = win("W2", 0, 0, "H2", "chrome.exe", "x");
    c.start_at = 10 * M - 3_000;
    c.end_at = Some(20 * M);
    i.windows = vec![t, c];
    i.events = vec![ev("1", 0, "S", SessionStart), ev("2", 10, "S", PromptSubmit)];
    let o = build(&i, None);
    assert_eq!(o.bindings.len(), 1);
    assert_eq!(o.bindings[0].hwnd, "H1");

    // 範囲外なら対応付け不可
    i.bind_lookback = 1_000;
    let o = build(&i, None);
    assert!(o.bindings.is_empty());
    assert!(of_kind(&o, AgentDialog).is_empty());
}

#[test]
fn watch_stops_at_idle() {
    let mut i = base();
    i.windows = vec![term("W", 0, 30, "claude")];
    i.idles = vec![idle("I", 8, 30)];
    i.events = vec![ev("1", 0, "S", SessionStart), ev("2", 5, "S", PromptSubmit)];
    let o = build(&i, None);
    assert_eq!(
        shape(&o),
        vec![(0, 5, AgentDialog), (5, 8, AgentWatch), (8, 30, Idle)]
    );
}

#[test]
fn watch_stops_at_watch_max_and_at_window_switch() {
    let mut i = base();
    i.windows = vec![term("W", 0, 30, "claude")];
    i.events = vec![ev("1", 5, "S", PromptSubmit)];
    let o = build(&i, None);
    assert_eq!(of_kind(&o, AgentWatch), vec![(5, 15)]);
    assert_eq!(find(&o, 15).kind, Work);

    let mut i = base();
    i.windows = vec![
        term("W", 0, 8, "claude"),
        win("X", 8, 30, "H2", "chrome.exe", "x"),
    ];
    i.events = vec![ev("1", 5, "S", PromptSubmit)];
    let o = build(&i, None);
    assert_eq!(of_kind(&o, AgentWatch), vec![(5, 8)]);
}

#[test]
fn watch_stops_at_response_complete() {
    let mut i = base();
    i.windows = vec![term("W", 0, 30, "claude")];
    i.events = vec![ev("1", 5, "S", PromptSubmit), ev("2", 7, "S", ResponseComplete)];
    let o = build(&i, None);
    assert_eq!(of_kind(&o, AgentWatch), vec![(5, 7)]);
}

#[test]
fn agent_task_by_cwd_rule_then_doing_then_none() {
    let mut i = agent_input();
    i.windows = vec![term("W", 0, 10, "claude")];
    let mut e = ev("1", 5, "S", PromptSubmit);
    e.cwd = Some(r"C:\src\foo\bar".into());
    i.events = vec![e];
    i.task_rules = vec![TaskMatchRule {
        id: "R".into(),
        task_id: "T1".into(),
        field: MatchField::Cwd,
        pattern: "C:/src/foo/**".into(),
        pattern_type: PatternType::Glob,
        priority: 0,
        enabled: true,
    }];
    i.doing_logs = vec![doing("T9", 0, None)];
    let o = build(&i, None);
    let d = find(&o, 0);
    assert_eq!(
        (d.task_id.as_deref(), d.decided_by.as_deref()),
        (Some("T1"), Some("rule"))
    );
    assert_eq!((d.status, d.confidence), (SegmentStatus::Auto, Some(1.0)));

    // cwd が一致しなければセッション開始時点の doing
    i.events[0].cwd = Some(r"C:\other".into());
    let o = build(&i, None);
    let d = find(&o, 0);
    assert_eq!(
        (d.task_id.as_deref(), d.decided_by.as_deref()),
        (Some("T9"), Some("doing"))
    );
    assert_eq!((d.status, d.confidence), (SegmentStatus::NeedsReview, Some(0.5)));

    // doing も無ければ task なしの要確認
    i.doing_logs.clear();
    let o = build(&i, None);
    let d = find(&o, 0);
    assert_eq!(
        (d.task_id.as_deref(), d.status),
        (None, SegmentStatus::NeedsReview)
    );
}

// ===== 7.2 会議 =====

#[test]
fn idle_overlapping_calendar_is_meeting() {
    let mut i = base();
    i.windows = vec![win("A", 0, 10, "H1", "a.exe", "a")];
    i.idles = vec![idle("I", 10, 40)];
    i.calendar = vec![cal(20, 30, "週次 定例"), {
        let mut c = cal(35, 38, "終日");
        c.all_day = true;
        c
    }];
    i.task_rules = vec![rule("R", "TM", MatchField::Title, "定例", 0)];
    let o = build(&i, None);
    assert_eq!(
        shape(&o),
        vec![(0, 10, Work), (10, 20, Idle), (20, 30, Meeting), (30, 40, Idle)]
    );
    let m = find(&o, 20);
    assert_eq!(m.task_id.as_deref(), Some("TM"));
    assert_eq!(m.decided_by.as_deref(), Some("calendar"));
    assert_eq!((m.status, m.confidence), (SegmentStatus::Auto, Some(1.0)));
    assert_eq!(find(&o, 10).status, SegmentStatus::NeedsReview);
    assert_eq!(find(&o, 30).status, SegmentStatus::NeedsReview);
}

#[test]
fn meeting_app_in_front_during_event_is_meeting() {
    let mut i = base();
    i.windows = vec![
        win("A", 0, 30, "H1", "Zoom.exe", "Zoom"),
        win("B", 30, 40, "H2", "Zoom.exe", "Zoom"),
    ];
    i.calendar = vec![cal(10, 30, "sync")];
    let o = build(&i, None);
    assert_eq!(shape(&o), vec![(0, 10, Work), (10, 30, Meeting), (30, 40, Work)]);
}

// ===== override / ルール / 判定器 / 統合 / doing =====

#[test]
fn override_wins_and_splits() {
    let mut i = base();
    i.windows = vec![win("A", 0, 30, "H1", "a.exe", "docs")];
    i.task_rules = vec![rule("R", "T1", MatchField::Title, "docs", 0)];
    i.overrides = vec![SegmentOverride {
        id: "O".into(),
        start_at: 10 * M,
        end_at: 20 * M,
        kind: Some(Break),
        task_id: Some("T2".into()),
        activity_label: Some("休憩".into()),
        created_at: 0,
    }];
    let o = build(&i, None);
    assert_eq!(shape(&o), vec![(0, 10, Work), (10, 20, Break), (20, 30, Work)]);
    let b = find(&o, 10);
    assert_eq!(b.task_id.as_deref(), Some("T2"));
    assert_eq!(b.activity_label.as_deref(), Some("休憩"));
    assert_eq!(
        (b.status, b.decided_by.as_deref()),
        (SegmentStatus::Confirmed, Some("user"))
    );
    assert_eq!(find(&o, 0).task_id.as_deref(), Some("T1"));
}

#[test]
fn override_keeps_unspecified_fields() {
    let mut i = base();
    i.windows = vec![term("W", 0, 10, "claude")];
    i.watch_max = 0;
    i.events = vec![ev("1", 5, "S", PromptSubmit)];
    i.doing_logs = vec![doing("T9", 0, None)];
    i.overrides = vec![SegmentOverride {
        id: "O".into(),
        start_at: 0,
        end_at: 5 * M,
        kind: None,
        task_id: None,
        activity_label: Some("設計".into()),
        created_at: 0,
    }];
    let o = build(&i, None);
    let s = find(&o, 0);
    assert_eq!(s.kind, AgentDialog);
    assert_eq!(s.task_id.as_deref(), Some("T9"));
    assert_eq!(s.activity_label.as_deref(), Some("設計"));
    assert_eq!(s.status, SegmentStatus::Confirmed);
}

#[test]
fn rule_priority_and_activity_label() {
    let mut i = base();
    i.windows = vec![win("A", 0, 10, "H1", "chrome.exe", "Hitoiki docs")];
    i.task_rules = vec![
        rule("R1", "LOW", MatchField::Title, "hitoiki", 1),
        rule("R2", "HIGH", MatchField::Title, "DOCS", 5),
        rule("R3", "SAME_B", MatchField::Exe, "chrome", 5),
    ];
    i.activity_rules = vec![ActivityRule {
        id: "A1".into(),
        field: MatchField::Exe,
        pattern: "chrome.exe".into(),
        pattern_type: PatternType::Glob,
        activity_label: "調査".into(),
        priority: 0,
        enabled: true,
    }];
    let o = build(&i, None);
    let s = find(&o, 0);
    assert_eq!(s.task_id.as_deref(), Some("HIGH")); // R2 < R3 の id 順で先
    assert_eq!(s.activity_label.as_deref(), Some("調査"));
    assert_eq!(
        (s.decided_by.as_deref(), s.confidence, s.status),
        (Some("rule"), Some(1.0), SegmentStatus::Auto)
    );
}

#[test]
fn disabled_rule_is_ignored() {
    let mut i = base();
    i.windows = vec![win("A", 0, 10, "H1", "chrome.exe", "docs")];
    let mut r = rule("R", "T1", MatchField::Title, "docs", 0);
    r.enabled = false;
    i.task_rules = vec![r];
    let o = build(&i, None);
    assert_eq!(find(&o, 0).task_id, None);
    assert_eq!(find(&o, 0).status, SegmentStatus::NeedsReview);
}

#[test]
fn merge_gap_absorbs_short_interruption() {
    let mut i = base();
    i.merge_gap = 30_000;
    let mut b = win("B", 0, 0, "H2", "other.exe", "x");
    b.start_at = 10 * M;
    b.end_at = Some(10 * M + 30_000);
    let mut a2 = win("A2", 0, 20, "H1", "chrome.exe", "docs");
    a2.start_at = 10 * M + 30_000;
    i.windows = vec![win("A1", 0, 10, "H1", "chrome.exe", "docs"), b, a2];
    i.task_rules = vec![rule("R", "T1", MatchField::Title, "docs", 0)];
    let o = build(&i, None);
    assert_eq!(shape(&o), vec![(0, 20, Work)]);
    assert_eq!(o.segments[0].window_span_ids, vec!["A1", "B", "A2"]);
    assert_eq!(o.segments[0].exe_name.as_deref(), Some("chrome.exe"));

    // 閾値を超える中断は吸収しない
    i.merge_gap = 29_999;
    let o = build(&i, None);
    assert_eq!(o.segments.len(), 3);
}

#[test]
fn merge_adjacent_same_content() {
    let mut i = base();
    i.windows = vec![
        win("A", 0, 10, "H1", "chrome.exe", "docs 1"),
        win("B", 10, 25, "H1", "chrome.exe", "docs 2"),
    ];
    i.task_rules = vec![rule("R", "T1", MatchField::Title, "docs", 0)];
    let o = build(&i, None);
    assert_eq!(shape(&o), vec![(0, 25, Work)]);
    assert_eq!(o.segments[0].title.as_deref(), Some("docs 2")); // 長い方
}

#[test]
fn doing_fallback_for_unruled_work() {
    let mut i = base();
    i.windows = vec![win("A", 0, 30, "H1", "chrome.exe", "x")];
    i.doing_logs = vec![doing("T9", 10, Some(20))];
    i.task_rules = vec![];
    let o = build(&i, None);
    assert_eq!(shape(&o), vec![(0, 10, Work), (10, 20, Work), (20, 30, Work)]);
    let s = find(&o, 10);
    assert_eq!(s.task_id.as_deref(), Some("T9"));
    assert_eq!(s.decided_by.as_deref(), Some("doing"));
    assert_eq!((s.confidence, s.status), (Some(0.5), SegmentStatus::NeedsReview));
    assert_eq!(find(&o, 0).task_id, None);
}

struct Mock;
impl SegmentJudge for Mock {
    fn judge(&self, segs: &[ActivitySegment]) -> Vec<Option<JudgeAnswer>> {
        segs.iter()
            .map(|s| {
                (s.title.as_deref() == Some("mystery")).then(|| JudgeAnswer {
                    task_id: Some("TJ".into()),
                    activity_label: Some("調査".into()),
                    is_interrupt: None,
                    confidence: 0.9,
                    judge_name: "mock".into(),
                })
            })
            .collect()
    }
}

struct LowMock;
impl SegmentJudge for LowMock {
    fn judge(&self, segs: &[ActivitySegment]) -> Vec<Option<JudgeAnswer>> {
        segs.iter()
            .map(|_| {
                Some(JudgeAnswer {
                    task_id: Some("TJ".into()),
                    activity_label: None,
                    is_interrupt: None,
                    confidence: 0.4,
                    judge_name: "low".into(),
                })
            })
            .collect()
    }
}

#[test]
fn judge_only_asked_for_unruled_work() {
    let mut i = base();
    i.windows = vec![
        win("A", 0, 10, "H1", "chrome.exe", "mystery"),
        win("B", 10, 20, "H1", "chrome.exe", "docs"),
        win("C", 20, 30, "H1", "chrome.exe", "other"),
    ];
    i.task_rules = vec![rule("R", "T1", MatchField::Title, "docs", 0)];
    let o = build(&i, Some(&Mock));
    let a = find(&o, 0);
    assert_eq!(a.task_id.as_deref(), Some("TJ"));
    assert_eq!(a.decided_by.as_deref(), Some("judge:mock"));
    assert_eq!(
        (a.status, a.activity_label.as_deref()),
        (SegmentStatus::Auto, Some("調査"))
    );
    assert_eq!(find(&o, 10).decided_by.as_deref(), Some("rule"));
    assert_eq!(find(&o, 20).task_id, None);

    // 確信度が閾値未満なら要確認
    let o = build(&i, Some(&LowMock));
    assert_eq!(find(&o, 0).status, SegmentStatus::NeedsReview);
    assert_eq!(find(&o, 10).status, SegmentStatus::Auto);
}

// ===== 7.4 割り込み =====

fn interrupt_input() -> PipelineInput {
    let mut i = base();
    i.interrupt_min = 3 * M;
    i.merge_gap = 0;
    i.doing_logs = vec![doing("T1", 0, None)];
    i.task_rules = vec![rule("R", "T1", MatchField::Exe, "devenv", 0)];
    i
}

#[test]
fn interrupt_detected_after_communication_app() {
    let mut i = interrupt_input();
    i.windows = vec![
        win("A", 0, 10, "H1", "devenv.exe", "proj"),
        win("B", 10, 12, "H2", "ms-teams.exe", "chat"),
        win("C", 12, 20, "H3", "chrome.exe", "memes"),
        win("D", 20, 30, "H1", "devenv.exe", "proj"),
    ];
    let o = build(&i, None);
    assert_eq!(
        shape(&o),
        vec![(0, 10, Work), (10, 20, Interrupt), (20, 30, Work)]
    );
    let s = find(&o, 10);
    assert_eq!(s.status, SegmentStatus::NeedsReview);
    assert_eq!(s.task_id, None);
    assert_eq!(find(&o, 0).task_id.as_deref(), Some("T1"));
}

#[test]
fn interrupt_keeps_rule_task_of_following_work() {
    let mut i = interrupt_input();
    i.windows = vec![
        win("A", 0, 10, "H1", "devenv.exe", "proj"),
        win("B", 10, 12, "H2", "Slack.exe", "chat"),
        win("C", 12, 20, "H3", "chrome.exe", "other-task"),
    ];
    i.task_rules
        .push(rule("R2", "T2", MatchField::Title, "other-task", 0));
    let o = build(&i, None);
    assert_eq!(
        shape(&o),
        vec![(0, 10, Work), (10, 12, Interrupt), (12, 20, Interrupt)]
    );
    assert_eq!(find(&o, 12).task_id.as_deref(), Some("T2"));
}

#[test]
fn short_detour_after_communication_is_not_interrupt() {
    let mut i = interrupt_input();
    i.windows = vec![
        win("A", 0, 10, "H1", "devenv.exe", "proj"),
        win("B", 10, 12, "H2", "ms-teams.exe", "chat"),
        win("C", 12, 13, "H3", "chrome.exe", "memes"), // 1 分 < 3 分
        win("D", 13, 30, "H1", "devenv.exe", "proj"),
    ];
    let o = build(&i, None);
    assert!(of_kind(&o, Interrupt).is_empty());
}

#[test]
fn no_interrupt_without_doing_or_focus_block() {
    let mut i = interrupt_input();
    i.doing_logs.clear();
    i.windows = vec![
        win("A", 0, 10, "H1", "devenv.exe", "proj"),
        win("B", 10, 12, "H2", "ms-teams.exe", "chat"),
        win("C", 12, 20, "H3", "chrome.exe", "memes"),
    ];
    assert!(of_kind(&build(&i, None), Interrupt).is_empty());

    i.time_blocks = vec![TimeBlock {
        id: "B".into(),
        start_at: 0,
        end_at: 30 * M,
        task_id: Some("T1".into()),
        label: None,
        focus: true,
    }];
    assert_eq!(of_kind(&build(&i, None), Interrupt), vec![(10, 20)]);
}
