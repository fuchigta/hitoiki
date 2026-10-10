//! hitoiki-calendar: カレンダーアダプタ（14章）。初期実装は ICS URL。
//!
//! 取得（`IcsAdapter::fetch`）と解析（`parse_ics`）は分離している。
//! URL は秘密情報なので、ログ・エラーメッセージには source 名だけを出す。

mod ics;
mod parse;
mod tzmap;

pub use parse::parse_ics;
pub use tzmap::resolve_tzid;

use anyhow::{anyhow, Context};
use chrono_tz::Tz;
use hitoiki_core::config::{CalendarSource, Config};
use hitoiki_core::model::CalendarEvent;
use hitoiki_core::repo;
use hitoiki_core::timefmt::Range;
use hitoiki_core::Millis;
use rusqlite::Connection;
use std::io::Read;
use std::time::Duration;

/// 本文サイズの上限。
const MAX_BODY_BYTES: u64 = 32 * 1024 * 1024;

/// カレンダー取得元の共通インターフェース。
pub trait CalendarAdapter {
    fn name(&self) -> &str;
    /// `range` と重なる予定を取得する。
    fn fetch(&self, range: Range) -> anyhow::Result<Vec<CalendarEvent>>;
}

/// ICS URL から取得するアダプタ。
pub struct IcsAdapter {
    name: String,
    url: String,
    tz: Tz,
    fetched_at: Option<Millis>,
}

impl IcsAdapter {
    /// `tz` は浮動時刻・終日の解釈に使う（設定の `general.timezone`）。
    pub fn new(name: impl Into<String>, url: impl Into<String>, tz: Tz) -> Self {
        Self {
            name: name.into(),
            url: normalize_url(&url.into()),
            tz,
            fetched_at: None,
        }
    }

    /// `fetched_at` を固定する（未指定ならシステム時刻）。
    pub fn with_fetched_at(mut self, ms: Millis) -> Self {
        self.fetched_at = Some(ms);
        self
    }

    /// ICS 本文を取得する。エラーに URL を含めない。
    fn download(&self) -> anyhow::Result<String> {
        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(30))
            .user_agent("hitoiki-calendar")
            .build();
        let resp = agent.get(&self.url).call().map_err(|e| match e {
            ureq::Error::Status(code, _) => anyhow!("HTTP {code}"),
            // ureq のメッセージには URL が入りうるので種別だけ出す
            ureq::Error::Transport(t) => anyhow!("通信に失敗しました ({})", t.kind()),
        })?;
        let mut buf = Vec::new();
        resp.into_reader()
            .take(MAX_BODY_BYTES)
            .read_to_end(&mut buf)
            .map_err(|e| anyhow!("本文の読み込みに失敗しました ({:?})", e.kind()))?;
        Ok(String::from_utf8_lossy(&buf).into_owned())
    }
}

/// `webcal://` / `webcals://` を `https://` に読み替える。
fn normalize_url(url: &str) -> String {
    let u = url.trim();
    let lower = u.to_ascii_lowercase();
    for prefix in ["webcals://", "webcal://"] {
        if lower.starts_with(prefix) {
            return format!("https://{}", &u[prefix.len()..]);
        }
    }
    u.to_string()
}

impl CalendarAdapter for IcsAdapter {
    fn name(&self) -> &str {
        &self.name
    }

    fn fetch(&self, range: Range) -> anyhow::Result<Vec<CalendarEvent>> {
        let text = self
            .download()
            .with_context(|| format!("カレンダー {} の取得に失敗", self.name))?;
        let fetched_at = self
            .fetched_at
            .unwrap_or_else(|| chrono::Utc::now().timestamp_millis());
        parse_ics(&text, &self.name, self.tz, range, fetched_at)
            .with_context(|| format!("カレンダー {} の解析に失敗", self.name))
    }
}

/// 同期結果。
#[derive(Debug, Default, Clone)]
pub struct SyncStats {
    /// 成功した source 数。
    pub sources_ok: usize,
    /// 保存した予定の総数。
    pub events_saved: usize,
    /// 失敗した source の (名前, 理由)。理由に URL は含まれない。
    pub errors: Vec<(String, String)>,
}

/// 設定中の全 source を `now` の前 `past_days` 〜 後 `future_days` で取得して保存する。
/// 1 つの source の失敗は他に影響せず、`SyncStats::errors` に集計する。
pub fn sync_all(conn: &Connection, cfg: &Config, now: Millis) -> hitoiki_core::Result<SyncStats> {
    let tz = cfg.tz()?;
    let day = 86_400_000;
    let range = Range::new(
        now - cfg.calendar.past_days * day,
        now + cfg.calendar.future_days * day,
    );
    let mut stats = SyncStats::default();
    for src in &cfg.calendar.sources {
        match sync_source(conn, src, tz, range, now) {
            Ok(n) => {
                stats.sources_ok += 1;
                stats.events_saved += n;
            }
            Err(e) => {
                tracing::warn!("カレンダー {} の同期に失敗: {e:#}", src.name);
                stats.errors.push((src.name.clone(), format!("{e:#}")));
            }
        }
    }
    Ok(stats)
}

fn sync_source(
    conn: &Connection,
    src: &CalendarSource,
    tz: Tz,
    range: Range,
    now: Millis,
) -> anyhow::Result<usize> {
    let adapter: Box<dyn CalendarAdapter> = match src.kind.as_str() {
        "ics" => Box::new(IcsAdapter::new(&src.name, &src.url, tz).with_fetched_at(now)),
        other => return Err(anyhow!("未対応のカレンダー種別: {other}")),
    };
    let events = adapter.fetch(range)?;
    repo::calendar::replace_source_range(conn, adapter.name(), range, &events)
        .map_err(|e| anyhow!("保存に失敗しました: {e}"))?;
    Ok(events.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn webcal_is_rewritten() {
        assert_eq!(
            normalize_url("webcal://x.example/a.ics"),
            "https://x.example/a.ics"
        );
        assert_eq!(normalize_url("WEBCALS://x/a"), "https://x/a");
        assert_eq!(normalize_url("https://x/a"), "https://x/a");
    }

    #[test]
    fn failure_is_isolated_and_has_no_url() {
        let conn = hitoiki_core::db::open_in_memory().unwrap();
        let mut cfg = Config::default();
        cfg.calendar.sources = vec![CalendarSource {
            name: "bad".into(),
            kind: "unknown".into(),
            url: "https://secret.example/token123.ics".into(),
        }];
        let stats = sync_all(&conn, &cfg, 0).unwrap();
        assert_eq!(stats.sources_ok, 0);
        assert_eq!(stats.errors.len(), 1);
        assert!(!stats.errors[0].1.contains("token123"));
    }
}
