//! 割り当ての再計算（docs/dev/architecture.md「割り当てパイプライン」）。DB から生データを読み、パイプラインを実行して派生層を置き換える。
//!
//! 再計算の結果は「どの範囲をどの順で再計算してきたか」に依存しないようにする:
//! - 範囲は、境界をまたぐ既存区間と、範囲内の送信の対象期間の開始まで広げる。
//! - 自動判定の呼び出しは書き込みトランザクションの外で行い、最後の「範囲の確定 → 重なる区間の削除
//!   → 挿入」は 1 つの `BEGIN IMMEDIATE` トランザクションで行う。確定範囲が読み込み時から
//!   変わっていたら（他の再計算が書き込んでいたら）読み直してやり直す。

use crate::config::Config;
use crate::model::ActivitySegment;
use crate::pipeline::{self, norm_title, JudgeAnswer, PipelineInput, PipelineOutput, SegmentJudge};
use crate::repo::{raw, segment};
use crate::timefmt::Range;
use crate::{Millis, Result};
use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use std::cell::RefCell;
use std::collections::HashMap;

/// 範囲の手前から読む生データの量。対話の対象期間や AI の作業待ちの文脈のため。
const LOOKBACK_MS: Millis = 6 * 60 * 60 * 1000;
/// 対話の対象期間による範囲の拡張の上限（要求範囲の開始からどこまで遡るか）。
/// 1 晩放置したセッションへの送信で毎分の再計算が際限なく重くならないように。
const MAX_PERIOD_EXTEND_MS: Millis = 24 * 60 * 60 * 1000;
/// 範囲の確定の繰り返し上限（通常は 2〜3 回で収束する）。
const RESOLVE_ITER: usize = 16;
/// 楽観的再試行の上限。最後の 1 回は書き込みロックを取ったまま計算する。
const MAX_ATTEMPTS: usize = 4;
/// 自動判定キャッシュの保持期間。
const JUDGE_CACHE_TTL_MS: Millis = 30 * 24 * 60 * 60 * 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RebuildStats {
    /// 実際に再計算した範囲（境界をまたぐ既存区間や、対話の対象期間を含めて広げることがある）。
    pub range: Range,
    pub segments: usize,
    pub bindings: usize,
}

/// 範囲の派生層（`activity_segment` と `agent_session_window`）を作り直す。
pub fn rebuild(conn: &Connection, cfg: &Config, range: Range, now: Millis) -> Result<RebuildStats> {
    rebuild_with_judge(conn, cfg, range, now, None)
}

/// 自動判定つきの再計算。自動判定の答えは `judge_cache` にキャッシュし、同じ内容の区間は問い合わせない。
pub fn rebuild_with_judge(
    conn: &Connection,
    cfg: &Config,
    range: Range,
    now: Millis,
    judge: Option<&dyn SegmentJudge>,
) -> Result<RebuildStats> {
    let cache = match judge {
        Some(j) => Some(JudgeCache::new(j, cache_prefix(conn, cfg)?)),
        None => None,
    };
    for attempt in 1..=MAX_ATTEMPTS {
        if attempt == MAX_ATTEMPTS {
            // 最後は書き込みロックを取ったまま読み・計算・書き込みを行う（必ず終わらせる）。
            let tx = Transaction::new_unchecked(conn, TransactionBehavior::Immediate)?;
            let (resolved, input) = resolve_range(&tx, cfg, range, now)?;
            let out = run_pipeline(&tx, &input, cache.as_ref());
            let stats = write(&tx, resolved, &out, cache.as_ref(), now)?;
            tx.commit()?;
            return Ok(stats);
        }
        // 自動判定の呼び出しを含む計算は書き込みトランザクションの外で行う。
        let (resolved, input) = resolve_range(conn, cfg, range, now)?;
        let out = run_pipeline(conn, &input, cache.as_ref());
        let tx = Transaction::new_unchecked(conn, TransactionBehavior::Immediate)?;
        let (again, _) = resolve_range(&tx, cfg, range, now)?;
        if again != resolved {
            tracing::debug!(attempt, "再計算中に範囲が変わったため読み直します");
            continue; // tx は Drop でロールバックされる
        }
        let stats = write(&tx, resolved, &out, cache.as_ref(), now)?;
        tx.commit()?;
        return Ok(stats);
    }
    unreachable!("最後の試行は必ず return する")
}

fn run_pipeline(conn: &Connection, input: &PipelineInput, cache: Option<&JudgeCache<'_>>) -> PipelineOutput {
    match cache {
        Some(c) => pipeline::build(input, Some(&CachedJudge { cache: c, conn })),
        None => pipeline::build(input, None),
    }
}

fn write(
    conn: &Connection,
    range: Range,
    out: &PipelineOutput,
    cache: Option<&JudgeCache<'_>>,
    now: Millis,
) -> Result<RebuildStats> {
    segment::replace_range(conn, range, &out.segments)?;
    raw::replace_session_windows(conn, range, &out.bindings)?;
    if let Some(c) = cache {
        c.flush(conn, now)?;
    }
    Ok(RebuildStats {
        range,
        segments: out.segments.len(),
        bindings: out.bindings.len(),
    })
}

/// 再計算する範囲を確定し、その範囲のパイプライン入力を返す。
/// - 境界をまたぐ既存区間を含むところまで広げる（区間を重ねない・欠けさせない）。
/// - 範囲内の `prompt_submit` の対象期間の開始まで広げる（範囲より前から始まる対話を取りこぼさない）。
fn resolve_range(
    conn: &Connection,
    cfg: &Config,
    requested: Range,
    now: Millis,
) -> Result<(Range, PipelineInput)> {
    let floor = requested.start - MAX_PERIOD_EXTEND_MS;
    let mut range = requested;
    for _ in 0..RESOLVE_ITER {
        let mut ext = range;
        for s in segment::list(conn, range)? {
            ext.start = ext.start.min(s.start_at);
            ext.end = ext.end.max(s.end_at);
        }
        if ext != range {
            range = ext;
            continue;
        }
        let input = load_input(conn, cfg, range, now)?;
        match pipeline::earliest_period_start(&input) {
            Some(ps) if ps.max(floor) < range.start => {
                range = Range::new(ps.max(floor), range.end);
            }
            _ => return Ok((range, input)),
        }
    }
    tracing::warn!("再計算範囲が収束しませんでした");
    let input = load_input(conn, cfg, range, now)?;
    Ok((range, input))
}

// ---- 自動判定のキャッシュ ----

/// FNV-1a（64 bit）。キャッシュキー用の安定したハッシュ（Rust のバージョンに依存しない）。
fn fnv1a(parts: &[&str]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for p in parts {
        for b in p.bytes().chain(std::iter::once(0x1f)) {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    h
}

/// キーの共通部分: 自動判定（judge.default とその設定）、活動、未完了タスク集合。
/// これらが変わると同じ区間でも答えが変わり得るので、キャッシュを無効にする。
fn cache_prefix(conn: &Connection, cfg: &Config) -> Result<String> {
    let backend = cfg
        .judge
        .backends
        .get(&cfg.judge.default)
        .and_then(|b| serde_json::to_string(b).ok())
        .unwrap_or_default();
    let mut tasks: Vec<String> = raw::open_tasks(conn)?.into_iter().map(|t| t.id).collect();
    tasks.sort();
    let labels = cfg.activity.labels.join("\u{1f}");
    let tasks = tasks.join("\u{1f}");
    let h = fnv1a(&[&backend, &labels, &tasks]);
    Ok(format!("v1|{}|{h:016x}", cfg.judge.default))
}

/// 区間の内容のキー。開始・終了時刻は含めない（範囲の切り取りや進行中の区間の伸びで変わるため）。
fn segment_key(prefix: &str, s: &ActivitySegment) -> String {
    let mut ids = s.window_span_ids.clone();
    ids.sort();
    ids.dedup();
    let content = format!(
        "{}\u{1f}{}\u{1f}{}",
        s.exe_name.as_deref().unwrap_or(""),
        norm_title(s.title.as_deref()),
        ids.join(",")
    );
    format!("{prefix}|{:016x}", fnv1a(&[&content]))
}

fn answer_to_json(a: &JudgeAnswer) -> String {
    serde_json::json!({
        "task_id": a.task_id,
        "activity": a.activity,
        "is_interrupt": a.is_interrupt,
        "confidence": a.confidence,
    })
    .to_string()
}

fn answer_from_json(json: &str, judge_name: String) -> Option<JudgeAnswer> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let s = |k: &str| v.get(k).and_then(|x| x.as_str()).map(String::from);
    Some(JudgeAnswer {
        task_id: s("task_id"),
        activity: s("activity"),
        is_interrupt: v.get("is_interrupt").and_then(|x| x.as_bool()),
        confidence: v.get("confidence")?.as_f64()?,
        judge_name,
    })
}

/// 1 回の再計算（再試行を含む）の間に使う自動判定キャッシュ。
struct JudgeCache<'j> {
    inner: &'j dyn SegmentJudge,
    prefix: String,
    mem: RefCell<HashMap<String, JudgeAnswer>>,
    /// 今回新しく得た答えのキー（コミット時に DB へ書く）。
    fresh: RefCell<Vec<String>>,
}

impl<'j> JudgeCache<'j> {
    fn new(inner: &'j dyn SegmentJudge, prefix: String) -> Self {
        Self {
            inner,
            prefix,
            mem: RefCell::new(HashMap::new()),
            fresh: RefCell::new(Vec::new()),
        }
    }

    fn flush(&self, conn: &Connection, now: Millis) -> Result<()> {
        let mem = self.mem.borrow();
        let mut stmt = conn.prepare(
            "INSERT OR REPLACE INTO judge_cache (key, answer_json, judge_name, created_at) VALUES (?1, ?2, ?3, ?4)",
        )?;
        for k in self.fresh.borrow().iter() {
            if let Some(a) = mem.get(k) {
                stmt.execute(params![k, answer_to_json(a), a.judge_name, now])?;
            }
        }
        conn.execute(
            "DELETE FROM judge_cache WHERE created_at < ?1",
            [now - JUDGE_CACHE_TTL_MS],
        )?;
        drop(mem);
        self.fresh.borrow_mut().clear();
        Ok(())
    }
}

/// キャッシュを引き、無い区間だけを実際の自動判定に問い合わせる。
struct CachedJudge<'a, 'j> {
    cache: &'a JudgeCache<'j>,
    conn: &'a Connection,
}

impl CachedJudge<'_, '_> {
    fn lookup(&self, key: &str) -> Option<JudgeAnswer> {
        let row = self
            .conn
            .query_row(
                "SELECT answer_json, judge_name FROM judge_cache WHERE key = ?1",
                [key],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
            )
            .optional();
        match row {
            Ok(Some((json, name))) => answer_from_json(&json, name),
            Ok(None) => None,
            Err(e) => {
                tracing::warn!(error = %e, "自動判定キャッシュを読めません");
                None
            }
        }
    }
}

impl SegmentJudge for CachedJudge<'_, '_> {
    fn judge(&self, segs: &[ActivitySegment]) -> Vec<Option<JudgeAnswer>> {
        let c = self.cache;
        let keys: Vec<String> = segs.iter().map(|s| segment_key(&c.prefix, s)).collect();
        let mut misses: Vec<usize> = Vec::new();
        {
            let mut mem = c.mem.borrow_mut();
            for (i, k) in keys.iter().enumerate() {
                if mem.contains_key(k) || misses.iter().any(|&m| keys[m] == *k) {
                    continue;
                }
                match self.lookup(k) {
                    Some(a) => {
                        mem.insert(k.clone(), a);
                    }
                    None => misses.push(i),
                }
            }
        }
        if !misses.is_empty() {
            let ask: Vec<ActivitySegment> = misses.iter().map(|&i| segs[i].clone()).collect();
            let answers = c.inner.judge(&ask);
            let mut mem = c.mem.borrow_mut();
            let mut fresh = c.fresh.borrow_mut();
            for (&i, a) in misses.iter().zip(answers) {
                // 未判定（タイムアウト等）はキャッシュしない。次回また問い合わせる。
                if let Some(a) = a {
                    mem.insert(keys[i].clone(), a);
                    fresh.push(keys[i].clone());
                }
            }
        }
        let mem = c.mem.borrow();
        keys.iter().map(|k| mem.get(k).cloned()).collect()
    }
}

/// DB からパイプラインの入力を組み立てる。
pub fn load_input(conn: &Connection, cfg: &Config, range: Range, now: Millis) -> Result<PipelineInput> {
    let wide = Range::new(range.start - LOOKBACK_MS, range.end);
    let mut input = PipelineInput::new(range, now, cfg);
    input.windows = raw::window_spans(conn, wide)?;
    input.idles = raw::idle_spans(conn, wide)?;
    input.events = raw::agent_events(conn, wide)?;
    input.calendar = raw::calendar_events(conn, wide)?;
    input.confirmations = raw::segment_confirmations(conn, range)?;
    input.task_rules = raw::task_assign_rules(conn)?;
    input.activity_rules = raw::activity_rules(conn)?;
    input.doing_logs = raw::task_doing_logs(conn, wide)?;
    input.time_blocks = raw::time_blocks(conn, wide)?;
    input.tasks = raw::open_tasks(conn)?;
    Ok(input)
}

/// `range` 内の `prompt_submit` のうち、ウインドウに対応付けできなかったもの（診断用）。DB は変更しない。
pub fn unbound_prompts(
    conn: &Connection,
    cfg: &Config,
    range: Range,
    now: Millis,
) -> Result<Vec<pipeline::UnboundPrompt>> {
    let input = load_input(conn, cfg, range, now)?;
    Ok(pipeline::unbound_prompts(&input))
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
        let conn = db::open_in_memory().unwrap();
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
            "INSERT INTO task_assign_rule (id, task_id, field, pattern, pattern_type) VALUES ('R1', 'T1', 'title', 'docs', 'contains')",
            &[],
        );
        let ins_ev = "INSERT INTO agent_event (id, at, agent, session_id, kind, cwd) VALUES (?1, ?2, 'claude-code', 'S1', ?3, 'C:\\src\\x')";
        exec(&conn, ins_ev, &[&"E1", &M, &"session_start"]);
        exec(&conn, ins_ev, &[&"E2", &(2 * M), &"prompt_submit"]);
        exec(&conn, ins_ev, &[&"E3", &(5 * M), &"response_complete"]);
        exec(&conn, ins_ev, &[&"E4", &(45 * M), &"prompt_submit"]);

        let range = Range::new(0, 60 * M);
        let now = 50 * M;
        let stats = rebuild(&conn, &cfg, range, now).unwrap();
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
                (2, 5, AgentWait),
                (5, 10, AgentDialog),
                (10, 40, Work),
                (40, 45, AgentDialog),
                (45, 50, AgentWait),
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
        let again = rebuild(&conn, &cfg, Range::new(20 * M, 30 * M), now).unwrap();
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

    // ---- 再計算の履歴に依存しないこと ----

    /// (id, 開始分, 終了分, hwnd, exe, title)
    type Win = (&'static str, i64, i64, &'static str, &'static str, &'static str);
    /// (id, 時刻分, セッション, kind)
    type Ev = (&'static str, i64, &'static str, &'static str);

    const TERM: &str = "WindowsTerminal.exe";

    fn seed_rules(conn: &Connection) {
        exec(
            conn,
            "INSERT INTO task (id, title, created_at, updated_at) VALUES ('T1', 't', 0, 0)",
            &[],
        );
        exec(
            conn,
            "INSERT INTO task_assign_rule (id, task_id, field, pattern, pattern_type) VALUES ('R1', 'T1', 'title', 'docs', 'contains')",
            &[],
        );
    }

    /// 時刻 `now` までに収集器が書いているはずの生データを反映する（進行中のウインドウは end_at = NULL）。
    fn sync_raw(conn: &Connection, wins: &[Win], evs: &[Ev], now: i64) {
        for &(id, s, e, hwnd, exe, title) in wins {
            if s * M > now {
                continue;
            }
            let end = (e * M <= now).then_some(e * M);
            exec(
                conn,
                "INSERT OR REPLACE INTO window_span (id, start_at, end_at, hwnd, exe_name, title, excluded) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0)",
                &[&id, &(s * M), &end, &hwnd, &exe, &title],
            );
        }
        for &(id, at, sid, kind) in evs {
            if at * M > now {
                continue;
            }
            exec(
                conn,
                "INSERT OR IGNORE INTO agent_event (id, at, agent, session_id, kind) VALUES (?1, ?2, 'claude-code', ?3, ?4)",
                &[&id, &(at * M), &sid, &kind],
            );
        }
    }

    /// (開始, 終了, 種別, タスク, セッション)
    type Norm = (i64, i64, SegmentKind, Option<String>, Option<String>);

    /// 隣接する同一内容の区間をつないだもの。
    fn normalized(segs: &[ActivitySegment]) -> Vec<Norm> {
        let mut out: Vec<Norm> = Vec::new();
        for s in segs {
            let cur = (
                s.start_at,
                s.end_at,
                s.kind,
                s.task_id.clone(),
                s.agent_session_id.clone(),
            );
            match out.last_mut() {
                Some(l) if l.1 == cur.0 && (l.2, &l.3, &l.4) == (cur.2, &cur.3, &cur.4) => l.1 = cur.1,
                _ => out.push(cur),
            }
        }
        out
    }

    fn assert_no_overlap(conn: &Connection) {
        let segs = segment::list(conn, Range::new(i64::MIN / 2, i64::MAX / 2)).unwrap();
        for w in segs.windows(2) {
            assert!(
                w[0].end_at <= w[1].start_at,
                "区間が重なっている: {:?} と {:?}",
                (w[0].start_at / M, w[0].end_at / M),
                (w[1].start_at / M, w[1].end_at / M)
            );
        }
    }

    #[test]
    fn rolling_rebuild_matches_full_rebuild() {
        // レビューの再現: 範囲（直近 180 分）より前の応答完了から始まる対話対象期間。
        let wins: Vec<Win> = vec![
            ("W1", 0, 10, "H1", TERM, "claude"),
            ("W2", 10, 200, "H2", "chrome.exe", "docs"),
            ("W3", 200, 220, "H1", TERM, "claude"),
            ("W4", 220, 260, "H2", "chrome.exe", "docs"),
        ];
        let evs: Vec<Ev> = vec![
            ("E0", 0, "S", "session_start"),
            ("E1", 1, "S", "prompt_submit"),
            ("E2", 2, "S", "response_complete"),
            ("E3", 210, "S", "prompt_submit"),
            ("E4", 215, "S", "response_complete"),
        ];
        let cfg = Config::default();
        let window = cfg.collect.rebuild_window_min as i64 * M;

        // 常駐の定期再計算（直近 180 分）を 1 分ごとに模す
        let rolling = db::open_in_memory().unwrap();
        seed_rules(&rolling);
        let end = 260 * M;
        let mut now = M;
        while now <= end {
            sync_raw(&rolling, &wins, &evs, now);
            rebuild(&rolling, &cfg, Range::new(now - window, now), now).unwrap();
            now += M;
        }

        // 最後に全範囲で一度だけ作った場合
        let full = db::open_in_memory().unwrap();
        seed_rules(&full);
        sync_raw(&full, &wins, &evs, end);
        rebuild(&full, &cfg, Range::new(0, end), end).unwrap();

        let all = Range::new(0, end);
        let r = normalized(&segment::list(&rolling, all).unwrap());
        let f = normalized(&segment::list(&full, all).unwrap());
        assert_eq!(r, f);
        // 2〜10 分（範囲より前の応答完了から始まる対象期間）が対話になっている
        assert!(f
            .iter()
            .any(|s| (s.0, s.1, s.2) == (2 * M, 10 * M, SegmentKind::AgentDialog)));
        assert_no_overlap(&rolling);
    }

    #[test]
    fn rolling_range_extends_to_period_start() {
        // 直前の全範囲再計算の後、範囲外から始まる対象期間を持つ送信が来る
        let conn = db::open_in_memory().unwrap();
        seed_rules(&conn);
        let cfg = Config::default();
        let wins: Vec<Win> = vec![
            ("W1", 0, 10, "H1", TERM, "claude"),
            ("W2", 10, 200, "H2", "chrome.exe", "docs"),
            ("W3", 200, 220, "H1", TERM, "claude"),
        ];
        let mut evs: Vec<Ev> = vec![
            ("E0", 0, "S", "session_start"),
            ("E1", 1, "S", "prompt_submit"),
            ("E2", 2, "S", "response_complete"),
        ];
        sync_raw(&conn, &wins, &evs, 150 * M);
        rebuild(&conn, &cfg, Range::new(0, 150 * M), 150 * M).unwrap();
        evs.push(("E3", 210, "S", "prompt_submit"));
        sync_raw(&conn, &wins, &evs, 211 * M);
        let st = rebuild(&conn, &cfg, Range::new(31 * M, 211 * M), 211 * M).unwrap();
        assert_eq!(st.range.start, 2 * M);
        let segs = segment::list(&conn, Range::new(0, 300 * M)).unwrap();
        assert!(segs
            .iter()
            .any(|s| (s.start_at, s.end_at, s.kind) == (2 * M, 10 * M, SegmentKind::AgentDialog)));
        assert_no_overlap(&conn);
    }

    // ---- 同時実行 ----

    /// 判定のたびに一度だけ、別接続で再計算を割り込ませる自動判定。
    struct Interleave<'a> {
        other: std::cell::RefCell<Option<Box<dyn FnOnce() + 'a>>>,
    }

    impl SegmentJudge for Interleave<'_> {
        fn judge(&self, segs: &[ActivitySegment]) -> Vec<Option<JudgeAnswer>> {
            if let Some(f) = self.other.borrow_mut().take() {
                f();
            }
            vec![None; segs.len()]
        }
    }

    #[test]
    fn concurrent_rebuilds_do_not_overlap() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hitoiki.db");
        let a = db::open(&path).unwrap();
        let cfg = Config::default();
        let now = 95 * M;
        let wins: Vec<Win> = vec![
            ("W0", 0, 25, "H0", "notepad.exe", "docs"),
            ("W1", 25, 95, "H1", "chrome.exe", "mystery"),
        ];
        seed_rules(&a);
        sync_raw(&a, &wins, &[], now);

        // A が自動判定を呼んでいる間（書き込みトランザクションの外）に B が [0, 60) を書き込む。
        let path_b = path.clone();
        let cfg_b = cfg.clone();
        let judge = Interleave {
            other: std::cell::RefCell::new(Some(Box::new(move || {
                let b = db::open(&path_b).unwrap();
                rebuild(&b, &cfg_b, Range::new(0, 60 * M), now).unwrap();
            }))),
        };
        let st = rebuild_with_judge(&a, &cfg, Range::new(30 * M, 90 * M), now, Some(&judge)).unwrap();
        // B が書いた 25〜60 の区間が A の範囲の開始をまたぐので、A は範囲を広げてやり直す。
        assert_eq!(st.range, Range::new(25 * M, 90 * M));
        assert_no_overlap(&a);
        let segs = segment::list(&a, Range::new(0, now)).unwrap();
        let shape: Vec<_> = segs.iter().map(|s| (s.start_at / M, s.end_at / M)).collect();
        assert_eq!(shape, vec![(0, 25), (25, 90)]);
    }

    // ---- 自動判定のキャッシュ ----

    struct Counting {
        asked: std::cell::Cell<usize>,
    }

    impl SegmentJudge for Counting {
        fn judge(&self, segs: &[ActivitySegment]) -> Vec<Option<JudgeAnswer>> {
            self.asked.set(self.asked.get() + segs.len());
            segs.iter()
                .map(|_| {
                    Some(JudgeAnswer {
                        task_id: Some("T1".into()),
                        activity: Some("調査".into()),
                        is_interrupt: None,
                        confidence: 0.9,
                        judge_name: "count".into(),
                    })
                })
                .collect()
        }
    }

    #[test]
    fn judge_answers_are_cached() {
        let conn = db::open_in_memory().unwrap();
        exec(
            &conn,
            "INSERT INTO task (id, title, created_at, updated_at) VALUES ('T1', 't', 0, 0)",
            &[],
        );
        let mut cfg = Config::default();
        let wins: Vec<Win> = vec![
            ("W1", 0, 30, "H1", "chrome.exe", "mystery"),
            ("W2", 30, 40, "H2", "notepad.exe", "⠂ memo"),
            ("W3", 40, 200, "H1", "chrome.exe", "mystery"),
        ];
        let j = Counting {
            asked: std::cell::Cell::new(0),
        };
        // 毎分の定期再計算を模す。進行中のウインドウが伸びても同じ内容なら問い合わせない。
        for now in [50 * M, 51 * M, 52 * M, 53 * M] {
            sync_raw(&conn, &wins, &[], now);
            rebuild_with_judge(&conn, &cfg, Range::new(now - 180 * M, now), now, Some(&j)).unwrap();
        }
        assert_eq!(j.asked.get(), 3);
        let segs = segment::list(&conn, Range::new(0, 53 * M)).unwrap();
        assert!(segs
            .iter()
            .all(|s| s.decided_by.as_deref() == Some("judge:count")));
        assert!(segs.iter().all(|s| s.task_id.as_deref() == Some("T1")));

        // 未完了タスクの集合が変われば問い合わせ直す
        exec(
            &conn,
            "INSERT INTO task (id, title, created_at, updated_at) VALUES ('T2', 'u', 0, 0)",
            &[],
        );
        rebuild_with_judge(&conn, &cfg, Range::new(0, 53 * M), 53 * M, Some(&j)).unwrap();
        assert_eq!(j.asked.get(), 6);
        rebuild_with_judge(&conn, &cfg, Range::new(0, 53 * M), 53 * M, Some(&j)).unwrap();
        assert_eq!(j.asked.get(), 6);

        // 自動判定（judge.default）が変われば問い合わせ直す
        cfg.judge.default = "other".into();
        rebuild_with_judge(&conn, &cfg, Range::new(0, 53 * M), 53 * M, Some(&j)).unwrap();
        assert_eq!(j.asked.get(), 9);
    }
}
