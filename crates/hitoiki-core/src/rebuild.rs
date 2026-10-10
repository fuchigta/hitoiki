//! 帰属判定の再計算（7章）。DB から生データを読み、パイプラインを実行して派生層を置き換える。

use crate::config::Config;
use crate::pipeline::{self, PipelineInput, SegmentJudge};
use crate::repo::{raw, segment};
use crate::timefmt::Range;
use crate::{Millis, Result};
use rusqlite::Connection;

/// 範囲の手前から読む生データの量。対話の対象期間や見守りの文脈のため。
const LOOKBACK_MS: Millis = 6 * 60 * 60 * 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RebuildStats {
    /// 実際に再計算した範囲（境界をまたぐ既存区間を含めて広げることがある）。
    pub range: Range,
    pub segments: usize,
    pub bindings: usize,
}

/// 範囲の派生層（`activity_segment` と `agent_session_window`）を作り直す。
pub fn rebuild(conn: &mut Connection, cfg: &Config, range: Range, now: Millis) -> Result<RebuildStats> {
    rebuild_with_judge(conn, cfg, range, now, None)
}

/// 判定器つきの再計算。
pub fn rebuild_with_judge(
    conn: &mut Connection,
    cfg: &Config,
    range: Range,
    now: Millis,
    judge: Option<&dyn SegmentJudge>,
) -> Result<RebuildStats> {
    // 境界をまたぐ既存区間が残らないよう、範囲をそれらを含むところまで広げる。
    let mut range = range;
    for _ in 0..3 {
        let existing = segment::list(conn, range)?;
        let start = existing
            .iter()
            .map(|s| s.start_at)
            .min()
            .map_or(range.start, |s| s.min(range.start));
        let end = existing
            .iter()
            .map(|s| s.end_at)
            .max()
            .map_or(range.end, |e| e.max(range.end));
        if start == range.start && end == range.end {
            break;
        }
        range = Range::new(start, end);
    }

    let input = load_input(conn, cfg, range, now)?;
    let out = pipeline::build(&input, judge);
    segment::replace_range(conn, range, &out.segments)?;
    raw::replace_session_windows(conn, range, &out.bindings)?;
    Ok(RebuildStats {
        range,
        segments: out.segments.len(),
        bindings: out.bindings.len(),
    })
}

/// DB からパイプラインの入力を組み立てる。
pub fn load_input(conn: &Connection, cfg: &Config, range: Range, now: Millis) -> Result<PipelineInput> {
    let wide = Range::new(range.start - LOOKBACK_MS, range.end);
    let mut input = PipelineInput::new(range, now, cfg);
    input.windows = raw::window_spans(conn, wide)?;
    input.idles = raw::idle_spans(conn, wide)?;
    input.events = raw::agent_events(conn, wide)?;
    input.calendar = raw::calendar_events(conn, wide)?;
    input.overrides = raw::segment_overrides(conn, range)?;
    input.task_rules = raw::task_match_rules(conn)?;
    input.activity_rules = raw::activity_rules(conn)?;
    input.doing_logs = raw::task_doing_logs(conn, wide)?;
    input.time_blocks = raw::time_blocks(conn, wide)?;
    input.tasks = raw::open_tasks(conn)?;
    Ok(input)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::model::*;

    const M: i64 = 60_000;

    fn exec(conn: &Connection, sql: &str, p: &[&dyn rusqlite::ToSql]) {
        conn.execute(sql, p).unwrap();
    }

    #[test]
    fn rebuild_roundtrip() {
        let mut conn = db::open_in_memory().unwrap();
        let cfg = Config::default();
        let ins_win = "INSERT INTO window_span (id, start_at, end_at, hwnd, exe_name, title, excluded) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0)";
        exec(
            &conn,
            ins_win,
            &[&"W1", &0i64, &(10 * M), &"H1", &"WindowsTerminal.exe", &"claude"],
        );
        exec(
            &conn,
            ins_win,
            &[&"W2", &(10 * M), &(40 * M), &"H2", &"chrome.exe", &"docs"],
        );
        exec(
            &conn,
            ins_win,
            &[
                &"W3",
                &(40 * M),
                &Option::<i64>::None,
                &"H1",
                &"WindowsTerminal.exe",
                &"claude",
            ],
        );
        exec(
            &conn,
            "INSERT INTO task (id, title, created_at, updated_at) VALUES ('T1', 't', 0, 0)",
            &[],
        );
        exec(
            &conn,
            "INSERT INTO task_match_rule (id, task_id, field, pattern, pattern_type) VALUES ('R1', 'T1', 'title', 'docs', 'contains')",
            &[],
        );
        let ins_ev = "INSERT INTO agent_event (id, at, agent, session_id, kind, cwd) VALUES (?1, ?2, 'claude-code', 'S1', ?3, 'C:\\src\\x')";
        exec(&conn, ins_ev, &[&"E1", &M, &"session_start"]);
        exec(&conn, ins_ev, &[&"E2", &(2 * M), &"prompt_submit"]);
        exec(&conn, ins_ev, &[&"E3", &(5 * M), &"response_complete"]);
        exec(&conn, ins_ev, &[&"E4", &(45 * M), &"prompt_submit"]);

        let range = Range::new(0, 60 * M);
        let now = 50 * M;
        let stats = rebuild(&mut conn, &cfg, range, now).unwrap();
        assert_eq!(stats.bindings, 2);
        let segs = segment::list(&conn, range).unwrap();
        assert_eq!(segs.len() as usize, stats.segments);
        let kinds: Vec<_> = segs
            .iter()
            .map(|s| (s.start_at / M, s.end_at / M, s.kind))
            .collect();
        use SegmentKind::*;
        assert_eq!(
            kinds,
            vec![
                (0, 1, Work),
                (1, 2, AgentDialog),
                (2, 5, AgentWatch),
                (5, 10, AgentDialog),
                (10, 40, Work),
                (40, 45, AgentDialog),
                (45, 50, AgentWatch),
            ]
        );
        let mid = &segs[4];
        assert_eq!(mid.task_id.as_deref(), Some("T1"));
        assert_eq!(mid.status, SegmentStatus::Auto);
        assert_eq!(segs[1].agent_session_id.as_deref(), Some("claude-code:S1"));
        let wins = raw::session_windows(&conn, range).unwrap();
        assert_eq!(wins.len(), 2);
        assert_eq!(wins[0].hwnd, "H1");
        assert_eq!(wins[1].bound_at, 45 * M);

        // 再実行しても結果は変わらない（ID 以外）。
        let again = rebuild(&mut conn, &cfg, Range::new(20 * M, 30 * M), now).unwrap();
        assert_eq!(again.range, Range::new(10 * M, 40 * M));
        let segs2 = segment::list(&conn, range).unwrap();
        let strip = |v: &[ActivitySegment]| -> Vec<_> {
            v.iter()
                .map(|s| (s.start_at, s.end_at, s.kind, s.task_id.clone(), s.status))
                .collect()
        };
        assert_eq!(strip(&segs), strip(&segs2));
        assert_eq!(raw::session_windows(&conn, range).unwrap().len(), 2);
    }
}
