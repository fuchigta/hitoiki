//! ヘッドレス常駐（`hitoiki daemon`）。UI とトースト以外の常駐処理を Tauri なしで動かす。

use crate::runtime::{Runtime, RuntimeOptions};
use crate::sink::{LogSink, RuntimeSink};
use anyhow::Context;
use crossbeam_channel::{unbounded, Sender};
use hitoiki_agents::ImportStats;
use hitoiki_core::config::Config;
use hitoiki_core::paths;
use hitoiki_core::push::PushPayload;
use hitoiki_core::timefmt::Range;
use std::sync::Arc;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

/// ログ書き込みスレッドのガード。Drop で残りをフラッシュするので、終了まで保持する。
pub struct LogGuard(#[allow(dead_code)] tracing_appender::non_blocking::WorkerGuard);

/// ログを `paths::log_dir()` の日付ローテーションファイルと stderr に出す。
/// 既に初期化済み（テストなど）なら何もしない。
pub fn init_logging() -> anyhow::Result<LogGuard> {
    let dir = paths::log_dir();
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("ログ用ディレクトリを作れません: {}", dir.display()))?;
    let file = tracing_appender::rolling::daily(&dir, "hitoiki.log");
    let (writer, guard) = tracing_appender::non_blocking(file);
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
        .with(
            tracing_subscriber::fmt::layer()
                .with_ansi(false)
                .with_writer(writer),
        )
        .try_init();
    Ok(LogGuard(guard))
}

/// ログ出力に加えて、IPC の終了要求を待機側へ伝える。
struct HeadlessSink {
    log: LogSink,
    shutdown: Sender<()>,
}

impl RuntimeSink for HeadlessSink {
    fn segments_updated(&self, range: Range) {
        self.log.segments_updated(range)
    }
    fn push(&self, push_id: &str, payload: &PushPayload) {
        self.log.push(push_id, payload)
    }
    fn doing_changed(&self, task_id: Option<&str>) {
        self.log.doing_changed(task_id)
    }
    fn import_completed(&self, stats: &ImportStats) {
        self.log.import_completed(stats)
    }
    fn open_ui(&self, screen: Option<&str>) {
        self.log.open_ui(screen)
    }
    fn shutdown_requested(&self) {
        self.log.shutdown_requested();
        let _ = self.shutdown.send(());
    }
}

/// 設定を読み、ログを初期化して常駐する。Ctrl+C または IPC の Shutdown で停止して戻る。
pub fn run_headless() -> anyhow::Result<()> {
    let _guard = init_logging()?;
    let cfg = Config::load().map_err(|e| anyhow::anyhow!("{e}"))?;
    run_headless_with(RuntimeOptions::new(cfg), true)
}

/// オプション指定版。`ctrl_c` が true なら Ctrl+C を停止要求として扱う（プロセスで 1 回だけ登録できる）。
pub fn run_headless_with(opts: RuntimeOptions, ctrl_c: bool) -> anyhow::Result<()> {
    let (tx, rx) = unbounded::<()>();
    if ctrl_c {
        let tx = tx.clone();
        ctrlc::set_handler(move || {
            let _ = tx.send(());
        })
        .context("Ctrl+C ハンドラを登録できません")?;
    }
    let sink = Arc::new(HeadlessSink {
        log: LogSink,
        shutdown: tx,
    });
    let handle = Runtime::start(opts, sink)?;
    tracing::info!("ヘッドレス常駐を開始しました（Ctrl+C で終了）");
    let _ = rx.recv();
    tracing::info!("停止しています");
    handle.stop();
    tracing::info!("停止しました");
    Ok(())
}
