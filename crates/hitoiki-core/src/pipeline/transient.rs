//! docs/dev/architecture.md「帰属判定パイプライン」の通過系アプリの吸収。
//!
//! タスクバー・スタートメニュー・Alt+Tab などを担う exe（`collect.transient_exes`、既定は explorer.exe など）が
//! 作業の合間に一瞬だけ前面になった区間を、独立した要確認区間にせず直前（無ければ直後）の作業に吸収する。
//! 無視するアプリ（`collect.ignore_exes`）も通過系として扱い、短いものは同じく吸収する。吸収されなかったもの
//! （`transient_max_sec` 以上続いた、または吸収先が無い）は `ignored` の区間にする。
//! 派生層だけの処理で、`window_span`（生データ）は変えない。

use super::piece::Piece;
use super::PipelineInput;
use crate::model::{SegmentKind, SegmentStatus};
use std::collections::HashMap;

fn exe_in(list: &[String], p: &Piece) -> bool {
    p.win.is_some()
        && p.exe
            .as_deref()
            .is_some_and(|x| list.iter().any(|t| t.eq_ignore_ascii_case(x)))
}

/// 通過系の exe か。`ignore_exes` に入っているものも通過系として扱う。
fn is_transient_exe(input: &PipelineInput, p: &Piece) -> bool {
    exe_in(&input.transient_exes, p) || exe_in(&input.ignore_exes, p)
}

/// 通過系アプリの exe が隙間なく連続する区間の合計が `transient_max` 未満なら、その区間に吸収の印を付ける。
/// `ignore_exes` の区間には、長さによらず無視の印を付ける。
/// 出力範囲に切り取る前に呼ぶ（連続時間を本来の長さで測る）。
pub(crate) fn mark(input: &PipelineInput, pieces: &mut [Piece]) {
    if input.transient_exes.is_empty() && input.ignore_exes.is_empty() {
        return;
    }
    let n = pieces.len();
    let mut i = 0;
    while i < n {
        if !is_transient_exe(input, &pieces[i]) {
            i += 1;
            continue;
        }
        let mut j = i + 1;
        while j < n && pieces[j - 1].end == pieces[j].start && is_transient_exe(input, &pieces[j]) {
            j += 1;
        }
        let short = pieces[j - 1].end - pieces[i].start < input.transient_max;
        for p in &mut pieces[i..j] {
            p.ignore = exe_in(&input.ignore_exes, p);
            p.transient = short;
        }
        i = j;
    }
}

/// 吸収の候補: 印があり、まだ何にも帰属していない（ルールで決まっていない）作業区間。
fn is_candidate(p: &Piece) -> bool {
    p.transient && p.kind == SegmentKind::Work && !p.fixed && p.task_id.is_none() && p.label.is_none()
}

/// 吸収先にしてよい区間: 確定していない通常の作業区間。
/// エージェント対話・見守り（そのウインドウにいた時間だけを計上する）、会議、無操作、override は伸ばさない。
fn is_target(p: &Piece) -> bool {
    p.kind == SegmentKind::Work && !p.fixed && !p.transient && !p.ignore && p.win.is_some()
}

/// 候補の連続（グループ）ごとに吸収先を決める。戻り値は 区間の添字 → 吸収先の添字。
/// 直前（隙間なく接する）の作業区間を優先し、無ければ直後。どちらも不適切なら含めない（通常の区間のまま）。
pub(crate) fn plan(pieces: &[Piece]) -> HashMap<usize, usize> {
    let n = pieces.len();
    let mut out = HashMap::new();
    let mut i = 0;
    while i < n {
        if !is_candidate(&pieces[i]) {
            i += 1;
            continue;
        }
        let mut j = i + 1;
        while j < n && pieces[j - 1].end == pieces[j].start && is_candidate(&pieces[j]) {
            j += 1;
        }
        let prev = i
            .checked_sub(1)
            .filter(|&k| pieces[k].end == pieces[i].start && is_target(&pieces[k]));
        let next = (j < n)
            .then_some(j)
            .filter(|&k| pieces[j - 1].end == pieces[k].start && is_target(&pieces[k]));
        if let Some(t) = prev.or(next) {
            for k in i..j {
                out.insert(k, t);
            }
        }
        i = j;
    }
    out
}

/// 吸収する区間に、吸収先の帰属（種別・タスク・活動・確信度・根拠・表示用の exe/title）を写す。
/// 以降の割り込み検出・統合では吸収先と同じ内容の区間として扱われ、`merge` が 1 区間にまとめる。
pub(crate) fn apply(pieces: &mut [Piece], plan: &HashMap<usize, usize>) {
    for (&k, &t) in plan {
        let src = pieces[t].clone();
        let p = &mut pieces[k];
        p.kind = src.kind;
        p.task_id = src.task_id;
        p.label = src.label;
        p.confidence = src.confidence;
        p.decided_by = src.decided_by;
        p.status = src.status;
        p.session = src.session;
        p.exe = src.exe;
        p.title = src.title;
        p.fixed = src.fixed;
        p.transient = false;
    }
}

/// 無視するアプリ（`ignore_exes`）の作業区間のうち、吸収されなかったもの（`plan` に無いもの）を `ignored` にする。
/// 吸収の候補と同じく、ルールでタスクや活動が決まった区間と override 済みの区間は対象外。
/// 無視の区間はタスクにも活動にも属さず、確信度 1.0・`auto`（`decided_by = ignore`）で、後続の段階（判定器・割り込み・
/// doing フォールバック・統合）では動かない。
pub(crate) fn apply_ignore(pieces: &mut [Piece], plan: &HashMap<usize, usize>) {
    for (i, p) in pieces.iter_mut().enumerate() {
        if !p.ignore
            || p.kind != SegmentKind::Work
            || p.fixed
            || p.task_id.is_some()
            || p.label.is_some()
            || plan.contains_key(&i)
        {
            continue;
        }
        p.kind = SegmentKind::Ignored;
        p.decided_by = Some("ignore".into());
        p.confidence = Some(1.0);
        p.status = Some(SegmentStatus::Auto);
        p.fixed = true;
    }
}
