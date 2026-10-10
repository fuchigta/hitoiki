//! ログディレクトリの監視。変更ファイルをデバウンスして Importer に渡す。

use crate::importer::{ImportStats, Importer};
use crossbeam_channel::{select, unbounded, Receiver, Sender};
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher as _};
use rusqlite::Connection;
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// 取り込み完了の通知。
#[derive(Debug, Clone)]
pub struct ImportNotice {
    pub stats: ImportStats,
    /// 今回取り込み対象になったファイル（起動時の全件取り込みでは空）。
    pub files: Vec<PathBuf>,
}

/// 監視スレッドのハンドル。Drop で停止する。
pub struct Watcher {
    /// 取り込み完了通知（新規イベントがあったときだけ流れる）。
    pub notices: Receiver<ImportNotice>,
    stop: Option<Sender<()>>,
    handle: Option<JoinHandle<()>>,
}

const RESCAN: Duration = Duration::from_secs(30);

impl Watcher {
    /// 起動時に全件を取り込んでから監視を始める。`debounce` は 1〜2 秒程度を想定。
    pub fn start(importer: Importer, conn: Connection, debounce: Duration) -> anyhow::Result<Self> {
        let (ntx, nrx) = unbounded();
        let (stop_tx, stop_rx) = unbounded::<()>();
        let handle = std::thread::Builder::new()
            .name("hitoiki-agent-watcher".into())
            .spawn(move || run(importer, conn, debounce, ntx, stop_rx))?;
        Ok(Self {
            notices: nrx,
            stop: Some(stop_tx),
            handle: Some(handle),
        })
    }

    pub fn stop(&mut self) {
        self.stop.take();
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        self.stop();
    }
}

fn run(
    importer: Importer,
    mut conn: Connection,
    debounce: Duration,
    notices: Sender<ImportNotice>,
    stop: Receiver<()>,
) {
    let (etx, erx) = unbounded::<notify::Result<notify::Event>>();
    let mut watcher: RecommendedWatcher = match notify::recommended_watcher(move |res| {
        let _ = etx.send(res);
    }) {
        Ok(w) => w,
        Err(e) => {
            tracing::error!(error = %e, "ファイル監視を開始できません");
            return;
        }
    };

    let roots = importer.log_roots();
    let mut watched: BTreeSet<PathBuf> = BTreeSet::new();
    try_watch(&mut watcher, &roots, &mut watched);

    // 起動時の取り込み。
    match importer.import_all(&mut conn, None) {
        Ok(stats) => {
            let _ = notices.send(ImportNotice {
                stats,
                files: Vec::new(),
            });
        }
        Err(e) => tracing::warn!(error = %e, "初回取り込みに失敗しました"),
    }

    let mut pending: BTreeSet<PathBuf> = BTreeSet::new();
    let mut last_event = Instant::now();
    loop {
        let wait = if pending.is_empty() {
            RESCAN
        } else {
            debounce.saturating_sub(last_event.elapsed())
        };
        select! {
            recv(stop) -> _ => return,
            recv(erx) -> msg => match msg {
                Ok(Ok(ev)) => {
                    if matches!(ev.kind, EventKind::Create(_) | EventKind::Modify(_)) {
                        for p in ev.paths {
                            if p.extension().is_some_and(|e| e == "jsonl") {
                                pending.insert(p);
                                last_event = Instant::now();
                            }
                        }
                    }
                }
                Ok(Err(e)) => tracing::warn!(error = %e, "監視イベントのエラー"),
                Err(_) => return,
            },
            default(wait) => {
                if pending.is_empty() {
                    // 未存在だったログルートが後から作られた場合に備えて再試行する。
                    if watched.len() < roots.len() {
                        try_watch(&mut watcher, &roots, &mut watched);
                    }
                } else {
                    flush(&importer, &mut conn, &mut pending, &notices);
                }
            }
        }
    }
}

fn try_watch(w: &mut RecommendedWatcher, roots: &[PathBuf], watched: &mut BTreeSet<PathBuf>) {
    for r in roots {
        if watched.contains(r) || !r.is_dir() {
            continue;
        }
        match w.watch(r, RecursiveMode::Recursive) {
            Ok(()) => {
                watched.insert(r.clone());
            }
            Err(e) => tracing::warn!(root = %r.display(), error = %e, "監視に失敗しました"),
        }
    }
}

fn flush(
    importer: &Importer,
    conn: &mut Connection,
    pending: &mut BTreeSet<PathBuf>,
    notices: &Sender<ImportNotice>,
) {
    let files: Vec<PathBuf> = std::mem::take(pending).into_iter().collect();
    let mut total = ImportStats::default();
    let mut touched = Vec::new();
    for f in files {
        if !f.is_file() {
            continue;
        }
        if let Some(s) = importer.import_file(conn, &f) {
            if s.files_updated > 0 {
                touched.push(f);
            }
            total.merge(&s);
        }
    }
    if total.files_updated > 0 {
        let _ = notices.send(ImportNotice {
            stats: total,
            files: touched,
        });
    }
}
