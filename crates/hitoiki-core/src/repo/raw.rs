//! 割り当てパイプラインの入力（生データ層・ユーザー層）の読み出しと、
//! パイプラインが出力する `agent_session_window` の置き換え。

use crate::model::*;
use crate::timefmt::Range;
use crate::Result;
use rusqlite::{params, Connection, Row};

/// 範囲と重なる `window_span`（進行中を含む）。開始時刻順。
pub fn window_spans(conn: &Connection, range: Range) -> Result<Vec<WindowSpan>> {
    let mut stmt = conn.prepare(
        "SELECT id, start_at, end_at, hwnd, pid, exe_path, exe_name, title, excluded
         FROM window_span
         WHERE start_at < ?2 AND (end_at IS NULL OR end_at > ?1)
         ORDER BY start_at, id",
    )?;
    let rows = stmt.query_map(params![range.start, range.end], |r: &Row| {
        Ok(WindowSpan {
            id: r.get(0)?,
            start_at: r.get(1)?,
            end_at: r.get(2)?,
            hwnd: r.get(3)?,
            pid: r.get(4)?,
            exe_path: r.get(5)?,
            exe_name: r.get(6)?,
            title: r.get(7)?,
            excluded: r.get(8)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// 範囲と重なる `idle_span`（進行中を含む）。
pub fn idle_spans(conn: &Connection, range: Range) -> Result<Vec<IdleSpan>> {
    let mut stmt = conn.prepare(
        "SELECT id, start_at, end_at, reason FROM idle_span
         WHERE start_at < ?2 AND (end_at IS NULL OR end_at > ?1)
         ORDER BY start_at, id",
    )?;
    let rows = stmt.query_map(params![range.start, range.end], |r| {
        Ok(IdleSpan {
            id: r.get(0)?,
            start_at: r.get(1)?,
            end_at: r.get(2)?,
            reason: r.get(3)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// 範囲 [start, end) 内の `agent_event`（時刻順）。
pub fn agent_events(conn: &Connection, range: Range) -> Result<Vec<AgentEvent>> {
    let mut stmt = conn.prepare(
        "SELECT id, at, agent, session_id, kind, cwd, git_branch, prompt_excerpt, source_file, source_offset
         FROM agent_event WHERE at >= ?1 AND at < ?2 ORDER BY at, id",
    )?;
    let rows = stmt.query_map(params![range.start, range.end], |r| {
        Ok(AgentEvent {
            id: r.get(0)?,
            at: r.get(1)?,
            agent: r.get(2)?,
            session_id: r.get(3)?,
            kind: r.get(4)?,
            cwd: r.get(5)?,
            git_branch: r.get(6)?,
            prompt_excerpt: r.get(7)?,
            source_file: r.get(8)?,
            source_offset: r.get(9)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// 範囲と重なる `calendar_event`。
pub fn calendar_events(conn: &Connection, range: Range) -> Result<Vec<CalendarEvent>> {
    let mut stmt = conn.prepare(
        "SELECT id, source, external_id, start_at, end_at, all_day, title, location, is_busy, fetched_at
         FROM calendar_event WHERE start_at < ?2 AND end_at > ?1 ORDER BY start_at, id",
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

/// 範囲と重なる `segment_confirmation`（作成順）。
pub fn segment_confirmations(conn: &Connection, range: Range) -> Result<Vec<SegmentConfirmation>> {
    let mut stmt = conn.prepare(
        "SELECT id, start_at, end_at, kind, task_id, activity, created_at, off_pc, correction_id
         FROM segment_confirmation WHERE start_at < ?2 AND end_at > ?1 ORDER BY created_at, id",
    )?;
    let rows = stmt.query_map(params![range.start, range.end], |r| {
        Ok(SegmentConfirmation {
            id: r.get(0)?,
            start_at: r.get(1)?,
            end_at: r.get(2)?,
            kind: r.get(3)?,
            task_id: r.get(4)?,
            activity: r.get(5)?,
            created_at: r.get(6)?,
            off_pc: r.get::<_, i64>(7)? != 0,
            correction_id: r.get(8)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// 有効な `task_assign_rule` すべて。
pub fn task_assign_rules(conn: &Connection) -> Result<Vec<TaskAssignRule>> {
    let mut stmt = conn.prepare(
        "SELECT id, task_id, field, pattern, pattern_type, priority, enabled
         FROM task_assign_rule WHERE enabled = 1 ORDER BY priority DESC, id",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(TaskAssignRule {
            id: r.get(0)?,
            task_id: r.get(1)?,
            field: r.get(2)?,
            pattern: r.get(3)?,
            pattern_type: r.get(4)?,
            priority: r.get(5)?,
            enabled: r.get(6)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// 有効な `activity_rule` すべて。
pub fn activity_rules(conn: &Connection) -> Result<Vec<ActivityRule>> {
    let mut stmt = conn.prepare(
        "SELECT id, field, pattern, pattern_type, activity, priority, enabled
         FROM activity_rule WHERE enabled = 1 ORDER BY priority DESC, id",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(ActivityRule {
            id: r.get(0)?,
            field: r.get(1)?,
            pattern: r.get(2)?,
            pattern_type: r.get(3)?,
            activity: r.get(4)?,
            priority: r.get(5)?,
            enabled: r.get(6)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// 範囲と重なる `task_doing_log`（進行中を含む）。
pub fn task_doing_logs(conn: &Connection, range: Range) -> Result<Vec<TaskDoingLog>> {
    let mut stmt = conn.prepare(
        "SELECT id, task_id, start_at, end_at FROM task_doing_log
         WHERE start_at < ?2 AND (end_at IS NULL OR end_at > ?1) ORDER BY start_at, id",
    )?;
    let rows = stmt.query_map(params![range.start, range.end], |r| {
        Ok(TaskDoingLog {
            id: r.get(0)?,
            task_id: r.get(1)?,
            start_at: r.get(2)?,
            end_at: r.get(3)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// 範囲と重なる `time_block`。
pub fn time_blocks(conn: &Connection, range: Range) -> Result<Vec<TimeBlock>> {
    let mut stmt = conn.prepare(
        "SELECT id, start_at, end_at, task_id, label, focus FROM time_block
         WHERE start_at < ?2 AND end_at > ?1 ORDER BY start_at, id",
    )?;
    let rows = stmt.query_map(params![range.start, range.end], |r| {
        Ok(TimeBlock {
            id: r.get(0)?,
            start_at: r.get(1)?,
            end_at: r.get(2)?,
            task_id: r.get(3)?,
            label: r.get(4)?,
            focus: r.get(5)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// 未完了（todo / doing）のタスク。
pub fn open_tasks(conn: &Connection) -> Result<Vec<Task>> {
    let mut stmt = conn.prepare(
        "SELECT id, title, body_md, status, kind, urgent, important, estimate_min, planned_start_at,
                planned_end_at, key_result_id, external_url, created_at, updated_at, done_at
         FROM task WHERE status IN ('todo', 'doing') ORDER BY created_at, id",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(Task {
            id: r.get(0)?,
            title: r.get(1)?,
            body_md: r.get(2)?,
            status: r.get(3)?,
            kind: r.get(4)?,
            urgent: r.get(5)?,
            important: r.get(6)?,
            estimate_min: r.get(7)?,
            planned_start_at: r.get(8)?,
            planned_end_at: r.get(9)?,
            key_result_id: r.get(10)?,
            external_url: r.get(11)?,
            created_at: r.get(12)?,
            updated_at: r.get(13)?,
            done_at: r.get(14)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// 範囲 [start, end) の `bound_at` を持つ `agent_session_window` を読む。
pub fn session_windows(conn: &Connection, range: Range) -> Result<Vec<AgentSessionWindow>> {
    let mut stmt = conn.prepare(
        "SELECT session_id, agent, hwnd, title_at_bind, bound_at FROM agent_session_window
         WHERE bound_at >= ?1 AND bound_at < ?2 ORDER BY bound_at, agent, session_id",
    )?;
    let rows = stmt.query_map(params![range.start, range.end], |r| {
        Ok(AgentSessionWindow {
            session_id: r.get(0)?,
            agent: r.get(1)?,
            hwnd: r.get(2)?,
            title_at_bind: r.get(3)?,
            bound_at: r.get(4)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// 範囲 [start, end) の `agent_session_window` を削除し、新しい行で置き換える。
/// トランザクションは張らない（呼び出し側が区間の置き換えと同じトランザクションで呼ぶ）。
pub fn replace_session_windows(conn: &Connection, range: Range, rows: &[AgentSessionWindow]) -> Result<()> {
    conn.execute(
        "DELETE FROM agent_session_window WHERE bound_at >= ?1 AND bound_at < ?2",
        params![range.start, range.end],
    )?;
    {
        let mut stmt = conn.prepare(
            "INSERT OR REPLACE INTO agent_session_window (session_id, agent, hwnd, title_at_bind, bound_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
        )?;
        for w in rows {
            stmt.execute(params![
                w.session_id,
                w.agent,
                w.hwnd,
                w.title_at_bind,
                w.bound_at
            ])?;
        }
    }
    Ok(())
}
