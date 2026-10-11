//! 収集器（生データ層 `window_span` / `idle_span` / ハートビート）の書き込み。

use crate::model::{IdleSpan, WindowSpan};
use crate::{Millis, Result};
use rusqlite::{params, Connection, OptionalExtension};

/// ハートビートを保存する `setting` のキー。
pub const HEARTBEAT_KEY: &str = "collector.heartbeat";

/// 進行中（end_at = NULL）の前面ウインドウ区間を開く。
pub fn open_window_span(conn: &Connection, s: &WindowSpan) -> Result<()> {
    conn.execute(
        "INSERT INTO window_span(id, start_at, end_at, hwnd, pid, exe_path, exe_name, title, excluded)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![s.id, s.start_at, s.end_at, s.hwnd, s.pid, s.exe_path, s.exe_name, s.title, s.excluded],
    )?;
    Ok(())
}

/// 進行中の前面ウインドウ区間を閉じる（終了が開始より前なら開始に丸める）。
pub fn close_window_span(conn: &Connection, id: &str, end_at: Millis) -> Result<()> {
    conn.execute(
        "UPDATE window_span SET end_at = MAX(start_at, ?2) WHERE id = ?1 AND end_at IS NULL",
        params![id, end_at],
    )?;
    Ok(())
}

/// 進行中の無操作区間を開く。
pub fn open_idle_span(conn: &Connection, s: &IdleSpan) -> Result<()> {
    conn.execute(
        "INSERT INTO idle_span(id, start_at, end_at, reason) VALUES (?1, ?2, ?3, ?4)",
        params![s.id, s.start_at, s.end_at, s.reason],
    )?;
    Ok(())
}

/// 進行中の無操作区間を閉じる。
pub fn close_idle_span(conn: &Connection, id: &str, end_at: Millis) -> Result<()> {
    conn.execute(
        "UPDATE idle_span SET end_at = MAX(start_at, ?2) WHERE id = ?1 AND end_at IS NULL",
        params![id, end_at],
    )?;
    Ok(())
}

/// 前回の異常終了などで残った end_at NULL の区間を `heartbeat` で閉じる。
/// 開始が `heartbeat` より後（またはハートビート無し）の区間は開始時刻で閉じる。閉じた件数を返す。
pub fn close_dangling(conn: &Connection, heartbeat: Option<Millis>) -> Result<usize> {
    let h = heartbeat.unwrap_or(i64::MIN);
    let a = conn.execute(
        "UPDATE window_span SET end_at = MAX(start_at, ?1) WHERE end_at IS NULL",
        [h],
    )?;
    let b = conn.execute(
        "UPDATE idle_span SET end_at = MAX(start_at, ?1) WHERE end_at IS NULL",
        [h],
    )?;
    Ok(a + b)
}

/// 最後のハートビート時刻。
pub fn get_heartbeat(conn: &Connection) -> Result<Option<Millis>> {
    let v: Option<String> = conn
        .query_row(
            "SELECT value_json FROM setting WHERE key = ?1",
            [HEARTBEAT_KEY],
            |r| r.get(0),
        )
        .optional()?;
    Ok(v.and_then(|s| s.trim().parse::<i64>().ok()))
}

/// ハートビート時刻を保存する。
pub fn set_heartbeat(conn: &Connection, at: Millis) -> Result<()> {
    conn.execute(
        "INSERT INTO setting(key, value_json) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value_json = excluded.value_json",
        params![HEARTBEAT_KEY, at.to_string()],
    )?;
    Ok(())
}

/// 収集器の稼働記録を保存する `setting` のキー（JSON 配列。古い順）。
pub const RUNS_KEY: &str = "collector.runs";
/// 稼働記録の保持期間と上限。
const RUNS_KEEP_MS: Millis = 60 * 24 * 60 * 60 * 1000;
const RUNS_KEEP_MAX: usize = 400;

/// 収集器（Recorder）の 1 回の稼働。`end_at` は最後に動いていると確認できた時刻（ハートビート）。
/// 記録が無かった時間帯は、稼働の外側（次の起動までの空白）として求める。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CollectorRun {
    pub start_at: Millis,
    pub end_at: Millis,
    /// 正常に停止した（終了時の後始末まで行った）か。
    #[serde(default)]
    pub clean: bool,
}

/// 稼働記録を古い順に返す。壊れていれば空。
pub fn list_runs(conn: &Connection) -> Result<Vec<CollectorRun>> {
    let v: Option<String> = conn
        .query_row("SELECT value_json FROM setting WHERE key = ?1", [RUNS_KEY], |r| {
            r.get(0)
        })
        .optional()?;
    Ok(v.and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default())
}

fn save_runs(conn: &Connection, runs: &[CollectorRun]) -> Result<()> {
    let json = serde_json::to_string(runs).unwrap_or_else(|_| "[]".into());
    conn.execute(
        "INSERT INTO setting(key, value_json) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value_json = excluded.value_json",
        params![RUNS_KEY, json],
    )?;
    Ok(())
}

/// 新しい稼働を始める。古い記録は保持期間・上限を超えた分を捨てる。
pub fn begin_run(conn: &Connection, now: Millis) -> Result<()> {
    let mut runs = list_runs(conn)?;
    runs.retain(|r| r.end_at >= now - RUNS_KEEP_MS);
    runs.push(CollectorRun {
        start_at: now,
        end_at: now,
        clean: false,
    });
    if runs.len() > RUNS_KEEP_MAX {
        runs.drain(..runs.len() - RUNS_KEEP_MAX);
    }
    save_runs(conn, &runs)
}

/// 最新の稼働の終了時刻を `now` に進める（`clean` ならそのまま正常停止の印も付ける）。
pub fn touch_run(conn: &Connection, now: Millis, clean: bool) -> Result<()> {
    let mut runs = list_runs(conn)?;
    let Some(last) = runs.last_mut() else {
        return Ok(());
    };
    last.end_at = last.end_at.max(now);
    last.clean = clean;
    save_runs(conn, &runs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;
    use crate::model::IdleReason;

    fn ws(id: &str, start: Millis) -> WindowSpan {
        WindowSpan {
            id: id.into(),
            start_at: start,
            end_at: None,
            hwnd: Some("1".into()),
            pid: Some(1),
            exe_path: None,
            exe_name: Some("a.exe".into()),
            title: Some("t".into()),
            excluded: false,
        }
    }

    #[test]
    fn heartbeat_roundtrip() {
        let c = open_in_memory().unwrap();
        assert_eq!(get_heartbeat(&c).unwrap(), None);
        set_heartbeat(&c, 100).unwrap();
        set_heartbeat(&c, 200).unwrap();
        assert_eq!(get_heartbeat(&c).unwrap(), Some(200));
    }

    #[test]
    fn close_dangling_uses_heartbeat() {
        let c = open_in_memory().unwrap();
        open_window_span(&c, &ws("a", 100)).unwrap();
        open_window_span(&c, &ws("b", 500)).unwrap();
        open_idle_span(
            &c,
            &IdleSpan {
                id: "i".into(),
                start_at: 150,
                end_at: None,
                reason: IdleReason::NoInput,
            },
        )
        .unwrap();
        assert_eq!(close_dangling(&c, Some(300)).unwrap(), 3);
        let end = |t: &str, id: &str| -> i64 {
            c.query_row(&format!("SELECT end_at FROM {t} WHERE id=?1"), [id], |r| r.get(0))
                .unwrap()
        };
        assert_eq!(end("window_span", "a"), 300);
        assert_eq!(end("window_span", "b"), 500);
        assert_eq!(end("idle_span", "i"), 300);
    }
}
