//! 新しいバージョンの確認に使う、通信を伴わない部分（docs/dev/architecture.md「更新の確認」）。
//!
//! 取得（GitHub への問い合わせ）は `hitoiki-daemon` の `update` モジュールが行う。
//! ここにはバージョンの比較、確認・通知の要否の判定、`setting` テーブルへの保存だけを置く。
//! 時刻はすべて引数で受け取る。

use crate::{Millis, Result};
use rusqlite::{params, Connection, OptionalExtension};

/// 最後に確認した時刻（Unix ミリ秒）を保存する `setting` のキー。
pub const LAST_CHECKED_KEY: &str = "update.last_checked_at";
/// 最後に確認して分かった最新版（`v` なしのバージョン文字列）を保存するキー。
pub const LATEST_VERSION_KEY: &str = "update.latest_version";
/// 通知済みのバージョンを保存するキー。同じバージョンは 1 回だけ通知する。
pub const NOTIFIED_VERSION_KEY: &str = "update.notified_version";

/// 確認の間隔（24 時間）。
pub const CHECK_INTERVAL_MS: Millis = 24 * 60 * 60 * 1000;
/// 常駐の起動から最初の確認までの待ち時間（3 分）。
pub const STARTUP_DELAY_MS: Millis = 3 * 60 * 1000;

/// 最新リリースのページ（ダウンロード先）。
pub const RELEASE_PAGE_URL: &str = "https://github.com/fuchigta/hitoiki/releases/latest";

/// `v1.2.3` / `1.2.3-rc.1` / `1.2.3+meta` を数値の並びにする。プレリリース・ビルドメタデータは無視する。
fn parse_version(s: &str) -> Option<Vec<u64>> {
    let s = s.trim().trim_start_matches('v');
    let core = s.split(['-', '+']).next()?;
    core.split('.').map(|p| p.parse().ok()).collect()
}

/// `latest` が `current` より新しいか（どちらかを解釈できなければ false）。桁数が違えば 0 で埋めて比べる。
pub fn is_newer(latest: &str, current: &str) -> bool {
    match (parse_version(latest), parse_version(current)) {
        (Some(mut a), Some(mut b)) => {
            let n = a.len().max(b.len());
            a.resize(n, 0);
            b.resize(n, 0);
            a > b
        }
        _ => false,
    }
}

/// `v` 接頭辞と空白を除いたバージョン文字列。
pub fn normalize(tag: &str) -> String {
    tag.trim().trim_start_matches('v').to_string()
}

/// 確認の時期か。一度も確認していない、24 時間以上経った、または時計が戻っているとき true。
pub fn is_due(last_checked_at: Option<Millis>, now: Millis) -> bool {
    match last_checked_at {
        None => true,
        Some(last) => now < last || now - last >= CHECK_INTERVAL_MS,
    }
}

/// 次に確認するまでの待ち時間（ミリ秒）。時期になっていれば 0。
pub fn wait_until_due(last_checked_at: Option<Millis>, now: Millis) -> Millis {
    match last_checked_at {
        Some(last) if now >= last && now - last < CHECK_INTERVAL_MS => CHECK_INTERVAL_MS - (now - last),
        _ => 0,
    }
}

/// 通知すべきか。現在より新しく、かつまだ通知していないバージョンのとき true。
pub fn should_notify(latest: &str, current: &str, notified: Option<&str>) -> bool {
    is_newer(latest, current) && notified.map(normalize).as_deref() != Some(normalize(latest).as_str())
}

/// 保存済みの確認状態。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UpdateState {
    pub last_checked_at: Option<Millis>,
    pub latest_version: Option<String>,
    pub notified_version: Option<String>,
}

pub(crate) fn get(conn: &Connection, key: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row("SELECT value_json FROM setting WHERE key = ?1", [key], |r| {
            r.get(0)
        })
        .optional()?)
}

pub(crate) fn put(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO setting(key, value_json) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value_json = excluded.value_json",
        params![key, value],
    )?;
    Ok(())
}

/// 保存済みの確認状態を読む。
pub fn load(conn: &Connection) -> Result<UpdateState> {
    Ok(UpdateState {
        last_checked_at: get(conn, LAST_CHECKED_KEY)?.and_then(|s| s.trim().parse().ok()),
        latest_version: get(conn, LATEST_VERSION_KEY)?.filter(|s| !s.is_empty()),
        notified_version: get(conn, NOTIFIED_VERSION_KEY)?.filter(|s| !s.is_empty()),
    })
}

/// 確認を終えたことを記録する。`latest` は取得できた最新版（取得できなければ None で、前回の値を残す）。
pub fn record_check(conn: &Connection, now: Millis, latest: Option<&str>) -> Result<()> {
    put(conn, LAST_CHECKED_KEY, &now.to_string())?;
    if let Some(v) = latest {
        put(conn, LATEST_VERSION_KEY, &normalize(v))?;
    }
    Ok(())
}

/// 通知したことを記録する。
pub fn record_notified(conn: &Connection, version: &str) -> Result<()> {
    put(conn, NOTIFIED_VERSION_KEY, &normalize(version))
}

/// 保存済みの情報から、`current` より新しい版があればそのバージョン（`v` なし）を返す。ネットワークには出ない。
pub fn available(conn: &Connection, current: &str) -> Result<Option<String>> {
    Ok(load(conn)?.latest_version.filter(|v| is_newer(v, current)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    #[test]
    fn バージョン比較() {
        assert!(is_newer("v0.2.0", "0.1.0"));
        assert!(is_newer("0.1.1", "0.1.0"));
        assert!(!is_newer("v0.1.0", "0.1.0"));
        assert!(!is_newer("v0.1", "0.1.0"));
        assert!(is_newer("1.0.0", "0.9.9"));
        assert!(is_newer("0.10.0", "0.9.0"));
        assert!(!is_newer("garbage", "0.1.0"));
        assert!(!is_newer("0.1.0", "garbage"));
        // プレリリースは無視して数値部分だけを比べる。
        assert!(!is_newer("v0.2.0-rc.1", "0.2.0"));
        assert!(is_newer("v0.3.0-rc.1", "0.2.0"));
    }

    #[test]
    fn 確認の時期() {
        let h = 60 * 60 * 1000;
        assert!(is_due(None, 1000));
        assert!(!is_due(Some(1000), 1000 + 23 * h));
        assert!(is_due(Some(1000), 1000 + 24 * h));
        // 時計が戻ったときは確認する。
        assert!(is_due(Some(1000), 500));
        assert_eq!(wait_until_due(None, 1000), 0);
        assert_eq!(wait_until_due(Some(1000), 1000 + 20 * h), 4 * h);
        assert_eq!(wait_until_due(Some(1000), 1000 + 25 * h), 0);
    }

    #[test]
    fn 通知済み判定() {
        assert!(should_notify("v0.3.0", "0.2.0", None));
        assert!(!should_notify("v0.3.0", "0.2.0", Some("0.3.0")));
        assert!(!should_notify("0.3.0", "0.2.0", Some("v0.3.0")));
        // さらに新しい版が出たら、あらためて通知する。
        assert!(should_notify("0.4.0", "0.2.0", Some("0.3.0")));
        // 新しくなければ通知しない。
        assert!(!should_notify("0.2.0", "0.2.0", None));
    }

    #[test]
    fn 保存と読み出し() {
        let conn = db::open_in_memory().unwrap();
        assert_eq!(load(&conn).unwrap(), UpdateState::default());
        assert_eq!(available(&conn, "0.2.0").unwrap(), None);
        record_check(&conn, 1234, Some("v0.3.0")).unwrap();
        record_notified(&conn, "v0.3.0").unwrap();
        let s = load(&conn).unwrap();
        assert_eq!(s.last_checked_at, Some(1234));
        assert_eq!(s.latest_version.as_deref(), Some("0.3.0"));
        assert_eq!(s.notified_version.as_deref(), Some("0.3.0"));
        assert_eq!(available(&conn, "0.2.0").unwrap().as_deref(), Some("0.3.0"));
        assert_eq!(available(&conn, "0.3.0").unwrap(), None);
        // 取得に失敗した確認では、前回の最新版を残す。
        record_check(&conn, 9999, None).unwrap();
        let s = load(&conn).unwrap();
        assert_eq!(s.last_checked_at, Some(9999));
        assert_eq!(s.latest_version.as_deref(), Some("0.3.0"));
    }
}
