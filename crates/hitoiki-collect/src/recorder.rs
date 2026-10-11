//! 収集イベントを `window_span` / `idle_span` に書き込む状態機械。

use crate::{ForegroundChange, IdleChange, SessionChange, StopToken};
use anyhow::Result;
use crossbeam_channel::{never, select, Receiver};
use hitoiki_core::config::Collect;
use hitoiki_core::id::new_id;
use hitoiki_core::model::{IdleReason, IdleSpan, WindowSpan};
use hitoiki_core::repo::collect as repo;
use hitoiki_core::{Clock, Millis};
use rusqlite::Connection;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Claude のスピナーはタイトル先頭の記号だけが毎秒変わるため、正規化せずに区間を切ると行が増えすぎる。
/// 正規化は帰属判定と同じ規則（`hitoiki_core::rules::normalize_title`）。
fn norm_title(t: &str) -> String {
    hitoiki_core::rules::normalize_title(t)
}

/// Hitoiki 自身のウインドウか（exe 名が hitoiki-app.exe、または自プロセスの pid）。
fn is_self(ev: &ForegroundChange) -> bool {
    ev.exe_name
        .as_deref()
        .is_some_and(|e| e.eq_ignore_ascii_case("hitoiki-app.exe"))
        || ev.pid == Some(i64::from(std::process::id()))
}

struct CurWin {
    hwnd: String,
    title: String,
    /// 除外で区間を作らない場合は None。
    span_id: Option<String>,
}

pub struct Recorder {
    conn: Connection,
    cfg: Collect,
    clock: Arc<dyn Clock>,
    cur_win: Option<CurWin>,
    idle_open: Option<(String, IdleReason)>,
    no_input: bool,
    locked: bool,
    sleeping: bool,
    last_idle_change: Millis,
    /// ハートビートの保存間隔。
    pub heartbeat_interval: Duration,
}

impl Recorder {
    /// 起動時に前回の残骸（end_at NULL）を最後のハートビートで閉じる。
    pub fn new(conn: Connection, cfg: Collect, clock: Arc<dyn Clock>) -> Result<Self> {
        let hb = repo::get_heartbeat(&conn)?;
        let n = repo::close_dangling(&conn, hb)?;
        if n > 0 {
            tracing::warn!(count = n, "前回の未終了区間を閉じました");
        }
        repo::set_heartbeat(&conn, clock.now_ms())?;
        Ok(Self {
            conn,
            cfg,
            last_idle_change: clock.now_ms(),
            clock,
            cur_win: None,
            idle_open: None,
            no_input: false,
            locked: false,
            sleeping: false,
            heartbeat_interval: Duration::from_secs(10),
        })
    }

    pub fn on_foreground(&mut self, ev: ForegroundChange) -> Result<()> {
        if let Some(c) = &self.cur_win {
            if c.hwnd == ev.hwnd && norm_title(&c.title) == norm_title(&ev.title) {
                return Ok(());
            }
        }
        if let Some(CurWin {
            span_id: Some(id), ..
        }) = self.cur_win.take()
        {
            repo::close_window_span(&self.conn, &id, ev.at)?;
        }
        let excluded = self.cfg.is_excluded(ev.exe_name.as_deref(), Some(&ev.title));
        let span_id = if (excluded && self.cfg.drop_excluded) || (!self.cfg.record_self && is_self(&ev)) {
            None
        } else {
            let id = new_id();
            repo::open_window_span(
                &self.conn,
                &WindowSpan {
                    id: id.clone(),
                    start_at: ev.at,
                    end_at: None,
                    hwnd: Some(ev.hwnd.clone()),
                    pid: ev.pid,
                    exe_path: if excluded { None } else { ev.exe_path.clone() },
                    exe_name: ev.exe_name.clone(),
                    title: if excluded { None } else { Some(ev.title.clone()) },
                    excluded,
                },
            )?;
            Some(id)
        };
        self.cur_win = Some(CurWin {
            hwnd: ev.hwnd,
            title: ev.title,
            span_id,
        });
        Ok(())
    }

    pub fn on_idle(&mut self, ev: IdleChange) -> Result<()> {
        match ev {
            IdleChange::Started { at } => {
                self.no_input = true;
                self.update_idle(at)
            }
            IdleChange::Ended { at } => {
                self.no_input = false;
                self.update_idle(at)
            }
        }
    }

    pub fn on_session(&mut self, ev: SessionChange) -> Result<()> {
        let at = match ev {
            SessionChange::Locked { at } => {
                self.locked = true;
                at
            }
            SessionChange::Unlocked { at } => {
                self.locked = false;
                at
            }
            SessionChange::Suspended { at } => {
                self.sleeping = true;
                at
            }
            SessionChange::Resumed { at } => {
                self.sleeping = false;
                at
            }
        };
        self.update_idle(at)
    }

    /// 優先度 sleep > locked > no_input で、現在あるべき無操作区間に揃える。
    fn update_idle(&mut self, at: Millis) -> Result<()> {
        let desired = if self.sleeping {
            Some(IdleReason::Sleep)
        } else if self.locked {
            Some(IdleReason::Locked)
        } else if self.no_input {
            Some(IdleReason::NoInput)
        } else {
            None
        };
        if self.idle_open.as_ref().map(|(_, r)| *r) == desired {
            return Ok(());
        }
        // no_input の開始は最終入力時刻へ遡るが、直前の区間とは重ねない。
        let t = at.max(self.last_idle_change);
        if let Some((id, _)) = self.idle_open.take() {
            repo::close_idle_span(&self.conn, &id, t)?;
        }
        if let Some(reason) = desired {
            let id = new_id();
            repo::open_idle_span(
                &self.conn,
                &IdleSpan {
                    id: id.clone(),
                    start_at: t,
                    end_at: None,
                    reason,
                },
            )?;
            self.idle_open = Some((id, reason));
        }
        self.last_idle_change = t;
        Ok(())
    }

    pub fn heartbeat(&self) -> Result<()> {
        repo::set_heartbeat(&self.conn, self.clock.now_ms())?;
        Ok(())
    }

    /// 進行中の区間をすべて現在時刻で閉じる（正常終了時）。
    pub fn shutdown(&mut self) -> Result<()> {
        let now = self.clock.now_ms();
        if let Some(CurWin {
            span_id: Some(id), ..
        }) = self.cur_win.take()
        {
            repo::close_window_span(&self.conn, &id, now)?;
        }
        if let Some((id, _)) = self.idle_open.take() {
            repo::close_idle_span(&self.conn, &id, now.max(self.last_idle_change))?;
        }
        self.heartbeat()
    }

    /// イベントを受けて書き込み続ける。`stop` が立つか全チャネルが閉じたら
    /// 進行中の区間を閉じて戻る。個々の書き込み失敗はログに残して継続する。
    pub fn run(
        &mut self,
        mut fg: Receiver<ForegroundChange>,
        mut idle: Receiver<IdleChange>,
        mut sess: Receiver<SessionChange>,
        stop: StopToken,
    ) -> Result<()> {
        let mut last_hb = Instant::now();
        let (mut fg_open, mut idle_open, mut sess_open) = (true, true, true);
        while !stop.is_stopped() && (fg_open || idle_open || sess_open) {
            select! {
                recv(fg) -> m => match m {
                    Ok(e) => log_err(self.on_foreground(e)),
                    Err(_) => { fg_open = false; fg = never(); }
                },
                recv(idle) -> m => match m {
                    Ok(e) => log_err(self.on_idle(e)),
                    Err(_) => { idle_open = false; idle = never(); }
                },
                recv(sess) -> m => match m {
                    Ok(e) => log_err(self.on_session(e)),
                    Err(_) => { sess_open = false; sess = never(); }
                },
                default(Duration::from_millis(200)) => {}
            }
            if last_hb.elapsed() >= self.heartbeat_interval {
                log_err(self.heartbeat());
                last_hb = Instant::now();
            }
        }
        self.shutdown()
    }
}

fn log_err(r: Result<()>) {
    if let Err(e) = r {
        tracing::error!(error = %e, "収集データの書き込みに失敗しました");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ForegroundCollector, IdleCollector, MockCollector, SessionCollector};
    use hitoiki_core::config::ExcludeRule;
    use hitoiki_core::db::open_in_memory;
    use hitoiki_core::FixedClock;

    fn fg(at: Millis, hwnd: &str, exe: &str, title: &str) -> ForegroundChange {
        ForegroundChange {
            at,
            hwnd: hwnd.into(),
            pid: Some(10),
            exe_path: Some(format!("C:\\x\\{exe}")),
            exe_name: Some(exe.into()),
            title: title.into(),
        }
    }

    fn rec(cfg: Collect) -> (Recorder, Arc<FixedClock>) {
        let clock = Arc::new(FixedClock::new(1_000));
        let r = Recorder::new(open_in_memory().unwrap(), cfg, clock.clone()).unwrap();
        (r, clock)
    }

    type WinRow = (i64, Option<i64>, Option<String>, bool);
    fn wins(r: &Recorder) -> Vec<WinRow> {
        let mut s = r
            .conn
            .prepare("SELECT start_at, end_at, title, excluded FROM window_span ORDER BY start_at")
            .unwrap();
        s.query_map([], |x| Ok((x.get(0)?, x.get(1)?, x.get(2)?, x.get(3)?)))
            .unwrap()
            .map(|x| x.unwrap())
            .collect()
    }

    fn idles(r: &Recorder) -> Vec<(i64, Option<i64>, String)> {
        let mut s = r
            .conn
            .prepare("SELECT start_at, end_at, reason FROM idle_span ORDER BY start_at, rowid")
            .unwrap();
        s.query_map([], |x| Ok((x.get(0)?, x.get(1)?, x.get(2)?)))
            .unwrap()
            .map(|x| x.unwrap())
            .collect()
    }

    #[test]
    fn foreground_switch_and_title_change() {
        let (mut r, _) = rec(Collect::default());
        r.on_foreground(fg(100, "1", "a.exe", "A")).unwrap();
        r.on_foreground(fg(150, "1", "a.exe", "A")).unwrap(); // 重複は無視
        r.on_foreground(fg(200, "1", "a.exe", "A2")).unwrap(); // タイトル変更
        r.on_foreground(fg(300, "2", "b.exe", "B")).unwrap();
        assert_eq!(
            wins(&r),
            vec![
                (100, Some(200), Some("A".into()), false),
                (200, Some(300), Some("A2".into()), false),
                (300, None, Some("B".into()), false),
            ]
        );
    }

    #[test]
    fn spinner_only_title_change_keeps_span() {
        let (mut r, _) = rec(Collect::default());
        r.on_foreground(fg(100, "1", "t.exe", "\u{2802} Claude Code"))
            .unwrap();
        r.on_foreground(fg(200, "1", "t.exe", "\u{2810} Claude Code"))
            .unwrap();
        r.on_foreground(fg(300, "1", "t.exe", "\u{2733} Claude Code"))
            .unwrap();
        r.on_foreground(fg(400, "1", "t.exe", "* Claude Code")).unwrap();
        r.on_foreground(fg(500, "1", "t.exe", "\u{2733} Other")).unwrap();
        assert_eq!(
            wins(&r),
            vec![
                (100, Some(500), Some("\u{2802} Claude Code".into()), false),
                (500, None, Some("\u{2733} Other".into()), false),
            ]
        );
    }

    #[test]
    fn excluded_masks_title() {
        let (mut r, _) = rec(Collect::default());
        r.on_foreground(fg(100, "1", "KeePassXC.exe", "secret")).unwrap();
        r.on_foreground(fg(200, "2", "chrome.exe", "x - InPrivate"))
            .unwrap();
        r.on_foreground(fg(300, "3", "a.exe", "ok")).unwrap();
        let w = wins(&r);
        assert_eq!(w[0], (100, Some(200), None, true));
        assert_eq!(w[1], (200, Some(300), None, true));
        assert_eq!(w[2].2.as_deref(), Some("ok"));
        let exe: String = r
            .conn
            .query_row("SELECT exe_name FROM window_span WHERE start_at=100", [], |x| {
                x.get(0)
            })
            .unwrap();
        assert_eq!(exe, "KeePassXC.exe");
    }

    #[test]
    fn drop_excluded_creates_no_span() {
        let cfg = Collect {
            drop_excluded: true,
            exclude: vec![ExcludeRule {
                exe: Some("k.exe".into()),
                title_regex: None,
            }],
            ..Collect::default()
        };
        let (mut r, _) = rec(cfg);
        r.on_foreground(fg(100, "1", "a.exe", "A")).unwrap();
        r.on_foreground(fg(200, "2", "k.exe", "s")).unwrap();
        r.on_foreground(fg(300, "1", "a.exe", "A")).unwrap();
        let w = wins(&r);
        assert_eq!(w.len(), 2);
        assert_eq!(w[0].1, Some(200));
        assert_eq!(w[1].0, 300);
    }

    #[test]
    fn self_window_closes_span_and_creates_none() {
        let (mut r, _) = rec(Collect::default());
        r.on_foreground(fg(100, "1", "a.exe", "A")).unwrap();
        r.on_foreground(fg(200, "2", "Hitoiki-App.exe", "Hitoiki"))
            .unwrap();
        // 自プロセスの pid でも対象（exe 名が別でも）
        let mut own = fg(250, "3", "other.exe", "own");
        own.pid = Some(i64::from(std::process::id()));
        r.on_foreground(own).unwrap();
        r.on_foreground(fg(300, "1", "a.exe", "A")).unwrap();
        let w = wins(&r);
        assert_eq!(w.len(), 2);
        assert_eq!(w[0], (100, Some(200), Some("A".into()), false));
        assert_eq!(w[1], (300, None, Some("A".into()), false));
    }

    #[test]
    fn record_self_true_records_self_window() {
        let cfg = Collect {
            record_self: true,
            ..Collect::default()
        };
        let (mut r, _) = rec(cfg);
        r.on_foreground(fg(100, "1", "a.exe", "A")).unwrap();
        r.on_foreground(fg(200, "2", "hitoiki-app.exe", "Hitoiki"))
            .unwrap();
        let w = wins(&r);
        assert_eq!(w.len(), 2);
        assert_eq!(w[1], (200, None, Some("Hitoiki".into()), false));
    }

    #[test]
    fn no_input_starts_at_last_input() {
        let (mut r, _) = rec(Collect::default());
        r.on_idle(IdleChange::Started { at: 1_500 }).unwrap();
        r.on_idle(IdleChange::Ended { at: 9_000 }).unwrap();
        assert_eq!(idles(&r), vec![(1_500, Some(9_000), "no_input".into())]);
    }

    #[test]
    fn lock_takes_priority_over_no_input() {
        let (mut r, _) = rec(Collect::default());
        r.on_idle(IdleChange::Started { at: 2_000 }).unwrap();
        r.on_session(SessionChange::Locked { at: 3_000 }).unwrap();
        // ロック中の Ended/Started では区間を増やさない
        r.on_idle(IdleChange::Ended { at: 3_500 }).unwrap();
        r.on_idle(IdleChange::Started { at: 3_600 }).unwrap();
        r.on_session(SessionChange::Unlocked { at: 5_000 }).unwrap();
        r.on_idle(IdleChange::Ended { at: 5_100 }).unwrap();
        assert_eq!(
            idles(&r),
            vec![
                (2_000, Some(3_000), "no_input".into()),
                (3_000, Some(5_000), "locked".into()),
                (5_000, Some(5_100), "no_input".into()),
            ]
        );
    }

    #[test]
    fn sleep_overrides_lock() {
        let (mut r, _) = rec(Collect::default());
        r.on_session(SessionChange::Locked { at: 2_000 }).unwrap();
        r.on_session(SessionChange::Suspended { at: 3_000 }).unwrap();
        r.on_session(SessionChange::Resumed { at: 4_000 }).unwrap();
        r.on_session(SessionChange::Unlocked { at: 5_000 }).unwrap();
        assert_eq!(
            idles(&r),
            vec![
                (2_000, Some(3_000), "locked".into()),
                (3_000, Some(4_000), "sleep".into()),
                (4_000, Some(5_000), "locked".into()),
            ]
        );
    }

    #[test]
    fn dangling_closed_at_heartbeat_on_startup() {
        let conn = open_in_memory().unwrap();
        // 前回プロセス相当: 区間を開いたままハートビートだけ残す
        repo::open_window_span(
            &conn,
            &WindowSpan {
                id: "old".into(),
                start_at: 1_000,
                end_at: None,
                hwnd: None,
                pid: None,
                exe_path: None,
                exe_name: None,
                title: None,
                excluded: false,
            },
        )
        .unwrap();
        repo::set_heartbeat(&conn, 4_000).unwrap();
        let clock = Arc::new(FixedClock::new(9_000));
        let r = Recorder::new(conn, Collect::default(), clock).unwrap();
        assert_eq!(wins(&r), vec![(1_000, Some(4_000), None, false)]);
        assert_eq!(repo::get_heartbeat(&r.conn).unwrap(), Some(9_000));
    }

    #[test]
    fn shutdown_closes_open_spans() {
        let (mut r, clock) = rec(Collect::default());
        r.on_foreground(fg(1_000, "1", "a.exe", "A")).unwrap();
        r.on_idle(IdleChange::Started { at: 1_100 }).unwrap();
        clock.set(2_000);
        r.shutdown().unwrap();
        assert_eq!(wins(&r)[0].1, Some(2_000));
        assert_eq!(idles(&r)[0].1, Some(2_000));
    }

    #[test]
    fn run_with_mock_collectors() {
        let (mut r, _) = rec(Collect::default());
        let (ftx, frx) = crossbeam_channel::unbounded();
        let (itx, irx) = crossbeam_channel::unbounded();
        let (stx, srx) = crossbeam_channel::unbounded();
        let stop = StopToken::new();
        MockCollector::new(vec![fg(100, "1", "a.exe", "A"), fg(200, "2", "b.exe", "B")])
            .run(ftx, stop.clone())
            .unwrap();
        MockCollector::new(vec![IdleChange::Started { at: 150 }])
            .run(itx, stop.clone())
            .unwrap();
        MockCollector::new(vec![SessionChange::Locked { at: 180 }])
            .run(stx, stop.clone())
            .unwrap();
        // 送信側は閉じているので、取り込み後に run が戻る
        r.run(frx, irx, srx, stop).unwrap();
        assert_eq!(wins(&r).len(), 2);
        assert!(wins(&r).iter().all(|w| w.1.is_some()));
        assert_eq!(idles(&r).len(), 2);
    }
}
