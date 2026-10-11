//! 最新リリースの取得と、定期確認 1 回分の処理（docs/dev/architecture.md「更新の確認」）。
//!
//! `hitoiki version --check`（CLI）と常駐ランタイムの両方がここを使う。
//! 送るのは GitHub API への GET だけで、User-Agent は `hitoiki/<version>`。識別子や利用状況は送らない。
//! 自動ダウンロード・自動インストールはしない。

use crate::sink::RuntimeSink;
use anyhow::Context;
use hitoiki_core::service::update;
use hitoiki_core::Millis;
use rusqlite::Connection;
use serde::Deserialize;
use std::time::Duration;

/// 最新リリースを返す GitHub API。
pub const RELEASES_API_URL: &str = "https://api.github.com/repos/fuchigta/hitoiki/releases/latest";

/// 取得に失敗した確認を再試行するまでの待ち時間。
pub const RETRY_AFTER: Duration = Duration::from_secs(60 * 60);

/// 取得した最新リリース。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LatestRelease {
    /// タグ名（例: `v0.3.0`）。
    pub tag: String,
    /// リリースページの URL。
    pub url: String,
}

#[derive(Deserialize)]
struct ApiRelease {
    tag_name: String,
    html_url: Option<String>,
}

/// 最新リリースを GitHub から取得する。公開されたリリースがなければ `Ok(None)`。
/// `current` は User-Agent にだけ使う。
pub fn fetch_latest(current: &str) -> anyhow::Result<Option<LatestRelease>> {
    let resp = ureq::get(RELEASES_API_URL)
        .set("User-Agent", &format!("hitoiki/{current}"))
        .set("Accept", "application/vnd.github+json")
        .timeout(Duration::from_secs(10))
        .call();
    match resp {
        Ok(r) => {
            let body: ApiRelease = r.into_json().context("応答を解釈できません")?;
            Ok(Some(LatestRelease {
                tag: body.tag_name,
                url: body
                    .html_url
                    .unwrap_or_else(|| update::RELEASE_PAGE_URL.to_string()),
            }))
        }
        Err(ureq::Error::Status(404, _)) => Ok(None),
        Err(e) => Err(anyhow::anyhow!("最新版を確認できません: {e}")),
    }
}

/// 定期確認 1 回の結果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckOutcome {
    /// 取得に失敗した（何も保存せず、後で再試行する）。
    Failed,
    /// 現在が最新（または公開リリースなし）。
    UpToDate,
    /// 新版があるが通知済み。
    AlreadyNotified(String),
    /// 新版を通知した（バージョンは `v` なし）。
    Notified(String),
}

/// 確認を 1 回行い、結果を保存して、必要なら `sink` に通知する。
///
/// `fetch` は最新リリースの取得（テストでは偽物を渡す）。取得失敗は静かに `Failed` を返し、
/// 通知もしない。時刻は呼び出し側が `Clock` から渡す。
pub fn check_once(
    conn: &Connection,
    now: Millis,
    current: &str,
    fetch: impl FnOnce() -> anyhow::Result<Option<LatestRelease>>,
    sink: &dyn RuntimeSink,
) -> anyhow::Result<CheckOutcome> {
    let latest = match fetch() {
        Ok(l) => l,
        Err(e) => {
            // URL 以外の詳細はログに出してよいが、利用者には通知しない。
            tracing::info!(error = %e, "新しいバージョンの確認に失敗しました");
            return Ok(CheckOutcome::Failed);
        }
    };
    let Some(rel) = latest else {
        update::record_check(conn, now, None)?;
        return Ok(CheckOutcome::UpToDate);
    };
    update::record_check(conn, now, Some(&rel.tag))?;
    let version = update::normalize(&rel.tag);
    if !update::is_newer(&version, current) {
        return Ok(CheckOutcome::UpToDate);
    }
    let state = update::load(conn)?;
    if !update::should_notify(&version, current, state.notified_version.as_deref()) {
        return Ok(CheckOutcome::AlreadyNotified(version));
    }
    sink.update_available(&version, &rel.url);
    update::record_notified(conn, &version)?;
    Ok(CheckOutcome::Notified(version))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sink::LogSink;
    use hitoiki_core::db;
    use std::sync::Mutex;

    #[derive(Default)]
    struct Rec(Mutex<Vec<String>>);

    impl RuntimeSink for Rec {
        fn segments_updated(&self, r: hitoiki_core::timefmt::Range) {
            LogSink.segments_updated(r)
        }
        fn push(&self, id: &str, p: &hitoiki_core::push::PushPayload) {
            LogSink.push(id, p)
        }
        fn doing_changed(&self, t: Option<&str>) {
            LogSink.doing_changed(t)
        }
        fn import_completed(&self, s: &hitoiki_agents::ImportStats) {
            LogSink.import_completed(s)
        }
        fn open_ui(&self, s: Option<&str>) {
            LogSink.open_ui(s)
        }
        fn shutdown_requested(&self) {}
        fn update_available(&self, version: &str, _url: &str) {
            self.0.lock().unwrap().push(version.to_string());
        }
    }

    fn rel(tag: &str) -> anyhow::Result<Option<LatestRelease>> {
        Ok(Some(LatestRelease {
            tag: tag.into(),
            url: "https://example.invalid/r".into(),
        }))
    }

    #[test]
    fn 新版は一度だけ通知する() {
        let conn = db::open_in_memory().unwrap();
        let sink = Rec::default();
        let o = check_once(&conn, 1000, "0.2.0", || rel("v0.3.0"), &sink).unwrap();
        assert_eq!(o, CheckOutcome::Notified("0.3.0".into()));
        let o = check_once(&conn, 2000, "0.2.0", || rel("v0.3.0"), &sink).unwrap();
        assert_eq!(o, CheckOutcome::AlreadyNotified("0.3.0".into()));
        assert_eq!(*sink.0.lock().unwrap(), vec!["0.3.0".to_string()]);
        // さらに新しい版は、あらためて通知する。
        let o = check_once(&conn, 3000, "0.2.0", || rel("v0.4.0"), &sink).unwrap();
        assert_eq!(o, CheckOutcome::Notified("0.4.0".into()));
        let st = update::load(&conn).unwrap();
        assert_eq!(st.last_checked_at, Some(3000));
        assert_eq!(st.latest_version.as_deref(), Some("0.4.0"));
    }

    #[test]
    fn 最新なら通知しない() {
        let conn = db::open_in_memory().unwrap();
        let sink = Rec::default();
        let o = check_once(&conn, 1000, "0.3.0", || rel("v0.3.0"), &sink).unwrap();
        assert_eq!(o, CheckOutcome::UpToDate);
        let o = check_once(&conn, 2000, "0.3.0", || Ok(None), &sink).unwrap();
        assert_eq!(o, CheckOutcome::UpToDate);
        assert!(sink.0.lock().unwrap().is_empty());
        assert_eq!(update::load(&conn).unwrap().last_checked_at, Some(2000));
    }

    #[test]
    fn 取得失敗は静かに失敗し何も保存しない() {
        let conn = db::open_in_memory().unwrap();
        let sink = Rec::default();
        let o = check_once(&conn, 1000, "0.2.0", || Err(anyhow::anyhow!("offline")), &sink).unwrap();
        assert_eq!(o, CheckOutcome::Failed);
        assert!(sink.0.lock().unwrap().is_empty());
        assert_eq!(update::load(&conn).unwrap(), update::UpdateState::default());
    }
}
