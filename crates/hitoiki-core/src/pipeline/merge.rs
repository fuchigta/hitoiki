//! docs/dev/architecture.md「帰属判定パイプライン」の統合: 隣接する同一内容の区間の統合と、短い中断の吸収。

use super::piece::Piece;
use crate::model::{SegmentKind, SegmentStatus};
use crate::Millis;

fn same_content(a: &Piece, b: &Piece) -> bool {
    a.kind == b.kind
        && a.task_id == b.task_id
        && a.label == b.label
        && a.status == b.status
        && a.session == b.session
        && a.offline == b.offline
}

/// 前後の区間として、間の短い中断を吸収してよいか。
/// エージェント対話・見守りは「そのウインドウにいた時間だけ」を計上するため（docs/dev/architecture.md「帰属判定パイプライン」、ラグを計上しない）、
/// ユーザー確定（override）はユーザーが決めた範囲を広げないため、無視の区間は作業時間として数えない時間を挟んで作業を水増ししないため、吸収しない。
fn can_absorb_around(p: &Piece) -> bool {
    !matches!(
        p.kind,
        SegmentKind::AgentDialog | SegmentKind::AgentWatch | SegmentKind::Ignored
    ) && p.status != Some(SegmentStatus::Confirmed)
}

/// `b` を `a` の後ろに連結する。ids は和集合。exe/title は同種の区間のうち長い方の代表値で、
/// 吸収した別内容の区間（`absorbed`）からは選ばない。
fn join(a: &mut Piece, b: Piece, absorbed: bool) {
    a.end = b.end;
    for id in b.ids {
        if !a.ids.contains(&id) {
            a.ids.push(id);
        }
    }
    if !absorbed && b.rep_len > a.rep_len {
        a.rep_len = b.rep_len;
        a.exe = b.exe;
        a.title = b.title;
    }
}

pub(crate) fn merge(mut pieces: Vec<Piece>, merge_gap: Millis) -> Vec<Piece> {
    pieces.retain(|p| p.start < p.end);
    pieces.sort_by_key(|p| (p.start, p.end));

    // 前後が同一内容で、間の短い作業区間を吸収できるなら先に統合する。
    loop {
        let mut changed = false;
        let mut i = 0;
        while i + 2 < pieces.len() {
            let (a, b, c) = (&pieces[i], &pieces[i + 1], &pieces[i + 2]);
            let absorb = a.end == b.start
                && b.end == c.start
                && b.kind == SegmentKind::Work
                && b.status != Some(SegmentStatus::Confirmed)
                && b.len() <= merge_gap
                && can_absorb_around(a)
                && same_content(a, c);
            if absorb {
                let c = pieces.remove(i + 2);
                let b = pieces.remove(i + 1);
                join(&mut pieces[i], b, true);
                join(&mut pieces[i], c, false);
                changed = true;
            } else {
                i += 1;
            }
        }
        if !changed {
            break;
        }
    }

    let mut out: Vec<Piece> = Vec::new();
    for p in pieces {
        match out.last_mut() {
            Some(l) if l.end == p.start && same_content(l, &p) => join(l, p, false),
            _ => out.push(p),
        }
    }
    out
}
