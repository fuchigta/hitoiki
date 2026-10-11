//! 要確認区間の一覧と確定（docs/dev/architecture.md「設計原則」、docs/dev/architecture.md「CLI」）。

use super::{range_view, segment_view, task_titles, Ctx, RangeArgs, RangeView, SegmentView};
use crate::id::new_id;
use crate::model::{
    ActivityRule, MatchField, PatternType, SegmentKind, SegmentOverride, SegmentStatus, TaskMatchRule,
};
use crate::repo::{self, overrides, rule};
use crate::{Error, Millis, Result};
use rusqlite::{params, OptionalExtension};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct ReviewList {
    pub range: RangeView,
    /// 返した区間の数。
    pub count: usize,
    pub segments: Vec<SegmentView>,
    /// `review.min_sec` 未満のため隠した区間の数（`all` のときは 0）。データは変更しない。
    pub hidden_short: usize,
    /// 短い区間とみなす長さ（秒、`review.min_sec`）。
    pub min_sec: u64,
}

/// 範囲内の needs_review 区間のうち、`review.min_sec` 以上のもの。短い区間は隠し、件数を `hidden_short` で返す。
pub fn list(ctx: &Ctx, args: &RangeArgs) -> Result<ReviewList> {
    list_with(ctx, args, false)
}

/// `all` が true なら短い区間も含めて全件返す。
pub fn list_with(ctx: &Ctx, args: &RangeArgs, all: bool) -> Result<ReviewList> {
    let tz = ctx.tz()?;
    let range = ctx.range(args)?;
    let titles = task_titles(ctx.conn)?;
    let min_sec = ctx.cfg.review.min_sec;
    let min_ms = min_sec as i64 * 1000;
    let mut hidden_short = 0;
    let mut segments: Vec<SegmentView> = Vec::new();
    for s in repo::segment::list_by_status(ctx.conn, range, SegmentStatus::NeedsReview)? {
        if !all && s.end_at - s.start_at < min_ms {
            hidden_short += 1;
        } else {
            segments.push(segment_view(tz, &s, &titles));
        }
    }
    Ok(ReviewList {
        range: range_view(tz, range),
        count: segments.len(),
        segments,
        hidden_short,
        min_sec,
    })
}

#[derive(Debug, Clone, Default)]
pub struct ReviewSetInput {
    /// 区間 ID（前方一致）。
    pub segment: String,
    /// タスク ID（前方一致）。
    pub task: Option<String>,
    pub kind: Option<SegmentKind>,
    pub activity: Option<String>,
    /// 区間からルールを生成する（タスク指定ならタスク帰属ルール、活動のみなら活動ルール）。
    pub rule: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct CreatedRule {
    /// `task_match_rule` | `activity_rule`
    pub rule_type: &'static str,
    pub id: String,
    pub field: MatchField,
    pub pattern: String,
    pub pattern_type: PatternType,
    pub task_id: Option<String>,
    pub activity_label: Option<String>,
    /// 同一内容の既存ルールを再利用した場合 true。
    pub reused: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReviewSetResult {
    pub segment: SegmentView,
    pub override_id: String,
    pub rule: Option<CreatedRule>,
}

/// 区間からルールの元になる (field, pattern, type) を導く。
/// agent_session_id があればその session の cwd の glob、無ければ title の contains、それも無ければ exe。
fn derive_rule_source(
    ctx: &Ctx,
    seg: &crate::model::ActivitySegment,
) -> Result<(MatchField, String, PatternType)> {
    if let Some(sid) = &seg.agent_session_id {
        // パイプラインは "<agent>:<session_id>" で保存する。素の session_id の場合も許容する。
        let (agent, session) = match sid.split_once(':') {
            Some((a, s)) => (Some(a), s),
            None => (None, sid.as_str()),
        };
        let cwd: Option<String> = ctx
            .conn
            .query_row(
                "SELECT cwd FROM agent_event WHERE session_id = ?1 AND (?2 IS NULL OR agent = ?2)                  AND cwd IS NOT NULL AND cwd <> '' ORDER BY at DESC LIMIT 1",
                params![session, agent],
                |r| r.get(0),
            )
            .optional()?;
        return match cwd {
            Some(cwd) => {
                let norm = cwd.replace('\\', "/");
                let base = norm.trim_end_matches('/');
                Ok((
                    MatchField::Cwd,
                    format!("{}/**", globset::escape(base)),
                    PatternType::Glob,
                ))
            }
            None => Err(Error::InvalidArgument(
                "このエージェント区間には作業ディレクトリが無いためルール化できません".into(),
            )),
        };
    }
    if let Some(t) = seg.title.as_deref().filter(|t| !t.trim().is_empty()) {
        return Ok((MatchField::Title, t.to_string(), PatternType::Contains));
    }
    if let Some(e) = seg.exe_name.as_deref().filter(|e| !e.is_empty()) {
        return Ok((MatchField::Exe, e.to_string(), PatternType::Contains));
    }
    Err(Error::InvalidArgument(
        "この区間からルールを作れる手がかり（セッション・タイトル・exe）がありません".into(),
    ))
}

/// 同じタスクの同じ条件のルールを探し、無ければ作る。戻り値の bool は既存を再利用したか。
fn ensure_task_rule(
    ctx: &Ctx,
    task_id: &str,
    field: MatchField,
    pattern: String,
    ty: PatternType,
) -> Result<(TaskMatchRule, bool)> {
    if let Some(r) = rule::find_task_rule(ctx.conn, task_id, field, &pattern, ty)? {
        return Ok((r, true));
    }
    let r = TaskMatchRule {
        id: new_id(),
        task_id: task_id.to_string(),
        field,
        pattern,
        pattern_type: ty,
        priority: 0,
        enabled: true,
    };
    rule::insert_task_rule(ctx.conn, &r)?;
    Ok((r, false))
}

fn created_task_rule(r: TaskMatchRule, reused: bool) -> CreatedRule {
    CreatedRule {
        rule_type: "task_match_rule",
        id: r.id,
        field: r.field,
        pattern: r.pattern,
        pattern_type: r.pattern_type,
        task_id: Some(r.task_id),
        activity_label: None,
        reused,
    }
}

/// 区間から `--rule` のルールを作る（タスク指定ならタスク帰属ルール、活動のみなら活動ルール）。
fn create_rule(
    ctx: &Ctx,
    seg: &crate::model::ActivitySegment,
    task_id: Option<&str>,
    activity: Option<&str>,
) -> Result<CreatedRule> {
    let (field, pattern, ty) = derive_rule_source(ctx, seg)?;
    if let Some(tid) = task_id {
        let (r, reused) = ensure_task_rule(ctx, tid, field, pattern, ty)?;
        return Ok(created_task_rule(r, reused));
    }
    let label = activity.expect("checked by caller").to_string();
    let existing = rule::find_activity_rule(ctx.conn, field, &pattern, ty, &label)?;
    let reused = existing.is_some();
    let r = match existing {
        Some(r) => r,
        None => {
            let r = ActivityRule {
                id: new_id(),
                field,
                pattern,
                pattern_type: ty,
                activity_label: label,
                priority: 0,
                enabled: true,
            };
            rule::insert_activity_rule(ctx.conn, &r)?;
            r
        }
    };
    Ok(CreatedRule {
        rule_type: "activity_rule",
        id: r.id,
        field: r.field,
        pattern: r.pattern,
        pattern_type: r.pattern_type,
        task_id: None,
        activity_label: Some(r.activity_label),
        reused,
    })
}

/// 区間の確定内容。`set` / `set_many` / 無操作のワンタップ仕分けが共有する。
struct Decision {
    task_id: Option<String>,
    kind: Option<SegmentKind>,
    activity: Option<String>,
}

/// 範囲 [start, end) を確定する: `segment_override` を作り、範囲に収まる `activity_segment` 行にも即時に反映する。
/// 戻り値は作った override の ID。
fn confirm_range(
    conn: &rusqlite::Connection,
    start: Millis,
    end: Millis,
    d: &Decision,
    now: Millis,
) -> Result<String> {
    let ov = SegmentOverride {
        id: new_id(),
        start_at: start,
        end_at: end,
        kind: d.kind,
        task_id: d.task_id.clone(),
        activity_label: d.activity.clone(),
        created_at: now,
        offline: false,
        correction_id: None,
    };
    overrides::insert(conn, &ov)?;
    // task_id が空文字列なら「タスクなし」（外す）。無視（ignored）にする区間はタスクも活動も持たない。
    conn.execute(
        "UPDATE activity_segment SET kind = COALESCE(?3, kind), \
         task_id = CASE WHEN ?3 = 'ignored' OR ?4 = '' THEN NULL ELSE COALESCE(?4, task_id) END, \
         activity_label = CASE WHEN ?3 = 'ignored' THEN NULL ELSE COALESCE(?5, activity_label) END, \
         status = 'confirmed', decided_by = 'user', confidence = 1.0 \
         WHERE start_at >= ?1 AND end_at <= ?2",
        params![start, end, d.kind, d.task_id, d.activity],
    )?;
    Ok(ov.id)
}

/// 区間を確定する。`segment_override` を作成し、対象の `activity_segment` 行も即時に反映する。
pub fn set(ctx: &Ctx, input: &ReviewSetInput) -> Result<ReviewSetResult> {
    let tz = ctx.tz()?;
    let seg = repo::segment::resolve(ctx.conn, &input.segment)?;
    let d = decision(
        ctx,
        input.task.as_deref(),
        input.kind,
        input.activity.as_deref(),
        input.rule,
    )?;
    let tx = ctx.conn.unchecked_transaction()?;
    let override_id = confirm_range(ctx.conn, seg.start_at, seg.end_at, &d, ctx.now())?;
    let created = if input.rule {
        Some(create_rule(
            ctx,
            &seg,
            d.task_id.as_deref(),
            d.activity.as_deref(),
        )?)
    } else {
        None
    };
    tx.commit()?;

    let updated = repo::segment::get(ctx.conn, &seg.id)?;
    Ok(ReviewSetResult {
        segment: segment_view(tz, &updated, &task_titles(ctx.conn)?),
        override_id,
        rule: created,
    })
}

/// 引数の検証とタスク ID の解決。
fn decision(
    ctx: &Ctx,
    task: Option<&str>,
    kind: Option<SegmentKind>,
    activity: Option<&str>,
    rule: bool,
) -> Result<Decision> {
    if task.is_none() && kind.is_none() && activity.is_none() {
        return Err(Error::InvalidArgument(
            "--task / --kind / --activity のいずれかが必要です".into(),
        ));
    }
    if rule && task.is_none() && activity.is_none() {
        return Err(Error::InvalidArgument(
            "--rule には --task または --activity が必要です".into(),
        ));
    }
    let task_id = match task {
        Some(p) => Some(repo::task::resolve(ctx.conn, p)?.id),
        None => None,
    };
    let activity = activity
        .map(str::trim)
        .filter(|a| !a.is_empty())
        .map(String::from);
    Ok(Decision {
        task_id,
        kind,
        activity,
    })
}

/// まとめて仕分ける入力。`segments` の全区間に同じ割り当てを適用する。
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(default)]
pub struct ReviewBatchInput {
    /// 区間 ID（前方一致）の一覧。
    pub segments: Vec<String>,
    pub task: Option<String>,
    pub kind: Option<SegmentKind>,
    pub activity: Option<String>,
    /// 各区間からルールを生成する（同じ条件のルールは 1 つにまとめる）。
    pub rule: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReviewBatchResult {
    /// 確定した区間の数。
    pub count: usize,
    pub segments: Vec<SegmentView>,
    pub override_ids: Vec<String>,
    /// 生成した（または再利用した）ルール。同じ条件は 1 件にまとめる。
    pub rules: Vec<CreatedRule>,
}

/// 複数の区間にまとめて同じ割り当てをする。1 つでも解決できなければ何も変更しない。
pub fn set_many(ctx: &Ctx, input: &ReviewBatchInput) -> Result<ReviewBatchResult> {
    if input.segments.is_empty() {
        return Err(Error::InvalidArgument("区間を 1 つ以上指定してください".into()));
    }
    let d = decision(
        ctx,
        input.task.as_deref(),
        input.kind,
        input.activity.as_deref(),
        input.rule,
    )?;
    let mut segs = Vec::new();
    for p in &input.segments {
        let s = repo::segment::resolve(ctx.conn, p)?;
        if !segs.iter().any(|x: &crate::model::ActivitySegment| x.id == s.id) {
            segs.push(s);
        }
    }
    apply_batch(ctx, &segs, &d, input.rule)
}

fn apply_batch(
    ctx: &Ctx,
    segs: &[crate::model::ActivitySegment],
    d: &Decision,
    with_rule: bool,
) -> Result<ReviewBatchResult> {
    let tz = ctx.tz()?;
    let now = ctx.now();
    let tx = ctx.conn.unchecked_transaction()?;
    let mut override_ids = Vec::new();
    let mut rules: Vec<CreatedRule> = Vec::new();
    for seg in segs {
        override_ids.push(confirm_range(ctx.conn, seg.start_at, seg.end_at, d, now)?);
        if with_rule {
            let r = create_rule(ctx, seg, d.task_id.as_deref(), d.activity.as_deref())?;
            if !rules.iter().any(|x| x.id == r.id) {
                rules.push(r);
            }
        }
    }
    tx.commit()?;
    let titles = task_titles(ctx.conn)?;
    let segments = segs
        .iter()
        .map(|s| Ok(segment_view(tz, &repo::segment::get(ctx.conn, &s.id)?, &titles)))
        .collect::<Result<Vec<_>>>()?;
    Ok(ReviewBatchResult {
        count: segments.len(),
        segments,
        override_ids,
        rules,
    })
}

// ===== 区間の無視 =====

/// 区間（まとめて指定可）を「無視」にする: 種別 `ignored`・タスクなしの override として保存する
/// （集計・仕分け・レポート・タスクの実績に数えない。再計算でも残る）。生データは変えない。
/// 取り消しは [`unignore`]。
pub fn ignore(ctx: &Ctx, segments: &[String]) -> Result<ReviewBatchResult> {
    if segments.is_empty() {
        return Err(Error::InvalidArgument("区間を 1 つ以上指定してください".into()));
    }
    let mut segs: Vec<crate::model::ActivitySegment> = Vec::new();
    for p in segments {
        let s = repo::segment::resolve(ctx.conn, p)?;
        if !segs.iter().any(|x| x.id == s.id) {
            segs.push(s);
        }
    }
    let d = Decision {
        task_id: Some(String::new()),
        kind: Some(SegmentKind::Ignored),
        activity: None,
    };
    apply_batch(ctx, &segs, &d, false)
}

#[derive(Debug, Clone, Serialize)]
pub struct UnignoreResult {
    /// 無視を取り消した区間の数。
    pub count: usize,
    /// 削除した override の数。
    pub overrides_removed: usize,
    /// 再計算した範囲。
    pub range: RangeView,
}

/// 「無視」にした区間（override）を取り消し、その範囲を再計算して元に戻す。
/// アプリの無視リスト（`collect.ignore_exes`）による無視は override ではないので、`ignore remove` で外す。
/// `judge` は再計算で使う判定器（無ければルールのみ）。
pub fn unignore(
    ctx: &Ctx,
    segments: &[String],
    judge: Option<&dyn crate::pipeline::SegmentJudge>,
) -> Result<UnignoreResult> {
    if segments.is_empty() {
        return Err(Error::InvalidArgument("区間を 1 つ以上指定してください".into()));
    }
    let tz = ctx.tz()?;
    let mut segs: Vec<crate::model::ActivitySegment> = Vec::new();
    for p in segments {
        let s = repo::segment::resolve(ctx.conn, p)?;
        if s.kind != SegmentKind::Ignored {
            return Err(Error::InvalidArgument(format!(
                "区間 {} は無視された区間ではありません（種別: {}）",
                s.id, s.kind
            )));
        }
        if !segs.iter().any(|x| x.id == s.id) {
            segs.push(s);
        }
    }
    let lo = segs.iter().map(|s| s.start_at).min().unwrap_or(0);
    let hi = segs.iter().map(|s| s.end_at).max().unwrap_or(0);
    let found: Vec<SegmentOverride> = overrides::list(ctx.conn, crate::timefmt::Range::new(lo, hi))?
        .into_iter()
        .filter(|o| o.kind == Some(SegmentKind::Ignored))
        .filter(|o| {
            segs.iter()
                .any(|s| o.start_at < s.end_at && o.end_at > s.start_at)
        })
        .collect();
    if found.is_empty() {
        return Err(Error::InvalidArgument(
            "この区間は無視リスト（collect.ignore_exes）による無視です。`hitoiki ignore remove <exe>` でアプリを外してください"
                .into(),
        ));
    }
    let range = crate::timefmt::Range::new(
        found.iter().map(|o| o.start_at).min().unwrap_or(lo).min(lo),
        found.iter().map(|o| o.end_at).max().unwrap_or(hi).max(hi),
    );
    let ids: Vec<String> = found.iter().map(|o| o.id.clone()).collect();
    let tx = ctx.conn.unchecked_transaction()?;
    let removed = overrides::delete_many(ctx.conn, &ids)?;
    tx.commit()?;
    let stats = crate::rebuild::rebuild_with_judge(ctx.conn, ctx.cfg, range, ctx.now(), judge)?;
    Ok(UnignoreResult {
        count: segs.len(),
        overrides_removed: removed,
        range: range_view(tz, stats.range),
    })
}

// ===== 無操作区間のワンタップ仕分け =====

/// 無操作（idle）区間につけるワンタップの分類。種別と活動ラベルの組で表し、`segment_override` として保存する
/// （再計算しても残る）。新しい種別は作らず、既存の `break` / `meeting` / 活動ラベルで表す。
///
/// | 分類 | 種別 | 活動ラベル |
/// |---|---|---|
/// | 休憩 (`break`) | break | なし |
/// | 会議 (`meeting`) | meeting | なし |
/// | 考えごと (`thinking`) | work | 考えごと |
/// | 離席 (`away`) | break | 離席 |
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdlePreset {
    Break,
    Meeting,
    Thinking,
    Away,
}

impl IdlePreset {
    pub const ALL: &'static [IdlePreset] = &[
        IdlePreset::Break,
        IdlePreset::Meeting,
        IdlePreset::Thinking,
        IdlePreset::Away,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            IdlePreset::Break => "break",
            IdlePreset::Meeting => "meeting",
            IdlePreset::Thinking => "thinking",
            IdlePreset::Away => "away",
        }
    }

    /// 表示名。
    pub fn label(&self) -> &'static str {
        match self {
            IdlePreset::Break => "休憩",
            IdlePreset::Meeting => "会議",
            IdlePreset::Thinking => "考えごと",
            IdlePreset::Away => "離席",
        }
    }

    /// 上書きする (種別, 活動ラベル)。
    pub fn kind_and_activity(&self) -> (SegmentKind, Option<&'static str>) {
        match self {
            IdlePreset::Break => (SegmentKind::Break, None),
            IdlePreset::Meeting => (SegmentKind::Meeting, None),
            IdlePreset::Thinking => (SegmentKind::Work, Some("考えごと")),
            IdlePreset::Away => (SegmentKind::Break, Some("離席")),
        }
    }

    fn decision(&self) -> Decision {
        let (kind, activity) = self.kind_and_activity();
        Decision {
            task_id: None,
            kind: Some(kind),
            activity: activity.map(String::from),
        }
    }
}

impl std::str::FromStr for IdlePreset {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|p| p.as_str() == s)
            .ok_or_else(|| {
                Error::InvalidArgument(format!(
                    "無操作の分類が不正です: {s} (候補: {})",
                    Self::ALL
                        .iter()
                        .map(|p| p.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ))
            })
    }
}

/// 無操作区間（まとめて指定可）に分類をつける。無操作（または以前に分類した休憩・会議・考えごと）以外の区間が
/// 混じっていたら、何も変更せずエラーにする。
pub fn set_idle(ctx: &Ctx, segments: &[String], preset: IdlePreset) -> Result<ReviewBatchResult> {
    if segments.is_empty() {
        return Err(Error::InvalidArgument("区間を 1 つ以上指定してください".into()));
    }
    let mut segs: Vec<crate::model::ActivitySegment> = Vec::new();
    for p in segments {
        let s = repo::segment::resolve(ctx.conn, p)?;
        let was_idle = match s.kind {
            SegmentKind::Idle | SegmentKind::Break | SegmentKind::Meeting => true,
            SegmentKind::Work => s.activity_label.as_deref() == Some("考えごと"),
            _ => false,
        };
        if !was_idle {
            return Err(Error::InvalidArgument(format!(
                "区間 {} は無操作ではありません（種別: {}）",
                s.id, s.kind
            )));
        }
        if !segs.iter().any(|x| x.id == s.id) {
            segs.push(s);
        }
    }
    apply_batch(ctx, &segs, &preset.decision(), false)
}

/// 無操作の期間 [start, end) に分類をつける（通知のボタン用）。期間全体への override を保存するので、
/// 区間がまだ作られていなくても、次の再計算で反映される。戻り値は即時に反映した区間の数。
pub fn set_idle_range(
    conn: &rusqlite::Connection,
    start: Millis,
    end: Millis,
    preset: IdlePreset,
    now: Millis,
) -> Result<usize> {
    if start >= end {
        return Err(Error::InvalidArgument("期間が不正です".into()));
    }
    let tx = conn.unchecked_transaction()?;
    confirm_range(conn, start, end, &preset.decision(), now)?;
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM activity_segment WHERE start_at >= ?1 AND end_at <= ?2 AND status = 'confirmed'",
        params![start, end],
        |r| r.get(0),
    )?;
    tx.commit()?;
    Ok(n as usize)
}

// ===== ルール化の提案 =====

/// 提案を断った組み合わせを記録する `setting` のキー（値は `{ "<キー>": <再提案してよい時刻> }` の JSON）。
const SUGGEST_DISMISSED_KEY: &str = "review.rule_suggest_dismissed";
/// 提案のために数える、確定した区間の遡る日数。
const SUGGEST_LOOKBACK_DAYS: i64 = 30;

/// 提案するルールの内容（受け入れ・却下の指定にも使う）。
#[derive(Debug, Clone, PartialEq, Serialize, serde::Deserialize)]
pub struct RuleSuggestionRef {
    pub task_id: String,
    pub field: MatchField,
    pub pattern: String,
    pub pattern_type: PatternType,
}

impl RuleSuggestionRef {
    fn key(&self) -> String {
        format!(
            "{}|{}|{}|{}",
            self.task_id, self.field, self.pattern_type, self.pattern
        )
    }
}

/// 「この条件はいつもこのタスクにしますか？」という提案。
#[derive(Debug, Clone, Serialize)]
pub struct RuleSuggestion {
    #[serde(flatten)]
    pub rule: RuleSuggestionRef,
    pub task_title: Option<String>,
    /// 直近に同じ割り当てを確定した回数。
    pub count: u32,
    /// 表示用の条件名（exe 名、または作業ディレクトリ）。
    pub target: String,
}

/// 提案の元になる (field, pattern, type)。エージェント区間は作業ディレクトリ、それ以外は exe 名。
/// 手がかりが無ければ None。
fn suggest_source(
    ctx: &Ctx,
    seg: &crate::model::ActivitySegment,
) -> Option<(MatchField, String, PatternType)> {
    if seg.agent_session_id.is_some() {
        return derive_rule_source(ctx, seg).ok();
    }
    let exe = seg.exe_name.as_deref().filter(|e| !e.is_empty())?;
    Some((MatchField::Exe, exe.to_string(), PatternType::Contains))
}

fn load_dismissed(conn: &rusqlite::Connection) -> Result<std::collections::BTreeMap<String, Millis>> {
    Ok(super::update::get(conn, SUGGEST_DISMISSED_KEY)?
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default())
}

/// ユーザーが確定した区間（decided_by = user）を (条件, タスク) ごとに数え、
/// `review.rule_suggest_min_count` 回以上のものでルールが未登録・断られていないものを多い順に返す。
pub fn suggest_rules(ctx: &Ctx) -> Result<Vec<RuleSuggestion>> {
    let min = ctx.cfg.review.rule_suggest_min_count;
    if min == 0 {
        return Ok(vec![]);
    }
    let now = ctx.now();
    let range = crate::timefmt::Range::new(now - SUGGEST_LOOKBACK_DAYS * 86_400_000, now + 1);
    let dismissed = load_dismissed(ctx.conn)?;
    let titles = task_titles(ctx.conn)?;
    let mut counts: std::collections::BTreeMap<String, (RuleSuggestionRef, u32)> = Default::default();
    for s in repo::segment::list_by_status(ctx.conn, range, SegmentStatus::Confirmed)? {
        let (Some(tid), Some("user")) = (s.task_id.as_deref(), s.decided_by.as_deref()) else {
            continue;
        };
        if !matches!(
            s.kind,
            SegmentKind::Work | SegmentKind::AgentDialog | SegmentKind::AgentWatch
        ) {
            continue;
        }
        let Some((field, pattern, pattern_type)) = suggest_source(ctx, &s) else {
            continue;
        };
        let r = RuleSuggestionRef {
            task_id: tid.to_string(),
            field,
            pattern,
            pattern_type,
        };
        counts.entry(r.key()).or_insert((r, 0)).1 += 1;
    }
    let mut out = Vec::new();
    for (key, (r, count)) in counts {
        if count < min || dismissed.get(&key).is_some_and(|&until| until > now) {
            continue;
        }
        // 完了済みなどで消えたタスクは提案しない。
        let Some(title) = titles.get(&r.task_id) else {
            continue;
        };
        if rule::find_task_rule(ctx.conn, &r.task_id, r.field, &r.pattern, r.pattern_type)?.is_some() {
            continue;
        }
        out.push(RuleSuggestion {
            target: r.pattern.trim_end_matches("/**").to_string(),
            task_title: Some(title.clone()),
            count,
            rule: r,
        });
    }
    out.sort_by(|a, b| {
        b.count
            .cmp(&a.count)
            .then_with(|| a.rule.key().cmp(&b.rule.key()))
    });
    Ok(out)
}

/// 提案を受け入れ、タスク帰属ルールを追加する（同じ内容があれば再利用）。
pub fn accept_suggestion(ctx: &Ctx, r: &RuleSuggestionRef) -> Result<CreatedRule> {
    let task = repo::task::resolve(ctx.conn, &r.task_id)?;
    rule::validate_pattern(&r.pattern, r.pattern_type)?;
    let (rule, reused) = ensure_task_rule(ctx, &task.id, r.field, r.pattern.clone(), r.pattern_type)?;
    Ok(created_task_rule(rule, reused))
}

/// 提案を断る。同じ組み合わせは `review.rule_suggest_snooze_days` 日のあいだ提案しない。
pub fn dismiss_suggestion(ctx: &Ctx, r: &RuleSuggestionRef) -> Result<()> {
    let now = ctx.now();
    let mut map = load_dismissed(ctx.conn)?;
    map.retain(|_, until| *until > now);
    map.insert(
        r.key(),
        now + ctx.cfg.review.rule_suggest_snooze_days as Millis * 86_400_000,
    );
    super::update::put(
        ctx.conn,
        SUGGEST_DISMISSED_KEY,
        &serde_json::to_string(&map).map_err(anyhow::Error::from)?,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::task::{self, TaskInput};
    use crate::service::testutil::{insert_segment, Env};

    fn mk_task(ctx: &Ctx) -> String {
        task::add(
            ctx,
            &TaskInput {
                title: Some("T".into()),
                ..Default::default()
            },
        )
        .unwrap()
        .task
        .id
    }

    #[test]
    fn list_hides_short_segments_and_counts_them() {
        let env = Env::new();
        let ctx = env.ctx();
        let d = env.day_start();
        let ins = |id: &str, s: i64, e: i64, st: SegmentStatus| {
            insert_segment(ctx.conn, id, d + s, d + e, SegmentKind::Work, None, st)
        };
        ins("LONG", 0, 120_000, SegmentStatus::NeedsReview);
        ins("EDGE", 120_000, 180_000, SegmentStatus::NeedsReview); // ちょうど 60 秒は表示
        ins("SHORT1", 180_000, 239_999, SegmentStatus::NeedsReview);
        ins("SHORT2", 240_000, 250_000, SegmentStatus::NeedsReview);
        ins("AUTO", 250_000, 255_000, SegmentStatus::Auto); // 要確認でなければ数えない
        let args = RangeArgs::default();

        let l = list(&ctx, &args).unwrap();
        assert_eq!((l.count, l.hidden_short, l.min_sec), (2, 2, 60));
        assert_eq!(l.segments.len(), 2);

        let all = list_with(&ctx, &args, true).unwrap();
        assert_eq!((all.count, all.hidden_short), (4, 0));

        // データは変えない
        let (long, short) =
            repo::segment::count_by_status_split(ctx.conn, SegmentStatus::NeedsReview, 60_000).unwrap();
        assert_eq!((long, short), (2, 2));
    }

    #[test]
    fn list_and_set_updates_segment() {
        let env = Env::new();
        let ctx = env.ctx();
        let d = env.day_start();
        insert_segment(
            ctx.conn,
            "SEG1",
            d,
            d + 600_000,
            SegmentKind::Work,
            None,
            SegmentStatus::NeedsReview,
        );
        insert_segment(
            ctx.conn,
            "SEG2",
            d + 600_000,
            d + 900_000,
            SegmentKind::Work,
            None,
            SegmentStatus::Auto,
        );
        let l = list(&ctx, &RangeArgs::default()).unwrap();
        assert_eq!(l.count, 1);
        assert_eq!(l.hidden_short, 0);
        let t = mk_task(&ctx);
        let r = set(
            &ctx,
            &ReviewSetInput {
                segment: "SEG1".into(),
                task: Some(t.clone()),
                activity: Some("コーディング".into()),
                rule: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(r.segment.status, SegmentStatus::Confirmed);
        assert_eq!(r.segment.task_id.as_deref(), Some(t.as_str()));
        assert_eq!(r.segment.decided_by.as_deref(), Some("user"));
        let rule = r.rule.unwrap();
        // agent_session_id なし -> title の contains
        assert_eq!(rule.field, MatchField::Title);
        assert_eq!(rule.pattern, "main.rs");
        assert_eq!(list(&ctx, &RangeArgs::default()).unwrap().count, 0);
        let ovs = overrides::list(ctx.conn, crate::timefmt::Range::new(d, d + 900_000)).unwrap();
        assert_eq!(ovs.len(), 1);
        assert_eq!(ovs[0].task_id.as_deref(), Some(t.as_str()));
    }

    #[test]
    fn rule_from_agent_session_cwd() {
        let env = Env::new();
        let ctx = env.ctx();
        let d = env.day_start();
        insert_segment(
            ctx.conn,
            "SEG1",
            d,
            d + 600_000,
            SegmentKind::AgentDialog,
            None,
            SegmentStatus::NeedsReview,
        );
        ctx.conn
            .execute(
                "UPDATE activity_segment SET agent_session_id = 'claude-code:S1' WHERE id = 'SEG1'",
                [],
            )
            .unwrap();
        ctx.conn
            .execute(
                "INSERT INTO agent_event (id, at, agent, session_id, kind, cwd) \
                 VALUES ('E1', ?1, 'claude-code', 'S1', 'prompt_submit', 'C:\\src\\foo\\')",
                [d],
            )
            .unwrap();
        let t = mk_task(&ctx);
        let r = set(
            &ctx,
            &ReviewSetInput {
                segment: "SEG1".into(),
                task: Some(t),
                rule: true,
                ..Default::default()
            },
        )
        .unwrap();
        let rule = r.rule.unwrap();
        assert_eq!(rule.field, MatchField::Cwd);
        assert_eq!(rule.pattern, "C:/src/foo/**");
        assert_eq!(rule.pattern_type, PatternType::Glob);
        assert!(!rule.reused);
    }

    fn agent_seg(env: &Env, cwd: Option<&str>) -> (String, String) {
        let ctx = env.ctx();
        let d = env.day_start();
        insert_segment(
            ctx.conn,
            "SEG1",
            d,
            d + 600_000,
            SegmentKind::AgentDialog,
            Some("✳ Claude Code"),
            SegmentStatus::NeedsReview,
        );
        ctx.conn
            .execute(
                "UPDATE activity_segment SET agent_session_id = 'claude-code:S1' WHERE id = 'SEG1'",
                [],
            )
            .unwrap();
        if let Some(cwd) = cwd {
            ctx.conn
                .execute(
                    "INSERT INTO agent_event (id, at, agent, session_id, kind, cwd)                      VALUES ('E1', ?1, 'claude-code', 'S1', 'prompt_submit', ?2)",
                    params![d, cwd],
                )
                .unwrap();
        }
        (mk_task(&ctx), "SEG1".into())
    }

    #[test]
    fn rule_cwd_is_glob_escaped() {
        let env = Env::new();
        let (t, seg) = agent_seg(&env, Some(r"C:\src\a[1]{x}?*"));
        let r = set(
            &env.ctx(),
            &ReviewSetInput {
                segment: seg,
                task: Some(t),
                rule: true,
                ..Default::default()
            },
        )
        .unwrap();
        let rule = r.rule.unwrap();
        assert_eq!(rule.pattern, "C:/src/a[[]1[]][{]x[}][?][*]/**");
        let g = globset::Glob::new(&rule.pattern).unwrap().compile_matcher();
        assert!(g.is_match("C:/src/a[1]{x}?*/sub/f.rs"));
        assert!(!g.is_match("C:/src/a1x/f.rs"));
    }

    #[test]
    fn agent_segment_without_cwd_is_error() {
        let env = Env::new();
        let (t, seg) = agent_seg(&env, None);
        let e = set(
            &env.ctx(),
            &ReviewSetInput {
                segment: seg,
                task: Some(t),
                rule: true,
                ..Default::default()
            },
        )
        .unwrap_err();
        assert_eq!(e.exit_code(), 2);
    }

    #[test]
    fn requires_something_to_set() {
        let env = Env::new();
        let ctx = env.ctx();
        let d = env.day_start();
        insert_segment(
            ctx.conn,
            "SEG1",
            d,
            d + 1000,
            SegmentKind::Work,
            None,
            SegmentStatus::NeedsReview,
        );
        let e = set(
            &ctx,
            &ReviewSetInput {
                segment: "SEG1".into(),
                ..Default::default()
            },
        )
        .unwrap_err();
        assert_eq!(e.exit_code(), 2);
        let e = set(
            &ctx,
            &ReviewSetInput {
                segment: "NOPE".into(),
                kind: Some(SegmentKind::Break),
                ..Default::default()
            },
        )
        .unwrap_err();
        assert_eq!(e.exit_code(), 3);
    }

    // ===== 無操作のワンタップ仕分け =====

    fn seg_row(
        conn: &rusqlite::Connection,
        id: &str,
    ) -> (SegmentKind, Option<String>, SegmentStatus, Option<String>) {
        conn.query_row(
            "SELECT kind, activity_label, status, decided_by FROM activity_segment WHERE id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap()
    }

    #[test]
    fn idle_presets_map_to_kind_and_activity() {
        use IdlePreset::*;
        assert_eq!(Break.kind_and_activity(), (SegmentKind::Break, None));
        assert_eq!(Meeting.kind_and_activity(), (SegmentKind::Meeting, None));
        assert_eq!(
            Thinking.kind_and_activity(),
            (SegmentKind::Work, Some("考えごと"))
        );
        assert_eq!(Away.kind_and_activity(), (SegmentKind::Break, Some("離席")));
        assert_eq!("away".parse::<IdlePreset>().unwrap(), Away);
        assert_eq!("nap".parse::<IdlePreset>().unwrap_err().exit_code(), 2);
    }

    #[test]
    fn set_idle_saves_override_for_each_preset() {
        let env = Env::new();
        let ctx = env.ctx();
        let d = env.day_start();
        let cases = [
            (IdlePreset::Break, SegmentKind::Break, None),
            (IdlePreset::Meeting, SegmentKind::Meeting, None),
            (IdlePreset::Thinking, SegmentKind::Work, Some("考えごと")),
            (IdlePreset::Away, SegmentKind::Break, Some("離席")),
        ];
        for (i, (preset, kind, label)) in cases.into_iter().enumerate() {
            let id = format!("IDLE{i}");
            let s = d + i as i64 * 3_600_000;
            insert_segment(
                ctx.conn,
                &id,
                s,
                s + 600_000,
                SegmentKind::Idle,
                None,
                SegmentStatus::NeedsReview,
            );
            let r = set_idle(&ctx, std::slice::from_ref(&id), preset).unwrap();
            assert_eq!(r.count, 1);
            let (k, l, st, by) = seg_row(ctx.conn, &id);
            assert_eq!(
                (k, l.as_deref(), st, by.as_deref()),
                (kind, label, SegmentStatus::Confirmed, Some("user"))
            );
            // 再計算で残る override が、区間の範囲に保存されている。
            let ovs = overrides::list(ctx.conn, crate::timefmt::Range::new(s, s + 600_000)).unwrap();
            assert_eq!(ovs.len(), 1);
            assert_eq!(
                (ovs[0].kind, ovs[0].activity_label.as_deref()),
                (Some(kind), label)
            );
            assert_eq!(ovs[0].task_id, None);
        }
        // 仕分け済みの休憩は別の分類へ付け替えられる。
        set_idle(&ctx, &["IDLE0".into()], IdlePreset::Away).unwrap();
        assert_eq!(seg_row(ctx.conn, "IDLE0").1.as_deref(), Some("離席"));
    }

    #[test]
    fn set_idle_rejects_non_idle_and_changes_nothing() {
        let env = Env::new();
        let ctx = env.ctx();
        let d = env.day_start();
        insert_segment(
            ctx.conn,
            "I1",
            d,
            d + 600_000,
            SegmentKind::Idle,
            None,
            SegmentStatus::NeedsReview,
        );
        insert_segment(
            ctx.conn,
            "W1",
            d + 600_000,
            d + 900_000,
            SegmentKind::Work,
            None,
            SegmentStatus::NeedsReview,
        );
        let e = set_idle(&ctx, &["I1".into(), "W1".into()], IdlePreset::Break).unwrap_err();
        assert_eq!(e.exit_code(), 2);
        assert_eq!(seg_row(ctx.conn, "I1").2, SegmentStatus::NeedsReview);
        assert!(
            overrides::list(ctx.conn, crate::timefmt::Range::new(d, d + 900_000))
                .unwrap()
                .is_empty()
        );
        assert_eq!(set_idle(&ctx, &[], IdlePreset::Break).unwrap_err().exit_code(), 2);
    }

    // ===== まとめて仕分け =====

    #[test]
    fn set_many_assigns_same_task_to_all_and_dedups_rule() {
        let env = Env::new();
        let ctx = env.ctx();
        let d = env.day_start();
        for i in 0..3 {
            insert_segment(
                ctx.conn,
                &format!("S{i}"),
                d + i * 600_000,
                d + (i + 1) * 600_000,
                SegmentKind::Work,
                None,
                SegmentStatus::NeedsReview,
            );
        }
        let t = mk_task(&ctx);
        let r = set_many(
            &ctx,
            &ReviewBatchInput {
                segments: vec!["S0".into(), "S1".into(), "S2".into(), "S1".into()],
                task: Some(t.clone()),
                activity: Some("開発".into()),
                rule: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!((r.count, r.override_ids.len()), (3, 3));
        for i in 0..3 {
            let v = &r.segments[i];
            assert_eq!(
                (v.status, v.task_id.as_deref(), v.activity_label.as_deref()),
                (SegmentStatus::Confirmed, Some(t.as_str()), Some("開発"))
            );
        }
        // 3 区間とも同じタイトル -> ルールは 1 つ
        assert_eq!(r.rules.len(), 1);
        assert_eq!(list(&ctx, &RangeArgs::default()).unwrap().count, 0);
    }

    #[test]
    fn set_many_is_all_or_nothing() {
        let env = Env::new();
        let ctx = env.ctx();
        let d = env.day_start();
        insert_segment(
            ctx.conn,
            "S0",
            d,
            d + 600_000,
            SegmentKind::Work,
            None,
            SegmentStatus::NeedsReview,
        );
        let e = set_many(
            &ctx,
            &ReviewBatchInput {
                segments: vec!["S0".into(), "NOPE".into()],
                kind: Some(SegmentKind::Break),
                ..Default::default()
            },
        )
        .unwrap_err();
        assert_eq!(e.exit_code(), 3);
        assert_eq!(seg_row(ctx.conn, "S0").2, SegmentStatus::NeedsReview);
        let e = set_many(&ctx, &ReviewBatchInput::default()).unwrap_err();
        assert_eq!(e.exit_code(), 2);
    }

    // ===== ルール化の提案 =====

    /// ユーザーが確定した作業区間を n 件作る（exe は Code.exe）。
    fn confirm_n(env: &Env, task: &str, n: i64, prefix: &str) {
        let ctx = env.ctx();
        let d = env.day_start();
        for i in 0..n {
            let id = format!("{prefix}{i}");
            insert_segment(
                ctx.conn,
                &id,
                d + i * 600_000,
                d + (i + 1) * 600_000,
                SegmentKind::Work,
                Some(task),
                SegmentStatus::Confirmed,
            );
            ctx.conn
                .execute(
                    "UPDATE activity_segment SET decided_by = 'user' WHERE id = ?1",
                    [&id],
                )
                .unwrap();
        }
    }

    #[test]
    fn suggestion_appears_at_threshold_and_not_below() {
        let env = Env::new();
        let ctx = env.ctx();
        let t = mk_task(&ctx);
        confirm_n(&env, &t, 2, "A");
        assert!(suggest_rules(&ctx).unwrap().is_empty());
        confirm_n(&env, &t, 3, "B");
        let s = suggest_rules(&ctx).unwrap();
        assert_eq!(s.len(), 1);
        assert_eq!((s[0].count, s[0].target.as_str()), (5, "Code.exe"));
        assert_eq!(
            (s[0].rule.field, s[0].rule.pattern_type),
            (MatchField::Exe, PatternType::Contains)
        );
        assert_eq!(s[0].task_title.as_deref(), Some("T"));
    }

    #[test]
    fn suggestion_ignores_auto_decisions_and_can_be_disabled() {
        let mut env = Env::new();
        let t = mk_task(&env.ctx());
        confirm_n(&env, &t, 3, "A");
        // 自動判定（decided_by が user でない）は数えない
        env.conn
            .execute(
                "UPDATE activity_segment SET decided_by = 'rule' WHERE id = 'A0'",
                [],
            )
            .unwrap();
        assert!(suggest_rules(&env.ctx()).unwrap().is_empty());
        env.conn
            .execute(
                "UPDATE activity_segment SET decided_by = 'user' WHERE id = 'A0'",
                [],
            )
            .unwrap();
        assert_eq!(suggest_rules(&env.ctx()).unwrap().len(), 1);
        env.cfg.review.rule_suggest_min_count = 0;
        assert!(suggest_rules(&env.ctx()).unwrap().is_empty());
    }

    #[test]
    fn accepting_suggestion_adds_rule_and_hides_it() {
        let env = Env::new();
        let ctx = env.ctx();
        let t = mk_task(&ctx);
        confirm_n(&env, &t, 3, "A");
        let s = suggest_rules(&ctx).unwrap().remove(0);
        let r = accept_suggestion(&ctx, &s.rule).unwrap();
        assert_eq!(
            (r.rule_type, r.reused, r.pattern.as_str()),
            ("task_match_rule", false, "Code.exe")
        );
        assert!(suggest_rules(&ctx).unwrap().is_empty());
        assert!(accept_suggestion(&ctx, &s.rule).unwrap().reused);
    }

    #[test]
    fn dismissed_suggestion_returns_after_snooze_days() {
        let env = Env::new();
        let ctx = env.ctx();
        let t = mk_task(&ctx);
        confirm_n(&env, &t, 3, "A");
        let s = suggest_rules(&ctx).unwrap().remove(0);
        dismiss_suggestion(&ctx, &s.rule).unwrap();
        assert!(suggest_rules(&ctx).unwrap().is_empty());
        env.clock.advance(13 * 86_400_000);
        assert!(suggest_rules(&env.ctx()).unwrap().is_empty());
        env.clock.advance(2 * 86_400_000);
        // 14 日を過ぎれば再び提案する（確定した区間は 30 日以内）
        assert_eq!(suggest_rules(&env.ctx()).unwrap().len(), 1);
    }
}

#[cfg(test)]
mod ignore_tests {
    use super::*;
    use crate::service::testutil::{insert_segment, Env};
    use crate::service::{report, RangeArgs};
    use crate::timefmt::Range;

    const MIN: Millis = 60_000;

    fn wspan(conn: &rusqlite::Connection, id: &str, s: Millis, e: Millis, exe: &str) {
        conn.execute(
            "INSERT INTO window_span (id, start_at, end_at, hwnd, exe_name, title, excluded) VALUES (?1,?2,?3,?1,?4,'t',0)",
            params![id, s, e, exe],
        )
        .unwrap();
    }

    fn total(ctx: &Ctx) -> i64 {
        report::report(
            ctx,
            &report::ReportInput {
                range: RangeArgs::default(),
                by: report::ReportBy::Task,
                include_idle: false,
            },
        )
        .unwrap()
        .total_ms
    }

    #[test]
    fn ignore_many_then_unignore_restores() {
        let env = Env::new();
        let ctx = env.ctx();
        let d = env.day_start();
        wspan(ctx.conn, "A", d, d + 30 * MIN, "devenv.exe");
        wspan(ctx.conn, "B", d + 30 * MIN, d + 40 * MIN, "notepad.exe");
        wspan(ctx.conn, "C", d + 40 * MIN, d + 70 * MIN, "devenv.exe");
        let range = Range::new(d, d + 120 * MIN);
        // devenv は T に帰属させ、notepad は別の区間として残す。
        let t = crate::service::task::add(
            &ctx,
            &crate::service::task::TaskInput {
                title: Some("T".into()),
                ..Default::default()
            },
        )
        .unwrap()
        .task
        .id;
        ensure_task_rule(&ctx, &t, MatchField::Exe, "devenv".into(), PatternType::Contains).unwrap();
        crate::rebuild::rebuild(ctx.conn, ctx.cfg, range, ctx.now()).unwrap();
        let segs = repo::segment::list(ctx.conn, range).unwrap();
        assert_eq!(total(&ctx), 70 * MIN);

        let ids: Vec<String> = segs
            .iter()
            .filter(|s| s.exe_name.as_deref() == Some("notepad.exe"))
            .map(|s| s.id.clone())
            .collect();
        let r = ignore(&ctx, &ids).unwrap();
        assert_eq!(r.count, 1);
        assert_eq!(r.segments[0].kind, SegmentKind::Ignored);
        assert_eq!(r.segments[0].task_id, None);
        assert_eq!(total(&ctx), 60 * MIN);
        // 再計算しても残る
        crate::rebuild::rebuild(ctx.conn, ctx.cfg, range, ctx.now()).unwrap();
        assert_eq!(total(&ctx), 60 * MIN);
        let ign: Vec<String> = repo::segment::list(ctx.conn, range)
            .unwrap()
            .into_iter()
            .filter(|s| s.kind == SegmentKind::Ignored)
            .map(|s| s.id)
            .collect();
        assert_eq!(ign.len(), 1);

        // 取り消し
        let u = unignore(&ctx, &ign, None).unwrap();
        assert_eq!((u.count, u.overrides_removed), (1, 1));
        assert_eq!(total(&ctx), 70 * MIN);
        // 生データは変わらない
        let n: i64 = ctx
            .conn
            .query_row("SELECT COUNT(*) FROM window_span", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 3);
    }

    #[test]
    fn unignore_rejects_non_ignored_and_list_ignored() {
        let env = Env::new();
        let ctx = env.ctx();
        let d = env.day_start();
        insert_segment(
            ctx.conn,
            "WWWW",
            d,
            d + MIN,
            SegmentKind::Work,
            None,
            SegmentStatus::NeedsReview,
        );
        assert!(matches!(
            unignore(&ctx, &["WWWW".into()], None),
            Err(Error::InvalidArgument(_))
        ));
        // 無視リスト由来の無視（override なし）は取り消せない
        insert_segment(
            ctx.conn,
            "IIII",
            d + 2 * MIN,
            d + 3 * MIN,
            SegmentKind::Ignored,
            None,
            SegmentStatus::Auto,
        );
        assert!(matches!(
            unignore(&ctx, &["IIII".into()], None),
            Err(Error::InvalidArgument(_))
        ));
        // 無視の区間は要確認に出ない
        assert!(list(&ctx, &RangeArgs::default())
            .unwrap()
            .segments
            .iter()
            .all(|s| s.id != "IIII"));
        assert!(matches!(ignore(&ctx, &[]), Err(Error::InvalidArgument(_))));
    }
}
