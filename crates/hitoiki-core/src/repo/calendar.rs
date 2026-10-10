//! カレンダー予定（`calendar_event`）の保存と読み出し。

use crate::model::CalendarEvent;
use crate::timefmt::Range;
use crate::Result;
use rusqlite::{params, Connection};

/// `source` の `range` と重なる既存行を消して `events` を入れ直す（同一トランザクション）。
/// 重複キー (source, external_id, start_at) は後勝ち。
pub fn replace_source_range(
    conn: &Connection,
    source: &str,
    range: Range,
    events: &[CalendarEvent],
) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "DELETE FROM calendar_event WHERE source = ?1 AND start_at < ?3 AND end_at > ?2",
        params![source, range.start, range.end],
    )?;
    {
        let mut stmt = tx.prepare(
            "INSERT OR REPLACE INTO calendar_event
             (id, source, external_id, start_at, end_at, all_day, title, location, is_busy, fetched_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        )?;
        for e in events {
            stmt.execute(params![
                e.id,
                source,
                e.external_id,
                e.start_at,
                e.end_at,
                e.all_day,
                e.title,
                e.location,
                e.is_busy,
                e.fetched_at
            ])?;
        }
    }
    tx.commit()?;
    Ok(())
}

/// 範囲と重なる予定を開始時刻順で返す。
pub fn list(conn: &Connection, range: Range) -> Result<Vec<CalendarEvent>> {
    let mut stmt = conn.prepare(
        "SELECT id, source, external_id, start_at, end_at, all_day, title, location, is_busy, fetched_at
         FROM calendar_event
         WHERE start_at < ?2 AND end_at > ?1
         ORDER BY start_at, end_at, id",
    )?;
    let rows = stmt.query_map(params![range.start, range.end], |r| {
        Ok(CalendarEvent {
            id: r.get(0)?,
            source: r.get(1)?,
            external_id: r.get(2)?,
            start_at: r.get(3)?,
            end_at: r.get(4)?,
            all_day: r.get(5)?,
            title: r.get(6)?,
            location: r.get(7)?,
            is_busy: r.get(8)?,
            fetched_at: r.get(9)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(source: &str, ext: &str, s: i64, e: i64) -> CalendarEvent {
        CalendarEvent {
            id: crate::id::new_id(),
            source: source.into(),
            external_id: ext.into(),
            start_at: s,
            end_at: e,
            all_day: false,
            title: "t".into(),
            location: None,
            is_busy: true,
            fetched_at: 0,
        }
    }

    #[test]
    fn replace_and_list() {
        let conn = crate::db::open_in_memory().unwrap();
        let r = Range::new(0, 1000);
        replace_source_range(&conn, "a", r, &[ev("a", "x", 10, 20), ev("a", "y", 30, 40)]).unwrap();
        replace_source_range(&conn, "b", r, &[ev("b", "x", 10, 20)]).unwrap();
        replace_source_range(&conn, "a", r, &[ev("a", "z", 50, 60)]).unwrap();
        let all = list(&conn, r).unwrap();
        let names: Vec<_> = all
            .iter()
            .map(|e| (e.source.as_str(), e.external_id.as_str()))
            .collect();
        assert_eq!(names, vec![("b", "x"), ("a", "z")]);
        assert_eq!(list(&conn, Range::new(55, 56)).unwrap().len(), 1);
        assert!(list(&conn, Range::new(60, 70)).unwrap().is_empty());
    }
}
