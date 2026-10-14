//! 成果の指標の現在値の履歴と時系列（`kr_progress`。docs/dev/architecture.md「データモデル」）。
//!
//! 現在値（`key_result.current`）が変わるたびに 1 行残す。時系列は、週ごとに「その週の終わり時点の
//! 現在値」と「その週に成果の指標に紐付いたタスクへかけた時間」（`kr_summary` の週ごとの時間と同じ規則）を
//! 並べるだけで、目標値との比較や評価はしない。

use super::okr_summary::kr_summary;
use super::{Ctx, RangeView};
use crate::id::new_id;
use crate::model::KeyResult;
use crate::repo::edit;
use crate::repo::kr_progress::{self, KrProgress};
use crate::timefmt::{self, Range};
use crate::{Error, Millis, Result};
use chrono::{Duration, NaiveDate};
use serde::Serialize;

/// 現在値の文字列から先頭の数値を取り出す（`"12"` `"3.5 回"` `"-2"`）。数として読めなければ `None`。
pub fn parse_value(s: &str) -> Option<f64> {
    let t = s.trim();
    let mut end = 0;
    let mut seen_digit = false;
    let mut seen_dot = false;
    for (i, c) in t.char_indices() {
        match c {
            '+' | '-' if i == 0 => {}
            '0'..='9' => seen_digit = true,
            '.' if !seen_dot && seen_digit => seen_dot = true,
            _ => break,
        }
        end = i + c.len_utf8();
    }
    if !seen_digit {
        return None;
    }
    t[..end].trim_end_matches('.').parse().ok()
}

fn blank_to_none(s: &str) -> Option<String> {
    let t = s.trim();
    (!t.is_empty()).then(|| t.to_string())
}

/// 履歴を 1 行残す（`edit_kr` からも使う）。
pub(super) fn insert_entry(
    ctx: &Ctx,
    kr_id: &str,
    text: Option<&str>,
    note: Option<&str>,
) -> Result<KrProgress> {
    let p = KrProgress {
        id: new_id(),
        kr_id: kr_id.to_string(),
        value: text.and_then(parse_value),
        text: text.map(str::to_string),
        note: note.and_then(blank_to_none),
        at: ctx.now(),
    };
    kr_progress::insert(ctx.conn, &p)?;
    Ok(p)
}

#[derive(Debug, Clone, Serialize)]
pub struct ProgressRecorded {
    pub key_result: KeyResult,
    pub entry: KrProgress,
}

/// 現在値を記録する。成果の指標の現在値を更新し、同じ値でも履歴に 1 行残す（メモだけ足したいとき用）。
pub fn record(ctx: &Ctx, kr: &str, value: &str, note: Option<&str>) -> Result<ProgressRecorded> {
    let Some(text) = blank_to_none(value) else {
        return Err(Error::InvalidArgument("値が空です".into()));
    };
    let mut k = super::okr::find_kr(ctx, kr)?;
    let tx = ctx.conn.unchecked_transaction()?;
    k.current = Some(text.clone());
    edit::update_kr(ctx.conn, &k)?;
    let entry = insert_entry(ctx, &k.id, Some(&text), note)?;
    tx.commit()?;
    Ok(ProgressRecorded { key_result: k, entry })
}

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, Serialize)]
pub struct HistoryWeek {
    /// 週の月曜（`YYYY-MM-DD`）。
    pub week_start: String,
    /// その週に成果の指標に紐付いたタスクへかけた時間（ミリ秒）。
    pub duration_ms: i64,
    /// その週の終わり（現在の週は今）時点の現在値の数値。記録がまだ無い・数として読めないときは null。
    pub value: Option<f64>,
    /// その時点の現在値の文字列。
    pub text: Option<String>,
    /// その週に残した履歴の数。
    pub changes: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct KrHistory {
    pub key_result: KeyResult,
    pub range: RangeView,
    /// `given` / `objective` / `recent_weeks`（`kr_summary` と同じ）。
    pub range_source: &'static str,
    pub weeks: Vec<HistoryWeek>,
    /// 履歴の全件（古い順）。
    pub entries: Vec<KrProgress>,
}

/// 成果の指標の時系列。`range` が `None` なら `kr_summary` と同じ期間。
pub fn kr_history(ctx: &Ctx, kr: &str, range: Option<Range>) -> Result<KrHistory> {
    let s = kr_summary(ctx, kr, range)?;
    let tz = ctx.tz()?;
    let entries = kr_progress::list_of_kr(ctx.conn, &s.key_result.id)?;
    let mut weeks = Vec::new();
    for w in &s.weekly {
        let start = NaiveDate::parse_from_str(&w.week_start, "%Y-%m-%d")
            .map_err(|e| Error::Other(anyhow::anyhow!("週の日付を読めません: {e}")))?;
        let from = timefmt::day_range(tz, start).start;
        let to: Millis = timefmt::day_range(tz, start + Duration::days(6)).end;
        let last = entries.iter().rev().find(|e| e.at < to);
        weeks.push(HistoryWeek {
            week_start: w.week_start.clone(),
            duration_ms: w.duration_ms,
            value: last.and_then(|e| e.value),
            text: last.and_then(|e| e.text.clone()),
            changes: entries.iter().filter(|e| e.at >= from && e.at < to).count() as i64,
        });
    }
    Ok(KrHistory {
        key_result: s.key_result,
        range: s.range,
        range_source: s.range_source,
        weeks,
        entries,
    })
}
