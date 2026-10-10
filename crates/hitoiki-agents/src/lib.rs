//! エージェント（Claude Code / Codex CLI）のセッションログ取り込み（設計書 13 章）。
//!
//! フック設定を必須にせず、各エージェントが残す JSONL を増分解析して `agent_event` に保存する。

pub mod claude;
pub mod codex;
pub mod common;
pub mod importer;
pub mod watcher;

pub use claude::ClaudeCodeAdapter;
pub use codex::CodexAdapter;
pub use importer::{adapters_from_config, import_all, import_with, ImportStats, Importer};
pub use watcher::{ImportNotice, Watcher};

use hitoiki_core::model::AgentEvent;
use std::path::{Path, PathBuf};

/// エージェントログのアダプタ（13.1）。
pub trait AgentAdapter: Send + Sync {
    /// `agent_event.agent` に入る名前（`claude-code` / `codex`）。
    fn name(&self) -> &str;

    /// 監視・走査対象のディレクトリ。
    fn log_roots(&self) -> Vec<PathBuf>;

    /// 取り込み対象のファイルか（拡張子が `.jsonl` であることは呼び出し側で確認済み）。
    /// サブエージェントのログ等を除外するために使う。
    fn accepts_file(&self, _file: &Path) -> bool {
        true
    }

    /// `from_offset` 以降の追記分を解析する。戻り値は (イベント, 次回のオフセット)。
    /// 改行で終わっていない最終行は書き込み途中とみなし、オフセットを進めない。
    fn parse_incremental(&self, file: &Path, from_offset: u64) -> anyhow::Result<(Vec<AgentEvent>, u64)>;
}
