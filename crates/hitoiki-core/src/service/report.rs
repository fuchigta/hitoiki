//! 期間内の `activity_segment` の集計（docs/dev/architecture.md「CLI」）。
//!
//! 実績は常に区間から算出する（予実の「実績」フィールドは持たない）。

use super::{range_view, Ctx, RangeArgs, RangeView};
use crate::model::{SegmentKind, Task};
use crate::repo::{self, segment::kind_label};
use crate::timefmt::Range;
use crate::{Error, Result};
use rusqlite::Connection;
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReportBy {
    Task,
    Activity,
    UrgentImportant,
    KeyResult,
    Skill,
    Agent,
    Kind,
}

impl ReportBy {
    pub fn as_str(&self) -> &'static str {
        match self {
            ReportBy::Task => "task",
            ReportBy::Activity => "activity",
            ReportBy::UrgentImportant => "urgent_important",
            ReportBy::KeyResult => "key_result",
            ReportBy::Skill => "skill",
            ReportBy::Agent => "agent",
            ReportBy::Kind => "kind",
        }
    }
}

impl FromStr for ReportBy {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self> {
        Ok(match s {
            "task" => ReportBy::Task,
            "activity" => ReportBy::Activity,
            "urgent_important" => ReportBy::UrgentImportant,
            "key_result" => ReportBy::KeyResult,
            "skill" => ReportBy::Skill,
            "agent" => ReportBy::Agent,
            "kind" => ReportBy::Kind,
            _ => {
                return Err(Error::InvalidArgument(format!(
                    "--by の値が不正です: {s} (候補: task, activity, urgent_important, key_result, skill, agent, kind)"
                )))
            }
        })
    }
}

#[derive(Debug, Clone)]
pub struct ReportInput {
    pub range: RangeArgs,
    pub by: ReportBy,
    /// idle / break も合計に含める（既定は除外して別枠表示）。
    pub include_idle: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReportRow {
    /// 集計キー。どれにも紐付かない行は null。
    pub key: Option<String>,
    pub label: String,
    pub duration_ms: i64,
    /// `total_ms` に対する割合（0〜1）。
    pub ratio: f64,
    pub segments: usize,
    /// 内訳（agent の dialog / watch など）。
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub breakdown: BTreeMap<String, i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Excluded {
    pub idle_ms: i64,
    pub break_ms: i64,
    /// 無視した区間（`ignored`）の合計。`include_idle` に関わらず合計には含めない。
    pub ignored_ms: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub range: RangeView,
    pub by: ReportBy,
    /// 集計対象の合計（重複計上しない実時間）。
    pub total_ms: i64,
    pub rows: Vec<ReportRow>,
    /// 既定では合計から除外した idle / break の時間。
    pub excluded: Excluded,
    pub include_idle: bool,
    /// true のとき行の合計が `total_ms` を超えうる（skill: 複数スキルは各スキルに全時間を計上）。
    pub multi_attribution: bool,
    pub note: Option<String>,
}

const NONE_LABEL: &str = "(なし)";

struct Lookup {
    tasks: HashMap<String, Task>,
    /// key_result id -> (key_result title, goal id, goal title)
    key_results: HashMap<String, (String, String, String)>,
    task_skills: HashMap<String, Vec<(String, String)>>,
}

fn load_lookup(conn: &Connection) -> Result<Lookup> {
    let tasks = repo::task::list(conn, &[], None)?
        .into_iter()
        .map(|t| (t.id.clone(), t))
        .collect();
    let mut key_results = HashMap::new();
    let mut stmt = conn
        .prepare("SELECT k.id, k.title, o.id, o.title FROM key_result k JOIN goal o ON o.id = k.goal_id")?;
    for row in stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
        ))
    })? {
        let (id, t, oid, ot) = row?;
        key_results.insert(id, (t, oid, ot));
    }
    let mut task_skills: HashMap<String, Vec<(String, String)>> = HashMap::new();
    let mut stmt = conn.prepare(
        "SELECT ts.task_id, s.id, s.name FROM task_skill ts JOIN skill s ON s.id = ts.skill_id ORDER BY s.name",
    )?;
    for row in stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
        ))
    })? {
        let (tid, sid, name) = row?;
        task_skills.entry(tid).or_default().push((sid, name));
    }
    Ok(Lookup {
        tasks,
        key_results,
        task_skills,
    })
}

fn urgent_important(t: &Task) -> (&'static str, &'static str) {
    match (t.urgent, t.important) {
        (Some(true), Some(true)) => ("urgent_important", "緊急かつ重要"),
        (Some(false), Some(true)) => ("important", "重要（緊急でない）"),
        (Some(true), Some(false)) => ("urgent", "緊急（重要でない）"),
        (Some(false), Some(false)) => ("neither", "緊急でも重要でもない"),
        _ => ("unset", "未設定"),
    }
}

type Group = (Option<String>, String);

pub fn report(ctx: &Ctx, input: &ReportInput) -> Result<Report> {
    let tz = ctx.tz()?;
    let range = ctx.range(&input.range)?;
    aggregate(ctx.conn, range, tz, input)
}

fn aggregate(conn: &Connection, range: Range, tz: chrono_tz::Tz, input: &ReportInput) -> Result<Report> {
    let lookup = load_lookup(conn)?;
    let segs = repo::segment::list(conn, range)?;
    let mut excluded = Excluded {
        idle_ms: 0,
        break_ms: 0,
        ignored_ms: 0,
    };
    let mut total = 0i64;
    // (key, label) -> (ms, count, breakdown)
    let mut acc: BTreeMap<Group, (i64, usize, BTreeMap<String, i64>)> = BTreeMap::new();

    for s in &segs {
        let ms = repo::segment::clipped_len(s, range);
        if ms == 0 {
            continue;
        }
        // 無視した区間は、集計のどの切り口でも数えない。
        if s.kind == SegmentKind::Ignored {
            excluded.ignored_ms += ms;
            continue;
        }
        if !input.include_idle {
            match s.kind {
                SegmentKind::Idle => {
                    excluded.idle_ms += ms;
                    continue;
                }
                SegmentKind::Break => {
                    excluded.break_ms += ms;
                    continue;
                }
                _ => {}
            }
        }
        let task = s.task_id.as_ref().and_then(|id| lookup.tasks.get(id));
        let mut breakdown_key: Option<String> = None;
        let groups: Vec<Group> = match input.by {
            ReportBy::Task => vec![match (&s.task_id, task) {
                (Some(id), Some(t)) => (Some(id.clone()), t.title.clone()),
                (Some(id), None) => (Some(id.clone()), id.clone()),
                _ => (None, NONE_LABEL.into()),
            }],
            ReportBy::Activity => vec![match &s.activity_label {
                Some(l) if !l.is_empty() => (Some(l.clone()), l.clone()),
                _ => (None, NONE_LABEL.into()),
            }],
            ReportBy::Kind => vec![(Some(s.kind.to_string()), kind_label(s.kind).to_string())],
            ReportBy::UrgentImportant => vec![match task {
                Some(t) => {
                    let (k, l) = urgent_important(t);
                    (Some(k.into()), l.into())
                }
                None => (None, NONE_LABEL.into()),
            }],
            ReportBy::KeyResult => vec![match task.and_then(|t| t.key_result_id.as_ref()) {
                Some(kid) => match lookup.key_results.get(kid) {
                    Some((kt, _, ot)) => (Some(kid.clone()), format!("{ot} / {kt}")),
                    None => (Some(kid.clone()), kid.clone()),
                },
                None => (None, NONE_LABEL.into()),
            }],
            ReportBy::Skill => match task.and_then(|t| lookup.task_skills.get(&t.id)) {
                Some(list) if !list.is_empty() => list
                    .iter()
                    .map(|(id, name)| (Some(id.clone()), name.clone()))
                    .collect(),
                _ => vec![(None, NONE_LABEL.into())],
            },
            ReportBy::Agent => {
                if !matches!(s.kind, SegmentKind::AgentDialog | SegmentKind::AgentWatch) {
                    continue;
                }
                breakdown_key = Some(s.kind.to_string());
                vec![match &s.agent_session_id {
                    Some(id) => (Some(id.clone()), id.clone()),
                    None => (None, "(セッション不明)".into()),
                }]
            }
        };
        total += ms;
        for g in groups {
            let e = acc.entry(g).or_default();
            e.0 += ms;
            e.1 += 1;
            if let Some(bk) = &breakdown_key {
                *e.2.entry(bk.clone()).or_default() += ms;
            }
        }
    }

    let mut rows: Vec<ReportRow> = acc
        .into_iter()
        .map(|((key, label), (ms, n, breakdown))| ReportRow {
            key,
            label,
            duration_ms: ms,
            ratio: if total > 0 { ms as f64 / total as f64 } else { 0.0 },
            segments: n,
            breakdown,
        })
        .collect();
    rows.sort_by(|a, b| {
        b.duration_ms
            .cmp(&a.duration_ms)
            .then_with(|| a.label.cmp(&b.label))
    });

    let multi = input.by == ReportBy::Skill;
    let note = match input.by {
        ReportBy::Skill => Some(
            "複数のスキルが付いたタスクの時間は、各スキルに全時間を計上しています（行の合計は total_ms を超えることがあります）。"
                .to_string(),
        ),
        ReportBy::Agent => Some("AI との対話・作業待ちの区間のみを集計しています。".to_string()),
        ReportBy::UrgentImportant => Some(
            "urgent / important のどちらかが未設定のタスクは「未設定」に集計しています。".to_string(),
        ),
        _ => None,
    };
    Ok(Report {
        range: range_view(tz, range),
        by: input.by,
        total_ms: total,
        rows,
        excluded,
        include_idle: input.include_idle,
        multi_attribution: multi,
        note,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::SegmentStatus;
    use crate::service::goal::{self, GoalInput, KeyResultInput};
    use crate::service::skill::{self, SkillInput};
    use crate::service::task::{self, TaskInput};
    use crate::service::testutil::{insert_segment, Env};

    const MIN: i64 = 60_000;

    fn input(by: ReportBy) -> ReportInput {
        ReportInput {
            range: RangeArgs::default(),
            by,
            include_idle: false,
        }
    }

    fn setup(env: &Env) -> (String, String) {
        let ctx = env.ctx();
        let o = goal::add_goal(
            &ctx,
            &GoalInput {
                title: "O1".into(),
                from: None,
                to: None,
            },
        )
        .unwrap();
        let k = goal::add_key_result(
            &ctx,
            &KeyResultInput {
                goal: o.id,
                title: "KR1".into(),
                target: None,
                skills: vec![],
            },
        )
        .unwrap();
        skill::add(
            &ctx,
            &SkillInput {
                name: "Rust".into(),
                parent: None,
            },
        )
        .unwrap();
        skill::add(
            &ctx,
            &SkillInput {
                name: "設計".into(),
                parent: None,
            },
        )
        .unwrap();
        let a = task::add(
            &ctx,
            &TaskInput {
                title: Some("A".into()),
                urgent: Some(true),
                important: Some(true),
                key_result: Some(k.id),
                skills: vec!["Rust".into(), "設計".into()],
                ..Default::default()
            },
        )
        .unwrap();
        let b = task::add(
            &ctx,
            &TaskInput {
                title: Some("B".into()),
                ..Default::default()
            },
        )
        .unwrap();
        (a.task.id, b.task.id)
    }

    fn fill(env: &Env, a: &str, b: &str) {
        let d = env.day_start();
        let c = env.ctx().conn;
        let st = SegmentStatus::Confirmed;
        insert_segment(c, "S1", d, d + 60 * MIN, SegmentKind::Work, Some(a), st);
        insert_segment(
            c,
            "S2",
            d + 60 * MIN,
            d + 90 * MIN,
            SegmentKind::AgentDialog,
            Some(b),
            st,
        );
        insert_segment(c, "S3", d + 90 * MIN, d + 100 * MIN, SegmentKind::Work, None, st);
        insert_segment(c, "S4", d + 100 * MIN, d + 130 * MIN, SegmentKind::Idle, None, st);
        insert_segment(
            c,
            "S5",
            d + 130 * MIN,
            d + 140 * MIN,
            SegmentKind::Break,
            None,
            st,
        );
        c.execute(
            "UPDATE activity_segment SET agent_session_id='SESS' WHERE id='S2'",
            [],
        )
        .unwrap();
    }

    #[test]
    fn by_task_excludes_idle_and_shows_none() {
        let env = Env::new();
        let (a, b) = setup(&env);
        fill(&env, &a, &b);
        let r = report(&env.ctx(), &input(ReportBy::Task)).unwrap();
        assert_eq!(r.total_ms, 100 * MIN);
        assert_eq!(r.excluded.idle_ms, 30 * MIN);
        assert_eq!(r.excluded.break_ms, 10 * MIN);
        assert_eq!(r.rows.len(), 3);
        assert_eq!(r.rows[0].label, "A");
        assert!(r
            .rows
            .iter()
            .any(|x| x.key.is_none() && x.label == "(なし)" && x.duration_ms == 10 * MIN));

        let mut i = input(ReportBy::Kind);
        i.include_idle = true;
        let r = report(&env.ctx(), &i).unwrap();
        assert_eq!(r.total_ms, 140 * MIN);
        assert_eq!(r.excluded.idle_ms, 0);
    }

    #[test]
    fn by_urgent_important_key_result_skill_agent() {
        let env = Env::new();
        let (a, b) = setup(&env);
        fill(&env, &a, &b);
        let q = report(&env.ctx(), &input(ReportBy::UrgentImportant)).unwrap();
        let labels: Vec<_> = q.rows.iter().map(|r| r.label.as_str()).collect();
        assert_eq!(labels, ["緊急かつ重要", "未設定", "(なし)"]);

        let o = report(&env.ctx(), &input(ReportBy::KeyResult)).unwrap();
        assert_eq!(o.rows[0].label, "O1 / KR1");
        assert_eq!(o.rows[0].duration_ms, 60 * MIN);
        assert_eq!(o.rows[1].label, "(なし)");
        assert_eq!(o.rows[1].duration_ms, 40 * MIN);

        let s = report(&env.ctx(), &input(ReportBy::Skill)).unwrap();
        assert!(s.multi_attribution);
        assert_eq!(s.total_ms, 100 * MIN);
        let rust = s.rows.iter().find(|r| r.label == "Rust").unwrap();
        let design = s.rows.iter().find(|r| r.label == "設計").unwrap();
        assert_eq!(rust.duration_ms, 60 * MIN);
        assert_eq!(design.duration_ms, 60 * MIN);
        assert!(s.rows.iter().map(|r| r.duration_ms).sum::<i64>() > s.total_ms);

        let ag = report(&env.ctx(), &input(ReportBy::Agent)).unwrap();
        assert_eq!(ag.total_ms, 30 * MIN);
        assert_eq!(ag.rows.len(), 1);
        assert_eq!(ag.rows[0].key.as_deref(), Some("SESS"));
        assert_eq!(ag.rows[0].breakdown["agent_dialog"], 30 * MIN);
    }

    #[test]
    fn clips_to_range() {
        let env = Env::new();
        let d = env.day_start();
        insert_segment(
            env.ctx().conn,
            "S1",
            d - 30 * MIN,
            d + 30 * MIN,
            SegmentKind::Work,
            None,
            SegmentStatus::Auto,
        );
        let r = report(&env.ctx(), &input(ReportBy::Kind)).unwrap();
        assert_eq!(r.total_ms, 30 * MIN);
        assert!("bogus".parse::<ReportBy>().is_err());
    }
}

#[cfg(test)]
mod ignored_tests {
    use super::*;
    use crate::model::SegmentStatus;
    use crate::service::testutil::{insert_segment, Env};

    #[test]
    fn ignored_is_excluded_from_every_view() {
        let env = Env::new();
        let c = env.ctx();
        let d = env.day_start();
        const MIN: i64 = 60_000;
        let st = SegmentStatus::Auto;
        insert_segment(c.conn, "W", d, d + 30 * MIN, SegmentKind::Work, None, st);
        insert_segment(
            c.conn,
            "G",
            d + 30 * MIN,
            d + 90 * MIN,
            SegmentKind::Ignored,
            None,
            st,
        );
        for by in [ReportBy::Task, ReportBy::Kind, ReportBy::Activity] {
            for include_idle in [false, true] {
                let r = report(
                    &c,
                    &ReportInput {
                        range: crate::service::RangeArgs::default(),
                        by,
                        include_idle,
                    },
                )
                .unwrap();
                assert_eq!(r.total_ms, 30 * MIN);
                assert_eq!(r.excluded.ignored_ms, 60 * MIN);
            }
        }
        let s = crate::service::status::status(&c).unwrap();
        assert_eq!((s.today.total_ms, s.today.ignored_ms), (30 * MIN, 60 * MIN));
    }
}
