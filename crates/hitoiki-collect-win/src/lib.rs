//! Windows 収集器（前面ウインドウ・無操作・ロック/スリープ）。
//!
//! EDR 対策（docs/dev/architecture.md「禁止事項（セキュリティ・プライバシー）」）: 低レベルキーボード/マウスフック、DLL インジェクション、
//! 他プロセスのメモリ読み取りは使わない。管理者権限不要の公開 API のみ。
//! 非 Windows ではビルドは通るが何も収集しない。

use anyhow::Result;
use crossbeam_channel::Sender;
use hitoiki_collect::{ForegroundChange, IdleChange, SessionChange, StopToken};
use hitoiki_core::config::Collect;
use hitoiki_core::{Clock, SystemClock};
use std::sync::Arc;
use std::thread::JoinHandle;

#[cfg(windows)]
mod foreground;
#[cfg(windows)]
mod idle;
#[cfg(windows)]
mod session;

#[cfg(windows)]
pub use foreground::WinForegroundCollector;
#[cfg(windows)]
pub use idle::WinIdleCollector;
#[cfg(windows)]
pub use session::WinSessionCollector;

/// 起動した収集スレッドと停止手段。
pub struct CollectorHandles {
    pub stop: StopToken,
    pub handles: Vec<JoinHandle<Result<()>>>,
}

impl CollectorHandles {
    /// 停止を要求する（スレッドは数百ミリ秒以内に終了する）。
    pub fn stop(&self) {
        self.stop.stop();
    }

    /// 停止を要求し、全スレッドの終了を待つ。最初のエラーを返す。
    pub fn stop_and_join(self) -> Result<()> {
        self.stop.stop();
        let mut first = Ok(());
        for h in self.handles {
            let r = h
                .join()
                .unwrap_or_else(|_| Err(anyhow::anyhow!("収集スレッドが panic しました")));
            if first.is_ok() {
                first = r;
            }
        }
        first
    }
}

/// 3 つのコレクタをそれぞれ専用スレッドで起動する（システム時計を使う）。
pub fn spawn_all(
    cfg: &Collect,
    tx_fg: Sender<ForegroundChange>,
    tx_idle: Sender<IdleChange>,
    tx_sess: Sender<SessionChange>,
) -> Result<CollectorHandles> {
    spawn_all_with_clock(cfg, Arc::new(SystemClock), tx_fg, tx_idle, tx_sess)
}

/// `spawn_all` の時計差し替え版。
#[cfg(windows)]
pub fn spawn_all_with_clock(
    cfg: &Collect,
    clock: Arc<dyn Clock>,
    tx_fg: Sender<ForegroundChange>,
    tx_idle: Sender<IdleChange>,
    tx_sess: Sender<SessionChange>,
) -> Result<CollectorHandles> {
    use hitoiki_collect::{ForegroundCollector, IdleCollector, SessionCollector};

    let stop = StopToken::new();
    let mut handles = Vec::new();

    let fg = WinForegroundCollector::new(clock.clone(), cfg.poll_interval_sec);
    let s = stop.clone();
    handles.push(
        std::thread::Builder::new()
            .name("hitoiki-collect-fg".into())
            .spawn(move || fg.run(tx_fg, s))?,
    );

    let idle = WinIdleCollector::new(clock.clone(), cfg.idle_threshold_sec);
    let s = stop.clone();
    handles.push(
        std::thread::Builder::new()
            .name("hitoiki-collect-idle".into())
            .spawn(move || idle.run(tx_idle, s))?,
    );

    let sess = WinSessionCollector::new(clock);
    let s = stop.clone();
    handles.push(
        std::thread::Builder::new()
            .name("hitoiki-collect-session".into())
            .spawn(move || sess.run(tx_sess, s))?,
    );

    Ok(CollectorHandles { stop, handles })
}

/// 非 Windows: 何も起動しない。
#[cfg(not(windows))]
pub fn spawn_all_with_clock(
    _cfg: &Collect,
    _clock: Arc<dyn Clock>,
    _tx_fg: Sender<ForegroundChange>,
    _tx_idle: Sender<IdleChange>,
    _tx_sess: Sender<SessionChange>,
) -> Result<CollectorHandles> {
    tracing::warn!("Windows 以外では収集器は動作しません");
    Ok(CollectorHandles {
        stop: StopToken::new(),
        handles: Vec::new(),
    })
}
