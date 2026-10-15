//! 時間帯の範囲指定による実績の補正（docs/dev/architecture.md「実績の補正」）。
//!
//! 範囲に含まれる区間（と記録なしの時間帯）を候補として挙げ、チェックされたものを 1 つのタスク（または
//! 「タスクなし」「休憩」）へ付け替える。補正は `segment_confirmation`（ユーザー確定）として保存し、1 回の付け替えを
//! `segment_correction` でひとまとまりにして取り消せるようにする。実績を直接入力するのではなく、
//! 実績は常に `activity_segment` から算出する（確定が再計算でも最優先で適用される）。
//!
//! - 境界にまたがる区間は、範囲の内側の部分だけを対象にする（確定の開始・終了を範囲で切る）。
//! - 無操作（idle）と記録なしは、割り当てると種別 work の PC 外の時間（`off_pc`）として計上する。
//! - 記録なしの時間帯には区間が無いので、`off_pc` の確定から区間を作る（パイプラインが再計算でも作る）。

use super::{range_view, task_titles, Ctx, RangeView};
use crate::id::new_id;
use crate::model::{ActivitySegment, SegmentConfirmation, SegmentKind, SegmentStatus};
use crate::pipeline::SegmentJudge;
use crate::recording;
use crate::repo::{self, confirmation, correction as repo_corr};
use crate::timefmt::{self, Range};
use crate::{Error, Millis, Result};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// 記録なしの候補の ID の接頭辞（`uncollected:<開始のミリ秒>`）。
pub const UNCOLLECTED_PREFIX: &str = "uncollected:";

/// 候補の分類。初期のチェックと画面の表示に使う。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    /// 通常の作業（auto / needs_review の work・interrupt）。
    Work,
    /// エージェント対話・作業待ち（タスクだけ付け替え、種別は維持する）。
    AgentDialog,
    /// ユーザー確定済み。
    Confirmed,
    /// 会議（カレンダー由来）。
    Meeting,
    /// 無操作・休憩。
    Idle,
    /// 記録なし（収集器が動いていなかった時間帯）。
    Uncollected,
}

/// 初期チェックの方針（設定 `review.correct_include_*`。CLI のフラグで上書きできる）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Include {
    pub confirmed: bool,
    pub meeting: bool,
    pub idle: bool,
}

impl Include {
    pub fn from_config(ctx: &Ctx) -> Self {
        Self {
            confirmed: ctx.cfg.actual.correct_include_confirmed,
            meeting: ctx.cfg.actual.correct_include_meeting,
            idle: ctx.cfg.actual.correct_include_idle,
        }
    }
}

/// 補正の対象になりうる区間（範囲で切った後の部分）。
#[derive(Debug, Clone, Serialize)]
pub struct Candidate {
    /// 区間 ID。記録なしは `uncollected:<開始のミリ秒>`。
    pub id: String,
    pub uncollected: bool,
    /// 範囲で切った後の開始・終了。
    pub start_at: Millis,
    pub end_at: Millis,
    pub start: String,
    pub end: String,
    pub duration_ms: i64,
    /// 区間そのものの全体の長さ（範囲で切る前）。記録なしは `duration_ms` と同じ。
    pub full_duration_ms: i64,
    pub kind: Option<SegmentKind>,
    pub status: Option<SegmentStatus>,
    pub task_id: Option<String>,
    pub task_title: Option<String>,
    pub activity: Option<String>,
    pub exe_name: Option<String>,
    pub title: Option<String>,
    pub category: Category,
    /// すでに PC 外として計上されている区間。
    pub off_pc: bool,
    /// 割り当てると PC 外の時間（種別 work の `off_pc`）として計上されるか（無操作・休憩・記録なし）。
    pub becomes_off_pc: bool,
    /// 既定でチェックされるか。
    pub checked: bool,
}

/// 範囲内の候補の一覧。
#[derive(Debug, Clone, Serialize)]
pub struct CandidateList {
    pub range: RangeView,
    /// 範囲の端の吸着の幅（秒、`actual.snap_sec`）。
    pub snap_sec: u64,
    pub candidates: Vec<Candidate>,
}

fn classify(s: &ActivitySegment) -> Category {
    if s.status == SegmentStatus::Confirmed {
        return Category::Confirmed;
    }
    match s.kind {
        SegmentKind::Work | SegmentKind::Interrupt => Category::Work,
        SegmentKind::AgentDialog | SegmentKind::AgentWait => Category::AgentDialog,
        SegmentKind::Meeting => Category::Meeting,
        SegmentKind::Idle | SegmentKind::Break => Category::Idle,
        // 無視の区間は `candidates` が除くので、ここには来ない。
        SegmentKind::Ignored => Category::Idle,
    }
}

fn is_idle_like(kind: SegmentKind) -> bool {
    matches!(kind, SegmentKind::Idle | SegmentKind::Break)
}

/// 初期のチェック。該当する除外条件のすべてを満たしたときだけ ON（確定済みの会議は両方のスイッチが必要）。
fn default_checked(s: &ActivitySegment, inc: Include) -> bool {
    (s.status != SegmentStatus::Confirmed || inc.confirmed)
        && (s.kind != SegmentKind::Meeting || inc.meeting)
        && (!is_idle_like(s.kind) || inc.idle)
}

/// `[start, end)` のうち `covered`（開始順）に覆われていない部分。
pub(crate) fn uncovered(start: Millis, end: Millis, covered: &[(Millis, Millis)]) -> Vec<(Millis, Millis)> {
    let mut sorted = covered.to_vec();
    sorted.sort_unstable();
    let mut out = Vec::new();
    let mut cur = start;
    for (s, e) in sorted {
        if e <= cur {
            continue;
        }
        if s >= end {
            break;
        }
        if s > cur {
            out.push((cur, s.min(end)));
        }
        cur = cur.max(e);
        if cur >= end {
            break;
        }
    }
    if cur < end {
        out.push((cur, end));
    }
    out
}

/// 範囲 `[start_at, end_at)` の候補を、開始順に返す。
pub fn candidates(ctx: &Ctx, start_at: Millis, end_at: Millis, inc: Include) -> Result<CandidateList> {
    if start_at >= end_at {
        return Err(Error::InvalidArgument(
            "範囲の終了は開始より後にしてください".into(),
        ));
    }
    let tz = ctx.tz()?;
    let range = Range::new(start_at, end_at);
    let titles = task_titles(ctx.conn)?;
    let segs = repo::segment::list(ctx.conn, range)?;
    let mut out: Vec<Candidate> = Vec::new();
    for s in &segs {
        // 無視した区間は付け替えの対象にしない（数えない時間なので、割り当てると意図せず実績に入る）。
        if s.kind == SegmentKind::Ignored {
            continue;
        }
        let Some(r) = Range::new(s.start_at, s.end_at).intersect(&range) else {
            continue;
        };
        let category = classify(s);
        out.push(Candidate {
            id: s.id.clone(),
            uncollected: false,
            start_at: r.start,
            end_at: r.end,
            start: timefmt::fmt_rfc3339(tz, r.start),
            end: timefmt::fmt_rfc3339(tz, r.end),
            duration_ms: r.len(),
            full_duration_ms: s.end_at - s.start_at,
            kind: Some(s.kind),
            status: Some(s.status),
            task_id: s.task_id.clone(),
            task_title: s.task_id.as_ref().and_then(|id| titles.get(id).cloned()),
            activity: s.activity.clone(),
            exe_name: s.exe_name.clone(),
            title: s.title.clone(),
            category,
            off_pc: s.off_pc,
            becomes_off_pc: is_idle_like(s.kind),
            checked: default_checked(s, inc),
        });
    }
    // 記録なし（区間が無い時間帯）。すでに区間で覆われた部分（前の補正が作ったものなど）は除く。
    let covered: Vec<(Millis, Millis)> = segs.iter().map(|s| (s.start_at, s.end_at)).collect();
    for g in recording::uncollected(ctx.conn, range, ctx.now())? {
        for (s, e) in uncovered(g.start_at, g.end_at, &covered) {
            out.push(Candidate {
                id: format!("{UNCOLLECTED_PREFIX}{s}"),
                uncollected: true,
                start_at: s,
                end_at: e,
                start: timefmt::fmt_rfc3339(tz, s),
                end: timefmt::fmt_rfc3339(tz, e),
                duration_ms: e - s,
                full_duration_ms: e - s,
                kind: None,
                status: None,
                task_id: None,
                task_title: None,
                activity: None,
                exe_name: None,
                title: None,
                category: Category::Uncollected,
                off_pc: false,
                becomes_off_pc: true,
                checked: inc.idle,
            });
        }
    }
    out.sort_by(|a, b| a.start_at.cmp(&b.start_at).then_with(|| a.id.cmp(&b.id)));
    Ok(CandidateList {
        range: range_view(tz, range),
        snap_sec: ctx.cfg.actual.snap_sec,
        candidates: out,
    })
}

/// 補正の入力。割り当て先は `task` / `no_task` / `rest` のいずれか 1 つ（活動だけでもよい）。
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct CorrectionInput {
    pub start_at: Millis,
    pub end_at: Millis,
    /// 割り当てるタスク（ID の前方一致）。
    pub task: Option<String>,
    /// 「タスクなし」にする（元のタスクを外す）。
    pub no_task: bool,
    /// 「休憩」にする（種別 break）。
    pub rest: bool,
    pub activity: Option<String>,
    /// 対象にする候補の ID。None なら既定のチェック（`include_*` と `exclude`）に従う。
    pub selected: Option<Vec<String>>,
    /// 既定のチェックの上書き（None は設定 `review.correct_include_*`）。`selected` があれば使わない。
    pub include_confirmed: Option<bool>,
    pub include_meeting: Option<bool>,
    pub include_idle: Option<bool>,
    /// 既定のチェックから外す候補の ID（前方一致）。`selected` があれば使わない。
    pub exclude: Vec<String>,
}

/// 検証済みの割り当て先。
struct Target {
    task_id: Option<String>,
    task_title: Option<String>,
    no_task: bool,
    rest: bool,
    activity: Option<String>,
}

impl Target {
    fn resolve(ctx: &Ctx, input: &CorrectionInput) -> Result<Self> {
        let chosen = input.task.is_some() as u8 + input.no_task as u8 + input.rest as u8;
        let activity = input
            .activity
            .as_deref()
            .map(str::trim)
            .filter(|a| !a.is_empty())
            .map(String::from);
        if chosen > 1 {
            return Err(Error::InvalidArgument(
                "割り当て先はタスク・タスクなし・休憩のどれか 1 つにしてください".into(),
            ));
        }
        if chosen == 0 && activity.is_none() {
            return Err(Error::InvalidArgument(
                "割り当て先（--task / --no-task / --rest）か --activity が必要です".into(),
            ));
        }
        let (task_id, task_title) = match &input.task {
            Some(p) => {
                let t = repo::task::resolve(ctx.conn, p)?;
                (Some(t.id), Some(t.title))
            }
            None => (None, None),
        };
        Ok(Self {
            task_id,
            task_title,
            no_task: input.no_task,
            rest: input.rest,
            activity,
        })
    }

    /// 割り当て先の表示（「タスク X に」の X の部分を含む）。
    fn label(&self) -> String {
        let base = if let Some(t) = &self.task_title {
            format!("タスク {t}")
        } else if self.no_task {
            "「タスクなし」".to_string()
        } else if self.rest {
            "「休憩」".to_string()
        } else {
            "同じタスク".to_string()
        };
        match &self.activity {
            Some(a) => format!("{base}（活動 {a}）"),
            None => base,
        }
    }

    /// 候補 1 件分の確定。
    fn make_confirmation(&self, c: &Candidate, correction_id: &str, now: Millis) -> SegmentConfirmation {
        let kind = if self.rest {
            Some(SegmentKind::Break)
        } else if c.becomes_off_pc {
            Some(SegmentKind::Work)
        } else {
            None
        };
        let task_id = if let Some(t) = &self.task_id {
            Some(t.clone())
        } else if self.no_task || self.rest {
            Some(String::new())
        } else {
            None
        };
        SegmentConfirmation {
            id: new_id(),
            start_at: c.start_at,
            end_at: c.end_at,
            kind,
            task_id,
            activity: self.activity.clone(),
            created_at: now,
            off_pc: c.becomes_off_pc,
            correction_id: Some(correction_id.to_string()),
        }
    }
}

/// 確定前の見積もり。
#[derive(Debug, Clone, Serialize)]
pub struct Preview {
    pub range: RangeView,
    /// 割り当て先の表示。
    pub target: String,
    /// 対象にする区間の数と合計時間。
    pub selected_count: usize,
    pub total_ms: i64,
    /// うち PC 外（無操作・記録なし）の時間。
    pub off_pc_ms: i64,
    /// 外した（チェックされていない）候補の数。
    pub excluded_count: usize,
    /// 「タスク X に 1時間12分を割り当て（うち PC 外 15分）。外した区間 3 件」の形の説明。
    pub summary: String,
    /// 全候補（`checked` が最終的な選択を表す）。
    pub candidates: Vec<Candidate>,
}

/// 秒まで見る日本語の時間表示（`1時間12分`、`45分`、`30秒`）。
pub fn fmt_jp_duration(ms: Millis) -> String {
    let secs = (ms.max(0) + 500) / 1000;
    if secs < 60 {
        return format!("{secs}秒");
    }
    let min = (secs + 30) / 60;
    match (min / 60, min % 60) {
        (0, m) => format!("{m}分"),
        (h, 0) => format!("{h}時間"),
        (h, m) => format!("{h}時間{m}分"),
    }
}

/// 前方一致（記録なしは完全一致）で候補の ID を 1 つに解決する。
fn resolve_candidate_id(cands: &[Candidate], prefix: &str) -> Result<String> {
    let p = prefix.trim();
    if p.is_empty() {
        return Err(Error::InvalidArgument("区間 ID が空です".into()));
    }
    let upper = p.to_ascii_uppercase();
    let hits: Vec<&Candidate> = cands
        .iter()
        .filter(|c| c.id == p || (!c.uncollected && c.id.starts_with(&upper)))
        .collect();
    match hits.len() {
        0 => Err(Error::NotFound(format!("範囲内の区間 {prefix}"))),
        1 => Ok(hits[0].id.clone()),
        _ => match hits.iter().find(|c| c.id == p || c.id == upper) {
            Some(c) => Ok(c.id.clone()),
            None => Err(Error::Ambiguous(format!(
                "{prefix}: {}",
                hits.iter().map(|c| c.id.as_str()).collect::<Vec<_>>().join(", ")
            ))),
        },
    }
}

/// 候補と入力から、最終的な選択を反映した候補の一覧を作る。
fn select(ctx: &Ctx, input: &CorrectionInput) -> Result<CandidateList> {
    let base = Include::from_config(ctx);
    let inc = Include {
        confirmed: input.include_confirmed.unwrap_or(base.confirmed),
        meeting: input.include_meeting.unwrap_or(base.meeting),
        idle: input.include_idle.unwrap_or(base.idle),
    };
    let mut list = candidates(ctx, input.start_at, input.end_at, inc)?;
    match &input.selected {
        Some(ids) => {
            let mut chosen = Vec::new();
            for id in ids {
                chosen.push(resolve_candidate_id(&list.candidates, id)?);
            }
            for c in &mut list.candidates {
                c.checked = chosen.contains(&c.id);
            }
        }
        None => {
            let mut excluded = Vec::new();
            for p in &input.exclude {
                excluded.push(resolve_candidate_id(&list.candidates, p)?);
            }
            for c in &mut list.candidates {
                if excluded.contains(&c.id) {
                    c.checked = false;
                }
            }
        }
    }
    Ok(list)
}

fn build_preview(target: &Target, list: CandidateList) -> Preview {
    let mut total = 0;
    let mut off_pc = 0;
    let mut selected = 0;
    for c in list.candidates.iter().filter(|c| c.checked) {
        selected += 1;
        total += c.duration_ms;
        if c.becomes_off_pc {
            off_pc += c.duration_ms;
        }
    }
    let excluded = list.candidates.len() - selected;
    let summary = if selected == 0 {
        format!("{} に割り当てる区間がありません。", target.label())
    } else {
        let mut s = format!("{} に {}を割り当て", target.label(), fmt_jp_duration(total));
        if off_pc > 0 {
            s.push_str(&format!("（うち PC 外 {}）", fmt_jp_duration(off_pc)));
        }
        s.push('。');
        if excluded > 0 {
            s.push_str(&format!("外した区間 {excluded} 件"));
        }
        s
    };
    Preview {
        range: list.range,
        target: target.label(),
        selected_count: selected,
        total_ms: total,
        off_pc_ms: off_pc,
        excluded_count: excluded,
        summary,
        candidates: list.candidates,
    }
}

/// 確定前のプレビュー（DB は変更しない）。
pub fn preview(ctx: &Ctx, input: &CorrectionInput) -> Result<Preview> {
    let target = Target::resolve(ctx, input)?;
    let list = select(ctx, input)?;
    Ok(build_preview(&target, list))
}

/// 補正の JSON 表現。
#[derive(Debug, Clone, Serialize)]
pub struct CorrectionView {
    pub id: String,
    pub start_at: Millis,
    pub end_at: Millis,
    pub start: String,
    pub end: String,
    pub task_id: Option<String>,
    pub task_title: Option<String>,
    /// `break` のとき休憩への補正。
    pub kind: Option<SegmentKind>,
    pub activity: Option<String>,
    pub total_ms: i64,
    pub off_pc_ms: i64,
    pub segment_count: i64,
    pub created_at: Millis,
    pub created: String,
}

fn view(tz: chrono_tz::Tz, c: &repo_corr::Correction, titles: &HashMap<String, String>) -> CorrectionView {
    CorrectionView {
        id: c.id.clone(),
        start_at: c.start_at,
        end_at: c.end_at,
        start: timefmt::fmt_rfc3339(tz, c.start_at),
        end: timefmt::fmt_rfc3339(tz, c.end_at),
        task_id: c.task_id.clone(),
        task_title: c.task_id.as_ref().and_then(|id| titles.get(id).cloned()),
        kind: c.kind,
        activity: c.activity.clone(),
        total_ms: c.total_ms,
        off_pc_ms: c.off_pc_ms,
        segment_count: c.segment_count,
        created_at: c.created_at,
        created: timefmt::fmt_rfc3339(tz, c.created_at),
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ApplyResult {
    pub correction: CorrectionView,
    pub preview: Preview,
    pub confirmation_ids: Vec<String>,
}

/// 補正を確定する。チェックされた候補ごとに確定を作り、`activity_segment` にも即時に反映する
/// （範囲で切って、記録なしの部分には区間を作る）。1 つのトランザクションで行う。
pub fn apply(ctx: &Ctx, input: &CorrectionInput) -> Result<ApplyResult> {
    let tz = ctx.tz()?;
    let target = Target::resolve(ctx, input)?;
    let list = select(ctx, input)?;
    let preview = build_preview(&target, list);
    if preview.selected_count == 0 {
        return Err(Error::InvalidArgument("補正する区間がありません".into()));
    }
    let now = ctx.now();
    let correction = repo_corr::Correction {
        id: new_id(),
        start_at: input.start_at,
        end_at: input.end_at,
        task_id: target.task_id.clone(),
        kind: target.rest.then_some(SegmentKind::Break),
        activity: target.activity.clone(),
        total_ms: preview.total_ms,
        off_pc_ms: preview.off_pc_ms,
        segment_count: preview.selected_count as i64,
        created_at: now,
    };
    let tx = ctx.conn.unchecked_transaction()?;
    repo_corr::insert(ctx.conn, &correction)?;
    let mut confirmation_ids = Vec::new();
    for c in preview.candidates.iter().filter(|c| c.checked) {
        let ov = target.make_confirmation(c, &correction.id, now);
        confirmation::insert(ctx.conn, &ov)?;
        patch_derived(ctx.conn, &ov, now)?;
        confirmation_ids.push(ov.id);
    }
    tx.commit()?;
    let titles = task_titles(ctx.conn)?;
    Ok(ApplyResult {
        correction: view(tz, &correction, &titles),
        preview,
        confirmation_ids,
    })
}

/// 区間を `t` で 2 つに分ける（`t` が区間の内側にあるときだけ）。後ろ側が新しい ID の行になる。
fn split_row(conn: &Connection, t: Millis) -> Result<()> {
    let ids: Vec<String> = {
        let mut stmt = conn.prepare("SELECT id FROM activity_segment WHERE start_at < ?1 AND end_at > ?1")?;
        let rows = stmt.query_map([t], |r| r.get(0))?;
        rows.collect::<rusqlite::Result<_>>()?
    };
    for id in ids {
        conn.execute(
            "INSERT INTO activity_segment (id, start_at, end_at, kind, task_id, activity, confidence, \
             decided_by, status, window_span_ids, agent_session_id, exe_name, title, off_pc) \
             SELECT ?2, ?3, end_at, kind, task_id, activity, confidence, decided_by, status, \
             window_span_ids, agent_session_id, exe_name, title, off_pc FROM activity_segment WHERE id = ?1",
            params![id, new_id(), t],
        )?;
        conn.execute(
            "UPDATE activity_segment SET end_at = ?2 WHERE id = ?1",
            params![id, t],
        )?;
    }
    Ok(())
}

/// 確定を派生層へ即時に反映する（次の再計算でも同じ結果になるはずの内容）。
/// 範囲の端で区間を切り、内側を更新し、`off_pc` なら区間の無い部分に区間を作る。
fn patch_derived(conn: &Connection, ov: &SegmentConfirmation, now: Millis) -> Result<()> {
    split_row(conn, ov.start_at)?;
    split_row(conn, ov.end_at)?;
    conn.execute(
        "UPDATE activity_segment SET kind = COALESCE(?3, kind), \
         task_id = CASE WHEN ?4 = '' THEN NULL ELSE COALESCE(?4, task_id) END, \
         activity = COALESCE(?5, activity), status = 'confirmed', decided_by = 'confirmation', \
         confidence = 1.0, off_pc = CASE WHEN ?6 THEN 1 ELSE off_pc END \
         WHERE start_at >= ?1 AND end_at <= ?2",
        params![
            ov.start_at,
            ov.end_at,
            ov.kind,
            ov.task_id,
            ov.activity,
            ov.off_pc
        ],
    )?;
    if ov.off_pc {
        let hi = ov.end_at.min(now);
        if ov.start_at < hi {
            let covered: Vec<(Millis, Millis)> = repo::segment::list(conn, Range::new(ov.start_at, hi))?
                .iter()
                .map(|s| (s.start_at, s.end_at))
                .collect();
            for (s, e) in uncovered(ov.start_at, hi, &covered) {
                conn.execute(
                    "INSERT INTO activity_segment (id, start_at, end_at, kind, task_id, activity, \
                     confidence, decided_by, status, window_span_ids, off_pc) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1.0, 'confirmation', 'confirmed', '[]', 1)",
                    params![
                        new_id(),
                        s,
                        e,
                        ov.kind.unwrap_or(SegmentKind::Work),
                        ov.task_id.as_deref().filter(|t| !t.is_empty()),
                        ov.activity
                    ],
                )?;
            }
        }
    }
    Ok(())
}

/// 無操作の期間 [start, end) を、タスクの PC 外の作業として確定する（通知「{タスク名}をしていた」用）。
/// 範囲指定の補正（[`apply`]）で無操作・記録なしを選んだときと同じ形で保存する: 種別 work・`off_pc` の
/// 確定を期間全体に作り、派生層にも即時に反映する（区間の無い部分には区間を作る）。
/// 補正（`segment_correction`）としても残すので、補正の一覧から取り消せる。戻り値は期間に収まる確定済み区間の数。
pub fn assign_off_pc_range(
    conn: &Connection,
    start: Millis,
    end: Millis,
    task_id: &str,
    now: Millis,
) -> Result<usize> {
    if start >= end {
        return Err(Error::InvalidArgument("期間が不正です".into()));
    }
    repo::task::get(conn, task_id)?;
    let tx = conn.unchecked_transaction()?;
    let correction = repo_corr::Correction {
        id: new_id(),
        start_at: start,
        end_at: end,
        task_id: Some(task_id.to_string()),
        kind: None,
        activity: None,
        total_ms: end - start,
        off_pc_ms: end - start,
        segment_count: 1,
        created_at: now,
    };
    repo_corr::insert(conn, &correction)?;
    let ov = SegmentConfirmation {
        id: new_id(),
        start_at: start,
        end_at: end,
        kind: Some(SegmentKind::Work),
        task_id: Some(task_id.to_string()),
        activity: None,
        created_at: now,
        off_pc: true,
        correction_id: Some(correction.id.clone()),
    };
    confirmation::insert(conn, &ov)?;
    patch_derived(conn, &ov, now)?;
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM activity_segment WHERE start_at >= ?1 AND end_at <= ?2 AND status = 'confirmed'",
        params![start, end],
        |r| r.get(0),
    )?;
    conn.execute(
        "UPDATE segment_correction SET segment_count = ?2 WHERE id = ?1",
        params![correction.id, n.max(1)],
    )?;
    tx.commit()?;
    Ok(n as usize)
}

/// 補正の一覧の絞り込み。
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct CorrectionFilter {
    /// タスク ID（前方一致）。
    pub task: Option<String>,
    /// `today` / `yesterday` / `YYYY-MM-DD` など。その日と重なる補正だけ。
    pub date: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CorrectionList {
    pub count: usize,
    pub corrections: Vec<CorrectionView>,
}

/// 補正の一覧（新しい順）。
pub fn list(ctx: &Ctx, filter: &CorrectionFilter) -> Result<CorrectionList> {
    let tz = ctx.tz()?;
    let task_id = match &filter.task {
        Some(p) => Some(repo::task::resolve(ctx.conn, p)?.id),
        None => None,
    };
    let range = match &filter.date {
        Some(d) => Some(timefmt::parse_period(tz, d, ctx.now())?),
        None => None,
    };
    let titles = task_titles(ctx.conn)?;
    let corrections: Vec<CorrectionView> = repo_corr::list(ctx.conn, range, task_id.as_deref())?
        .iter()
        .map(|c| view(tz, c, &titles))
        .collect();
    Ok(CorrectionList {
        count: corrections.len(),
        corrections,
    })
}

#[derive(Debug, Clone, Serialize)]
pub struct UndoResult {
    pub correction: CorrectionView,
    /// 削除した確定の数。
    pub confirmations_removed: usize,
    /// 再計算した範囲。
    pub range: RangeView,
}

/// 補正を取り消す: その補正で作った確定を削除して、範囲を再計算する。
/// `judge` は再計算で使う自動判定（無ければルールのみ）。
pub fn undo(ctx: &Ctx, id: &str, judge: Option<&dyn SegmentJudge>) -> Result<UndoResult> {
    let tz = ctx.tz()?;
    let c = repo_corr::resolve(ctx.conn, id)?;
    let titles = task_titles(ctx.conn)?;
    let tx = ctx.conn.unchecked_transaction()?;
    let removed = repo_corr::delete(ctx.conn, &c.id)?;
    tx.commit()?;
    let stats = crate::rebuild::rebuild_with_judge(
        ctx.conn,
        ctx.cfg,
        Range::new(c.start_at, c.end_at),
        ctx.now(),
        judge,
    )?;
    Ok(UndoResult {
        correction: view(tz, &c, &titles),
        confirmations_removed: removed,
        range: range_view(tz, stats.range),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo::collect as runs;
    use crate::service::task::{self, TaskInput};
    use crate::service::testutil::{insert_segment, Env};

    const MIN: Millis = 60_000;

    fn mk_task(ctx: &Ctx, title: &str) -> String {
        task::add(
            ctx,
            &TaskInput {
                title: Some(title.into()),
                ..Default::default()
            },
        )
        .unwrap()
        .task
        .id
    }

    /// 07:00 を 0 分とした分で区間を作る。
    /// W 0-20 作業(要確認) / D 20-30 対話(auto) / C 30-40 作業(確定) / M 40-50 会議 / I 50-60 無操作、60-120 は記録なし。
    fn setup(env: &Env) -> Millis {
        let ctx = env.ctx();
        let m0 = ctx.now() - 300 * MIN;
        let ins = |id: &str, s: i64, e: i64, k: SegmentKind, st: SegmentStatus| {
            insert_segment(ctx.conn, id, m0 + s * MIN, m0 + e * MIN, k, None, st)
        };
        ins("WWWW", 0, 20, SegmentKind::Work, SegmentStatus::NeedsReview);
        ins("DDDD", 20, 30, SegmentKind::AgentDialog, SegmentStatus::Auto);
        ins("CCCC", 30, 40, SegmentKind::Work, SegmentStatus::Confirmed);
        ins("MMMM", 40, 50, SegmentKind::Meeting, SegmentStatus::Auto);
        ins("IIII", 50, 60, SegmentKind::Idle, SegmentStatus::NeedsReview);
        runs::begin_run(ctx.conn, m0).unwrap();
        runs::touch_run(ctx.conn, m0 + 60 * MIN, true).unwrap();
        runs::begin_run(ctx.conn, m0 + 120 * MIN).unwrap();
        runs::touch_run(ctx.conn, ctx.now() - 1_000, false).unwrap();
        m0
    }

    fn input(m0: Millis, s: i64, e: i64, task: &str) -> CorrectionInput {
        CorrectionInput {
            start_at: m0 + s * MIN,
            end_at: m0 + e * MIN,
            task: Some(task.into()),
            ..Default::default()
        }
    }

    fn cand<'a>(p: &'a Preview, id: &str) -> &'a Candidate {
        p.candidates.iter().find(|c| c.id.starts_with(id)).unwrap()
    }

    fn segs(env: &Env, m0: Millis, s: i64, e: i64) -> Vec<ActivitySegment> {
        repo::segment::list(&env.conn, Range::new(m0 + s * MIN, m0 + e * MIN)).unwrap()
    }

    #[test]
    fn 初期のチェックの規則と範囲での切り出し() {
        let env = Env::new();
        let m0 = setup(&env);
        let ctx = env.ctx();
        let t = mk_task(&ctx, "T");
        let p = preview(&ctx, &input(m0, 10, 90, &t)).unwrap();
        // 範囲の外の部分は切る（W は 10-20 の 10 分だけ）。
        let w = cand(&p, "WWWW");
        assert_eq!(
            (w.start_at, w.end_at, w.duration_ms),
            (m0 + 10 * MIN, m0 + 20 * MIN, 10 * MIN)
        );
        assert_eq!(w.full_duration_ms, 20 * MIN);
        let on: Vec<(&str, bool)> = p.candidates.iter().map(|c| (c.id.as_str(), c.checked)).collect();
        let gap_id = format!("uncollected:{}", m0 + 60 * MIN);
        assert_eq!(
            on,
            vec![
                ("WWWW", true),
                ("DDDD", true),
                ("CCCC", false),
                ("MMMM", false),
                ("IIII", false),
                (gap_id.as_str(), false)
            ]
        );
        assert_eq!(cand(&p, "WWWW").category, Category::Work);
        assert_eq!(cand(&p, "DDDD").category, Category::AgentDialog);
        assert_eq!(cand(&p, "CCCC").category, Category::Confirmed);
        assert_eq!(cand(&p, "MMMM").category, Category::Meeting);
        assert_eq!(cand(&p, "IIII").category, Category::Idle);
        let g = p.candidates.last().unwrap();
        assert_eq!(
            (g.category, g.start_at, g.end_at),
            (Category::Uncollected, m0 + 60 * MIN, m0 + 90 * MIN)
        );
        // 合計: 10 + 10 分。外した 4 件。
        assert_eq!(
            (p.selected_count, p.total_ms, p.off_pc_ms, p.excluded_count),
            (2, 20 * MIN, 0, 4)
        );
    }

    #[test]
    fn 設定とフラグで初期のチェックを変えられる() {
        let mut env = Env::new();
        let m0 = setup(&env);
        env.cfg.actual.correct_include_idle = true;
        let ctx = env.ctx();
        let t = mk_task(&ctx, "T");
        let p = preview(&ctx, &input(m0, 10, 90, &t)).unwrap();
        assert!(cand(&p, "IIII").checked && p.candidates.last().unwrap().checked);
        assert!(!cand(&p, "CCCC").checked && !cand(&p, "MMMM").checked);
        // フラグは設定を上書きする。
        let mut i = input(m0, 10, 90, &t);
        i.include_idle = Some(false);
        i.include_confirmed = Some(true);
        i.include_meeting = Some(true);
        let p = preview(&ctx, &i).unwrap();
        assert!(!cand(&p, "IIII").checked);
        assert!(cand(&p, "CCCC").checked && cand(&p, "MMMM").checked);
    }

    #[test]
    fn 通常の作業とエージェント対話を付け替え_確定済みは守る() {
        let env = Env::new();
        let m0 = setup(&env);
        let ctx = env.ctx();
        let t = mk_task(&ctx, "T");
        let r = apply(&ctx, &input(m0, 10, 90, &t)).unwrap();
        assert_eq!(
            (
                r.correction.total_ms,
                r.correction.off_pc_ms,
                r.correction.segment_count
            ),
            (20 * MIN, 0, 2)
        );
        let all = segs(&env, m0, 0, 60);
        let by = |s: i64| all.iter().find(|x| x.start_at == m0 + s * MIN).unwrap();
        // W は 10 分で切られ、前半(0-10)は元のまま、後半(10-20)だけ付け替え。
        assert_eq!(by(0).task_id, None);
        assert_eq!(by(0).status, SegmentStatus::NeedsReview);
        assert_eq!(by(10).task_id.as_deref(), Some(t.as_str()));
        assert_eq!(
            (by(10).status, by(10).decided_by.as_deref()),
            (SegmentStatus::Confirmed, Some("confirmation"))
        );
        // agent_dialog は kind を維持してタスクだけ付く。
        assert_eq!(by(20).kind, SegmentKind::AgentDialog);
        assert_eq!(by(20).task_id.as_deref(), Some(t.as_str()));
        // 確定済み・会議・無操作は触らない。
        assert_eq!(by(30).task_id, None);
        assert_eq!(by(40).kind, SegmentKind::Meeting);
        assert_eq!(by(40).task_id, None);
        assert_eq!(by(50).kind, SegmentKind::Idle);
        // 確定は範囲で切った期間で保存され、補正に結び付く。
        // 同じミリ秒に作った確定の並び（created_at, id）は ULID の乱数部で決まるので、開始時刻で並べ直す。
        let mut ovs = repo::confirmation::list(ctx.conn, Range::new(m0, m0 + 200 * MIN)).unwrap();
        ovs.sort_by_key(|o| o.start_at);
        assert_eq!(ovs.len(), 2);
        assert!(ovs
            .iter()
            .all(|o| o.correction_id.as_deref() == Some(r.correction.id.as_str()) && !o.off_pc));
        assert_eq!((ovs[0].start_at, ovs[0].end_at), (m0 + 10 * MIN, m0 + 20 * MIN));
    }

    #[test]
    fn 確定済みもチェックすれば上書きできる() {
        let env = Env::new();
        let m0 = setup(&env);
        let ctx = env.ctx();
        let t = mk_task(&ctx, "T");
        let mut i = input(m0, 30, 40, &t);
        assert!(matches!(apply(&ctx, &i), Err(Error::InvalidArgument(_)))); // 既定では対象なし
        i.selected = Some(vec!["CCCC".into()]);
        apply(&ctx, &i).unwrap();
        assert_eq!(segs(&env, m0, 30, 40)[0].task_id.as_deref(), Some(t.as_str()));
    }

    #[test]
    fn 無操作と記録なしはpc外の作業として計上しプレビューに出る() {
        let env = Env::new();
        let m0 = setup(&env);
        let ctx = env.ctx();
        let t = mk_task(&ctx, "仕事");
        let mut i = input(m0, 10, 90, &t);
        i.include_idle = Some(true);
        let p = preview(&ctx, &i).unwrap();
        // W10 + D10 + I10 + 記録なし30 = 60 分、うち PC 外 40 分。外したのは確定済みと会議の 2 件。
        assert_eq!(
            (p.total_ms, p.off_pc_ms, p.excluded_count),
            (60 * MIN, 40 * MIN, 2)
        );
        assert_eq!(
            p.summary,
            "タスク 仕事 に 1時間を割り当て（うち PC 外 40分）。外した区間 2 件"
        );
        // プレビューは DB を変更しない。
        assert!(repo_corr::list(ctx.conn, None, None).unwrap().is_empty());

        apply(&ctx, &i).unwrap();
        let idle = segs(&env, m0, 50, 60);
        assert_eq!(idle.len(), 1);
        assert_eq!((idle[0].kind, idle[0].off_pc), (SegmentKind::Work, true));
        assert_eq!(idle[0].task_id.as_deref(), Some(t.as_str()));
        // 記録なしの時間帯に区間ができる（範囲の内側だけ）。
        let uncollected = segs(&env, m0, 60, 120);
        assert_eq!(uncollected.len(), 1);
        assert_eq!(
            (uncollected[0].start_at, uncollected[0].end_at),
            (m0 + 60 * MIN, m0 + 90 * MIN)
        );
        assert_eq!(
            (uncollected[0].kind, uncollected[0].off_pc, uncollected[0].status),
            (SegmentKind::Work, true, SegmentStatus::Confirmed)
        );
        // 通常の作業に off_pc は付かない。
        assert!(!segs(&env, m0, 10, 20)[0].off_pc);
        // ログの「記録なし」からも、計上した部分は消える（残りの 90-120 だけ）。
        let l = crate::service::actual::list(
            &ctx,
            &crate::service::actual::ActualListInput {
                range: crate::service::RangeArgs {
                    date: None,
                    from: Some(timefmt::fmt_rfc3339(ctx.tz().unwrap(), m0)),
                    to: Some(timefmt::fmt_rfc3339(ctx.tz().unwrap(), m0 + 150 * MIN)),
                },
                raw: false,
            },
        )
        .unwrap();
        assert_eq!(l.uncollected.len(), 1);
        assert_eq!(
            (l.uncollected[0].start_at, l.uncollected[0].end_at),
            (m0 + 90 * MIN, m0 + 120 * MIN)
        );
        // 補正済みの記録なしは、もう記録なしの候補ではない（確定済みの区間として出る）。
        let again = candidates(&ctx, m0 + 60 * MIN, m0 + 90 * MIN, Include::default()).unwrap();
        assert_eq!(again.candidates.len(), 1);
        assert_eq!(
            (again.candidates[0].uncollected, again.candidates[0].category),
            (false, Category::Confirmed)
        );
    }

    #[test]
    fn pc外の計上は再計算しても残り_取り消しで消える() {
        let env = Env::new();
        let m0 = setup(&env);
        let ctx = env.ctx();
        let t = mk_task(&ctx, "T");
        let mut i = input(m0, 50, 90, &t);
        i.include_idle = Some(true);
        let r = apply(&ctx, &i).unwrap();
        assert_eq!(r.correction.off_pc_ms, 40 * MIN);
        let range = Range::new(m0, m0 + 150 * MIN);

        // 生データが無い範囲を再計算しても、確定から区間が作られる（無操作と記録なしは連続して 1 つになる）。
        crate::rebuild::rebuild(ctx.conn, ctx.cfg, range, ctx.now()).unwrap();
        let all = repo::segment::list(ctx.conn, range).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!((all[0].start_at, all[0].end_at), (m0 + 50 * MIN, m0 + 90 * MIN));
        assert_eq!((all[0].kind, all[0].off_pc), (SegmentKind::Work, true));
        assert_eq!(all[0].task_id.as_deref(), Some(t.as_str()));

        // 一覧に出て、取り消すと確定も区間も消える。
        let l = list(
            &ctx,
            &CorrectionFilter {
                task: Some(t.clone()),
                date: None,
            },
        )
        .unwrap();
        assert_eq!(l.count, 1);
        assert_eq!(l.corrections[0].id, r.correction.id);
        let u = undo(&ctx, &r.correction.id[..8], None).unwrap();
        assert_eq!(u.confirmations_removed, 2);
        assert!(list(&ctx, &CorrectionFilter::default())
            .unwrap()
            .corrections
            .is_empty());
        assert!(repo::confirmation::list(ctx.conn, range).unwrap().is_empty());
        assert!(repo::segment::list(ctx.conn, range).unwrap().is_empty());
        assert!(matches!(
            undo(&ctx, &r.correction.id, None),
            Err(Error::NotFound(_))
        ));
    }

    #[test]
    fn タスクなしと休憩へ付け替える() {
        let env = Env::new();
        let m0 = setup(&env);
        let ctx = env.ctx();
        let t = mk_task(&ctx, "T");
        apply(&ctx, &input(m0, 0, 20, &t)).unwrap();
        assert_eq!(segs(&env, m0, 0, 20)[0].task_id.as_deref(), Some(t.as_str()));

        // 確定済みも対象にして「タスクなし」にする。
        let mut i = CorrectionInput {
            start_at: m0,
            end_at: m0 + 20 * MIN,
            no_task: true,
            include_confirmed: Some(true),
            ..Default::default()
        };
        let p = preview(&ctx, &i).unwrap();
        assert_eq!(p.summary, "「タスクなし」 に 20分を割り当て。");
        apply(&ctx, &i).unwrap();
        assert_eq!(segs(&env, m0, 0, 20)[0].task_id, None);

        i.no_task = false;
        i.rest = true;
        apply(&ctx, &i).unwrap();
        let s = &segs(&env, m0, 0, 20)[0];
        assert_eq!((s.kind, s.task_id.clone()), (SegmentKind::Break, None));
    }

    #[test]
    fn 除外と引数の検証() {
        let env = Env::new();
        let m0 = setup(&env);
        let ctx = env.ctx();
        let t = mk_task(&ctx, "T");
        let mut i = input(m0, 0, 30, &t);
        i.exclude = vec!["DD".into()];
        let p = preview(&ctx, &i).unwrap();
        assert!(!cand(&p, "DDDD").checked && cand(&p, "WWWW").checked);
        assert_eq!(p.excluded_count, 1);
        i.exclude = vec!["ZZZ".into()];
        assert!(matches!(preview(&ctx, &i), Err(Error::NotFound(_))));
        // 割り当て先の指定ミス。
        let mut bad = input(m0, 0, 30, &t);
        bad.rest = true;
        assert!(matches!(preview(&ctx, &bad), Err(Error::InvalidArgument(_))));
        let none = CorrectionInput {
            start_at: m0,
            end_at: m0 + 30 * MIN,
            ..Default::default()
        };
        assert!(matches!(preview(&ctx, &none), Err(Error::InvalidArgument(_))));
        assert!(matches!(
            candidates(&ctx, m0 + 30 * MIN, m0, Include::default()),
            Err(Error::InvalidArgument(_))
        ));
    }

    #[test]
    fn 日本語の時間表示() {
        assert_eq!(fmt_jp_duration(72 * MIN), "1時間12分");
        assert_eq!(fmt_jp_duration(45 * MIN), "45分");
        assert_eq!(fmt_jp_duration(120 * MIN), "2時間");
        assert_eq!(fmt_jp_duration(30_000), "30秒");
    }
}
