//! 要確認区間の一覧と確定（docs/architecture.md「設計原則」、docs/architecture.md「CLI」）。

use super::{range_view, segment_view, task_titles, Ctx, RangeArgs, RangeView, SegmentView};
use crate::id::new_id;
use crate::model::{
    ActivityRule, MatchField, PatternType, SegmentKind, SegmentOverride, SegmentStatus, TaskMatchRule,
};
use crate::repo::{self, overrides, rule};
use crate::{Error, Result};
use rusqlite::{params, OptionalExtension};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct ReviewList {
    pub range: RangeView,
    pub count: usize,
    pub segments: Vec<SegmentView>,
}

/// 範囲内の needs_review 区間。
pub fn list(ctx: &Ctx, args: &RangeArgs) -> Result<ReviewList> {
    let tz = ctx.tz()?;
    let range = ctx.range(args)?;
    let titles = task_titles(ctx.conn)?;
    let segments: Vec<SegmentView> =
        repo::segment::list_by_status(ctx.conn, range, SegmentStatus::NeedsReview)?
            .iter()
            .map(|s| segment_view(tz, s, &titles))
            .collect();
    Ok(ReviewList {
        range: range_view(tz, range),
        count: segments.len(),
        segments,
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

/// 区間を確定する。`segment_override` を作成し、対象の `activity_segment` 行も即時に反映する。
pub fn set(ctx: &Ctx, input: &ReviewSetInput) -> Result<ReviewSetResult> {
    let tz = ctx.tz()?;
    let seg = repo::segment::resolve(ctx.conn, &input.segment)?;
    if input.task.is_none() && input.kind.is_none() && input.activity.is_none() {
        return Err(Error::InvalidArgument(
            "--task / --kind / --activity のいずれかが必要です".into(),
        ));
    }
    if input.rule && input.task.is_none() && input.activity.is_none() {
        return Err(Error::InvalidArgument(
            "--rule には --task または --activity が必要です".into(),
        ));
    }
    let task_id = match &input.task {
        Some(p) => Some(repo::task::resolve(ctx.conn, p)?.id),
        None => None,
    };
    let activity = input
        .activity
        .as_deref()
        .map(str::trim)
        .filter(|a| !a.is_empty())
        .map(String::from);
    let now = ctx.now();
    let tx = ctx.conn.unchecked_transaction()?;

    let ov = SegmentOverride {
        id: new_id(),
        start_at: seg.start_at,
        end_at: seg.end_at,
        kind: input.kind,
        task_id: task_id.clone(),
        activity_label: activity.clone(),
        created_at: now,
    };
    overrides::insert(ctx.conn, &ov)?;

    ctx.conn.execute(
        "UPDATE activity_segment SET kind = COALESCE(?2, kind), task_id = COALESCE(?3, task_id), \
         activity_label = COALESCE(?4, activity_label), status = 'confirmed', decided_by = 'user', \
         confidence = 1.0 WHERE id = ?1",
        params![seg.id, input.kind, task_id, activity],
    )?;

    let created = if input.rule {
        let (field, pattern, ty) = derive_rule_source(ctx, &seg)?;
        if let Some(tid) = &task_id {
            let existing = rule::find_task_rule(ctx.conn, tid, field, &pattern, ty)?;
            let reused = existing.is_some();
            let r = match existing {
                Some(r) => r,
                None => {
                    let r = TaskMatchRule {
                        id: new_id(),
                        task_id: tid.clone(),
                        field,
                        pattern,
                        pattern_type: ty,
                        priority: 0,
                        enabled: true,
                    };
                    rule::insert_task_rule(ctx.conn, &r)?;
                    r
                }
            };
            Some(CreatedRule {
                rule_type: "task_match_rule",
                id: r.id,
                field: r.field,
                pattern: r.pattern,
                pattern_type: r.pattern_type,
                task_id: Some(r.task_id),
                activity_label: None,
                reused,
            })
        } else {
            let label = activity.clone().expect("checked above");
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
            Some(CreatedRule {
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
    } else {
        None
    };
    tx.commit()?;

    let updated = repo::segment::get(ctx.conn, &seg.id)?;
    Ok(ReviewSetResult {
        segment: segment_view(tz, &updated, &task_titles(ctx.conn)?),
        override_id: ov.id,
        rule: created,
    })
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
}
