//! 設定ファイルにある「ユーザーが作る一覧」（カレンダーソース、活動ラベル）の追加・編集・削除。
//!
//! 設定ファイルを直接書き換える代わりに、検証つきで更新する。ICS の URL は秘密情報なので、
//! 一覧や結果には出さない（ホスト名だけ見せる）。

use super::impact::{DeleteMode, DeleteOutcome, Impact};
use crate::config::{self, CalendarSource, Config};
use crate::repo::edit;
use crate::{Error, Result};
use rusqlite::Connection;
use serde::Serialize;
use std::path::Path;

/// 一覧に出すカレンダーソース（URL は出さない）。
#[derive(Debug, Clone, Serialize)]
pub struct CalendarSourceView {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
    /// URL のホスト部分だけ（秘密のトークンを含まない）。
    pub host: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CalendarSourceList {
    pub sources: Vec<CalendarSourceView>,
}

fn view(s: &CalendarSource) -> CalendarSourceView {
    let rest = s.url.split_once("://").map_or(s.url.as_str(), |(_, r)| r);
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    // `user:pass@host` の認証部分は出さない。
    let host = host.rsplit('@').next().unwrap_or("");
    CalendarSourceView {
        name: s.name.clone(),
        kind: s.kind.clone(),
        host: host.to_string(),
    }
}

pub fn calendar_list(cfg: &Config) -> CalendarSourceList {
    CalendarSourceList {
        sources: cfg.calendar.sources.iter().map(view).collect(),
    }
}

fn check_url(url: &str) -> Result<String> {
    let u = url.trim();
    let ok = ["https://", "http://", "webcal://", "webcals://"]
        .iter()
        .any(|p| u.to_ascii_lowercase().starts_with(p));
    if !ok {
        return Err(Error::InvalidArgument(
            "URL は https:// か webcal:// で始めてください".into(),
        ));
    }
    Ok(u.to_string())
}

fn check_name(name: &str) -> Result<String> {
    let n = name.trim();
    if n.is_empty() {
        return Err(Error::InvalidArgument("ソース名が空です".into()));
    }
    Ok(n.to_string())
}

/// `calendar.sources` を書き戻す。
fn save_sources(path: &Path, sources: &[CalendarSource]) -> Result<()> {
    let arr = toml::Value::Array(
        sources
            .iter()
            .map(|s| {
                let mut t = toml::Table::new();
                t.insert("name".into(), toml::Value::String(s.name.clone()));
                t.insert("type".into(), toml::Value::String(s.kind.clone()));
                t.insert("url".into(), toml::Value::String(s.url.clone()));
                toml::Value::Table(t)
            })
            .collect(),
    );
    config::set_value(path, "calendar.sources", &arr.to_string())
}

pub fn calendar_add(path: &Path, name: &str, url: &str) -> Result<CalendarSourceView> {
    let name = check_name(name)?;
    let url = check_url(url)?;
    let mut sources = Config::load_from(path)?.calendar.sources;
    if sources.iter().any(|s| s.name == name) {
        return Err(Error::InvalidArgument(format!(
            "ソース「{name}」は既に存在します"
        )));
    }
    let s = CalendarSource {
        name,
        kind: "ics".into(),
        url,
    };
    sources.push(s.clone());
    save_sources(path, &sources)?;
    Ok(view(&s))
}

/// 名前や URL を変える。名前を変えたときは、取得済みの予定のソース名も付け替える（重複して取り込まれないように）。
pub fn calendar_edit(
    conn: &Connection,
    path: &Path,
    name: &str,
    new_name: Option<&str>,
    url: Option<&str>,
) -> Result<CalendarSourceView> {
    let mut sources = Config::load_from(path)?.calendar.sources;
    let idx = sources
        .iter()
        .position(|s| s.name == name)
        .ok_or_else(|| Error::NotFound(format!("カレンダーソース「{name}」")))?;
    let mut s = sources[idx].clone();
    if let Some(n) = new_name {
        let n = check_name(n)?;
        if n != s.name && sources.iter().any(|o| o.name == n) {
            return Err(Error::InvalidArgument(format!("ソース「{n}」は既に存在します")));
        }
        s.name = n;
    }
    if let Some(u) = url {
        s.url = check_url(u)?;
    }
    sources[idx] = s.clone();
    save_sources(path, &sources)?;
    if s.name != name {
        edit::rename_calendar_source(conn, name, &s.name)?;
    }
    Ok(view(&s))
}

/// ソースの削除。取得済みの予定（キャッシュ）も消える。予定そのものはカレンダー側に残る。
pub fn calendar_delete(
    conn: &Connection,
    path: &Path,
    name: &str,
    mode: DeleteMode,
) -> Result<DeleteOutcome> {
    let mut sources = Config::load_from(path)?.calendar.sources;
    let idx = sources
        .iter()
        .position(|s| s.name == name)
        .ok_or_else(|| Error::NotFound(format!("カレンダーソース「{name}」")))?;
    let im = Impact::new("calendar_source", name, name)
        .line(
            "取得済みの予定（このアプリ内のコピーが消える）",
            edit::count_calendar_events(conn, name)?,
        )
        .note("カレンダー側の予定は消えません。");
    let go = DeleteOutcome::gate(mode, &im)?;
    if go {
        sources.remove(idx);
        save_sources(path, &sources)?;
        edit::delete_calendar_events(conn, name)?;
    }
    Ok(DeleteOutcome::new(mode, im, go))
}

// ===== 活動ラベル =====

#[derive(Debug, Clone, Serialize)]
pub struct LabelList {
    pub labels: Vec<String>,
}

pub fn labels_list(cfg: &Config) -> LabelList {
    LabelList {
        labels: cfg.activity.labels.clone(),
    }
}

fn save_labels(path: &Path, labels: &[String]) -> Result<()> {
    let arr = toml::Value::Array(labels.iter().cloned().map(toml::Value::String).collect());
    config::set_value(path, "activity.labels", &arr.to_string())
}

pub fn labels_add(path: &Path, label: &str) -> Result<LabelList> {
    let l = label.trim();
    if l.is_empty() {
        return Err(Error::InvalidArgument("ラベルが空です".into()));
    }
    let mut labels = Config::load_from(path)?.activity.labels;
    if labels.iter().any(|x| x == l) {
        return Err(Error::InvalidArgument(format!("ラベル「{l}」は既にあります")));
    }
    labels.push(l.to_string());
    save_labels(path, &labels)?;
    Ok(LabelList { labels })
}

/// 判定器へ渡す候補から外す。過去の区間や活動ルールに付いたラベルはそのまま残る。
pub fn labels_remove(path: &Path, label: &str) -> Result<LabelList> {
    let mut labels = Config::load_from(path)?.activity.labels;
    let before = labels.len();
    labels.retain(|x| x != label);
    if labels.len() == before {
        return Err(Error::NotFound(format!("ラベル「{label}」")));
    }
    save_labels(path, &labels)?;
    Ok(LabelList { labels })
}

/// ラベルの名前を変える（位置は保つ）。活動ルールの `activity_label` も同じ名前に付け替える。
pub fn labels_rename(conn: &Connection, path: &Path, from: &str, to: &str) -> Result<LabelList> {
    let to = to.trim();
    if to.is_empty() {
        return Err(Error::InvalidArgument("ラベルが空です".into()));
    }
    let mut labels = Config::load_from(path)?.activity.labels;
    let idx = labels
        .iter()
        .position(|x| x == from)
        .ok_or_else(|| Error::NotFound(format!("ラベル「{from}」")))?;
    if to != from && labels.iter().any(|x| x == to) {
        return Err(Error::InvalidArgument(format!("ラベル「{to}」は既にあります")));
    }
    labels[idx] = to.to_string();
    save_labels(path, &labels)?;
    conn.execute(
        "UPDATE activity_rule SET activity_label = ?2 WHERE activity_label = ?1",
        rusqlite::params![from, to],
    )?;
    Ok(LabelList { labels })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::model::{ActivityRule, MatchField, PatternType};
    use crate::repo::rule;

    fn tmp() -> (tempfile::TempDir, std::path::PathBuf) {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("config.toml");
        (d, p)
    }

    #[test]
    fn カレンダーソースの追加_編集_削除() {
        let (_d, p) = tmp();
        let conn = db::open_in_memory().unwrap();
        let v = calendar_add(&p, "仕事", "https://cal.example.com/secret-token/x.ics").unwrap();
        assert_eq!(v.host, "cal.example.com");
        assert!(calendar_add(&p, "仕事", "https://a.example/x.ics").is_err());
        assert!(calendar_add(&p, "x", "ftp://a").is_err());
        assert!(calendar_add(&p, " ", "https://a").is_err());
        calendar_add(&p, "私用", "webcal://b.example/y.ics").unwrap();
        let cfg = Config::load_from(&p).unwrap();
        assert_eq!(cfg.calendar.sources.len(), 2);
        assert_eq!(
            cfg.calendar.sources[0].url,
            "https://cal.example.com/secret-token/x.ics"
        );
        // JSON に URL が出ない
        let json = serde_json::to_string(&calendar_list(&cfg)).unwrap();
        assert!(!json.contains("secret-token"));

        // 取得済みの予定を作っておく
        conn.execute(
            "INSERT INTO calendar_event (id, source, external_id, start_at, end_at, all_day, title, is_busy, fetched_at) \
             VALUES ('E1','仕事','x',1,2,0,'t',1,0)",
            [],
        )
        .unwrap();
        calendar_edit(&conn, &p, "仕事", Some("会社"), Some("https://c.example/z.ics")).unwrap();
        let src: String = conn
            .query_row("SELECT source FROM calendar_event", [], |r| r.get(0))
            .unwrap();
        assert_eq!(src, "会社");
        let cfg = Config::load_from(&p).unwrap();
        assert_eq!(cfg.calendar.sources[0].name, "会社");
        assert_eq!(cfg.calendar.sources[0].url, "https://c.example/z.ics");
        assert!(calendar_edit(&conn, &p, "会社", Some("私用"), None).is_err());
        assert!(calendar_edit(&conn, &p, "nope", None, None).is_err());

        let im = calendar_delete(&conn, &p, "会社", DeleteMode::default());
        assert!(im.is_err());
        assert_eq!(Config::load_from(&p).unwrap().calendar.sources.len(), 2);
        calendar_delete(&conn, &p, "会社", DeleteMode::FORCE).unwrap();
        assert_eq!(Config::load_from(&p).unwrap().calendar.sources.len(), 1);
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM calendar_event", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0);
        // 最後の 1 件を消すと空配列になる
        calendar_delete(&conn, &p, "私用", DeleteMode::default()).unwrap();
        assert!(Config::load_from(&p).unwrap().calendar.sources.is_empty());
    }

    #[test]
    fn 活動ラベルの追加_改名_削除() {
        let (_d, p) = tmp();
        let conn = db::open_in_memory().unwrap();
        let n0 = Config::default().activity.labels.len();
        let l = labels_add(&p, " 運動 ").unwrap();
        assert_eq!(l.labels.len(), n0 + 1);
        assert!(labels_add(&p, "運動").is_err());
        assert!(labels_add(&p, " ").is_err());
        rule::insert_activity_rule(
            &conn,
            &ActivityRule {
                id: "R1".into(),
                field: MatchField::Exe,
                pattern: "x".into(),
                pattern_type: PatternType::Contains,
                activity_label: "運動".into(),
                priority: 0,
                enabled: true,
            },
        )
        .unwrap();
        let l = labels_rename(&conn, &p, "運動", "トレーニング").unwrap();
        assert_eq!(l.labels.last().map(String::as_str), Some("トレーニング"));
        assert_eq!(
            rule::list_activity_rules(&conn).unwrap()[0].activity_label,
            "トレーニング"
        );
        assert!(labels_rename(&conn, &p, "トレーニング", "開発").is_err());
        let l = labels_remove(&p, "トレーニング").unwrap();
        assert_eq!(l.labels.len(), n0);
        assert!(labels_remove(&p, "トレーニング").is_err());
        assert_eq!(Config::load_from(&p).unwrap().activity.labels.len(), n0);
    }
}
