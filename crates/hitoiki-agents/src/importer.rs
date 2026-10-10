//! ログファイルの走査と DB への取り込み（オフセットは同一トランザクションで保存）。

use crate::{AgentAdapter, ClaudeCodeAdapter, CodexAdapter};
use hitoiki_core::config::Config;
use hitoiki_core::repo::agent as repo;
use hitoiki_core::{Clock, Millis, Result, SystemClock};
use rusqlite::Connection;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImportStats {
    /// 走査した .jsonl ファイル数。
    pub files_scanned: usize,
    /// 新しい行があり取り込んだファイル数。
    pub files_updated: usize,
    /// 解析で得たイベント数。
    pub events_parsed: usize,
    /// 実際に DB へ挿入した数（重複は除く）。
    pub events_inserted: usize,
    /// 失敗したファイル数（ログに警告を出してスキップ）。
    pub files_failed: usize,
}

impl ImportStats {
    pub fn merge(&mut self, o: &ImportStats) {
        self.files_scanned += o.files_scanned;
        self.files_updated += o.files_updated;
        self.events_parsed += o.events_parsed;
        self.events_inserted += o.events_inserted;
        self.files_failed += o.files_failed;
    }
}

/// 設定で有効なアダプタを作る。
pub fn adapters_from_config(cfg: &Config) -> Vec<Box<dyn AgentAdapter>> {
    let n = cfg.agent.prompt_excerpt_chars;
    let mut v: Vec<Box<dyn AgentAdapter>> = Vec::new();
    for name in &cfg.agent.enabled {
        match name.as_str() {
            crate::claude::NAME => v.push(Box::new(ClaudeCodeAdapter::new(n))),
            crate::codex::NAME => v.push(Box::new(CodexAdapter::new(n))),
            other => tracing::warn!(agent = other, "未対応のエージェント名です"),
        }
    }
    v
}

/// 有効なアダプタすべてのログを取り込む。
pub fn import_all(conn: &mut Connection, cfg: &Config, since: Option<Millis>) -> Result<ImportStats> {
    Importer::new(adapters_from_config(cfg)).import_all(conn, since)
}

/// 指定アダプタで取り込む（テスト・Watcher 用）。
pub fn import_with(
    conn: &mut Connection,
    adapters: Vec<Box<dyn AgentAdapter>>,
    since: Option<Millis>,
) -> Result<ImportStats> {
    Importer::new(adapters).import_all(conn, since)
}

pub struct Importer {
    adapters: Vec<Box<dyn AgentAdapter>>,
    clock: Arc<dyn Clock>,
}

impl Importer {
    pub fn new(adapters: Vec<Box<dyn AgentAdapter>>) -> Self {
        Self {
            adapters,
            clock: Arc::new(SystemClock),
        }
    }

    pub fn with_clock(mut self, clock: Arc<dyn Clock>) -> Self {
        self.clock = clock;
        self
    }

    pub fn log_roots(&self) -> Vec<PathBuf> {
        self.adapters.iter().flat_map(|a| a.log_roots()).collect()
    }

    /// すべてのログルートを再帰走査して取り込む。
    pub fn import_all(&self, conn: &mut Connection, since: Option<Millis>) -> Result<ImportStats> {
        let mut stats = ImportStats::default();
        for adapter in &self.adapters {
            let mut files = Vec::new();
            for root in adapter.log_roots() {
                collect_jsonl(&root, &mut files);
            }
            files.sort();
            for file in files {
                if !adapter.accepts_file(&file) {
                    continue;
                }
                if let Some(since) = since {
                    if mtime_ms(&file).is_some_and(|m| m < since) {
                        continue;
                    }
                }
                self.import_one(conn, adapter.as_ref(), &file, &mut stats);
            }
        }
        Ok(stats)
    }

    /// 1 ファイルだけ取り込む（Watcher から）。どのアダプタの対象でもなければ None。
    pub fn import_file(&self, conn: &mut Connection, file: &Path) -> Option<ImportStats> {
        if file.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            return None;
        }
        let adapter = self
            .adapters
            .iter()
            .find(|a| a.accepts_file(file) && a.log_roots().iter().any(|r| file.starts_with(r)))?;
        let mut stats = ImportStats::default();
        self.import_one(conn, adapter.as_ref(), file, &mut stats);
        Some(stats)
    }

    fn import_one(
        &self,
        conn: &mut Connection,
        adapter: &dyn AgentAdapter,
        file: &Path,
        stats: &mut ImportStats,
    ) {
        stats.files_scanned += 1;
        match self.try_import(conn, adapter, file) {
            Ok(Some((parsed, inserted))) => {
                stats.files_updated += 1;
                stats.events_parsed += parsed;
                stats.events_inserted += inserted;
            }
            Ok(None) => {}
            Err(e) => {
                stats.files_failed += 1;
                tracing::warn!(file = %file.display(), error = %e, "ログの取り込みに失敗しました");
            }
        }
    }

    fn try_import(
        &self,
        conn: &mut Connection,
        adapter: &dyn AgentAdapter,
        file: &Path,
    ) -> anyhow::Result<Option<(usize, usize)>> {
        let key = file.to_string_lossy().to_string();
        let size = std::fs::metadata(file)?.len();
        let mut offset = repo::get_offset(conn, &key)?.unwrap_or(0);
        if offset > size {
            offset = 0; // ファイルが縮んだ: 先頭から読み直す（重複は一意索引が防ぐ）
        }
        if offset == size && repo::get_offset(conn, &key)?.is_some() {
            return Ok(None);
        }
        let (events, next) = adapter.parse_incremental(file, offset)?;
        if next == offset && events.is_empty() {
            return Ok(None);
        }
        let tx = conn.transaction().map_err(hitoiki_core::Error::from)?;
        let inserted = repo::insert_events(&tx, &events)?;
        repo::set_offset(&tx, &key, adapter.name(), next, self.clock.now_ms())?;
        tx.commit().map_err(hitoiki_core::Error::from)?;
        Ok(Some((events.len(), inserted)))
    }
}

fn mtime_ms(p: &Path) -> Option<Millis> {
    let t = std::fs::metadata(p).ok()?.modified().ok()?;
    let d = t.duration_since(std::time::UNIX_EPOCH).ok()?;
    Some(d.as_millis() as Millis)
}

fn collect_jsonl(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in rd.flatten() {
        let path = entry.path();
        let Ok(ft) = entry.file_type() else { continue };
        if ft.is_dir() {
            collect_jsonl(&path, out);
        } else if ft.is_file() && path.extension().is_some_and(|e| e == "jsonl") {
            out.push(path);
        }
    }
}
