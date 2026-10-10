//! Claude Code のセッションログ（`~/.claude/projects/<encoded>/<session>.jsonl`）。
//!
//! 実ファイルで確認した形式（13.2）:
//! - 1 行 1 JSON。`type` は user / assistant / system / attachment のほか、メタ行
//!   （summary, last-prompt, mode, ai-title, file-history-snapshot 等。timestamp なし）が混在する。
//! - 会話行は `timestamp`（RFC 3339）, `sessionId`, `cwd`, `gitBranch`, `isSidechain`, `isMeta` を持つ。
//! - ツール結果は `type=user` で `message.content` が `tool_result` 配列。人間の入力は
//!   `content` が文字列（または text/image の配列）で、新しい版では `origin.kind == "human"` が付く。
//! - 最終応答は `type=assistant` かつ `message.stop_reason == "end_turn"`。
//!   thinking と text が別行になるため、同じ `message.id` の end_turn 行は 1 件にまとめる
//!   （バッチ境界に依存しない決定的な処理。バッチをまたぐ重複はパイプライン側で除去する）。
//! - `entrypoint` は cli / claude-desktop（対話）/ sdk-cli 等（`claude -p` などの非対話）。
//!   `sdk-` で始まる行は Hitoiki 自身の AI 振り返り実行などを作業時間に混ぜないため、セッションごと除外する。
//! - サブエージェントのログは `subagents/` 配下（および `isSidechain=true`）。対象外。

use crate::common::{excerpt, parse_ts, read_complete_lines};
use crate::AgentAdapter;
use hitoiki_core::id::new_id;
use hitoiki_core::model::{AgentEvent, AgentEventKind};
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub const NAME: &str = "claude-code";

pub struct ClaudeCodeAdapter {
    root: Option<PathBuf>,
    prompt_excerpt_chars: usize,
}

impl ClaudeCodeAdapter {
    /// 既定の場所（`CLAUDE_CONFIG_DIR` または `~/.claude` の `projects`）を使う。
    pub fn new(prompt_excerpt_chars: usize) -> Self {
        let root = std::env::var_os("CLAUDE_CONFIG_DIR")
            .map(PathBuf::from)
            .or_else(|| dirs::home_dir().map(|h| h.join(".claude")))
            .map(|d| d.join("projects"));
        Self {
            root,
            prompt_excerpt_chars,
        }
    }

    /// 場所を指定する（テスト用）。
    pub fn with_root(root: PathBuf, prompt_excerpt_chars: usize) -> Self {
        Self {
            root: Some(root),
            prompt_excerpt_chars,
        }
    }
}

impl AgentAdapter for ClaudeCodeAdapter {
    fn name(&self) -> &str {
        NAME
    }

    fn log_roots(&self) -> Vec<PathBuf> {
        self.root.iter().cloned().collect()
    }

    fn accepts_file(&self, file: &Path) -> bool {
        !file.components().any(|c| c.as_os_str() == "subagents")
    }

    fn parse_incremental(&self, file: &Path, from_offset: u64) -> anyhow::Result<(Vec<AgentEvent>, u64)> {
        let (lines, next) = read_complete_lines(file, from_offset)?;
        let source_file = file.to_string_lossy().to_string();
        let stem = file
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        let mut events: Vec<AgentEvent> = Vec::new();
        let mut started = from_offset > 0;
        let mut skipped = 0usize;
        // message.id -> events 内の位置（同一メッセージの複数行を 1 件にする）。
        let mut completed: HashMap<String, usize> = HashMap::new();

        for line in lines {
            if line.bytes.iter().all(|b| b.is_ascii_whitespace()) {
                continue;
            }
            let Ok(v) = serde_json::from_slice::<Value>(&line.bytes) else {
                skipped += 1;
                continue;
            };
            let ty = v.get("type").and_then(Value::as_str).unwrap_or("");
            if !matches!(ty, "user" | "assistant" | "system" | "attachment") {
                continue; // メタ行
            }
            if v.get("isSidechain").and_then(Value::as_bool) == Some(true) {
                continue;
            }
            // 非対話実行（`claude -p` 等）はセッションごと対象外。
            if v.get("entrypoint")
                .and_then(Value::as_str)
                .is_some_and(|e| e.starts_with("sdk"))
            {
                continue;
            }
            let Some(at) = v.get("timestamp").and_then(Value::as_str).and_then(parse_ts) else {
                skipped += 1;
                continue;
            };
            let session_id = v
                .get("sessionId")
                .and_then(Value::as_str)
                .unwrap_or(&stem)
                .to_string();
            let cwd = v.get("cwd").and_then(Value::as_str).map(str::to_string);
            let git_branch = v
                .get("gitBranch")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty() && *s != "HEAD")
                .map(str::to_string);
            let mk = |kind, prompt_excerpt| AgentEvent {
                id: new_id(),
                at,
                agent: NAME.into(),
                session_id: session_id.clone(),
                kind,
                cwd: cwd.clone(),
                git_branch: git_branch.clone(),
                prompt_excerpt,
                source_file: Some(source_file.clone()),
                source_offset: Some(line.offset as i64),
            };

            if !started {
                started = true;
                events.push(mk(AgentEventKind::SessionStart, None));
            }

            match ty {
                "user" => {
                    if let Some(text) = human_prompt(&v) {
                        let ex = text.and_then(|t| excerpt(&t, self.prompt_excerpt_chars));
                        events.push(mk(AgentEventKind::PromptSubmit, ex));
                    }
                }
                "assistant" => {
                    let stop = v.pointer("/message/stop_reason").and_then(Value::as_str);
                    if matches!(stop, Some("end_turn") | Some("stop_sequence")) {
                        let ev = mk(AgentEventKind::ResponseComplete, None);
                        let mid = v.pointer("/message/id").and_then(Value::as_str);
                        match mid.and_then(|m| completed.get(m).copied()) {
                            Some(i) => {
                                if ev.at >= events[i].at {
                                    events[i] = ev;
                                }
                            }
                            None => {
                                if let Some(m) = mid {
                                    completed.insert(m.to_string(), events.len());
                                }
                                events.push(ev);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        if skipped > 0 {
            tracing::warn!(file = %source_file, skipped, "解析できない行をスキップしました");
        }
        Ok((events, next))
    }
}

/// 人間が入力した user 行なら `Some(抜粋元テキスト)`、そうでなければ None。
fn human_prompt(v: &Value) -> Option<Option<String>> {
    if v.get("isMeta").and_then(Value::as_bool) == Some(true) {
        return None;
    }
    if let Some(kind) = v.pointer("/origin/kind").and_then(Value::as_str) {
        if kind != "human" {
            return None;
        }
    }
    let content = v.pointer("/message/content")?;
    let text = match content {
        Value::String(s) => {
            if is_system_injected(s) {
                return None;
            }
            Some(command_or_text(s))
        }
        Value::Array(items) => {
            if items.is_empty() || items.iter().any(|i| i["type"] == "tool_result") {
                return None;
            }
            let mut parts = Vec::new();
            let mut has_input = false;
            for i in items {
                match i["type"].as_str() {
                    Some("text") => {
                        let t = i["text"].as_str().unwrap_or("");
                        if is_system_injected(t) || t.starts_with("<ide_") {
                            continue;
                        }
                        has_input = true;
                        parts.push(command_or_text(t));
                    }
                    Some("image") | Some("document") => has_input = true,
                    _ => {}
                }
            }
            if !has_input {
                return None;
            }
            let joined = parts.join(" ");
            (!joined.trim().is_empty()).then_some(joined)
        }
        _ => return None,
    };
    Some(text)
}

/// コマンド出力・システム挿入など、人間の入力ではない文字列。
fn is_system_injected(s: &str) -> bool {
    let t = s.trim_start();
    const PREFIXES: &[&str] = &[
        "<local-command-",
        "<bash-",
        "<task-notification",
        "<system-reminder",
        "<user-prompt-submit-hook",
        "[Request interrupted by user",
        "Caveat: The messages below were generated by the user while running local commands",
    ];
    PREFIXES.iter().any(|p| t.starts_with(p))
}

/// `<command-name>/x</command-name> ... <command-args>y</command-args>` 形式なら "/x y" に整形。
fn command_or_text(s: &str) -> String {
    let t = s.trim_start();
    if t.starts_with("<command-") {
        if let Some(name) = tag(t, "command-name") {
            return match tag(t, "command-args").filter(|a| !a.trim().is_empty()) {
                Some(args) => format!("{} {}", name.trim(), args.trim()),
                None => name.trim().to_string(),
            };
        }
    }
    s.to_string()
}

fn tag<'a>(s: &'a str, name: &str) -> Option<&'a str> {
    let open = format!("<{name}>");
    let close = format!("</{name}>");
    let a = s.find(&open)? + open.len();
    let b = s[a..].find(&close)? + a;
    Some(&s[a..b])
}
