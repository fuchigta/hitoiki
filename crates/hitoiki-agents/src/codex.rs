//! Codex CLI のセッションログ（`~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl`）。
//!
//! 公開されている形式に基づく（docs/architecture.md「エージェントアダプタ」。このマシンに実ログが無く未検証）:
//! - 新形式: 各行 `{"timestamp", "type": "session_meta"|"response_item"|"event_msg"|"turn_context", "payload": {..}}`。
//!   `session_meta.payload` に `id`, `cwd`, `git.branch`。`event_msg.payload.type` が
//!   `user_message`（入力。`message`）/ `task_complete` または `turn_complete`（応答完了）。
//!   `response_item` は重複するので使わない。`turn_context.payload.cwd` があれば cwd を更新する。
//! - 旧形式: 先頭行が `{"id", "timestamp", ...}` のメタ、以降は `{"type":"message","role":..,"content":[..]}`
//!   のみで行ごとの時刻が無い。時刻を捏造しないため、session_start（メタの時刻）以外のイベントは出さない。
//! - 非対話実行（`codex exec`）は `session_meta.payload` の `originator == "codex_exec"` または
//!   `source == "exec"` で区別できる（公開されている Codex CLI の実装に基づく。実ログでは未検証）。
//!   該当セッションはイベントを一切出さない。

use crate::common::{excerpt, parse_ts, read_complete_lines, read_first_line};
use crate::AgentAdapter;
use hitoiki_core::id::new_id;
use hitoiki_core::model::{AgentEvent, AgentEventKind};
use hitoiki_core::Millis;
use serde_json::Value;
use std::path::{Path, PathBuf};

pub const NAME: &str = "codex";

pub struct CodexAdapter {
    root: Option<PathBuf>,
    prompt_excerpt_chars: usize,
}

impl CodexAdapter {
    /// `CODEX_HOME`（なければ `~/.codex`）の `sessions` を使う。
    pub fn new(prompt_excerpt_chars: usize) -> Self {
        let root = std::env::var_os("CODEX_HOME")
            .map(PathBuf::from)
            .or_else(|| dirs::home_dir().map(|h| h.join(".codex")))
            .map(|d| d.join("sessions"));
        Self {
            root,
            prompt_excerpt_chars,
        }
    }

    pub fn with_root(root: PathBuf, prompt_excerpt_chars: usize) -> Self {
        Self {
            root: Some(root),
            prompt_excerpt_chars,
        }
    }
}

#[derive(Default)]
struct Meta {
    session_id: Option<String>,
    at: Option<Millis>,
    cwd: Option<String>,
    git_branch: Option<String>,
    /// `codex exec` 等の非対話セッション。
    non_interactive: bool,
}

fn read_meta(v: &Value, meta: &mut Meta) {
    // 新形式は payload 内、旧形式は行の直下。
    let p = v.get("payload").unwrap_or(v);
    if let Some(id) = p.get("id").and_then(Value::as_str) {
        meta.session_id = Some(id.to_string());
    }
    if let Some(t) = p.get("timestamp").and_then(Value::as_str).and_then(parse_ts) {
        meta.at = Some(t);
    }
    if p.get("originator").and_then(Value::as_str) == Some("codex_exec")
        || p.get("source").and_then(Value::as_str) == Some("exec")
    {
        meta.non_interactive = true;
    }
    if let Some(c) = p.get("cwd").and_then(Value::as_str) {
        meta.cwd = Some(c.to_string());
    }
    if let Some(b) = p
        .pointer("/git/branch")
        .and_then(Value::as_str)
        .filter(|b| !b.is_empty())
    {
        meta.git_branch = Some(b.to_string());
    }
}

/// ファイル名 `rollout-<日時>-<uuid>.jsonl` 末尾の UUID。
fn id_from_filename(file: &Path) -> Option<String> {
    let stem = file.file_stem()?.to_string_lossy().to_string();
    let n = stem.len();
    if n >= 36 && stem.is_char_boundary(n - 36) {
        let tail = &stem[n - 36..];
        if tail.matches('-').count() == 4 {
            return Some(tail.to_string());
        }
    }
    None
}

fn is_meta_row(v: &Value) -> bool {
    v.get("type").and_then(Value::as_str) == Some("session_meta")
        || (v.get("type").is_none() && v.get("id").is_some() && v.get("timestamp").is_some())
}

impl AgentAdapter for CodexAdapter {
    fn name(&self) -> &str {
        NAME
    }

    fn log_roots(&self) -> Vec<PathBuf> {
        self.root.iter().cloned().collect()
    }

    fn parse_incremental(&self, file: &Path, from_offset: u64) -> anyhow::Result<(Vec<AgentEvent>, u64)> {
        let (lines, next) = read_complete_lines(file, from_offset)?;
        let source_file = file.to_string_lossy().to_string();

        // メタ情報: 途中からの読み込みでは先頭行から補う。
        let mut meta = Meta::default();
        if from_offset > 0 {
            if let Some(first) = read_first_line(file) {
                if let Ok(v) = serde_json::from_slice::<Value>(&first) {
                    if is_meta_row(&v) {
                        read_meta(&v, &mut meta);
                    }
                }
            }
        }
        if meta.non_interactive {
            return Ok((Vec::new(), next));
        }
        let file_id = id_from_filename(file);

        let mut events: Vec<AgentEvent> = Vec::new();
        let mut started = from_offset > 0;
        let mut skipped = 0usize;

        for line in lines.iter() {
            if line.bytes.iter().all(|b| b.is_ascii_whitespace()) {
                continue;
            }
            let Ok(v) = serde_json::from_slice::<Value>(&line.bytes) else {
                skipped += 1;
                continue;
            };
            let ty = v.get("type").and_then(Value::as_str);
            let line_ts = v.get("timestamp").and_then(Value::as_str).and_then(parse_ts);

            if is_meta_row(&v) {
                read_meta(&v, &mut meta);
                if meta.non_interactive {
                    return Ok((Vec::new(), next));
                }
                if !started {
                    started = true;
                    if let Some(at) = meta.at.or(line_ts) {
                        events.push(self.event(
                            &meta,
                            &file_id,
                            &source_file,
                            line.offset,
                            at,
                            AgentEventKind::SessionStart,
                            None,
                        ));
                    }
                }
                continue;
            }

            let is_new = matches!(
                ty,
                Some("response_item") | Some("event_msg") | Some("turn_context") | Some("compacted")
            );

            if is_new {
                let Some(at) = line_ts else {
                    skipped += 1;
                    continue;
                };
                if !started {
                    // メタ行が無いファイル: 最初の行の時刻をセッション開始とする。
                    started = true;
                    events.push(self.event(
                        &meta,
                        &file_id,
                        &source_file,
                        line.offset,
                        at,
                        AgentEventKind::SessionStart,
                        None,
                    ));
                }
                let payload = v.get("payload").unwrap_or(&Value::Null);
                match ty {
                    Some("turn_context") => {
                        if let Some(c) = payload.get("cwd").and_then(Value::as_str) {
                            meta.cwd = Some(c.to_string());
                        }
                    }
                    Some("event_msg") => match payload.get("type").and_then(Value::as_str) {
                        Some("user_message") => {
                            let text = payload.get("message").and_then(Value::as_str).unwrap_or("");
                            let ex = excerpt(text, self.prompt_excerpt_chars);
                            events.push(self.event(
                                &meta,
                                &file_id,
                                &source_file,
                                line.offset,
                                at,
                                AgentEventKind::PromptSubmit,
                                ex,
                            ));
                        }
                        Some("task_complete") | Some("turn_complete") => {
                            let ev = self.event(
                                &meta,
                                &file_id,
                                &source_file,
                                line.offset,
                                at,
                                AgentEventKind::ResponseComplete,
                                None,
                            );
                            events.push(ev);
                        }
                        _ => {}
                    },
                    _ => {}
                }
            }
            // 旧形式の message 行は行ごとの時刻が無いので出さない（時刻の捏造を避ける）。
            // それ以外（function_call 等の旧形式 item、record_type 行）は無視。
        }
        if skipped > 0 {
            tracing::warn!(file = %source_file, skipped, "解析できない行をスキップしました");
        }
        Ok((events, next))
    }
}

impl CodexAdapter {
    #[allow(clippy::too_many_arguments)]
    fn event(
        &self,
        meta: &Meta,
        file_id: &Option<String>,
        source_file: &str,
        offset: u64,
        at: Millis,
        kind: AgentEventKind,
        prompt_excerpt: Option<String>,
    ) -> AgentEvent {
        AgentEvent {
            id: new_id(),
            at,
            agent: NAME.into(),
            session_id: meta
                .session_id
                .clone()
                .or_else(|| file_id.clone())
                .unwrap_or_else(|| source_file.to_string()),
            kind,
            cwd: meta.cwd.clone(),
            git_branch: meta.git_branch.clone(),
            prompt_excerpt,
            source_file: Some(source_file.to_string()),
            source_offset: Some(offset as i64),
        }
    }
}
