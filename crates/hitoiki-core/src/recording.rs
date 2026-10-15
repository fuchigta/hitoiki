//! 「記録が無かった時間帯」（収集器が動いていなかった区間）の検出。
//!
//! 無操作・ロック・スリープは `idle_span`、Hitoiki 自身の前面（`record_self = false`）や除外の破棄は
//! 収集器が動いたうえでの空白なので、ここでは扱わない。収集器（Recorder）は起動時に稼働を記録し、
//! 10 秒ごとに稼働の終了時刻（ハートビート）を進める。稼働と稼働のあいだ、および最後の稼働の
//! 終了から現在までが「記録なし」になる。スリープ中はプロセスごと止まるが、復帰後の最初の
//! ハートビートで稼働が続いていたものとして扱い、その間は `idle_span`（sleep）が説明する。

use crate::repo::collect::{list_runs, CollectorRun};
use crate::timefmt::Range;
use crate::{Millis, Result};
use rusqlite::Connection;
use serde::Serialize;

/// これより短い空白は記録停止とみなさない（再起動の合間など）。
pub const MIN_UNCOLLECTED_MS: Millis = 60_000;
/// 最後の稼働が正常停止しておらず、終了時刻がこれ以内なら「いま動いている」とみなす。
pub const LIVE_GRACE_MS: Millis = 30_000;

/// 記録が無かった時間帯。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Uncollected {
    pub start_at: Millis,
    pub end_at: Millis,
}

impl Uncollected {
    pub fn duration_ms(&self) -> Millis {
        self.end_at - self.start_at
    }
}

/// 稼働記録 `runs` から、`range`（`now` までに切る）の記録なし区間を求める。純粋関数。
/// 最初の稼働より前（稼働記録を持たない古いバージョンの期間）は分からないので記録なしにしない。
pub fn uncollected_from_runs(runs: &[CollectorRun], range: Range, now: Millis) -> Vec<Uncollected> {
    let mut runs: Vec<CollectorRun> = runs.to_vec();
    runs.sort_by_key(|r| (r.start_at, r.end_at));
    let Some(first) = runs.first() else {
        return Vec::new();
    };
    let first_start = first.start_at;
    if let Some(last) = runs.last_mut() {
        if !last.clean && now - last.end_at <= LIVE_GRACE_MS {
            last.end_at = last.end_at.max(now);
        }
    }
    let lo = range.start.max(first_start);
    let hi = range.end.min(now);
    let mut out = Vec::new();
    let mut cur = lo;
    for r in &runs {
        if cur >= hi {
            break;
        }
        if r.start_at > cur {
            push_uncollected(&mut out, cur, r.start_at.min(hi));
        }
        cur = cur.max(r.end_at);
    }
    if cur < hi {
        push_uncollected(&mut out, cur, hi);
    }
    out
}

fn push_uncollected(out: &mut Vec<Uncollected>, start_at: Millis, end_at: Millis) {
    if end_at - start_at >= MIN_UNCOLLECTED_MS {
        out.push(Uncollected { start_at, end_at });
    }
}

/// 収集器がいま動いているか（最後の稼働が正常停止しておらず、ハートビートが `LIVE_GRACE_MS` 以内）。
/// 常駐アプリ自身でも別プロセスの daemon でも、同じ DB を見て判定できる。
pub fn is_recording(conn: &Connection, now: Millis) -> Result<bool> {
    Ok(list_runs(conn)?
        .last()
        .is_some_and(|r| !r.clean && now - r.end_at <= LIVE_GRACE_MS))
}

/// DB の稼働記録から記録なし区間を求める。
pub fn uncollected(conn: &Connection, range: Range, now: Millis) -> Result<Vec<Uncollected>> {
    Ok(uncollected_from_runs(&list_runs(conn)?, range, now))
}

#[cfg(test)]
mod tests {
    use super::*;

    const M: Millis = 60_000;

    fn run(s: Millis, e: Millis, clean: bool) -> CollectorRun {
        CollectorRun {
            start_at: s * M,
            end_at: e * M,
            clean,
        }
    }

    fn g(s: Millis, e: Millis) -> Uncollected {
        Uncollected {
            start_at: s * M,
            end_at: e * M,
        }
    }

    #[test]
    fn table() {
        let day = Range::new(0, 1000 * M);
        let short = CollectorRun {
            start_at: 200 * M + 30_000,
            end_at: 300 * M,
            clean: true,
        };
        let fresh = CollectorRun {
            start_at: 100 * M,
            end_at: 500 * M - 10_000,
            clean: false,
        };
        let cases: Vec<(&str, Vec<CollectorRun>, Millis, Vec<Uncollected>)> = vec![
            ("稼働記録なしは不明", vec![], 500, vec![]),
            (
                "稼働の外側は空白（最初の稼働より前は不明）",
                vec![run(100, 200, true), run(300, 400, false)],
                400,
                vec![g(200, 300)],
            ),
            (
                "最後の稼働が古ければ現在までが空白",
                vec![run(100, 200, false)],
                500,
                vec![g(200, 500)],
            ),
            ("最後の稼働が直近なら動作中", vec![fresh], 500, vec![]),
            (
                "正常停止後は現在までが空白",
                vec![run(100, 200, true)],
                500,
                vec![g(200, 500)],
            ),
            (
                "1 分未満の合間は無視",
                vec![run(100, 200, true), short],
                300,
                vec![],
            ),
            (
                "重なる稼働は連続",
                vec![run(100, 250, true), run(200, 300, false)],
                300,
                vec![],
            ),
        ];
        for (name, runs, now, want) in cases {
            assert_eq!(uncollected_from_runs(&runs, day, now * M), want, "{name}");
        }
    }

    #[test]
    fn is_recording_follows_last_run() {
        use crate::repo::collect as repo;
        let c = crate::db::open_in_memory().unwrap();
        assert!(!is_recording(&c, 1_000).unwrap());
        repo::begin_run(&c, 1_000).unwrap();
        assert!(is_recording(&c, 1_000 + LIVE_GRACE_MS).unwrap());
        assert!(!is_recording(&c, 1_000 + LIVE_GRACE_MS + 1).unwrap());
        repo::touch_run(&c, 5_000, true).unwrap();
        assert!(!is_recording(&c, 5_000).unwrap());
    }

    #[test]
    fn clipped_to_range() {
        let runs = vec![run(100, 200, true), run(400, 500, false)];
        let got = uncollected_from_runs(&runs, Range::new(250 * M, 350 * M), 500 * M);
        assert_eq!(got, vec![g(250, 350)]);
    }
}
