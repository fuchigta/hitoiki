//! 診断（`hitoiki doctor`、docs/dev/architecture.md「CLI」）。各項目を ok / warn / error と日本語メッセージで返す。
//!
//! ネットワークへの実アクセスは `DoctorOptions::network` が true のときだけ行う。
//! ウインドウタイトルやカレンダー URL などの内容はメッセージに含めない。

use hitoiki_agents::adapters_from_config;
use hitoiki_core::config::{Config, JudgeBackend};
use hitoiki_core::{paths, Millis};
use hitoiki_ipc::{IpcError, Message, PipeName};
use rusqlite::{Connection, OpenFlags, OptionalExtension};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Ok,
    Warn,
    Error,
}

#[derive(Debug, Clone, Serialize)]
pub struct DoctorItem {
    /// 項目名（config / db / collect / agent / calendar / notify / judge / ai）。
    pub name: String,
    pub status: Status,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DoctorReport {
    pub items: Vec<DoctorItem>,
}

impl DoctorReport {
    /// 最も悪い状態。
    pub fn overall(&self) -> Status {
        self.items
            .iter()
            .map(|i| i.status)
            .fold(Status::Ok, |a, b| match (a, b) {
                (Status::Error, _) | (_, Status::Error) => Status::Error,
                (Status::Warn, _) | (_, Status::Warn) => Status::Warn,
                _ => Status::Ok,
            })
    }

    pub fn get(&self, name: &str) -> Option<&DoctorItem> {
        self.items.iter().find(|i| i.name == name)
    }
}

#[derive(Debug, Clone, Default)]
pub struct DoctorOptions {
    /// true でカレンダー取得・判定器への疎通を実際に試す（`--network`）。
    pub network: bool,
    /// 常駐プロセスへの Ping 先。None なら現在のユーザーの既定名。
    pub pipe_name: Option<PipeName>,
}

/// 既定のオプション（ネットワークなし）で診断する。
pub fn doctor(cfg: &Config) -> DoctorReport {
    doctor_with(cfg, &DoctorOptions::default())
}

pub fn doctor_with(cfg: &Config, opts: &DoctorOptions) -> DoctorReport {
    let mut items = vec![check_config()];
    let db = open_ro();
    items.push(check_db(db.as_ref()));
    items.push(check_collect(db.as_ref(), opts));
    items.push(check_agent(cfg, db.as_ref()));
    items.push(check_calendar(cfg, opts));
    items.push(DoctorItem {
        name: "notify".into(),
        status: Status::Ok,
        message: "常駐アプリ側で確認してください（ヘッドレスでは通知を出しません）".into(),
    });
    items.push(check_judge(cfg, opts));
    items.push(check_ai(cfg));
    DoctorReport { items }
}

fn item(name: &str, status: Status, message: impl Into<String>) -> DoctorItem {
    DoctorItem {
        name: name.into(),
        status,
        message: message.into(),
    }
}

fn check_config() -> DoctorItem {
    let path = paths::config_file();
    if !path.exists() {
        return item(
            "config",
            Status::Ok,
            format!("設定ファイルはありません（既定値を使用）: {}", path.display()),
        );
    }
    match Config::load_from(&path) {
        Ok(_) => item(
            "config",
            Status::Ok,
            format!("読み込めました: {}", path.display()),
        ),
        Err(e) => item("config", Status::Error, e.to_string()),
    }
}

/// 読み取り専用で DB を開く（無ければ None。診断で DB を作らない）。
fn open_ro() -> Option<Connection> {
    let p = paths::db_file();
    if !p.exists() {
        return None;
    }
    Connection::open_with_flags(
        &p,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .ok()
}

fn check_db(db: Option<&Connection>) -> DoctorItem {
    let path = paths::db_file();
    let Some(conn) = db else {
        return if path.exists() {
            item(
                "db",
                Status::Error,
                format!("DB を開けません: {}", path.display()),
            )
        } else {
            item(
                "db",
                Status::Warn,
                format!("DB がまだありません: {}", path.display()),
            )
        };
    };
    match conn.query_row("SELECT MAX(version) FROM schema_migration", [], |r| {
        r.get::<_, Option<i64>>(0)
    }) {
        Ok(v) => item(
            "db",
            Status::Ok,
            format!("{} （スキーマバージョン {}）", path.display(), v.unwrap_or(0)),
        ),
        Err(e) => item("db", Status::Error, format!("スキーマを確認できません: {e}")),
    }
}

fn age_text(now: Millis, at: Millis) -> String {
    let s = ((now - at).max(0)) / 1000;
    match s {
        0..=89 => format!("{s} 秒前"),
        90..=5399 => format!("{} 分前", s / 60),
        _ => format!("{} 時間前", s / 3600),
    }
}

fn check_collect(db: Option<&Connection>, opts: &DoctorOptions) -> DoctorItem {
    let now = chrono_now();
    let daemon = ping(opts);
    let mut parts = Vec::new();
    let mut status = Status::Ok;
    match db {
        None => {
            status = Status::Warn;
            parts.push("収集データがありません".to_string());
        }
        Some(c) => {
            let last_span: Option<Millis> = c
                .query_row("SELECT MAX(start_at) FROM window_span", [], |r| r.get(0))
                .ok()
                .flatten();
            parts.push(match last_span {
                Some(t) => format!("直近のウインドウ記録 {}", age_text(now, t)),
                None => "ウインドウ記録なし".into(),
            });
            let hb = hitoiki_core::repo::collect::get_heartbeat(c).ok().flatten();
            match hb {
                Some(t) => {
                    parts.push(format!("ハートビート {}", age_text(now, t)));
                    if now - t > 60_000 && daemon == Some(true) {
                        status = Status::Warn;
                    }
                }
                None => {
                    parts.push("ハートビートなし".into());
                    status = Status::Warn;
                }
            }
            if let Some((msg, warn)) = gap_summary(c, now) {
                parts.push(msg);
                if warn {
                    status = Status::Warn;
                }
            }
        }
    }
    parts.push(match daemon {
        Some(true) => "常駐プロセス: 応答あり".into(),
        Some(false) => {
            status = Status::Warn;
            "常駐プロセス: 起動していません（収集は常駐中のみ行われます）".into()
        }
        None => {
            status = Status::Warn;
            "常駐プロセス: 確認できません".into()
        }
    });
    item("collect", status, parts.join(" / "))
}

/// 診断で見る期間。
const DAY_MS: Millis = 24 * 60 * 60 * 1000;
/// これ以上の記録停止があれば警告にする（再起動や短い中断では警告しない）。
const GAP_WARN_MS: Millis = 10 * 60 * 1000;

/// 直近 24 時間の記録停止（収集器が動いていなかった時間帯）の要約。メッセージと、警告にするか。
fn gap_summary(conn: &Connection, now: Millis) -> Option<(String, bool)> {
    use hitoiki_core::recording;
    let range = hitoiki_core::timefmt::Range::new(now - DAY_MS, now);
    let gaps = recording::gaps(conn, range, now).ok()?;
    let total: Millis = gaps.iter().map(|g| g.duration_ms()).sum();
    if total == 0 {
        return Some(("直近 24 時間の記録停止なし".into(), false));
    }
    let longest = gaps.iter().max_by_key(|g| g.duration_ms())?;
    Some((
        format!(
            "直近 24 時間に {} 分の記録停止（{} 回。最長 {} 分、{}）。PC を終了していた時間を含みます。詳しくは `hitoiki log` の「記録なし」とログを参照",
            total / 60_000,
            gaps.len(),
            longest.duration_ms() / 60_000,
            age_text(now, longest.start_at).replace("前", "前から"),
        ),
        total >= GAP_WARN_MS,
    ))
}

fn ping(opts: &DoctorOptions) -> Option<bool> {
    let name = match opts.pipe_name.clone() {
        Some(n) => n,
        None => PipeName::for_current_user().ok()?,
    };
    match hitoiki_ipc::send(&name, &Message::Ping, Duration::from_millis(800)) {
        Ok(r) => Some(r.ok),
        Err(IpcError::NotRunning) => Some(false),
        Err(_) => None,
    }
}

fn chrono_now() -> Millis {
    use hitoiki_core::Clock;
    hitoiki_core::SystemClock.now_ms()
}

fn count_jsonl(dir: &Path) -> usize {
    let Ok(rd) = std::fs::read_dir(dir) else { return 0 };
    rd.flatten()
        .map(|e| {
            let p = e.path();
            match e.file_type() {
                Ok(t) if t.is_dir() => count_jsonl(&p),
                Ok(t) if t.is_file() && p.extension().is_some_and(|x| x == "jsonl") => 1,
                _ => 0,
            }
        })
        .sum()
}

fn check_agent(cfg: &Config, db: Option<&Connection>) -> DoctorItem {
    let adapters = adapters_from_config(cfg);
    if adapters.is_empty() {
        return item(
            "agent",
            Status::Ok,
            "有効なエージェントがありません（agent.enabled が空）",
        );
    }
    let now = chrono_now();
    let mut status = Status::Ok;
    let mut parts = Vec::new();
    for a in &adapters {
        let roots: Vec<PathBuf> = a.log_roots();
        let found: Vec<&PathBuf> = roots.iter().filter(|r| r.is_dir()).collect();
        if found.is_empty() {
            status = Status::Warn;
            parts.push(format!("{}: ログのルートが見つかりません", a.name()));
            continue;
        }
        let files: usize = found.iter().map(|r| count_jsonl(r)).sum();
        let last: Option<Millis> = db.and_then(|c| {
            c.query_row(
                "SELECT MAX(updated_at) FROM agent_ingest_offset WHERE agent = ?1",
                [a.name()],
                |r| r.get(0),
            )
            .optional()
            .ok()
            .flatten()
            .flatten()
        });
        parts.push(format!(
            "{}: ログ {} 件、最終取り込み {}",
            a.name(),
            files,
            last.map_or("なし".to_string(), |t| age_text(now, t))
        ));
    }
    if let Some(c) = db {
        if let Some((msg, warn)) = unbound_summary(c, cfg, now) {
            parts.push(msg);
            if warn {
                status = Status::Warn;
            }
        }
    }
    item("agent", status, parts.join(" / "))
}

/// 直近 24 時間のプロンプトのうち、ウインドウに対応付けできなかったものの要約（前面の exe 名のみ。タイトルは出さない）。
fn unbound_summary(conn: &Connection, cfg: &Config, now: Millis) -> Option<(String, bool)> {
    let range = hitoiki_core::timefmt::Range::new(now - DAY_MS, now);
    let total: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM agent_event WHERE kind = 'prompt_submit' AND at >= ?1 AND at < ?2",
            [range.start, range.end],
            |r| r.get(0),
        )
        .ok()?;
    if total == 0 {
        return None;
    }
    let unbound = hitoiki_core::rebuild::unbound_prompts(conn, cfg, range, now).ok()?;
    if unbound.is_empty() {
        return Some((
            format!("直近 24 時間のプロンプト {total} 件はすべてウインドウに対応付け済み"),
            false,
        ));
    }
    let mut by_exe: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    for u in &unbound {
        *by_exe
            .entry(u.exe_name.clone().unwrap_or_else(|| "(前面の記録なし)".into()))
            .or_default() += 1;
    }
    let mut top: Vec<(String, usize)> = by_exe.into_iter().collect();
    top.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let top: Vec<String> = top.iter().take(3).map(|(e, n)| format!("{e} {n} 件")).collect();
    Some((
        format!(
            "直近 24 時間のプロンプト {total} 件のうち {} 件がウインドウに対応付けできていません（送信時の前面: {}）。対象アプリを collect.agent_host_exes（または collect.terminal_exes）に追加してください",
            unbound.len(),
            top.join("、")
        ),
        true,
    ))
}

fn check_calendar(cfg: &Config, opts: &DoctorOptions) -> DoctorItem {
    let n = cfg.calendar.sources.len();
    if n == 0 {
        return item("calendar", Status::Ok, "カレンダーは未設定です");
    }
    if !opts.network {
        return item(
            "calendar",
            Status::Ok,
            format!("ソース {n} 件（取得確認は --network で実行）"),
        );
    }
    let tz = match cfg.tz() {
        Ok(t) => t,
        Err(e) => return item("calendar", Status::Error, e.to_string()),
    };
    let now = chrono_now();
    let day = 86_400_000;
    let range = hitoiki_core::timefmt::Range::new(now - day, now + 7 * day);
    let mut status = Status::Ok;
    let mut parts = Vec::new();
    for src in &cfg.calendar.sources {
        use hitoiki_calendar::CalendarAdapter;
        let r = match src.kind.as_str() {
            "ics" => hitoiki_calendar::IcsAdapter::new(&src.name, &src.url, tz).fetch(range),
            other => Err(anyhow::anyhow!("未対応の種別: {other}")),
        };
        match r {
            // エラーメッセージは URL を含まない（hitoiki-calendar 側で除去済み）
            Ok(ev) => parts.push(format!("{}: 取得できました（予定 {} 件）", src.name, ev.len())),
            Err(e) => {
                status = Status::Error;
                parts.push(format!("{}: {e:#}", src.name));
            }
        }
    }
    item("calendar", status, parts.join(" / "))
}

/// PATH（Windows は PATHEXT も）または明示パスから実行ファイルを探す。
pub(crate) fn find_in_path(cmd: &str) -> Option<PathBuf> {
    let p = Path::new(cmd);
    if p.components().count() > 1 || p.is_absolute() {
        return p.is_file().then(|| p.to_path_buf());
    }
    let exts: Vec<String> = if cfg!(windows) {
        std::env::var("PATHEXT")
            .unwrap_or_else(|_| ".EXE;.CMD;.BAT;.COM".into())
            .split(';')
            .map(|s| s.to_string())
            .collect()
    } else {
        vec![]
    };
    for dir in std::env::split_paths(&std::env::var_os("PATH")?) {
        let base = dir.join(cmd);
        if base.is_file() {
            return Some(base);
        }
        for e in &exts {
            let c = dir.join(format!("{cmd}{e}"));
            if c.is_file() {
                return Some(c);
            }
        }
    }
    None
}

fn check_judge(cfg: &Config, opts: &DoctorOptions) -> DoctorItem {
    if !cfg.judge.enabled {
        return item("judge", Status::Ok, "判定器は無効です");
    }
    let name = &cfg.judge.default;
    let Some(backend) = cfg.judge.backends.get(name) else {
        return item(
            "judge",
            Status::Error,
            format!("judge.default「{name}」が judge.backends にありません"),
        );
    };
    match backend {
        JudgeBackend::ExternalCommand { command, .. } => match command.first() {
            None => item("judge", Status::Error, format!("「{name}」の command が空です")),
            Some(exe) => match find_in_path(exe) {
                Some(_) => item(
                    "judge",
                    Status::Ok,
                    format!("「{name}」: 実行ファイル {exe} を確認しました"),
                ),
                None => item(
                    "judge",
                    Status::Error,
                    format!("「{name}」: 実行ファイル {exe} が見つかりません"),
                ),
            },
        },
        JudgeBackend::OpenaiCompatible {
            base_url,
            api_key_env,
            ..
        } => {
            let mut status = Status::Ok;
            let mut parts = Vec::new();
            match api_key_env {
                Some(var) if std::env::var_os(var).is_some_and(|v| !v.is_empty()) => {
                    parts.push(format!("API キー（環境変数 {var}）あり"));
                }
                Some(var) => {
                    status = Status::Error;
                    parts.push(format!("環境変数 {var} が設定されていません"));
                }
                None => parts.push("API キーは未指定（認証なし）".into()),
            }
            if opts.network {
                let agent = ureq::AgentBuilder::new().timeout(Duration::from_secs(5)).build();
                match agent.get(base_url).call() {
                    // 認証エラーや 404 でもサーバには到達できている
                    Ok(_) | Err(ureq::Error::Status(..)) => parts.push("接続できました".into()),
                    Err(ureq::Error::Transport(t)) => {
                        status = Status::Error;
                        parts.push(format!("接続できません ({})", t.kind()));
                    }
                }
            } else {
                parts.push("疎通確認は --network で実行".into());
            }
            item("judge", status, format!("「{name}」: {}", parts.join("、")))
        }
    }
}

fn check_ai(cfg: &Config) -> DoctorItem {
    match cfg.ai.command.first() {
        None => item("ai", Status::Warn, "ai.command が空です"),
        Some(exe) => match find_in_path(exe) {
            Some(_) => item("ai", Status::Ok, format!("{exe} が見つかりました")),
            None => item("ai", Status::Warn, format!("{exe} が PATH 上に見つかりません")),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overall_takes_worst() {
        let r = DoctorReport {
            items: vec![item("a", Status::Ok, ""), item("b", Status::Warn, "")],
        };
        assert_eq!(r.overall(), Status::Warn);
        let r = DoctorReport {
            items: vec![item("a", Status::Error, ""), item("b", Status::Warn, "")],
        };
        assert_eq!(r.overall(), Status::Error);
    }

    #[test]
    fn judge_checks() {
        let mut cfg = Config::default();
        assert_eq!(check_judge(&cfg, &DoctorOptions::default()).status, Status::Ok);
        cfg.judge.enabled = true;
        assert_eq!(check_judge(&cfg, &DoctorOptions::default()).status, Status::Error);
        let cfg: Config = toml_cfg(
            r#"
[judge]
enabled = true
default = "x"
[judge.backends.x]
type = "external-command"
command = ["definitely-not-a-real-command-xyz"]
"#,
        );
        assert_eq!(check_judge(&cfg, &DoctorOptions::default()).status, Status::Error);
        let cfg: Config = toml_cfg(
            r#"
[judge]
enabled = true
default = "x"
[judge.backends.x]
type = "openai-compatible"
base_url = "http://localhost:1/v1"
model = "m"
api_key_env = "HITOIKI_TEST_NO_SUCH_KEY_VAR"
"#,
        );
        let it = check_judge(&cfg, &DoctorOptions::default());
        assert_eq!(it.status, Status::Error);
        assert!(it.message.contains("HITOIKI_TEST_NO_SUCH_KEY_VAR"));
    }

    fn toml_cfg(s: &str) -> Config {
        // hitoiki-core の Config は serde 対応。toml は core 経由で使えないため JSON 風に組み立てず、
        // 一時ファイル経由で読む。
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("c.toml");
        std::fs::write(&p, s).unwrap();
        Config::load_from(&p).unwrap()
    }

    #[test]
    fn gap_summary_reports_stopped_minutes() {
        use hitoiki_core::repo::collect as repo;
        let c = hitoiki_core::db::open_in_memory().unwrap();
        let now = 10 * 60 * 60_000;
        // 稼働記録なし
        assert!(gap_summary(&c, now).map(|(_, w)| !w).unwrap_or(true));
        // 5 時間前〜3 時間前に動いて停止、その後 2 時間前から現在まで稼働
        repo::begin_run(&c, now - 5 * 3_600_000).unwrap();
        repo::touch_run(&c, now - 3 * 3_600_000, true).unwrap();
        repo::begin_run(&c, now - 2 * 3_600_000).unwrap();
        repo::touch_run(&c, now - 5_000, false).unwrap();
        let (msg, warn) = gap_summary(&c, now).unwrap();
        assert!(warn, "{msg}");
        assert!(msg.contains("60 分の記録停止"), "{msg}");
    }

    #[test]
    fn unbound_summary_shows_exe_not_title() {
        let c = hitoiki_core::db::open_in_memory().unwrap();
        let cfg = Config::default();
        let now = 10 * 60 * 60_000;
        c.execute(
            "INSERT INTO window_span (id, start_at, end_at, hwnd, exe_name, title, excluded) VALUES ('W', ?1, ?2, 'H', 'chrome.exe', '秘密のタイトル', 0)",
            [now - 3_600_000, now],
        )
        .unwrap();
        c.execute(
            "INSERT INTO agent_event (id, at, agent, session_id, kind) VALUES ('E', ?1, 'claude-code', 'S', 'prompt_submit')",
            [now - 1_800_000],
        )
        .unwrap();
        let (msg, warn) = unbound_summary(&c, &cfg, now).unwrap();
        assert!(warn);
        assert!(
            msg.contains("1 件のうち 1 件") || msg.contains("1 件のうち"),
            "{msg}"
        );
        assert!(msg.contains("chrome.exe 1 件"), "{msg}");
        assert!(!msg.contains("秘密"), "{msg}");
    }

    #[test]
    fn ai_missing_command_warns() {
        let mut cfg = Config::default();
        cfg.ai.command = vec!["definitely-not-a-real-command-xyz".into()];
        assert_eq!(check_ai(&cfg).status, Status::Warn);
    }

    #[test]
    fn calendar_without_network_does_not_fetch() {
        let mut cfg = Config::default();
        cfg.calendar.sources = vec![hitoiki_core::config::CalendarSource {
            name: "w".into(),
            kind: "ics".into(),
            url: "http://127.0.0.1:1/secret.ics".into(),
        }];
        let it = check_calendar(&cfg, &DoctorOptions::default());
        assert_eq!(it.status, Status::Ok);
        assert!(!it.message.contains("secret"));
    }
}
