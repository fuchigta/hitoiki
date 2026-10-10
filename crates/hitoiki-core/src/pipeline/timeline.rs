//! 7.1-1 タイムライン構築: window_span と idle_span から区間列を作る。

use super::piece::Piece;
use crate::model::{IdleSpan, WindowSpan};
use crate::Millis;

/// ウインドウ層の 1 区間（無操作を考慮する前。重なりは解消済み）。
#[derive(Debug, Clone, Copy)]
pub(crate) struct WPiece {
    pub start: Millis,
    pub end: Millis,
    pub win: usize,
}

/// 重なるウインドウは後から始まった方を優先して解消し、時刻順の区間列にする。
/// 進行中（end_at = None）は now で閉じる。ギャップは区間を作らない。
pub(crate) fn window_layer(windows: &[WindowSpan], now: Millis) -> Vec<WPiece> {
    let spans: Vec<(Millis, Millis, usize)> = windows
        .iter()
        .enumerate()
        .map(|(i, w)| (w.start_at, w.end_at.unwrap_or(now), i))
        .filter(|(s, e, _)| s < e)
        .collect();
    let mut points: Vec<Millis> = spans.iter().flat_map(|(s, e, _)| [*s, *e]).collect();
    points.sort_unstable();
    points.dedup();

    let mut order: Vec<usize> = (0..spans.len()).collect();
    order.sort_by_key(|&i| (spans[i].0, spans[i].2));
    let mut next = 0;
    let mut active: Vec<usize> = Vec::new(); // spans の添字

    let mut out: Vec<WPiece> = Vec::new();
    for pair in points.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        while next < order.len() && spans[order[next]].0 <= a {
            active.push(order[next]);
            next += 1;
        }
        active.retain(|&i| spans[i].1 > a);
        // 後から始まった方（開始が遅い、同じなら添字が後）を優先。
        let Some(&top) = active.iter().max_by_key(|&&i| (spans[i].0, spans[i].2)) else {
            continue;
        };
        let win = spans[top].2;
        match out.last_mut() {
            Some(l) if l.win == win && l.end == a => l.end = b,
            _ => out.push(WPiece {
                start: a,
                end: b,
                win,
            }),
        }
    }
    out
}

/// idle_span を時刻順にマージした区間列。
pub(crate) fn idle_intervals(idles: &[IdleSpan], now: Millis) -> Vec<(Millis, Millis)> {
    let mut v: Vec<(Millis, Millis)> = idles
        .iter()
        .map(|i| (i.start_at, i.end_at.unwrap_or(now)))
        .filter(|(s, e)| s < e)
        .collect();
    v.sort_unstable();
    let mut out: Vec<(Millis, Millis)> = Vec::new();
    for (s, e) in v {
        match out.last_mut() {
            Some(l) if s <= l.1 => l.1 = l.1.max(e),
            _ => out.push((s, e)),
        }
    }
    out
}

/// ウインドウ層から無操作部分を切り取り、無操作区間を加えた区間列（時刻順）。
pub(crate) fn build_pieces(windows: &[WindowSpan], wl: &[WPiece], idle: &[(Millis, Millis)]) -> Vec<Piece> {
    let mut out: Vec<Piece> = Vec::new();
    for wp in wl {
        let mut cur = wp.start;
        for &(is, ie) in idle {
            if ie <= cur {
                continue;
            }
            if is >= wp.end {
                break;
            }
            if is > cur {
                out.push(Piece::from_window(cur, is, wp.win, &windows[wp.win]));
            }
            cur = cur.max(ie);
            if cur >= wp.end {
                break;
            }
        }
        if cur < wp.end {
            out.push(Piece::from_window(cur, wp.end, wp.win, &windows[wp.win]));
        }
    }
    for &(s, e) in idle {
        out.push(Piece::idle(s, e));
    }
    out.sort_by_key(|p| (p.start, p.end));
    out
}
