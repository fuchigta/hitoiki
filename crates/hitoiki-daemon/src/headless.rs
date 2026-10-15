//! ヘッドレス常駐（`hitoiki daemon`）。UI とトースト以外の常駐処理を Tauri なしで動かす。

use crate::runtime::{Runtime, RuntimeOptions};
use crate::sink::{LogSink, RuntimeSink};
use anyhow::Context;
use crossbeam_channel::{unbounded, Sender};
use hitoiki_agents::ImportStats;
use hitoiki_core::config::Config;
use hitoiki_core::nudge::NudgePayload;
use hitoiki_core::paths;
use hitoiki_core::timefmt::Range;
use std::sync::Arc;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

/// ログ書き込みスレッドのガード。Drop で残りをフラッシュするので、終了まで保持する。
pub struct LogGuard(#[allow(dead_code)] tracing_appender::non_blocking::WorkerGuard);

/// ログファイルを残す日数（日次ローテーションのファイル数）。これより古いものは新しいファイルを作るときに削除される。
pub const LOG_KEEP_DAYS: usize = 14;

/// ログを `paths::log_dir()` の日付ローテーションファイル（`LOG_KEEP_DAYS` 日分）と stderr に出す。
/// ヘッドレス daemon と常駐アプリの共通の初期化。ウインドウタイトル・プロンプト内容・ICS URL は
/// ログに出さない（呼び出し側の規約）。panic も場所だけをログに残す（メッセージは出さない）。
/// 既に初期化済み（テストなど）なら何もしない。
pub fn init_logging() -> anyhow::Result<LogGuard> {
    init_logging_with(true)
}

/// ログファイルにだけ出す（stderr には出さない）。CLI が記録の削除などをログに残すときに使う。
pub fn init_file_logging() -> anyhow::Result<LogGuard> {
    init_logging_with(false)
}

fn init_logging_with(stderr: bool) -> anyhow::Result<LogGuard> {
    let dir = paths::log_dir();
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("ログ用ディレクトリを作れません: {}", dir.display()))?;
    let file = tracing_appender::rolling::Builder::new()
        .rotation(tracing_appender::rolling::Rotation::DAILY)
        .filename_prefix("hitoiki.log")
        .max_log_files(LOG_KEEP_DAYS)
        .build(&dir)
        .with_context(|| format!("ログファイルを開けません: {}", dir.display()))?;
    let (writer, guard) = tracing_appender::non_blocking(file);
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = tracing_subscriber::registry()
        .with(filter)
        .with(stderr.then(|| tracing_subscriber::fmt::layer().with_writer(std::io::stderr)))
        .with(
            tracing_subscriber::fmt::layer()
                .with_ansi(false)
                .with_writer(writer),
        )
        .try_init();
    install_panic_hook();
    Ok(LogGuard(guard))
}

fn install_panic_hook() {
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current();
        match info.location() {
            Some(l) => tracing::error!(
                thread = thread.name().unwrap_or("?"),
                file = l.file(),
                line = l.line(),
                "スレッドが panic しました"
            ),
            None => tracing::error!(thread = thread.name().unwrap_or("?"), "スレッドが panic しました"),
        }
        prev(info);
    }));
}

/// daemon 実行ファイルと同じディレクトリの `hitoiki-app`。無ければ None。
fn app_path() -> Option<std::path::PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let app = exe.with_file_name(if cfg!(windows) {
        "hitoiki-app.exe"
    } else {
        "hitoiki-app"
    });
    app.exists().then_some(app)
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
    fn nudge(&self, nudge_id: &str, payload: &NudgePayload) {
        self.log.nudge(nudge_id, payload)
    }
    fn doing_changed(&self, task_id: Option<&str>) {
        self.log.doing_changed(task_id)
    }
    fn import_completed(&self, stats: &ImportStats) {
        self.log.import_completed(stats)
    }
    fn open_ui(&self, screen: Option<&str>) {
        // 既に起動済みなら、アプリ側の single-instance が画面を切り替える。
        match app_path() {
            Some(app) => {
                let mut cmd = std::process::Command::new(&app);
                if let Some(s) = screen {
                    cmd.arg("--screen").arg(s);
                }
                match cmd.spawn() {
                    Ok(_) => tracing::info!(app = %app.display(), "UI を起動しました"),
                    Err(e) => tracing::warn!(app = %app.display(), error = %e, "UI を起動できません"),
                }
            }
            None => tracing::warn!("hitoiki-app が見つからないため UI を開けません"),
        }
    }
    fn shutdown_requested(&self) {
        self.log.shutdown_requested();
        let _ = self.shutdown.send(());
    }
}

/// 設定を読み、ログを初期化して常駐する。Ctrl+C または IPC の Shutdown で停止して戻る。
pub fn run_headless() -> anyhow::Result<()> {
    let _guard = init_logging()?;
    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        "hitoiki daemon（ヘッドレス）を起動します"
    );
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
