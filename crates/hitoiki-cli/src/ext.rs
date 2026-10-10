//! rebuild / agent / ingest / native-host / version --check の実装。

use crate::args::*;
use crate::native_host;
use crate::table::{table, truncate};
use crate::{emit, range_args};
use hitoiki_agents::{AgentAdapter, ClaudeCodeAdapter, CodexAdapter, ImportStats};
use hitoiki_core::config::Config;
use hitoiki_core::model::{SegmentKind, TaskStatus};
use hitoiki_core::repo;
use hitoiki_core::service::{ingest, Ctx};
use hitoiki_core::timefmt::{self, fmt_duration, fmt_rfc3339, to_local, Range};
use hitoiki_core::{db, paths, Error, Result, SystemClock};
use hitoiki_ipc::Message;
use serde_json::json;
use std::io::Read;
use std::time::Duration;

/// 失敗しても無視するデーモンへの通知（設計 3.3）。
pub fn notify_data(what: &str) {
    hitoiki_ipc::notify_best_effort(&Message::DataChanged { what: what.into() });
}

pub fn notify_task(task_id: Option<&str>) {
    hitoiki_ipc::notify_best_effort(&Message::TaskChanged {
        task_id: task_id.map(str::to_string),
    });
}

/// 前後に広げる量（判定器へ渡すカレンダーの範囲）。
const CAL_MARGIN_MS: i64 = 24 * 60 * 60 * 1000;

pub fn rebuild(from: Option<String>, to: Option<String>, date: Option<String>, json: bool) -> Result<()> {
    let cfg = Config::load()?;
    let mut conn = db::open(&paths::db_file())?;
    let clock = SystemClock;
    let (tz, range, now, judge) = {
        let ctx = Ctx::new(&conn, &cfg, &clock);
        let tz = ctx.tz()?;
        let now = ctx.now();
        let range = ctx.range(&hitoiki_core::service::RangeArgs { date, from, to })?;
        let judge = if cfg.judge.enabled {
            let mut jc = hitoiki_judge::JudgeContext::new(tz);
            jc.open_tasks = repo::task::list(&conn, &[TaskStatus::Todo, TaskStatus::Doing], None)?;
            jc.doing_task = repo::task::list_doing(&conn)?.into_iter().next();
            jc.skills = repo::skill::list(&conn)?;
            jc.calendar = repo::calendar::list(
                &conn,
                Range::new(range.start - CAL_MARGIN_MS, range.end + CAL_MARGIN_MS),
            )?;
            hitoiki_judge::from_config(&cfg, jc).map_err(Error::Other)?
        } else {
            None
        };
        (tz, range, now, judge)
    };
    let stats = hitoiki_core::rebuild::rebuild_with_judge(&mut conn, &cfg, range, now, judge.as_deref())?;
    notify_data("segments");
    let v = json!({
        "range": {
            "start_at": stats.range.start, "end_at": stats.range.end,
            "start": fmt_rfc3339(tz, stats.range.start), "end": fmt_rfc3339(tz, stats.range.end),
        },
        "segments": stats.segments,
        "bindings": stats.bindings,
        "judge": judge.is_some(),
    });
    emit(json, &v, || {
        format!(
            "再計算しました: {} 〜 {}（区間 {} 件、セッション対応 {} 件{}）",
            to_local(tz, stats.range.start).format("%Y-%m-%d %H:%M"),
            to_local(tz, stats.range.end).format("%Y-%m-%d %H:%M"),
            stats.segments,
            stats.bindings,
            if judge.is_some() { "、判定器あり" } else { "" }
        )
    })
}

pub fn agent(cmd: AgentCmd, json: bool) -> Result<()> {
    match cmd {
        AgentCmd::Sessions { date } => agent_sessions(date, json),
        AgentCmd::Import { agent, since } => agent_import(agent, since, json),
    }
}

fn agent_sessions(date: Option<String>, json: bool) -> Result<()> {
    let cfg = Config::load()?;
    let conn = db::open(&paths::db_file())?;
    let clock = SystemClock;
    let ctx = Ctx::new(&conn, &cfg, &clock);
    let tz = ctx.tz()?;
    let range = ctx.range(&range_args(&RangeOpts {
        date,
        ..Default::default()
    }))?;
    let sessions = repo::agent::list_sessions(&conn, range)?;
    let segs = repo::segment::list(&conn, range)?;
    let total = |agent: &str, sid: &str, kind: SegmentKind| -> i64 {
        let key = format!("{agent}:{sid}");
        segs.iter()
            .filter(|s| s.kind == kind && s.agent_session_id.as_deref() == Some(key.as_str()))
            .map(|s| repo::segment::clipped_len(s, range))
            .sum()
    };
    let rows: Vec<serde_json::Value> = sessions
        .iter()
        .map(|s| {
            json!({
                "agent": s.agent,
                "session_id": s.session_id,
                "start_at": s.start_at, "end_at": s.end_at,
                "start": fmt_rfc3339(tz, s.start_at), "end": fmt_rfc3339(tz, s.end_at),
                "cwd": s.cwd, "git_branch": s.git_branch,
                "prompt_count": s.prompt_count,
                "dialog_ms": total(&s.agent, &s.session_id, SegmentKind::AgentDialog),
                "watch_ms": total(&s.agent, &s.session_id, SegmentKind::AgentWatch),
                "first_prompt": s.first_prompt,
            })
        })
        .collect();
    let v = json!({
        "range": {
            "start_at": range.start, "end_at": range.end,
            "start": fmt_rfc3339(tz, range.start), "end": fmt_rfc3339(tz, range.end),
        },
        "sessions": rows,
    });
    emit(json, &v, || {
        if rows.is_empty() {
            return "セッションはありません。".into();
        }
        let table_rows: Vec<Vec<String>> = sessions
            .iter()
            .zip(&rows)
            .map(|(s, r)| {
                vec![
                    s.agent.clone(),
                    s.session_id.chars().take(8).collect(),
                    format!(
                        "{}〜{}",
                        to_local(tz, s.start_at).format("%m-%d %H:%M"),
                        to_local(tz, s.end_at).format("%H:%M")
                    ),
                    truncate(s.cwd.as_deref().unwrap_or(""), 30),
                    s.git_branch.clone().unwrap_or_default(),
                    s.prompt_count.to_string(),
                    fmt_duration(r["dialog_ms"].as_i64().unwrap_or(0)),
                    fmt_duration(r["watch_ms"].as_i64().unwrap_or(0)),
                    truncate(s.first_prompt.as_deref().unwrap_or(""), 40),
                ]
            })
            .collect();
        table(
            &[
                "agent",
                "session",
                "期間",
                "cwd",
                "branch",
                "prompt",
                "対話",
                "見守り",
                "最初のプロンプト",
            ],
            &table_rows,
        )
    })
}

fn agent_import(agent: Option<String>, since: Option<String>, json: bool) -> Result<()> {
    let cfg = Config::load()?;
    let mut conn = db::open(&paths::db_file())?;
    let clock = SystemClock;
    let ctx = Ctx::new(&conn, &cfg, &clock);
    let tz = ctx.tz()?;
    let now = ctx.now();
    let since = since.map(|s| timefmt::parse_instant(tz, &s, now)).transpose()?;
    let stats: ImportStats = match agent.as_deref() {
        None => hitoiki_agents::import_all(&mut conn, &cfg, since)?,
        Some(name) => {
            let n = cfg.agent.prompt_excerpt_chars;
            let adapter: Box<dyn AgentAdapter> = match name {
                "claude-code" => Box::new(ClaudeCodeAdapter::new(n)),
                "codex" => Box::new(CodexAdapter::new(n)),
                other => {
                    return Err(Error::InvalidArgument(format!(
                        "未対応のエージェントです: {other}（claude-code | codex）"
                    )))
                }
            };
            hitoiki_agents::import_with(&mut conn, vec![adapter], since)?
        }
    };
    notify_data("agent_events");
    let v = json!({
        "files_scanned": stats.files_scanned,
        "files_updated": stats.files_updated,
        "events_parsed": stats.events_parsed,
        "events_inserted": stats.events_inserted,
        "files_failed": stats.files_failed,
    });
    emit(json, &v, || {
        format!(
            "取り込みました: ファイル {} 件走査（更新 {} 件、失敗 {} 件）、イベント {} 件解析（新規 {} 件）",
            stats.files_scanned,
            stats.files_updated,
            stats.files_failed,
            stats.events_parsed,
            stats.events_inserted
        )
    })
}

pub fn ingest_context(json: bool) -> Result<()> {
    let mut text = String::new();
    std::io::stdin().read_to_string(&mut text)?;
    let cfg = Config::load()?;
    let conn = db::open(&paths::db_file())?;
    let clock = SystemClock;
    let ctx = Ctx::new(&conn, &cfg, &clock);
    let r = ingest::ingest_json(&ctx, &text)?;
    emit(json, &r, || {
        format!("{} 件のコンテキストを保存しました。", r.saved)
    })
}

pub fn native_host(a: NativeHostArgs, json: bool) -> Result<()> {
    match a.action.as_deref() {
        Some("install") => {
            let r = native_host::install(&a.extension_id, &a.firefox_extension_id)?;
            emit(json, &r, || {
                format!(
                    "ネイティブメッセージングホストを登録しました。\n{}\n{}",
                    r.files.join("\n"),
                    r.registry.join("\n")
                )
            })
        }
        Some("uninstall") => {
            let r = native_host::uninstall()?;
            emit(json, &r, || {
                format!(
                    "ネイティブメッセージングホストの登録を解除しました（ファイル {} 件、キー {} 件）。",
                    r.files.len(),
                    r.registry.len()
                )
            })
        }
        other if native_host::is_host_invocation(other) => run_host(),
        Some(other) => Err(Error::InvalidArgument(format!(
            "不明な操作です: {other}（install | uninstall）"
        ))),
        None => unreachable!(),
    }
}

/// ネイティブメッセージングホストとして動作する。標準出力はフレーム専用。
fn run_host() -> Result<()> {
    let cfg = Config::load()?;
    let conn = db::open(&paths::db_file())?;
    let clock = SystemClock;
    let ctx = Ctx::new(&conn, &cfg, &clock);
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    native_host::serve(&mut stdin.lock(), &mut stdout.lock(), |text| {
        ingest::ingest_json(&ctx, text).map(|r| r.saved)
    })?;
    Ok(())
}

const RELEASES_URL: &str = "https://api.github.com/repos/fuchigta/hitoiki/releases/latest";

pub fn version_check(json: bool) -> Result<()> {
    let current = env!("CARGO_PKG_VERSION");
    let resp = ureq::get(RELEASES_URL)
        .set("User-Agent", &format!("hitoiki/{current}"))
        .set("Accept", "application/vnd.github+json")
        .timeout(Duration::from_secs(10))
        .call();
    let body: serde_json::Value = match resp {
        Ok(r) => r
            .into_json()
            .map_err(|e| Error::Other(anyhow::anyhow!("応答を解釈できません: {e}")))?,
        Err(ureq::Error::Status(404, _)) => {
            let v = json!({"current": current, "latest": null, "update_available": false});
            return emit(json, &v, || {
                format!("hitoiki {current}\n公開されているリリースはありません。")
            });
        }
        Err(e) => return Err(Error::Other(anyhow::anyhow!("最新版を確認できません: {e}"))),
    };
    let tag = body["tag_name"].as_str().unwrap_or("").to_string();
    let url = body["html_url"].as_str().map(str::to_string);
    let available = is_newer(&tag, current);
    let v = json!({"current": current, "latest": tag, "update_available": available, "url": url});
    emit(json, &v, || {
        if available {
            format!(
                "hitoiki {current}\n新しいバージョンがあります: {tag}\n{}",
                url.clone().unwrap_or_default()
            )
        } else {
            format!("hitoiki {current}\n最新です（最新リリース: {tag}）。")
        }
    })
}

fn parse_ver(s: &str) -> Option<Vec<u64>> {
    let s = s.trim().trim_start_matches('v');
    let core = s.split(['-', '+']).next()?;
    core.split('.').map(|p| p.parse().ok()).collect()
}

/// `latest` が `current` より新しいか（解釈できなければ false）。
pub fn is_newer(latest: &str, current: &str) -> bool {
    match (parse_ver(latest), parse_ver(current)) {
        (Some(mut a), Some(mut b)) => {
            let n = a.len().max(b.len());
            a.resize(n, 0);
            b.resize(n, 0);
            a > b
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_compare() {
        assert!(is_newer("v0.2.0", "0.1.0"));
        assert!(is_newer("0.1.1", "0.1.0"));
        assert!(!is_newer("v0.1.0", "0.1.0"));
        assert!(!is_newer("v0.1", "0.1.0"));
        assert!(is_newer("1.0.0", "0.9.9"));
        assert!(!is_newer("garbage", "0.1.0"));
    }
}
