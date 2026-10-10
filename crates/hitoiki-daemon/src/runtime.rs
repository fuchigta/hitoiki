//! 常駐ランタイム（3.3）。収集・ログ取り込み・再計算・カレンダー・プッシュ・IPC の各スレッドを束ねる。
//!
//! スレッドごとに別の SQLite 接続を持つ（WAL + busy_timeout）。収集データの書き込みは
//! このランタイム（Recorder）だけが行う。ネットワークポートは開かない。

use crate::push_ctx::push_tick;
use crate::sink::RuntimeSink;
use anyhow::{anyhow, Context};
use crossbeam_channel::{unbounded, Receiver, RecvTimeoutError, Sender};
use hitoiki_agents::{adapters_from_config, Importer, Watcher};
use hitoiki_collect::{Recorder, StopToken};
use hitoiki_core::config::Config;
use hitoiki_core::pipeline::SegmentJudge;
use hitoiki_core::rebuild::{rebuild_with_judge, RebuildStats};
use hitoiki_core::repo::{calendar as cal_repo, raw, skill, task};
use hitoiki_core::timefmt::Range;
use hitoiki_core::{db, paths, Clock, SystemClock};
use hitoiki_ipc::{IpcError, Message, PipeName, Reply, ServerHandle};
use hitoiki_judge::JudgeContext;
use rusqlite::Connection;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// 既に常駐プロセスが動いている（IPC の名前が使用中）。`Runtime::start` の失敗理由として
/// `anyhow::Error::downcast_ref::<AlreadyRunning>()` で判別できる。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AlreadyRunning;

impl std::fmt::Display for AlreadyRunning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("既に常駐プロセスが動いています")
    }
}

impl std::error::Error for AlreadyRunning {}

/// 起動オプション。`new` が既定値を与える。
#[derive(Clone)]
pub struct RuntimeOptions {
    pub config: Config,
    /// false で収集器と Recorder を起動しない（テスト用）。
    pub collect: bool,
    /// false でエージェントログの監視を起動しない（テスト用）。
    pub watch_agents: bool,
    /// None なら現在のユーザーの既定名。
    pub pipe_name: Option<PipeName>,
    /// None なら `paths::db_file()`。
    pub db_path: Option<PathBuf>,
    pub clock: Arc<dyn Clock>,
    /// 定期再計算の間隔（既定 60 秒）。
    pub rebuild_interval: Duration,
    /// 即時再計算要求のデバウンス（既定 3 秒）。
    pub rebuild_debounce: Duration,
    /// プッシュ評価の間隔（既定 60 秒）。
    pub push_interval: Duration,
    /// None なら `calendar.fetch_interval_min`。
    pub calendar_interval: Option<Duration>,
}

impl RuntimeOptions {
    pub fn new(config: Config) -> Self {
        Self {
            config,
            collect: true,
            watch_agents: true,
            pipe_name: None,
            db_path: None,
            clock: Arc::new(SystemClock),
            rebuild_interval: Duration::from_secs(60),
            rebuild_debounce: Duration::from_secs(3),
            push_interval: Duration::from_secs(60),
            calendar_interval: None,
        }
    }
}

struct Shared {
    cfg: RwLock<Config>,
    sink: Arc<dyn RuntimeSink>,
    clock: Arc<dyn Clock>,
    stop: StopToken,
    db_path: PathBuf,
    rebuild_tx: Sender<()>,
}

impl Shared {
    fn cfg(&self) -> Config {
        self.cfg.read().map(|c| c.clone()).unwrap_or_default()
    }
    fn request_rebuild(&self) {
        let _ = self.rebuild_tx.send(());
    }
    fn open_db(&self) -> anyhow::Result<Connection> {
        db::open(&self.db_path).map_err(|e| anyhow!("DB を開けません: {e}"))
    }
}

/// 稼働中のランタイム。`stop` または Drop で全スレッドを止めて join する。
pub struct RuntimeHandle {
    shared: Arc<Shared>,
    pipe: PipeName,
    ipc: Option<ServerHandle>,
    threads: Vec<JoinHandle<()>>,
}

impl RuntimeHandle {
    /// 全スレッドを停止して終了を待つ。Recorder は進行中の区間を閉じる。
    /// カレンダー取得中は通信の完了（最大 30 秒）を待つことがある。
    pub fn stop(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        self.shared.stop.stop();
        if let Some(ipc) = self.ipc.take() {
            ipc.stop();
        }
        for t in self.threads.drain(..) {
            let name = t.thread().name().unwrap_or("?").to_string();
            if t.join().is_err() {
                tracing::error!(thread = name, "スレッドが panic しました");
            }
        }
    }

    /// 再計算を要求する（デバウンスあり）。
    pub fn request_rebuild(&self) {
        self.shared.request_rebuild();
    }

    /// 設定を差し替える。再計算・カレンダー・プッシュ・判定器に次回から反映される
    /// （収集器とログ監視の設定は再起動が必要）。
    pub fn set_config(&self, cfg: Config) {
        if let Ok(mut w) = self.shared.cfg.write() {
            *w = cfg;
        }
    }

    pub fn pipe_name(&self) -> &PipeName {
        &self.pipe
    }
}

impl Drop for RuntimeHandle {
    fn drop(&mut self) {
        self.shutdown();
    }
}

pub struct Runtime;

impl Runtime {
    /// 常駐処理を起動する。既に常駐プロセスがいれば（IPC が `AlreadyRunning`）失敗する。
    /// IPC を最初に起動するので、二重起動時に DB や収集の残骸処理には触れない。
    pub fn start(opts: RuntimeOptions, sink: Arc<dyn RuntimeSink>) -> anyhow::Result<RuntimeHandle> {
        let pipe = match opts.pipe_name.clone() {
            Some(p) => p,
            None => PipeName::for_current_user().map_err(|e| anyhow!("IPC の名前を決められません: {e}"))?,
        };
        let (rebuild_tx, rebuild_rx) = unbounded::<()>();
        let shared = Arc::new(Shared {
            cfg: RwLock::new(opts.config.clone()),
            sink: sink.clone(),
            clock: opts.clock.clone(),
            stop: StopToken::new(),
            db_path: opts.db_path.clone().unwrap_or_else(paths::db_file),
            rebuild_tx,
        });

        let ipc = hitoiki_ipc::serve(&pipe, ipc_handler(shared.clone())).map_err(|e| match e {
            IpcError::AlreadyRunning => anyhow::Error::new(AlreadyRunning),
            other => anyhow!("IPC サーバを起動できません: {other}"),
        })?;
        let mut h = RuntimeHandle {
            shared: shared.clone(),
            pipe,
            ipc: Some(ipc),
            threads: Vec::new(),
        };

        // 再計算（先に接続を開き、DB の問題を起動時に検出する）
        let conn = shared.open_db()?;
        {
            let sh = shared.clone();
            let (interval, debounce) = (opts.rebuild_interval, opts.rebuild_debounce);
            h.threads.push(spawn("hitoiki-rebuild", move || {
                rebuild_loop(sh, conn, rebuild_rx, interval, debounce)
            })?);
        }

        if opts.collect {
            h.threads.push(spawn_collect(&shared)?);
        }
        if opts.watch_agents && !opts.config.agent.enabled.is_empty() {
            h.threads.push(spawn_agent_bridge(&shared)?);
        }
        {
            let sh = shared.clone();
            let ov = opts.calendar_interval;
            h.threads
                .push(spawn("hitoiki-calendar", move || calendar_loop(sh, ov))?);
        }
        {
            let sh = shared.clone();
            let iv = opts.push_interval;
            h.threads.push(spawn("hitoiki-push", move || push_loop(sh, iv))?);
        }
        tracing::info!("常駐処理を開始しました");
        Ok(h)
    }
}

fn spawn(name: &str, f: impl FnOnce() + Send + 'static) -> anyhow::Result<JoinHandle<()>> {
    std::thread::Builder::new()
        .name(name.into())
        .spawn(f)
        .with_context(|| format!("スレッド {name} を起動できません"))
}

fn ipc_handler(sh: Arc<Shared>) -> impl Fn(Message) -> Reply + Send + Sync + 'static {
    move |msg| {
        match msg {
            Message::TaskChanged { task_id } => {
                sh.sink.doing_changed(task_id.as_deref());
                sh.request_rebuild();
            }
            Message::DataChanged { .. } => sh.request_rebuild(),
            Message::OpenUi { screen } => sh.sink.open_ui(screen.as_deref()),
            Message::Ping => {}
            Message::Shutdown => sh.sink.shutdown_requested(),
        }
        Reply::ok()
    }
}

// ---- 収集 ----

fn spawn_collect(sh: &Arc<Shared>) -> anyhow::Result<JoinHandle<()>> {
    let cfg = sh.cfg();
    let conn = sh.open_db()?;
    let mut recorder = Recorder::new(conn, cfg.collect.clone(), sh.clock.clone())
        .map_err(|e| anyhow!("Recorder を初期化できません: {e}"))?;
    let (ftx, frx) = unbounded();
    let (itx, irx) = unbounded();
    let (stx, srx) = unbounded();
    let collectors =
        hitoiki_collect_win::spawn_all_with_clock(&cfg.collect, sh.clock.clone(), ftx, itx, stx)?;
    let stop = sh.stop.clone();
    spawn("hitoiki-recorder", move || {
        if let Err(e) = recorder.run(frx, irx, srx, stop) {
            tracing::error!(error = %e, "Recorder が異常終了しました");
        }
        if let Err(e) = collectors.stop_and_join() {
            tracing::warn!(error = %e, "収集スレッドの終了でエラー");
        }
    })
}

// ---- エージェントログ ----

fn spawn_agent_bridge(sh: &Arc<Shared>) -> anyhow::Result<JoinHandle<()>> {
    let cfg = sh.cfg();
    let importer = Importer::new(adapters_from_config(&cfg)).with_clock(sh.clock.clone());
    let watcher = Watcher::start(importer, sh.open_db()?, Duration::from_secs(2))?;
    let sh = sh.clone();
    spawn("hitoiki-agent-bridge", move || {
        while !sh.stop.is_stopped() {
            match watcher.notices.recv_timeout(Duration::from_millis(200)) {
                Ok(n) => {
                    sh.sink.import_completed(&n.stats);
                    sh.request_rebuild();
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
        drop(watcher); // 監視スレッドを停止して join する
    })
}

// ---- 再計算 ----

fn rebuild_loop(
    sh: Arc<Shared>,
    mut conn: Connection,
    rx: Receiver<()>,
    interval: Duration,
    debounce: Duration,
) {
    // 起動直後に一度、前回停止中の取り込み分を反映する。
    let mut pending: Option<Instant> = Some(Instant::now());
    let mut next_periodic = Instant::now() + interval;
    while !sh.stop.is_stopped() {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(()) => {
                if pending.is_none() {
                    pending = Some(Instant::now() + debounce);
                }
                while rx.try_recv().is_ok() {}
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
        let now = Instant::now();
        if pending.is_some_and(|t| t <= now) || next_periodic <= now {
            pending = None;
            match run_rebuild(&sh, &mut conn) {
                Ok(stats) => sh.sink.segments_updated(stats.range),
                Err(e) => tracing::warn!(error = %e, "再計算に失敗しました"),
            }
            next_periodic = Instant::now() + interval;
        }
    }
}

/// 直近 `collect.rebuild_window_min` 分を再計算する。判定器が有効なら毎回構築する。
fn run_rebuild(sh: &Shared, conn: &mut Connection) -> anyhow::Result<RebuildStats> {
    let cfg = sh.cfg();
    let now = sh.clock.now_ms();
    let window = cfg.collect.rebuild_window_min.max(1) as i64 * 60_000;
    let range = Range::new(now - window, now);
    let judge = build_judge(conn, &cfg, range);
    Ok(rebuild_with_judge(conn, &cfg, range, now, judge.as_deref())?)
}

fn build_judge(conn: &Connection, cfg: &Config, range: Range) -> Option<Box<dyn SegmentJudge>> {
    if !cfg.judge.enabled {
        return None;
    }
    let tz = cfg.tz().unwrap_or(chrono_tz::UTC);
    let mut ctx = JudgeContext::new(tz);
    ctx.open_tasks = raw::open_tasks(conn).unwrap_or_default();
    ctx.skills = skill::list(conn).unwrap_or_default();
    ctx.doing_task = task::list_doing(conn).ok().and_then(|v| v.into_iter().next());
    ctx.calendar = cal_repo::list(conn, range).unwrap_or_default();
    match hitoiki_judge::from_config(cfg, ctx) {
        Ok(j) => j,
        Err(e) => {
            tracing::warn!(error = %format!("{e:#}"), "判定器を構築できないためルールのみで再計算します");
            None
        }
    }
}

// ---- カレンダー ----

fn calendar_loop(sh: Arc<Shared>, override_interval: Option<Duration>) {
    let conn = match sh.open_db() {
        Ok(c) => c,
        Err(e) => {
            tracing::error!(error = %e, "カレンダー同期を開始できません");
            return;
        }
    };
    loop {
        let cfg = sh.cfg();
        if !cfg.calendar.sources.is_empty() {
            match hitoiki_calendar::sync_all(&conn, &cfg, sh.clock.now_ms()) {
                Ok(s) => {
                    tracing::info!(
                        ok = s.sources_ok,
                        events = s.events_saved,
                        failed = s.errors.len(),
                        "カレンダーを同期しました"
                    );
                    if s.sources_ok > 0 {
                        sh.request_rebuild();
                    }
                }
                Err(e) => tracing::warn!(error = %e, "カレンダー同期に失敗しました"),
            }
        }
        let wait = override_interval
            .unwrap_or_else(|| Duration::from_secs(cfg.calendar.fetch_interval_min.max(1) * 60));
        if sh.stop.sleep(wait) {
            break;
        }
    }
}

// ---- プッシュ ----

fn push_loop(sh: Arc<Shared>, interval: Duration) {
    let conn = match sh.open_db() {
        Ok(c) => c,
        Err(e) => {
            tracing::error!(error = %e, "プッシュ評価を開始できません");
            return;
        }
    };
    while !sh.stop.sleep(interval) {
        let cfg = sh.cfg();
        if let Err(e) = push_tick(&conn, &cfg, sh.clock.now_ms(), sh.sink.as_ref()) {
            tracing::warn!(error = %e, "プッシュ評価に失敗しました");
        }
    }
}
