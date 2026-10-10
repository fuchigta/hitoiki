//! 収集器の OS 非依存部分: トレイト（6.6）、イベント型、DB へ書き込む状態機械 `Recorder`。

mod mock;
mod recorder;

pub use mock::MockCollector;
pub use recorder::Recorder;

use crossbeam_channel::Sender;
use hitoiki_core::Millis;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// 前面ウインドウの変化（フォーカス切替・タイトル変更）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForegroundChange {
    pub at: Millis,
    /// OS 依存のウインドウ識別子（文字列化）。
    pub hwnd: String,
    pub pid: Option<i64>,
    pub exe_path: Option<String>,
    pub exe_name: Option<String>,
    pub title: String,
}

/// 無操作の開始・終了。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdleChange {
    /// `at` は最終入力時刻（検知時刻ではない）。
    Started { at: Millis },
    /// `at` は入力再開を検知した時刻。
    Ended { at: Millis },
}

/// 画面ロック・スリープの変化。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionChange {
    Locked { at: Millis },
    Unlocked { at: Millis },
    Suspended { at: Millis },
    Resumed { at: Millis },
}

/// 収集スレッドの停止要求。複製して共有する。
#[derive(Debug, Clone, Default)]
pub struct StopToken(Arc<AtomicBool>);

impl StopToken {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn stop(&self) {
        self.0.store(true, Ordering::SeqCst);
    }
    pub fn is_stopped(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
    /// 最大 `dur` 待つ。停止要求があれば早く戻り `true` を返す。
    pub fn sleep(&self, dur: Duration) -> bool {
        let end = Instant::now() + dur;
        while !self.is_stopped() {
            let left = end.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return false;
            }
            std::thread::sleep(left.min(Duration::from_millis(50)));
        }
        true
    }
}

/// いずれもブロッキングで動き、`stop` が立つか受信側が閉じたら `Ok` で戻る。
pub trait ForegroundCollector {
    fn run(&self, tx: Sender<ForegroundChange>, stop: StopToken) -> anyhow::Result<()>;
}

pub trait IdleCollector {
    fn run(&self, tx: Sender<IdleChange>, stop: StopToken) -> anyhow::Result<()>;
}

/// ロック/スリープ。
pub trait SessionCollector {
    fn run(&self, tx: Sender<SessionChange>, stop: StopToken) -> anyhow::Result<()>;
}
