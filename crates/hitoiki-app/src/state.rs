//! アプリ全体の共有状態。DB 接続は `Mutex<Connection>` で保持する。

use crate::error::AppError;
use hitoiki_core::config::Config;
use hitoiki_core::service::Ctx;
use hitoiki_core::{db, paths, Millis, SystemClock};
use rusqlite::Connection;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Mutex, MutexGuard};

pub struct AppState {
    conn: Mutex<Connection>,
    /// この時刻まで通知を一時停止する（0 なら停止していない）。ランタイムが参照する。
    snooze_until: AtomicI64,
    /// 直近のプッシュ（ポップアップ窓が読み出す）。
    last_push: Mutex<Option<serde_json::Value>>,
    /// 起動引数 `--screen` で指定された最初の画面（UI の読み込み後に一度だけ取り出す）。
    initial_screen: Mutex<Option<String>>,
}

impl AppState {
    pub fn open() -> hitoiki_core::Result<Self> {
        Self::from_conn(db::open(&paths::db_file())?)
    }

    pub fn from_conn(conn: Connection) -> hitoiki_core::Result<Self> {
        Ok(Self {
            conn: Mutex::new(conn),
            snooze_until: AtomicI64::new(0),
            last_push: Mutex::new(None),
            initial_screen: Mutex::new(None),
        })
    }

    fn lock(&self) -> MutexGuard<'_, Connection> {
        // 他スレッドの panic で poison されても、接続自体は使える。
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// 設定を読み直して `Ctx` を組み立て、`f` を呼ぶ（設定ファイルの変更が即時に反映される）。
    pub fn with_ctx<T>(&self, f: impl FnOnce(&Ctx) -> hitoiki_core::Result<T>) -> Result<T, AppError> {
        let cfg = Config::load()?;
        let conn = self.lock();
        let clock = SystemClock;
        let ctx = Ctx::new(&conn, &cfg, &clock);
        Ok(f(&ctx)?)
    }

    pub fn snooze_until(&self) -> Millis {
        self.snooze_until.load(Ordering::SeqCst)
    }

    pub fn set_snooze_until(&self, at: Millis) {
        self.snooze_until.store(at, Ordering::SeqCst);
    }

    pub fn is_snoozed(&self, now: Millis) -> bool {
        self.snooze_until() > now
    }

    pub fn set_last_push(&self, v: Option<serde_json::Value>) {
        *self.last_push.lock().unwrap_or_else(|e| e.into_inner()) = v;
    }

    pub fn set_initial_screen(&self, screen: Option<String>) {
        *self.initial_screen.lock().unwrap_or_else(|e| e.into_inner()) = screen;
    }

    pub fn take_initial_screen(&self) -> Option<String> {
        self.initial_screen
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
    }

    pub fn last_push(&self) -> Option<serde_json::Value> {
        self.last_push.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn スヌーズの判定() {
        let s = AppState::from_conn(db::open_in_memory().unwrap()).unwrap();
        assert!(!s.is_snoozed(100));
        s.set_snooze_until(200);
        assert!(s.is_snoozed(100));
        assert!(!s.is_snoozed(200));
    }
}
