//! 7.1-8 隣接する同一内容の区間の統合と、短い中断の吸収。

use super::piece::Piece;
use crate::model::{SegmentKind, SegmentStatus};
use crate::Millis;

fn same_content(a: &Piece, b: &Piece) -> bool {
    a.kind == b.kind
        && a.task_id == b.task_id
        && a.label == b.label
        && a.status == b.status
        && a.session == b.session
}

/// `b` を `a` の後ろに連結する。ids は和集合、exe/title は長い方の代表値。
fn join(a: &mut Piece, b: Piece) {
    a.end = b.end;
    for id in b.ids {
        if !a.ids.contains(&id) {
            a.ids.push(id);
        }
    }
    if b.rep_len > a.rep_len {
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
                && same_content(a, c);
            if absorb {
                let c = pieces.remove(i + 2);
                let b = pieces.remove(i + 1);
                join(&mut pieces[i], b);
                join(&mut pieces[i], c);
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
            Some(l) if l.end == p.start && same_content(l, &p) => join(l, p),
            _ => out.push(p),
        }
    }
    out
}
