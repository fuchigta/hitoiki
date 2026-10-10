//! アプリ全体の共有状態。DB 接続は `Mutex<Connection>` で保持する。

use crate::error::AppError;
use hitoiki_core::config::Config;
use hitoiki_core::push::PushPayload;
use hitoiki_core::service::Ctx;
use hitoiki_core::{db, paths, Millis, SystemClock};
use hitoiki_daemon::RuntimeHandle;
use rusqlite::Connection;
use serde::Serialize;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Mutex, MutexGuard};

/// 直近のプッシュ（UI の `push` イベントとポップアップ窓の表示用）。
#[derive(Debug, Clone, Serialize)]
pub struct PushEvent {
    /// `push_log.id`。応答の保存に使う。テスト通知では None。
    pub push_id: Option<String>,
    pub payload: PushPayload,
}

/// 収集・再計算などの常駐処理の状態。
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum RuntimeMode {
    /// まだ起動していない。
    Starting,
    /// このアプリが収集を行っている。
    Own,
    /// 別の常駐プロセス（`hitoiki daemon`）が動いており、収集はそちらで行われている。
    External { message: String },
    /// 起動に失敗した（UI のみ動作）。
    Failed { message: String },
}

pub const EXTERNAL_DAEMON_MESSAGE: &str =
    "別の常駐プロセス（hitoiki daemon）が動作中のため収集はそちらで行われています";

pub struct AppState {
    runtime: Mutex<Option<RuntimeHandle>>,
    mode: Mutex<RuntimeMode>,
    conn: Mutex<Connection>,
    /// この時刻まで通知を一時停止する（0 なら停止していない）。ランタイムが参照する。
    snooze_until: AtomicI64,
    /// 直近のプッシュ（ポップアップ窓が読み出す）。
    last_push: Mutex<Option<PushEvent>>,
    /// 起動引数 `--screen` で指定された最初の画面（UI の読み込み後に一度だけ取り出す）。
    initial_screen: Mutex<Option<String>>,
}

impl AppState {
    pub fn open() -> hitoiki_core::Result<Self> {
        Self::from_conn(db::open(&paths::db_file())?)
    }

    pub fn from_conn(conn: Connection) -> hitoiki_core::Result<Self> {
        Ok(Self {
            runtime: Mutex::new(None),
            mode: Mutex::new(RuntimeMode::Starting),
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

    /// 別の接続で `f` を実行する（AI 呼び出しなど時間のかかる処理が共有接続を塞がないように）。
    pub fn with_own_ctx<T>(&self, f: impl FnOnce(&Ctx) -> hitoiki_core::Result<T>) -> Result<T, AppError> {
        let cfg = Config::load()?;
        let conn = db::open(&paths::db_file())?;
        let clock = SystemClock;
        let ctx = Ctx::new(&conn, &cfg, &clock);
        Ok(f(&ctx)?)
    }

    pub fn set_runtime(&self, h: Option<RuntimeHandle>) {
        *self.runtime.lock().unwrap_or_else(|e| e.into_inner()) = h;
    }

    /// 常駐処理を停止する（進行中の区間を閉じる）。起動していなければ何もしない。
    pub fn stop_runtime(&self) {
        let h = self.runtime.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some(h) = h {
            tracing::info!("常駐処理を停止しています");
            h.stop();
            tracing::info!("常駐処理を停止しました");
        }
    }

    pub fn runtime_mode(&self) -> RuntimeMode {
        self.mode.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn set_runtime_mode(&self, m: RuntimeMode) {
        *self.mode.lock().unwrap_or_else(|e| e.into_inner()) = m;
    }

    /// 書き込み後の再計算を要求する。自前の常駐処理がなければ、別プロセスの常駐へ IPC で伝える。
    pub fn request_rebuild(&self) {
        let guard = self.runtime.lock().unwrap_or_else(|e| e.into_inner());
        match guard.as_ref() {
            Some(h) => h.request_rebuild(),
            None => {
                drop(guard);
                hitoiki_ipc::notify_best_effort(&hitoiki_ipc::Message::DataChanged { what: "app".into() });
            }
        }
    }

    /// 設定変更を常駐処理へ反映する。
    pub fn apply_config(&self) {
        let Ok(cfg) = Config::load() else {
            return;
        };
        if let Some(h) = self.runtime.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            h.set_config(cfg);
            h.request_rebuild();
        }
    }

    pub fn set_last_push(&self, v: Option<PushEvent>) {
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

    pub fn last_push(&self) -> Option<PushEvent> {
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
