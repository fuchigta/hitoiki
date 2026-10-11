use crate::Millis;
use std::sync::atomic::{AtomicI64, Ordering};

/// テスト可能な時計（docs/dev/architecture.md「配布とライセンス」）。時刻は必ずこれ経由で取得する。
pub trait Clock: Send + Sync {
    fn now_ms(&self) -> Millis;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_ms(&self) -> Millis {
        chrono::Utc::now().timestamp_millis()
    }
}

/// テスト用の固定時計。`set`/`advance` で進められる。
#[derive(Debug, Default)]
pub struct FixedClock(AtomicI64);

impl FixedClock {
    pub fn new(ms: Millis) -> Self {
        Self(AtomicI64::new(ms))
    }
    pub fn set(&self, ms: Millis) {
        self.0.store(ms, Ordering::SeqCst);
    }
    pub fn advance(&self, delta_ms: Millis) {
        self.0.fetch_add(delta_ms, Ordering::SeqCst);
    }
}

impl Clock for FixedClock {
    fn now_ms(&self) -> Millis {
        self.0.load(Ordering::SeqCst)
    }
}
