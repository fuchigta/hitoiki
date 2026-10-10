//! 7.1-2 / 7.3 エージェント対話時間の算出。
//!
//! 計上するのは「ユーザーがそのセッションのウインドウにいた時間」だけ。
//! 応答完了から次の送信までのラグ中に別ウインドウにいた時間は通常の作業として残る。

use super::piece::{relabel, Piece};
use super::timeline::WPiece;
use super::{doing_at, PipelineInput};
use crate::model::*;
use crate::rules::{MatchTarget, TaskRuleSet};
use crate::Millis;
use std::collections::BTreeMap;

/// タイトル比較用の正規化。先頭の記号（スピナー ✳ ⠂ ⠐ · * など）と空白を除く。
pub(crate) fn norm_title(t: Option<&str>) -> String {
    t.unwrap_or("")
        .trim_start_matches(|c: char| !c.is_alphanumeric())
        .trim()
        .to_string()
}

fn win_title(w: &WindowSpan) -> Option<&str> {
    if w.excluded {
        None
    } else {
        w.title.as_deref()
    }
}

/// セッションに対応付けたウインドウ。
struct Bound {
    hwnd: String,
    title: Option<String>,
    norm: String,
    /// 結び付けた時点のウインドウ層の添字。
    wl_idx: usize,
}

impl Bound {
    fn matches_window(&self, w: &WindowSpan) -> bool {
        w.hwnd.as_deref() == Some(self.hwnd.as_str()) && norm_title(win_title(w)) == self.norm
    }
}

fn is_terminal(input: &PipelineInput, w: &WindowSpan) -> bool {
    w.exe_name
        .as_deref()
        .is_some_and(|e| input.terminal_exes.iter().any(|t| t.eq_ignore_ascii_case(e)))
}

/// 時刻 `t` に前面だったウインドウ層の添字。区間の終端がちょうど `t` の場合も拾う。
fn wl_index_at(wl: &[WPiece], t: Millis) -> Option<usize> {
    wl.iter()
        .position(|p| p.start <= t && t < p.end)
        .or_else(|| wl.iter().rposition(|p| p.end == t))
}

fn bind(input: &PipelineInput, wl: &[WPiece], t: Millis) -> Option<Bound> {
    let mk = |idx: usize| {
        let w = &input.windows[wl[idx].win];
        w.hwnd.as_ref().map(|h| Bound {
            hwnd: h.clone(),
            title: win_title(w).map(String::from),
            norm: norm_title(win_title(w)),
            wl_idx: idx,
        })
    };
    if let Some(i) = wl_index_at(wl, t) {
        if is_terminal(input, &input.windows[wl[i].win]) {
            if let Some(b) = mk(i) {
                return Some(b);
            }
        }
    }
    // 直前 bind_lookback 内の最も新しいターミナル系区間。
    let from = t - input.bind_lookback;
    wl.iter()
        .enumerate()
        .rev()
        .filter(|(_, p)| p.end > from && p.start <= t)
        .find(|(_, p)| is_terminal(input, &input.windows[p.win]))
        .and_then(|(i, _)| mk(i))
}

/// セッションに紐づくタスク帰属の決定結果。
struct Attribution {
    task_id: Option<String>,
    decided_by: Option<String>,
    confidence: Option<f64>,
    status: SegmentStatus,
}

struct Job {
    key: String,
    agent: String,
    session_id: String,
    t: Millis,
    /// 対象期間の開始（直前の応答完了 / 送信 / セッション開始）。
    period_start: Option<Millis>,
    /// 次の応答完了または次の送信（見守りの打ち切り）。
    stop: Option<Millis>,
    attr: Attribution,
}

fn order(k: AgentEventKind) -> u8 {
    match k {
        AgentEventKind::SessionStart => 0,
        AgentEventKind::PromptSubmit => 1,
        AgentEventKind::ResponseComplete => 2,
        AgentEventKind::SessionEnd => 3,
    }
}

fn build_jobs(input: &PipelineInput, task_rules: &TaskRuleSet) -> Vec<Job> {
    let mut groups: BTreeMap<(&str, &str), Vec<&AgentEvent>> = BTreeMap::new();
    for e in &input.events {
        groups
            .entry((e.agent.as_str(), e.session_id.as_str()))
            .or_default()
            .push(e);
    }
    let mut jobs = Vec::new();
    for ((agent, sid), mut evs) in groups {
        evs.sort_by_key(|e| (e.at, order(e.kind)));
        let key = format!("{agent}:{sid}");
        let start_t = evs
            .iter()
            .find(|e| e.kind == AgentEventKind::SessionStart)
            .map_or(evs[0].at, |e| e.at);
        let doing = doing_at(&input.doing_logs, start_t).map(String::from);
        let mut boundary: Option<Millis> = None;
        for (i, e) in evs.iter().enumerate() {
            match e.kind {
                AgentEventKind::SessionStart | AgentEventKind::ResponseComplete => boundary = Some(e.at),
                AgentEventKind::SessionEnd => {}
                AgentEventKind::PromptSubmit => {
                    let latest = |f: &dyn Fn(&AgentEvent) -> Option<&str>| {
                        evs[..=i]
                            .iter()
                            .rev()
                            .find_map(|x| f(x))
                            .or_else(|| evs.iter().find_map(|x| f(x)))
                    };
                    let cwd = latest(&|x| x.cwd.as_deref());
                    let branch = latest(&|x| x.git_branch.as_deref());
                    let target = MatchTarget::session(cwd, branch);
                    let attr = if let Some(tid) = task_rules.find(&target) {
                        Attribution {
                            task_id: Some(tid.to_string()),
                            decided_by: Some("rule".into()),
                            confidence: Some(1.0),
                            status: SegmentStatus::Auto,
                        }
                    } else if let Some(tid) = &doing {
                        Attribution {
                            task_id: Some(tid.clone()),
                            decided_by: Some("doing".into()),
                            confidence: Some(0.5),
                            status: SegmentStatus::NeedsReview,
                        }
                    } else {
                        Attribution {
                            task_id: None,
                            decided_by: None,
                            confidence: None,
                            status: SegmentStatus::NeedsReview,
                        }
                    };
                    let stop = evs[i + 1..]
                        .iter()
                        .find(|x| {
                            matches!(
                                x.kind,
                                AgentEventKind::ResponseComplete | AgentEventKind::PromptSubmit
                            )
                        })
                        .map(|x| x.at);
                    jobs.push(Job {
                        key: key.clone(),
                        agent: agent.to_string(),
                        session_id: sid.to_string(),
                        t: e.at,
                        period_start: boundary,
                        stop,
                        attr,
                    });
                    boundary = Some(e.at);
                }
            }
        }
    }
    jobs.sort_by(|a, b| a.t.cmp(&b.t).then_with(|| a.key.cmp(&b.key)));
    jobs
}

fn piece_matches(input: &PipelineInput, p: &Piece, b: &Bound) -> bool {
    p.kind == SegmentKind::Work && !p.fixed && p.win.is_some_and(|i| b.matches_window(&input.windows[i]))
}

pub(crate) fn apply(
    input: &PipelineInput,
    task_rules: &TaskRuleSet,
    wl: &[WPiece],
    pieces: &mut Vec<Piece>,
) -> Vec<AgentSessionWindow> {
    let mut bindings = Vec::new();
    for job in build_jobs(input, task_rules) {
        let Some(b) = bind(input, wl, job.t) else {
            continue;
        };
        if job.t >= input.range.start && job.t < input.range.end {
            bindings.push(AgentSessionWindow {
                session_id: job.session_id.clone(),
                agent: job.agent.clone(),
                hwnd: b.hwnd.clone(),
                title_at_bind: b.title.clone(),
                bound_at: job.t,
            });
        }

        // agent_watch: 送信後、同じウインドウが前面のまま続いた時間。
        let mut limit = job.t + input.watch_max;
        if let Some(s) = job.stop {
            limit = limit.min(s);
        }
        let mut watch: Vec<(Millis, Millis)> = Vec::new();
        let mut cur = job.t;
        for p in pieces.iter() {
            if cur >= limit {
                break;
            }
            if p.end <= cur {
                continue;
            }
            if p.start > cur || !piece_matches(input, p, &b) {
                break;
            }
            watch.push((cur, p.end.min(limit)));
            cur = p.end;
        }
        for (s, e) in watch {
            mark(pieces, s, e, input, &b, &job, SegmentKind::AgentWatch);
        }

        // agent_dialog: 対象期間のうち、そのウインドウが前面で無操作でない部分。
        let ps = job.period_start.unwrap_or_else(|| backtrack_start(input, wl, &b));
        if ps < job.t {
            let ivs: Vec<(Millis, Millis)> = pieces
                .iter()
                .filter(|p| p.end > ps && p.start < job.t && piece_matches(input, p, &b))
                .map(|p| (p.start.max(ps), p.end.min(job.t)))
                .collect();
            for (s, e) in ivs {
                mark(pieces, s, e, input, &b, &job, SegmentKind::AgentDialog);
            }
        }
    }
    bindings
}

/// 期間の開始が無い場合: 送信直前で同じ (hwnd, title) が連続していた区間の開始まで遡る。
fn backtrack_start(input: &PipelineInput, wl: &[WPiece], b: &Bound) -> Millis {
    let mut start = wl[b.wl_idx].start;
    let mut j = b.wl_idx;
    while j > 0 {
        let prev = &wl[j - 1];
        if prev.end == start && b.matches_window(&input.windows[prev.win]) {
            start = prev.start;
            j -= 1;
        } else {
            break;
        }
    }
    start
}

fn mark(
    pieces: &mut Vec<Piece>,
    s: Millis,
    e: Millis,
    input: &PipelineInput,
    b: &Bound,
    job: &Job,
    kind: SegmentKind,
) {
    relabel(
        pieces,
        s,
        e,
        |p| piece_matches(input, p, b),
        |p| {
            p.kind = kind;
            p.session = Some(job.key.clone());
            p.task_id = job.attr.task_id.clone();
            p.decided_by = job.attr.decided_by.clone();
            p.confidence = job.attr.confidence;
            p.status = Some(job.attr.status);
            p.fixed = true;
        },
    );
}
