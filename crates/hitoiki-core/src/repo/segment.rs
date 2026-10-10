//! 派生層 `activity_segment` の読み書き。

use crate::model::{ActivitySegment, SegmentKind, SegmentStatus};
use crate::timefmt::Range;
use crate::{Error, Result};
use rusqlite::{params, Connection, OptionalExtension, Row};

const COLS: &str = "id, start_at, end_at, kind, task_id, activity_label, confidence, decided_by, status, \
                    window_span_ids, agent_session_id, exe_name, title";

fn from_row(r: &Row<'_>) -> rusqlite::Result<ActivitySegment> {
    let ids: String = r.get(9)?;
    Ok(ActivitySegment {
        id: r.get(0)?,
        start_at: r.get(1)?,
        end_at: r.get(2)?,
        kind: r.get(3)?,
        task_id: r.get(4)?,
        activity_label: r.get(5)?,
        confidence: r.get(6)?,
        decided_by: r.get(7)?,
        status: r.get(8)?,
        window_span_ids: serde_json::from_str(&ids).unwrap_or_default(),
        agent_session_id: r.get(10)?,
        exe_name: r.get(11)?,
        title: r.get(12)?,
    })
}

/// 範囲と重なる区間を開始時刻順に返す。
pub fn list(conn: &Connection, range: Range) -> Result<Vec<ActivitySegment>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLS} FROM activity_segment WHERE end_at > ?1 AND start_at < ?2 ORDER BY start_at, id"
    ))?;
    let rows = stmt.query_map(params![range.start, range.end], from_row)?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// 範囲と重なる区間のうち指定状態のもの。
pub fn list_by_status(
    conn: &Connection,
    range: Range,
    status: SegmentStatus,
) -> Result<Vec<ActivitySegment>> {
    Ok(list(conn, range)?
        .into_iter()
        .filter(|s| s.status == status)
        .collect())
}

pub fn get(conn: &Connection, id: &str) -> Result<ActivitySegment> {
    conn.query_row(
        &format!("SELECT {COLS} FROM activity_segment WHERE id = ?1"),
        [id],
        from_row,
    )
    .optional()?
    .ok_or_else(|| Error::NotFound(format!("区間 {id}")))
}

/// ID の前方一致で 1 件に解決する（12.1）。
pub fn resolve(conn: &Connection, prefix: &str) -> Result<ActivitySegment> {
    let ids = super::resolve_prefix(conn, "activity_segment", prefix)?;
    get(conn, &ids)
}

/// 範囲 [start, end) と重なる区間をすべて削除し、新しい区間で置き換える。
/// トランザクションは張らない。呼び出し側が範囲の確定（境界をまたぐ区間が無いことの確認）と
/// 合わせて 1 つのトランザクション（`BEGIN IMMEDIATE`）の中で呼ぶこと。
pub fn replace_range(conn: &Connection, range: Range, segments: &[ActivitySegment]) -> Result<()> {
    conn.execute(
        "DELETE FROM activity_segment WHERE start_at < ?2 AND end_at > ?1",
        params![range.start, range.end],
    )?;
    {
        let mut stmt = conn.prepare(&format!(
            "INSERT INTO activity_segment ({COLS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)"
        ))?;
        for s in segments {
            stmt.execute(params![
                s.id,
                s.start_at,
                s.end_at,
                s.kind,
                s.task_id,
                s.activity_label,
                s.confidence,
                s.decided_by,
                s.status,
                serde_json::to_string(&s.window_span_ids).expect("serialize ids"),
                s.agent_session_id,
                s.exe_name,
                s.title,
            ])?;
        }
    }
    Ok(())
}

/// 区間の合計時間（ミリ秒）を範囲内にクリップして返す。
pub fn clipped_len(s: &ActivitySegment, range: Range) -> i64 {
    Range::new(s.start_at, s.end_at)
        .intersect(&range)
        .map(|r| r.len())
        .unwrap_or(0)
}

/// kind ごとの表示名。
pub fn kind_label(kind: SegmentKind) -> &'static str {
    match kind {
        SegmentKind::Work => "作業",
        SegmentKind::AgentDialog => "エージェント対話",
        SegmentKind::AgentWatch => "エージェント見守り",
        SegmentKind::Meeting => "会議",
        SegmentKind::Idle => "無操作",
        SegmentKind::Break => "休憩",
        SegmentKind::Interrupt => "割り込み",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    fn seg(id: &str, s: i64, e: i64) -> ActivitySegment {
        ActivitySegment {
            id: id.into(),
            start_at: s,
            end_at: e,
            kind: SegmentKind::Work,
            task_id: None,
            activity_label: None,
            confidence: None,
            decided_by: None,
            status: SegmentStatus::NeedsReview,
            window_span_ids: vec!["w1".into()],
            agent_session_id: None,
            exe_name: Some("Code.exe".into()),
            title: Some("x".into()),
        }
    }

    #[test]
    fn replace_and_list() {
        let conn = db::open_in_memory().unwrap();
        replace_range(&conn, Range::new(0, 100), &[seg("A1", 0, 50), seg("A2", 50, 100)]).unwrap();
        replace_range(&conn, Range::new(50, 100), &[seg("B1", 50, 100)]).unwrap();
        let all = list(&conn, Range::new(0, 100)).unwrap();
        assert_eq!(
            all.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(),
            ["A1", "B1"]
        );
        assert_eq!(all[0].window_span_ids, ["w1"]);
        assert_eq!(resolve(&conn, "B").unwrap().id, "B1");
        assert!(matches!(resolve(&conn, "Z"), Err(Error::NotFound(_))));

        // 範囲の開始より前から始まり、範囲と重なる区間も消す（重なりを残さない）。
        replace_range(&conn, Range::new(40, 60), &[seg("C1", 40, 60)]).unwrap();
        let all = list(&conn, Range::new(0, 100)).unwrap();
        assert_eq!(all.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(), ["C1"]);
    }
}
