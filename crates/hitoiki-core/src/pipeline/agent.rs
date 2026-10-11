//! docs/architecture.md「帰属判定パイプライン」のエージェント対話時間の算出。
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

/// セッション開始がこれ以内に最初の送信と重なる場合は、開始が無いものとして扱う。
/// Claude Code のログは最初の行が最初のプロンプトとほぼ同時刻のことが多く、
/// その場合「セッション開始」は最初のプロンプト作成の開始を表さないため。
pub(crate) const SESSION_START_GRACE: Millis = 5_000;

/// タイトル比較用の正規化。先頭の記号（スピナー ✳ ⠂ ⠐ · * など）と空白を除く。
pub(crate) fn norm_title(t: Option<&str>) -> String {
    crate::rules::normalize_title(t.unwrap_or(""))
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
    /// 対象期間の開始（直前の応答完了 / 送信 / セッション開始）。None はバックトラックで決める。
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

fn attribution(
    task_rules: &TaskRuleSet,
    cwd: Option<&str>,
    branch: Option<&str>,
    doing: Option<&String>,
) -> Attribution {
    let target = MatchTarget::session(cwd, branch);
    if let Some(tid) = task_rules.find(&target) {
        Attribution {
            task_id: Some(tid.to_string()),
            decided_by: Some("rule".into()),
            confidence: Some(1.0),
            status: SegmentStatus::Auto,
        }
    } else if let Some(tid) = doing {
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
        evs.sort_by_key(|e| (e.at, order(e.kind), e.id.as_str()));
        let key = format!("{agent}:{sid}");
        let start_t = evs
            .iter()
            .find(|e| e.kind == AgentEventKind::SessionStart)
            .map_or(evs[0].at, |e| e.at);
        let doing = doing_at(&input.doing_logs, start_t).map(String::from);
        // 対象期間の境界。送信の後の最初の応答完了だけを境界にする（同じ送信への 2 件目や、
        // 送信なしで再開した応答の完了は使わない）。パーサ側の重複除去に依存しないため。
        let mut boundary: Option<Millis> = None;
        // 直前の境界の後に送信があり、まだ応答完了を見ていない。
        let mut pending_submit = false;
        // このセッションの送信・セッション開始・応答完了を見たか。見ていなければ手前の履歴が
        // 読み込み範囲の外にあるだけなので、最初の応答完了は境界として扱う。
        let mut known = false;
        for (i, e) in evs.iter().enumerate() {
            match e.kind {
                AgentEventKind::SessionStart => {
                    known = true;
                    pending_submit = false;
                    let next = evs[i + 1..].iter().find(|x| {
                        matches!(
                            x.kind,
                            AgentEventKind::PromptSubmit | AgentEventKind::ResponseComplete
                        )
                    });
                    let near_submit = next.is_some_and(|x| {
                        x.kind == AgentEventKind::PromptSubmit && x.at - e.at <= SESSION_START_GRACE
                    });
                    // 最初の送信とほぼ同時の開始は、開始が無いものとしてバックトラックに任せる。
                    boundary = if near_submit { None } else { Some(e.at) };
                }
                AgentEventKind::ResponseComplete => {
                    if pending_submit || !known {
                        boundary = Some(e.at);
                    }
                    pending_submit = false;
                    known = true;
                }
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
                    let attr = attribution(task_rules, cwd, branch, doing.as_ref());
                    // 送信の後の最初の応答完了か次の送信（2 件目以降の応答完了は見ない）。
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
                    pending_submit = true;
                    known = true;
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

/// ウインドウに対応付けできた送信。
struct Plan {
    job: Job,
    bound: Bound,
    /// 対象期間の開始（バックトラック済み）。
    ps: Millis,
}

fn plan(input: &PipelineInput, task_rules: &TaskRuleSet, wl: &[WPiece]) -> Vec<Plan> {
    build_jobs(input, task_rules)
        .into_iter()
        .filter_map(|job| {
            let bound = bind(input, wl, job.t)?;
            let ps = job
                .period_start
                .unwrap_or_else(|| backtrack_start(input, wl, &bound));
            Some(Plan { job, bound, ps })
        })
        .collect()
}

/// 出力範囲内の送信について、対象期間の開始のうち最も早いもの（再計算範囲の拡張用）。
pub(crate) fn earliest_period_start(
    input: &PipelineInput,
    task_rules: &TaskRuleSet,
    wl: &[WPiece],
) -> Option<Millis> {
    plan(input, task_rules, wl)
        .iter()
        .filter(|p| p.job.t >= input.range.start && p.job.t < input.range.end)
        .map(|p| p.ps.min(p.job.t))
        .min()
}

/// 同じ (hwnd, 正規化タイトル) に結び付いた別セッションと対象期間が重なる場合に、
/// [s, e) のうち `plans[j]` のセッションに帰属する部分を返す（ベストエフォート）。
/// 区別できない時間は「その時点の最も近い過去の送信を持つセッション」に帰属させる。
/// どちらにも過去の送信が無ければ、送信が早い方。
fn owned_parts(plans: &[Plan], j: usize, s: Millis, e: Millis) -> Vec<(Millis, Millis)> {
    let me = &plans[j];
    let cands: Vec<usize> = (0..plans.len())
        .filter(|&k| {
            let p = &plans[k];
            k == j
                || (p.job.key != me.job.key
                    && p.bound.hwnd == me.bound.hwnd
                    && p.bound.norm == me.bound.norm
                    && p.ps < e
                    && p.job.t > s)
        })
        .collect();
    if cands.len() <= 1 {
        return vec![(s, e)];
    }
    // 候補ごとの、そのセッションの送信時刻。
    let subs: Vec<(usize, Vec<Millis>)> = cands
        .iter()
        .map(|&k| {
            let key = &plans[k].job.key;
            let ts = plans
                .iter()
                .filter(|p| &p.job.key == key)
                .map(|p| p.job.t)
                .collect();
            (k, ts)
        })
        .collect();
    let mut points = vec![s, e];
    for (k, ts) in &subs {
        points.extend([plans[*k].ps, plans[*k].job.t]);
        points.extend(ts.iter().copied());
    }
    points.retain(|&x| x >= s && x <= e);
    points.sort_unstable();
    points.dedup();
    let mut out: Vec<(Millis, Millis)> = Vec::new();
    for w in points.windows(2) {
        let (a, b) = (w[0], w[1]);
        let owner = subs
            .iter()
            .filter(|(k, _)| plans[*k].ps <= a && a < plans[*k].job.t)
            .max_by_key(|(k, ts)| {
                let last = ts.iter().copied().filter(|&t| t <= a).max();
                (last, std::cmp::Reverse(plans[*k].job.t), std::cmp::Reverse(*k))
            })
            .map(|(k, _)| *k);
        if owner == Some(j) {
            match out.last_mut() {
                Some(l) if l.1 == a => l.1 = b,
                _ => out.push((a, b)),
            }
        }
    }
    out
}

pub(crate) fn apply(
    input: &PipelineInput,
    task_rules: &TaskRuleSet,
    wl: &[WPiece],
    pieces: &mut Vec<Piece>,
) -> Vec<AgentSessionWindow> {
    let plans = plan(input, task_rules, wl);
    let mut bindings = Vec::new();
    for (j, Plan { job, bound: b, ps }) in plans.iter().enumerate() {
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
            if p.start > cur || !piece_matches(input, p, b) {
                break;
            }
            watch.push((cur, p.end.min(limit)));
            cur = p.end;
        }
        for (s, e) in watch {
            mark(pieces, s, e, input, b, job, SegmentKind::AgentWatch);
        }

        // agent_dialog: 対象期間のうち、そのウインドウが前面で無操作でない部分。
        let ps = *ps;
        if ps < job.t {
            let ivs: Vec<(Millis, Millis)> = pieces
                .iter()
                .filter(|p| p.end > ps && p.start < job.t && piece_matches(input, p, b))
                .map(|p| (p.start.max(ps), p.end.min(job.t)))
                .collect();
            for (s, e) in ivs {
                for (s, e) in owned_parts(&plans, j, s, e) {
                    mark(pieces, s, e, input, b, job, SegmentKind::AgentDialog);
                }
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
