//! AI 振り返り用コンテキストの組み立て（docs/dev/architecture.md「AI 振り返り」）。
//!
//! 集計は [`super::report`] を再利用する。AI が読みやすいよう、時刻はローカル RFC 3339、時間は分で表す。
//! プライバシーのため、ウインドウタイトルの全文・プロンプト抜粋は含めず、タスク名・活動ラベル・集計値を中心にする。

use super::report::{self, Report, ReportBy, ReportInput, ReportRow};
use super::{range_view, Ctx, RangeArgs, RangeView};
use crate::model::{Checkin, ErgTag, Hassle, ReflectionKind, SegmentKind, SegmentStatus, Task};
use crate::repo::{self, plan, raw, record, segment};
use crate::timefmt::{self, Range};
use crate::{Error, Millis, Result};
use chrono::NaiveDate;
use chrono_tz::Tz;
use serde::Serialize;
use std::collections::BTreeMap;

/// コンテキスト JSON のスキーマ版（`"version"`）。
pub const CONTEXT_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize)]
pub struct RowView {
    pub label: String,
    pub minutes: i64,
    pub ratio: f64,
    pub segments: usize,
}

/// 期間の集計。`by` のキーは task / kind / activity / urgent_important / key_result / skill / agent。
#[derive(Debug, Clone, Serialize)]
pub struct Summary {
    pub range: RangeView,
    pub total_minutes: i64,
    pub idle_minutes: i64,
    /// 無視した区間（`ignored`）の分。`total_minutes` には含まれない。
    pub ignored_minutes: i64,
    pub break_minutes: i64,
    pub by: BTreeMap<String, Vec<RowView>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CalendarEventView {
    pub start: String,
    pub end: String,
    pub all_day: bool,
    pub title: String,
    pub is_busy: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct TimeBlockView {
    pub start: String,
    pub end: String,
    pub minutes: i64,
    pub task_title: Option<String>,
    pub label: Option<String>,
    pub focus: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Plan {
    pub calendar_events: Vec<CalendarEventView>,
    pub time_blocks: Vec<TimeBlockView>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DoingTaskView {
    pub title: String,
    pub urgent: Option<bool>,
    pub important: Option<bool>,
    pub estimate_min: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CheckinView {
    pub at: String,
    pub energy: Option<i64>,
    pub mood: Option<i64>,
    pub stress: Option<i64>,
    pub erg_tag: Option<ErgTag>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct HassleView {
    pub at: String,
    pub text: String,
    pub intensity: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct AbcView {
    pub at: String,
    pub activating_event: String,
    pub belief: Option<String>,
    pub consequence_emotion: Option<String>,
    pub consequence_intensity: Option<i64>,
    pub has_dispute: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct InterruptView {
    pub start: String,
    pub end: String,
    pub minutes: i64,
    pub task_title: Option<String>,
    pub activity_label: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Interrupts {
    pub total_minutes: i64,
    pub count: usize,
    pub items: Vec<InterruptView>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiffRow {
    pub label: String,
    pub current_minutes: i64,
    pub previous_minutes: i64,
    pub delta_minutes: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct WeekDiff {
    pub previous_range: RangeView,
    pub previous_total_minutes: i64,
    pub total_delta_minutes: i64,
    pub by_kind: Vec<DiffRow>,
    /// 差の絶対値が大きい上位 5 件。
    pub top_task_changes: Vec<DiffRow>,
}

#[derive(Debug, Clone, Serialize)]
pub struct KeyResultView {
    pub title: String,
    pub target: Option<String>,
    pub current: Option<String>,
    pub status: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct GoalView {
    pub title: String,
    pub status: String,
    pub key_results: Vec<KeyResultView>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GoalProgress {
    pub goals: Vec<GoalView>,
    /// 期間中の投下時間（目標別。どの成果の指標にも紐付かない行は「(なし)」）。
    pub invested: Vec<RowView>,
    /// どの成果の指標にも紐付かない時間の割合（0〜1）。
    pub unlinked_ratio: f64,
}

/// `hitoiki context` の出力。種別に応じて該当しない節は省かれる。
#[derive(Debug, Clone, Serialize)]
pub struct ContextJson {
    pub version: u32,
    pub kind: ReflectionKind,
    /// 基準日（`YYYY-MM-DD`）。weekly は週に含まれる日。
    pub date: String,
    pub generated_at: String,
    pub timezone: String,

    /// day_start: 前日 / day_end: 当日 / weekly: 当週。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<Summary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan: Option<Plan>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub doing_tasks: Option<Vec<DoingTaskView>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interrupts: Option<Interrupts>,
    pub hassles: Vec<HassleView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub abc_records: Option<Vec<AbcView>>,
    pub checkins: Vec<CheckinView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub needs_review_count: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub goals: Option<GoalProgress>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_week_diff: Option<WeekDiff>,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// 期間内の記録の ERG の分布（件数。事実の集計で、評価は含まない）。
    pub erg_distribution: Option<super::erg::ErgDistribution>,
}

fn rows(rows: &[ReportRow]) -> Vec<RowView> {
    rows.iter()
        .map(|r| RowView {
            label: r.label.clone(),
            minutes: r.duration_ms / 60_000,
            ratio: (r.ratio * 1000.0).round() / 1000.0,
            segments: r.segments,
        })
        .collect()
}

fn run_report(ctx: &Ctx, range: Range, tz: Tz, by: ReportBy) -> Result<Report> {
    report::report(
        ctx,
        &ReportInput {
            range: RangeArgs {
                date: None,
                from: Some(timefmt::fmt_rfc3339(tz, range.start)),
                to: Some(timefmt::fmt_rfc3339(tz, range.end)),
            },
            by,
            include_idle: false,
        },
    )
}

fn summary(ctx: &Ctx, range: Range, tz: Tz, bys: &[ReportBy]) -> Result<Summary> {
    let mut by = BTreeMap::new();
    let (mut total, mut idle, mut brk, mut ignored) = (0, 0, 0, 0);
    for b in bys {
        let r = run_report(ctx, range, tz, *b)?;
        if *b == ReportBy::Kind {
            total = r.total_ms;
            idle = r.excluded.idle_ms;
            brk = r.excluded.break_ms;
            ignored = r.excluded.ignored_ms;
        }
        by.insert(b.as_str().to_string(), rows(&r.rows));
    }
    Ok(Summary {
        range: range_view(tz, range),
        total_minutes: total / 60_000,
        idle_minutes: idle / 60_000,
        ignored_minutes: ignored / 60_000,
        break_minutes: brk / 60_000,
        by,
    })
}

fn checkin_views(tz: Tz, list: &[Checkin]) -> Vec<CheckinView> {
    list.iter()
        .map(|c| CheckinView {
            at: timefmt::fmt_rfc3339(tz, c.at),
            energy: c.energy,
            mood: c.mood,
            stress: c.stress,
            erg_tag: c.erg_tag,
            note: c.note.clone(),
        })
        .collect()
}

fn hassle_views(tz: Tz, list: &[Hassle]) -> Vec<HassleView> {
    list.iter()
        .map(|h| HassleView {
            at: timefmt::fmt_rfc3339(tz, h.at),
            text: h.text.clone(),
            intensity: h.intensity,
        })
        .collect()
}

fn interrupts(ctx: &Ctx, range: Range, tz: Tz) -> Result<Interrupts> {
    let titles = super::task_titles(ctx.conn)?;
    let mut items = Vec::new();
    let mut total = 0;
    for s in segment::list(ctx.conn, range)? {
        if s.kind != SegmentKind::Interrupt {
            continue;
        }
        let ms = segment::clipped_len(&s, range);
        if ms == 0 {
            continue;
        }
        total += ms;
        items.push(InterruptView {
            start: timefmt::fmt_rfc3339(tz, s.start_at.max(range.start)),
            end: timefmt::fmt_rfc3339(tz, s.end_at.min(range.end)),
            minutes: ms / 60_000,
            task_title: s.task_id.as_ref().and_then(|id| titles.get(id).cloned()),
            activity_label: s.activity_label.clone(),
        });
    }
    Ok(Interrupts {
        total_minutes: total / 60_000,
        count: items.len(),
        items,
    })
}

fn plan_view(ctx: &Ctx, range: Range, tz: Tz) -> Result<Plan> {
    let titles = super::task_titles(ctx.conn)?;
    let calendar_events = raw::calendar_events(ctx.conn, range)?
        .into_iter()
        .map(|e| CalendarEventView {
            start: timefmt::fmt_rfc3339(tz, e.start_at),
            end: timefmt::fmt_rfc3339(tz, e.end_at),
            all_day: e.all_day,
            title: e.title,
            is_busy: e.is_busy,
        })
        .collect();
    let time_blocks = plan::list(ctx.conn, range)?
        .into_iter()
        .map(|b| TimeBlockView {
            start: timefmt::fmt_rfc3339(tz, b.start_at),
            end: timefmt::fmt_rfc3339(tz, b.end_at),
            minutes: (b.end_at - b.start_at) / 60_000,
            task_title: b.task_id.as_ref().and_then(|id| titles.get(id).cloned()),
            label: b.label,
            focus: b.focus,
        })
        .collect();
    Ok(Plan {
        calendar_events,
        time_blocks,
    })
}

/// 要確認の件数。仕分け一覧の既定表示と同じく `review.min_sec` 以上のものだけを数える。
fn needs_review_count(ctx: &Ctx) -> Result<i64> {
    Ok(crate::repo::segment::count_by_status_split(
        ctx.conn,
        SegmentStatus::NeedsReview,
        ctx.cfg.review.min_sec as i64 * 1000,
    )?
    .0)
}

fn diff_rows(cur: &[ReportRow], prev: &[ReportRow]) -> Vec<DiffRow> {
    let mut map: BTreeMap<String, (i64, i64)> = BTreeMap::new();
    for r in cur {
        map.entry(r.label.clone()).or_default().0 += r.duration_ms / 60_000;
    }
    for r in prev {
        map.entry(r.label.clone()).or_default().1 += r.duration_ms / 60_000;
    }
    let mut out: Vec<DiffRow> = map
        .into_iter()
        .map(|(label, (c, p))| DiffRow {
            label,
            current_minutes: c,
            previous_minutes: p,
            delta_minutes: c - p,
        })
        .collect();
    out.sort_by(|a, b| {
        b.delta_minutes
            .abs()
            .cmp(&a.delta_minutes.abs())
            .then_with(|| a.label.cmp(&b.label))
    });
    out
}

fn doing_tasks(tasks: Vec<Task>) -> Vec<DoingTaskView> {
    tasks
        .into_iter()
        .map(|t| DoingTaskView {
            title: t.title,
            urgent: t.urgent,
            important: t.important,
            estimate_min: t.estimate_min,
        })
        .collect()
}

fn base_date(ctx: &Ctx, tz: Tz, date: Option<&str>) -> Result<NaiveDate> {
    timefmt::parse_date(tz, date.unwrap_or("today"), ctx.now())
}

/// 振り返り用コンテキストを組み立てる。`date` 省略時は今日（weekly は今日を含む週）。
pub fn context(ctx: &Ctx, kind: ReflectionKind, date: Option<&str>) -> Result<ContextJson> {
    let tz = ctx.tz()?;
    let d = base_date(ctx, tz, date)?;
    let now: Millis = ctx.now();
    let mut out = ContextJson {
        version: CONTEXT_VERSION,
        kind,
        date: d.format("%Y-%m-%d").to_string(),
        generated_at: timefmt::fmt_rfc3339(tz, now),
        timezone: ctx.cfg.general.timezone.clone(),
        summary: None,
        plan: None,
        doing_tasks: None,
        interrupts: None,
        hassles: vec![],
        abc_records: None,
        checkins: vec![],
        needs_review_count: None,
        goals: None,
        previous_week_diff: None,
        erg_distribution: None,
    };

    match kind {
        ReflectionKind::DayStart => {
            let today = timefmt::day_range(tz, d);
            let prev = timefmt::day_range(tz, d - chrono::Duration::days(1));
            out.summary = Some(summary(
                ctx,
                prev,
                tz,
                &[ReportBy::Task, ReportBy::Kind, ReportBy::Agent],
            )?);
            out.hassles = hassle_views(tz, &record::list_hassles(ctx.conn, prev.start, prev.end)?);
            out.plan = Some(plan_view(ctx, today, tz)?);
            out.doing_tasks = Some(doing_tasks(repo::task::list_doing(ctx.conn)?));
            // 直近のチェックイン: 前日の始まりから基準日の終わりまで。
            out.checkins = checkin_views(tz, &record::list_checkins(ctx.conn, prev.start, today.end)?);
            out.needs_review_count = Some(needs_review_count(ctx)?);
        }
        ReflectionKind::DayEnd => {
            let day = timefmt::day_range(tz, d);
            out.summary = Some(summary(
                ctx,
                day,
                tz,
                &[
                    ReportBy::Task,
                    ReportBy::Activity,
                    ReportBy::UrgentImportant,
                    ReportBy::KeyResult,
                    ReportBy::Skill,
                    ReportBy::Kind,
                    ReportBy::Agent,
                ],
            )?);
            out.interrupts = Some(interrupts(ctx, day, tz)?);
            out.hassles = hassle_views(tz, &record::list_hassles(ctx.conn, day.start, day.end)?);
            out.checkins = checkin_views(tz, &record::list_checkins(ctx.conn, day.start, day.end)?);
            out.needs_review_count = Some(needs_review_count(ctx)?);
        }
        ReflectionKind::Weekly => {
            let week = timefmt::week_range(tz, d);
            let prev_week = Range::new(
                timefmt::week_range(tz, d - chrono::Duration::days(7)).start,
                week.start,
            );
            let sum = summary(
                ctx,
                week,
                tz,
                &[
                    ReportBy::Task,
                    ReportBy::Activity,
                    ReportBy::UrgentImportant,
                    ReportBy::KeyResult,
                    ReportBy::Skill,
                    ReportBy::Kind,
                    ReportBy::Agent,
                ],
            )?;

            // 前週との差分（kind / task）。
            let cur_kind = run_report(ctx, week, tz, ReportBy::Kind)?;
            let prev_kind = run_report(ctx, prev_week, tz, ReportBy::Kind)?;
            let cur_task = run_report(ctx, week, tz, ReportBy::Task)?;
            let prev_task = run_report(ctx, prev_week, tz, ReportBy::Task)?;
            out.previous_week_diff = Some(WeekDiff {
                previous_range: range_view(tz, prev_week),
                previous_total_minutes: prev_kind.total_ms / 60_000,
                total_delta_minutes: (cur_kind.total_ms - prev_kind.total_ms) / 60_000,
                by_kind: diff_rows(&cur_kind.rows, &prev_kind.rows),
                top_task_changes: diff_rows(&cur_task.rows, &prev_task.rows)
                    .into_iter()
                    .take(5)
                    .collect(),
            });

            // 目標: 目標と成果の指標、期間中の投下時間。
            let key_result_report = run_report(ctx, week, tz, ReportBy::KeyResult)?;
            let unlinked_ms: i64 = key_result_report
                .rows
                .iter()
                .filter(|r| r.key.is_none())
                .map(|r| r.duration_ms)
                .sum();
            let list = super::goal::list(ctx)?;
            out.goals = Some(GoalProgress {
                goals: list
                    .goals
                    .into_iter()
                    .map(|o| GoalView {
                        title: o.goal.title,
                        status: o.goal.status,
                        key_results: o
                            .key_results
                            .into_iter()
                            .map(|k| KeyResultView {
                                title: k.title,
                                target: k.target,
                                current: k.current,
                                status: k.status,
                            })
                            .collect(),
                    })
                    .collect(),
                invested: rows(&key_result_report.rows),
                unlinked_ratio: if key_result_report.total_ms > 0 {
                    ((unlinked_ms as f64 / key_result_report.total_ms as f64) * 1000.0).round() / 1000.0
                } else {
                    0.0
                },
            });

            out.summary = Some(sum);
            out.interrupts = Some(interrupts(ctx, week, tz)?);
            out.hassles = hassle_views(tz, &record::list_hassles(ctx.conn, week.start, week.end)?);
            out.abc_records = Some(
                list_abc(ctx, week)?
                    .into_iter()
                    .map(|a| AbcView {
                        at: timefmt::fmt_rfc3339(tz, a.at),
                        activating_event: a.activating_event,
                        belief: a.belief,
                        consequence_emotion: a.consequence_emotion,
                        consequence_intensity: a.consequence_intensity,
                        has_dispute: a.dispute.is_some(),
                    })
                    .collect(),
            );
            let checkins = record::list_checkins(ctx.conn, week.start, week.end)?;
            out.erg_distribution = Some(super::erg::distribution(ctx, week)?);
            out.checkins = checkin_views(tz, &checkins);
            out.needs_review_count = Some(needs_review_count(ctx)?);
        }
    }
    Ok(out)
}

fn list_abc(ctx: &Ctx, range: Range) -> Result<Vec<crate::model::AbcRecord>> {
    repo::record::list_abc(ctx.conn, range.start, range.end)
}

/// `--kind` 文字列の解釈。
pub fn parse_kind(s: &str) -> Result<ReflectionKind> {
    s.parse().map_err(|e: Error| e)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::id::new_id;
    use crate::model::{AbcRecord, SegmentKind, SegmentStatus};
    use crate::service::task::{self, TaskInput};
    use crate::service::testutil::{insert_segment, Env};

    const MIN: i64 = 60_000;

    fn seeded() -> Env {
        let env = Env::new();
        let ctx = env.ctx();
        let t = task::add(
            &ctx,
            &TaskInput {
                title: Some("設計書".into()),
                ..Default::default()
            },
        )
        .unwrap();
        let d = env.day_start();
        let day = 24 * 60 * MIN;
        // 当日 / 前日 / 前週
        insert_segment(
            &env.conn,
            "S1",
            d + 9 * 60 * MIN,
            d + 10 * 60 * MIN,
            SegmentKind::Work,
            Some(&t.task.id),
            SegmentStatus::Auto,
        );
        insert_segment(
            &env.conn,
            "S2",
            d + 10 * 60 * MIN,
            d + 10 * 60 * MIN + 20 * MIN,
            SegmentKind::Interrupt,
            None,
            SegmentStatus::NeedsReview,
        );
        insert_segment(
            &env.conn,
            "S3",
            d - day + 9 * 60 * MIN,
            d - day + 11 * 60 * MIN,
            SegmentKind::AgentDialog,
            Some(&t.task.id),
            SegmentStatus::Auto,
        );
        insert_segment(
            &env.conn,
            "S4",
            d - 7 * day + 9 * 60 * MIN,
            d - 7 * day + 10 * 60 * MIN,
            SegmentKind::Work,
            Some(&t.task.id),
            SegmentStatus::Auto,
        );
        crate::service::record::check(
            &ctx,
            &crate::service::record::CheckInput {
                energy: Some(3),
                erg: Some(ErgTag::Growth),
                ..Default::default()
            },
        )
        .unwrap();
        crate::service::record::hassle(
            &ctx,
            &crate::service::record::HassleInput {
                text: "会議が長い".into(),
                intensity: Some(4),
            },
        )
        .unwrap();
        repo::record::insert_abc(
            &env.conn,
            &AbcRecord {
                id: new_id(),
                at: crate::service::testutil::now(),
                activating_event: "指摘された".into(),
                belief: Some("自分は駄目だ".into()),
                consequence_emotion: None,
                consequence_intensity: None,
                dispute: None,
                ai_dialog_md: None,
                hassle_id: None,
            },
        )
        .unwrap();
        env
    }

    fn json(env: &Env, kind: ReflectionKind) -> serde_json::Value {
        serde_json::to_value(context(&env.ctx(), kind, None).unwrap()).unwrap()
    }

    #[test]
    fn day_start_schema() {
        let env = seeded();
        let v = json(&env, ReflectionKind::DayStart);
        assert_eq!(v["version"], 1);
        assert_eq!(v["kind"], "day_start");
        assert_eq!(v["date"], "2026-10-10");
        // 前日はエージェント対話 120 分
        assert_eq!(v["summary"]["total_minutes"], 120);
        assert_eq!(v["summary"]["by"]["kind"][0]["minutes"], 120);
        assert!(v["summary"]["by"]["agent"].is_array());
        assert!(v["plan"]["calendar_events"].is_array());
        assert!(v["plan"]["time_blocks"].is_array());
        assert!(v["doing_tasks"].is_array());
        assert_eq!(v["checkins"].as_array().unwrap().len(), 1);
        assert!(v.get("interrupts").is_none());
    }

    #[test]
    fn day_end_schema_and_privacy() {
        let env = seeded();
        let v = json(&env, ReflectionKind::DayEnd);
        assert_eq!(v["summary"]["total_minutes"], 80);
        for k in [
            "task",
            "activity",
            "urgent_important",
            "key_result",
            "skill",
            "kind",
            "agent",
        ] {
            assert!(v["summary"]["by"][k].is_array(), "{k}");
        }
        assert_eq!(v["interrupts"]["total_minutes"], 20);
        assert_eq!(v["interrupts"]["count"], 1);
        assert_eq!(v["hassles"][0]["intensity"], 4);
        assert_eq!(v["needs_review_count"], 1);
        // ウインドウタイトル（main.rs）や exe 名は含まれない
        let s = v.to_string();
        assert!(!s.contains("main.rs"));
        assert!(!s.contains("Code.exe"));
        assert!(v["summary"]["range"]["start"]
            .as_str()
            .unwrap()
            .contains("+09:00"));
    }

    #[test]
    fn weekly_schema() {
        let env = seeded();
        let v = json(&env, ReflectionKind::Weekly);
        // 2026-10-10 は土曜。週は 10-05(月) 始まり。前週 S4 は含まれず前週側に入る。
        assert!(v["summary"]["range"]["start"]
            .as_str()
            .unwrap()
            .starts_with("2026-10-05"));
        assert_eq!(v["summary"]["total_minutes"], 80 + 120);
        assert_eq!(v["previous_week_diff"]["previous_total_minutes"], 60);
        assert_eq!(v["previous_week_diff"]["total_delta_minutes"], 140);
        assert!(v["previous_week_diff"]["top_task_changes"].is_array());
        assert!(v["goals"]["goals"].is_array());
        assert!(v["goals"]["unlinked_ratio"].is_number());
        assert_eq!(v["erg_distribution"]["growth"], 1);
        assert_eq!(v["abc_records"].as_array().unwrap().len(), 1);
        assert_eq!(v["abc_records"][0]["belief"], "自分は駄目だ");
    }

    #[test]
    fn bad_date_is_error() {
        let env = Env::new();
        assert!(context(&env.ctx(), ReflectionKind::DayEnd, Some("xx")).is_err());
    }
}
