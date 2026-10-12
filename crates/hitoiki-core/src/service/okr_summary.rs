//! KR の集計（紐付いたタスクの時間・取り組んだ日・完了数）。
//!
//! 実績は常に `activity_segment`（`segment_override` 適用後の派生層）から算出する。
//! 時間は「今日の合計」と同じ数え方で、無操作・休憩・無視は含めない。
//!
//! KR の時間はスキルが主: KR に付けたスキル（`kr_skill`。子スキルを含む）が付いたタスクの区間、または
//! `activity_skill_rule` でそのスキルが付く活動（区間の exe・タイトルに一致）の区間を数える。
//! タスクとの紐付け（`task.key_result_id`）は「このタスクも含める」補助で、紐付いたタスクの区間も数える。
//! 時間と日数は両方を区間単位の和集合で 1 つの数字にする（同じ区間を二重に数えない）。
//! 内訳（`from_skill_ms` / `from_task_ms`）は、スキルの条件に合う区間をスキルから、それ以外の紐付いたタスクの区間を
//! タスクから、と数える（両方に当てはまる区間はスキルから）。合計は `time_ms` と一致する。
//! 完了数の軸だけは、対象を紐付けたタスクに限る。いずれも事実の数値で、目標との比較はしない。
//!
//! `target` / `current` は自由記述のままで、達成率の計算や評価はしない（選んだ軸の値を出すだけ）。
//!
//! 期間: Objective に `period_start` / `period_end` があればそれ。無ければ直近 8 週
//! （今日を含む週の月曜から 7 週前の月曜〜今日の終わり）。片方だけのときは、開始のみなら今日まで、
//! 終了のみなら 8 週前から。週は月曜始まり、日付はローカル日付。

use super::heatmap::add_per_day;
use super::skill::descendants;
use super::{range_view, Ctx, RangeView};
use crate::model::{KeyResult, SegmentKind, TaskStatus};
use crate::repo::{self, edit};
use crate::rules::{MatchTarget, Matcher};
use crate::timefmt::{self, Range};
use crate::Result;
use chrono::{Datelike, Duration, NaiveDate};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap, HashSet};

/// 既定の集計期間（週）。
pub const DEFAULT_WEEKS: i64 = 8;

#[derive(Debug, Clone, Serialize)]
pub struct ObjectiveBrief {
    pub id: String,
    pub title: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct WeekBar {
    /// 週の月曜（`YYYY-MM-DD`）。
    pub week_start: String,
    pub duration_ms: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct SkillTime {
    /// スキルが付いていないタスクの時間は null。
    pub skill_id: Option<String>,
    pub name: String,
    pub duration_ms: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct KrTask {
    pub id: String,
    pub title: String,
    pub status: TaskStatus,
    pub estimate_min: Option<i64>,
    /// 期間内にこのタスクへ帰属した時間（ミリ秒）。
    pub tracked_ms: i64,
    pub done_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct EstimateSummary {
    /// 見積のあるタスク（やめたものを除く）の見積の合計（分）。
    pub estimate_min_total: i64,
    pub estimated_tasks: i64,
    /// 見積のあるタスクの、期間内の実績（ミリ秒）。
    pub actual_ms_of_estimated: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct KrSummary {
    pub key_result: KeyResult,
    pub objective: ObjectiveBrief,
    pub range: RangeView,
    /// `given`（引数で指定）/ `objective`（Objective の期間）/ `recent_weeks`（直近 8 週）。
    pub range_source: &'static str,
    /// 選んだ軸（`time` / `days` / `done`）。
    pub measure: String,
    /// 時間: KR のスキルが付いた区間と、紐付いたタスクの区間の和集合（ミリ秒）。
    pub time_ms: i64,
    /// 回数: 上の時間の合計が `day_min` 分以上あった日数。
    pub active_days: i64,
    pub day_min: i64,
    /// KR に付けたスキルの名前（名前順）。
    pub kr_skills: Vec<String>,
    /// 内訳: KR のスキルの条件に合う区間の時間（ミリ秒。タスクにスキルが付いている区間と、活動ルールで付く区間）。
    pub from_skill_ms: i64,
    /// 内訳: 上に当てはまらない、紐付いたタスクの区間の時間（ミリ秒）。`from_skill_ms` との合計が `time_ms`。
    pub from_task_ms: i64,
    /// 完了数: 期間内に完了したタスク。
    pub done_count: i64,
    /// 紐付いたタスクの数（やめたものを除く）。完了数の軸の対象。
    pub task_count: i64,
    pub weekly: Vec<WeekBar>,
    /// report の skill 集計と同じ扱い（複数スキルが付いた区間は各スキルに全時間を計上するので、合計は `time_ms` を超えうる）。
    pub skills: Vec<SkillTime>,
    pub estimate: EstimateSummary,
    pub tasks: Vec<KrTask>,
}

fn monday_of(d: NaiveDate) -> NaiveDate {
    d - Duration::days(d.weekday().num_days_from_monday() as i64)
}

fn resolve_range(
    ctx: &Ctx,
    tz: chrono_tz::Tz,
    start: Option<i64>,
    end: Option<i64>,
    given: Option<Range>,
) -> (Range, &'static str) {
    if let Some(r) = given {
        return (r, "given");
    }
    let today = timefmt::local_date_of(tz, ctx.now());
    let today_end = timefmt::day_range(tz, today).end;
    let recent_start = timefmt::day_range(tz, monday_of(today) - Duration::weeks(DEFAULT_WEEKS - 1)).start;
    match (start, end) {
        (Some(s), Some(e)) => (Range::new(s, e), "objective"),
        (Some(s), None) => (Range::new(s, today_end.max(s + 1)), "objective"),
        (None, Some(e)) => (
            Range::new((e - DEFAULT_WEEKS * 7 * 86_400_000).min(e - 1), e),
            "objective",
        ),
        (None, None) => (Range::new(recent_start, today_end), "recent_weeks"),
    }
}

/// KR の集計に入る区間を選ぶ判定（`okr show` とヒートマップの `kr:` で共有する）。
/// KR のスキル（子スキルを含む）が付いたタスク・活動ルールでそのスキルが付く活動・紐付けたタスクの和集合。
pub(crate) struct KrSelector {
    linked: HashSet<String>,
    skill_set: HashSet<String>,
    task_skills: HashMap<String, Vec<String>>,
    rules: Vec<(Matcher, String)>,
}

/// 選ばれた区間の内訳。
pub(crate) struct KrPick {
    /// KR のスキル経由で入る（紐付けたタスクだけで入るときは false）。
    pub via_skill: bool,
    /// 紐付けたタスクの区間。
    pub linked: bool,
    /// この区間に付くスキル（タスクのスキルと活動ルールで付くスキル）。
    pub skill_ids: Vec<String>,
}

impl KrSelector {
    pub(crate) fn new(ctx: &Ctx, kr_id: &str) -> Result<Self> {
        Self::with_conn(ctx.conn, kr_id)
    }

    /// DB 接続だけで作る（常駐プロセスの通知の判定用。`Ctx` を持たない呼び出し側向け）。
    pub(crate) fn with_conn(conn: &rusqlite::Connection, kr_id: &str) -> Result<Self> {
        let linked = repo::task::list_of_kr(conn, kr_id)?
            .into_iter()
            .map(|t| t.id)
            .collect();
        let all_skills = repo::skill::list(conn)?;
        let mut skill_set: HashSet<String> = HashSet::new();
        for s in repo::skill::kr_skills(conn, kr_id)? {
            skill_set.extend(descendants(&all_skills, &s.id));
            skill_set.insert(s.id.clone());
        }
        // タスクごとのスキル（report の skill 集計と同じく、付いているスキルそれぞれに全時間を計上する）。
        let mut task_skills: HashMap<String, Vec<String>> = HashMap::new();
        {
            let mut stmt = conn.prepare("SELECT task_id, skill_id FROM task_skill")?;
            let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
            for row in rows {
                let (tid, sid) = row?;
                task_skills.entry(tid).or_default().push(sid);
            }
        }
        // 活動からスキルを付けるルール（不正なパターンは無視する）。
        let rules = repo::skill::list_activity_skill_rules(conn)?
            .into_iter()
            .filter_map(|r| {
                Matcher::new(r.field, &r.pattern, r.pattern_type)
                    .ok()
                    .map(|m| (m, r.skill_id))
            })
            .collect();
        Ok(Self {
            linked,
            skill_set,
            task_skills,
            rules,
        })
    }

    /// 区間が KR の集計に入るなら内訳を返す（無操作・休憩・無視は入らない）。
    pub(crate) fn pick(&self, s: &crate::model::ActivitySegment) -> Option<KrPick> {
        if matches!(
            s.kind,
            SegmentKind::Idle | SegmentKind::Break | SegmentKind::Ignored
        ) {
            return None;
        }
        let linked = s.task_id.as_ref().is_some_and(|t| self.linked.contains(t));
        let mut skill_ids: Vec<String> = s
            .task_id
            .as_ref()
            .and_then(|t| self.task_skills.get(t))
            .cloned()
            .unwrap_or_default();
        if !self.rules.is_empty() {
            let target = MatchTarget::window(s.exe_name.as_deref(), s.title.as_deref());
            for (m, sid) in &self.rules {
                if m.is_match(&target) && !skill_ids.contains(sid) {
                    skill_ids.push(sid.clone());
                }
            }
        }
        let via_skill = skill_ids.iter().any(|sid| self.skill_set.contains(sid));
        (via_skill || linked).then_some(KrPick {
            via_skill,
            linked,
            skill_ids,
        })
    }
}

/// 今週（月曜始まり、今日まで）まだ時間が入っていない KR（kr_idle 通知の対象）。
///
/// 対象は、KR が active で軸が time / days、Objective が active で今が期間内（期間が無ければ対象。
/// 終わった・まだ始まっていない期間は対象外）、スキルか紐付けたタスク（やめたものを除く）のどちらかがある KR。
/// 「時間が入っている」は、`okr show` と同じ選び方（スキル経由と紐付けタスクの和集合）の区間が今週 1 つでもあること。
pub fn idle_krs_this_week(
    conn: &rusqlite::Connection,
    tz: chrono_tz::Tz,
    now: i64,
) -> Result<Vec<KeyResult>> {
    let week = timefmt::week_range(tz, timefmt::local_date_of(tz, now));
    let range = Range::new(week.start, now.max(week.start + 1));
    let objectives = repo::okr::list_objectives(conn)?;
    let mut segs: Option<Vec<crate::model::ActivitySegment>> = None;
    let mut out = Vec::new();
    for k in repo::okr::list_krs(conn)? {
        if k.status != "active" || !matches!(k.measure.as_str(), "time" | "days") {
            continue;
        }
        let Some(o) = objectives.iter().find(|o| o.id == k.objective_id) else {
            continue;
        };
        if o.status != "active"
            || o.period_start.is_some_and(|s| now < s)
            || o.period_end.is_some_and(|e| now >= e)
        {
            continue;
        }
        let has_skill = !repo::skill::kr_skills(conn, &k.id)?.is_empty();
        let has_task = repo::task::list_of_kr(conn, &k.id)?
            .iter()
            .any(|t| t.status != TaskStatus::Dropped);
        if !has_skill && !has_task {
            continue;
        }
        if segs.is_none() {
            segs = Some(repo::segment::list(conn, range)?);
        }
        let selector = KrSelector::with_conn(conn, &k.id)?;
        let worked = segs
            .iter()
            .flatten()
            .any(|s| selector.pick(s).is_some() && repo::segment::clipped_len(s, range) > 0);
        if !worked {
            out.push(k);
        }
    }
    Ok(out)
}

/// KR（ID の前方一致またはタイトル）の集計。`range` が `None` なら Objective の期間（無ければ直近 8 週）。
pub fn kr_summary(ctx: &Ctx, kr: &str, range: Option<Range>) -> Result<KrSummary> {
    let k = super::okr::find_kr(ctx, kr)?;
    let tz = ctx.tz()?;
    let o = edit::get_objective(ctx.conn, &k.objective_id)?;
    let (range, source) = resolve_range(ctx, tz, o.period_start, o.period_end, range);

    let linked = repo::task::list_of_kr(ctx.conn, &k.id)?;
    let selector = KrSelector::new(ctx, &k.id)?;

    let all_skills = repo::skill::list(ctx.conn)?;
    let skill_names: HashMap<&str, &str> = all_skills
        .iter()
        .map(|s| (s.id.as_str(), s.name.as_str()))
        .collect();
    let kr_skills = repo::skill::kr_skills(ctx.conn, &k.id)?;

    let mut per_day: BTreeMap<NaiveDate, i64> = BTreeMap::new();
    let mut per_task: HashMap<&str, i64> = HashMap::new();
    let mut per_skill: BTreeMap<(Option<String>, String), i64> = BTreeMap::new();
    let (mut from_skill_ms, mut from_task_ms) = (0i64, 0i64);
    for s in repo::segment::list(ctx.conn, range)? {
        let Some(pick) = selector.pick(&s) else {
            continue;
        };
        let ms = repo::segment::clipped_len(&s, range);
        if ms == 0 {
            continue;
        }
        add_per_day(&mut per_day, tz, range, s.start_at, s.end_at);
        if pick.via_skill {
            from_skill_ms += ms;
        } else {
            from_task_ms += ms;
        }
        if pick.linked {
            if let Some(tid) = s.task_id.as_deref() {
                if let Some(t) = linked.iter().find(|t| t.id == tid) {
                    *per_task.entry(t.id.as_str()).or_default() += ms;
                }
            }
        }
        let mut named: Vec<(&str, &str)> = pick
            .skill_ids
            .iter()
            .filter_map(|sid| skill_names.get(sid.as_str()).map(|n| (sid.as_str(), *n)))
            .collect();
        named.sort_by(|a, b| a.1.cmp(b.1));
        if named.is_empty() {
            *per_skill.entry((None, "(なし)".into())).or_default() += ms;
        }
        for (sid, name) in named {
            *per_skill
                .entry((Some(sid.to_string()), name.to_string()))
                .or_default() += ms;
        }
    }

    let time_ms: i64 = per_day.values().sum();
    let day_ms = k.day_min.max(1) * 60_000;
    let active_days = per_day.values().filter(|v| **v >= day_ms).count() as i64;

    // 週ごと（月曜始まり）。範囲の最初の週から最後の週まで、値が 0 の週も並べる。
    let first = monday_of(timefmt::local_date_of(tz, range.start));
    let last = timefmt::local_date_of(tz, (range.end - 1).max(range.start));
    let mut weekly = Vec::new();
    let mut w = first;
    while w <= last {
        let ms: i64 = per_day.range(w..w + Duration::days(7)).map(|(_, v)| *v).sum();
        weekly.push(WeekBar {
            week_start: w.format("%Y-%m-%d").to_string(),
            duration_ms: ms,
        });
        w += Duration::days(7);
    }

    let mut skills: Vec<SkillTime> = per_skill
        .into_iter()
        .map(|((skill_id, name), duration_ms)| SkillTime {
            skill_id,
            name,
            duration_ms,
        })
        .collect();
    skills.sort_by(|a, b| {
        b.duration_ms
            .cmp(&a.duration_ms)
            .then_with(|| a.name.cmp(&b.name))
    });

    let in_range = |t: i64| t >= range.start && t < range.end;
    let counted: Vec<_> = linked
        .iter()
        .filter(|t| t.status != TaskStatus::Dropped)
        .collect();
    let done_count = counted
        .iter()
        .filter(|t| t.status == TaskStatus::Done && t.done_at.is_some_and(in_range))
        .count() as i64;
    let with_est: Vec<_> = counted.iter().filter(|t| t.estimate_min.is_some()).collect();
    let estimate = EstimateSummary {
        estimate_min_total: with_est.iter().filter_map(|t| t.estimate_min).sum(),
        estimated_tasks: with_est.len() as i64,
        actual_ms_of_estimated: with_est
            .iter()
            .map(|t| per_task.get(t.id.as_str()).copied().unwrap_or(0))
            .sum(),
    };
    let tasks = linked
        .iter()
        .map(|t| KrTask {
            id: t.id.clone(),
            title: t.title.clone(),
            status: t.status,
            estimate_min: t.estimate_min,
            tracked_ms: per_task.get(t.id.as_str()).copied().unwrap_or(0),
            done_at: t.done_at,
        })
        .collect();

    Ok(KrSummary {
        objective: ObjectiveBrief {
            id: o.id,
            title: o.title,
        },
        range: range_view(tz, range),
        range_source: source,
        measure: k.measure.clone(),
        time_ms,
        active_days,
        day_min: k.day_min,
        kr_skills: kr_skills.into_iter().map(|s| s.name).collect(),
        from_skill_ms,
        from_task_ms,
        done_count,
        task_count: counted.len() as i64,
        weekly,
        skills,
        estimate,
        tasks,
        key_result: k,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::SegmentStatus;
    use crate::service::impact::DeleteMode;
    use crate::service::okr::{self as okr_svc, KrEdit, KrInput, ObjectiveInput};
    use crate::service::skill::{self, SkillInput};
    use crate::service::task::{self, TaskInput};
    use crate::service::testutil::{insert_segment, Env};

    const MIN: i64 = 60_000;
    const DAY: i64 = 24 * 60 * MIN;
    const ST: SegmentStatus = SegmentStatus::Auto;

    struct Fx {
        kr: String,
        t1: String,
        t2: String,
    }

    fn setup(ctx: &Ctx, from: Option<i64>, to: Option<i64>) -> Fx {
        let o = okr_svc::add_objective(
            ctx,
            &ObjectiveInput {
                title: "O".into(),
                from,
                to,
            },
        )
        .unwrap();
        let k = okr_svc::add_kr(
            ctx,
            &KrInput {
                objective: o.id,
                title: "K".into(),
                target: Some("週 3 回".into()),
                skills: vec![],
            },
        )
        .unwrap();
        let mk = |title: &str| {
            task::add(
                ctx,
                &TaskInput {
                    title: Some(title.into()),
                    key_result: Some(k.id.clone()),
                    ..Default::default()
                },
            )
            .unwrap()
            .task
            .id
        };
        Fx {
            t1: mk("T1"),
            t2: mk("T2"),
            kr: k.id,
        }
    }

    fn seg(c: &Ctx, id: &str, start: i64, end: i64, kind: SegmentKind, task: Option<&str>) {
        insert_segment(c.conn, id, start, end, kind, task, ST);
    }

    #[test]
    fn 既定の期間は直近8週で時間は帰属した作業だけを数える() {
        let env = Env::new();
        let c = env.ctx();
        let f = setup(&c, None, None);
        let d = env.day_start(); // 2026-10-10（土）00:00 JST
        seg(
            &c,
            "A",
            d + 60 * MIN,
            d + 120 * MIN,
            SegmentKind::Work,
            Some(&f.t1),
        );
        seg(
            &c,
            "B",
            d + 120 * MIN,
            d + 150 * MIN,
            SegmentKind::Work,
            Some(&f.t2),
        );
        // 無操作・休憩・無視・別タスク・未帰属は数えない。
        seg(
            &c,
            "C",
            d + 150 * MIN,
            d + 180 * MIN,
            SegmentKind::Idle,
            Some(&f.t1),
        );
        seg(
            &c,
            "D",
            d + 180 * MIN,
            d + 190 * MIN,
            SegmentKind::Break,
            Some(&f.t1),
        );
        seg(
            &c,
            "E",
            d + 190 * MIN,
            d + 200 * MIN,
            SegmentKind::Ignored,
            Some(&f.t1),
        );
        seg(&c, "F", d + 200 * MIN, d + 210 * MIN, SegmentKind::Work, None);
        let s = kr_summary(&c, &f.kr[..20], None).unwrap();
        assert_eq!(s.range_source, "recent_weeks");
        assert_eq!(s.time_ms, 90 * MIN);
        assert_eq!(s.active_days, 1);
        assert_eq!(s.weekly.len(), 8);
        // 今日（土曜）を含む週が最後の棒で、全時間がそこに入る。開始は 7 週前の月曜。
        assert_eq!(s.weekly[0].week_start, "2026-08-17");
        assert_eq!(s.weekly[7].week_start, "2026-10-05");
        assert_eq!(s.weekly[7].duration_ms, 90 * MIN);
        assert_eq!(s.weekly.iter().map(|w| w.duration_ms).sum::<i64>(), s.time_ms);
        let t1 = s.tasks.iter().find(|t| t.id == f.t1).unwrap();
        assert_eq!(t1.tracked_ms, 60 * MIN);
        // 期間外の作業は数えない。
        seg(
            &c,
            "G",
            d - 70 * DAY,
            d - 70 * DAY + 30 * MIN,
            SegmentKind::Work,
            Some(&f.t1),
        );
        assert_eq!(kr_summary(&c, &f.kr, None).unwrap().time_ms, 90 * MIN);
    }

    #[test]
    fn objective_の期間があればそれを使う() {
        let env = Env::new();
        let c = env.ctx();
        let d = env.day_start();
        let f = setup(&c, Some(d - 3 * DAY), Some(d - DAY)); // 10/7 0:00 〜 10/9 0:00
        seg(
            &c,
            "A",
            d - 4 * DAY,
            d - 4 * DAY + 30 * MIN,
            SegmentKind::Work,
            Some(&f.t1),
        ); // 前
        seg(
            &c,
            "B",
            d - 3 * DAY,
            d - 3 * DAY + 20 * MIN,
            SegmentKind::Work,
            Some(&f.t1),
        ); // 開始ちょうど
        seg(
            &c,
            "C",
            d - DAY - 10 * MIN,
            d - DAY + 10 * MIN,
            SegmentKind::Work,
            Some(&f.t1),
        ); // 終了をまたぐ
        seg(
            &c,
            "D",
            d - DAY,
            d - DAY + 30 * MIN,
            SegmentKind::Work,
            Some(&f.t1),
        ); // 終了ちょうど以降
        let s = kr_summary(&c, &f.kr, None).unwrap();
        assert_eq!(s.range_source, "objective");
        assert_eq!(s.time_ms, 30 * MIN);
        // 範囲を引数で上書きできる。
        let all = kr_summary(&c, &f.kr, Some(Range::new(d - 10 * DAY, d))).unwrap();
        assert_eq!(all.range_source, "given");
        assert_eq!(all.time_ms, 100 * MIN);
    }

    #[test]
    fn 取り組んだ日は_day_min_以上の日だけで日付の境界で分かれる() {
        let env = Env::new();
        let c = env.ctx();
        let f = setup(&c, None, None);
        let d = env.day_start();
        // 既定は 15 分: ちょうど 15 分は取り組んだ日、14 分は違う。
        seg(
            &c,
            "A",
            d - 2 * DAY + 600 * MIN,
            d - 2 * DAY + 615 * MIN,
            SegmentKind::Work,
            Some(&f.t1),
        );
        seg(
            &c,
            "B",
            d - DAY + 600 * MIN,
            d - DAY + 614 * MIN,
            SegmentKind::Work,
            Some(&f.t1),
        );
        // 同じ日の 2 つのタスクの合計で満たす。
        seg(
            &c,
            "C",
            d + 600 * MIN,
            d + 608 * MIN,
            SegmentKind::Work,
            Some(&f.t1),
        );
        seg(
            &c,
            "D",
            d + 610 * MIN,
            d + 618 * MIN,
            SegmentKind::Work,
            Some(&f.t2),
        );
        let s = kr_summary(&c, &f.kr, None).unwrap();
        assert_eq!(s.active_days, 2);
        // day_min を変えると数え方が変わる。
        okr_svc::edit_kr(
            &c,
            &f.kr,
            &KrEdit {
                day_min: Some(14),
                measure: Some("days".into()),
                ..Default::default()
            },
        )
        .unwrap();
        let s = kr_summary(&c, &f.kr, None).unwrap();
        assert_eq!((s.active_days, s.measure.as_str(), s.day_min), (3, "days", 14));
        okr_svc::edit_kr(
            &c,
            &f.kr,
            &KrEdit {
                day_min: Some(30),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(kr_summary(&c, &f.kr, None).unwrap().active_days, 0);
        // 日付をまたぐ作業は、日ごとに分けて数える（前日 23:50 〜 当日 00:20）。
        seg(
            &c,
            "E",
            d - 10 * MIN,
            d + 20 * MIN,
            SegmentKind::Work,
            Some(&f.t1),
        );
        okr_svc::edit_kr(
            &c,
            &f.kr,
            &KrEdit {
                day_min: Some(25),
                ..Default::default()
            },
        )
        .unwrap();
        // 当日は 8 + 8 + 20（00:00-00:20）= 36 分で足り、前日は 14 + 10 = 24 分で足りない。
        assert_eq!(kr_summary(&c, &f.kr, None).unwrap().active_days, 1);
    }

    #[test]
    fn 完了数は期間内に完了したものと全体() {
        let env = Env::new();
        let c = env.ctx();
        let f = setup(&c, None, None);
        let t3 = task::add(
            &c,
            &TaskInput {
                title: Some("T3".into()),
                key_result: Some(f.kr.clone()),
                ..Default::default()
            },
        )
        .unwrap()
        .task
        .id;
        let t4 = task::add(
            &c,
            &TaskInput {
                title: Some("T4".into()),
                key_result: Some(f.kr.clone()),
                ..Default::default()
            },
        )
        .unwrap()
        .task
        .id;
        task::done(&c, &f.t1).unwrap();
        task::drop_task(&c, &t3).unwrap();
        let s = kr_summary(&c, &f.kr, None).unwrap();
        // t1 が完了、t3 はやめた（全体に含めない）、t2・t4 は未完了。
        assert_eq!((s.done_count, s.task_count), (1, 3));
        assert_eq!(s.tasks.len(), 4);
        // 完了が期間より前なら、完了数に入らず全体には残る。
        let d = env.day_start();
        let s = kr_summary(&c, &f.kr, Some(Range::new(d - 30 * DAY, d - 20 * DAY))).unwrap();
        assert_eq!((s.done_count, s.task_count), (0, 3));
        let _ = t4;
    }

    #[test]
    fn スキル別は各スキルに全時間を計上し見積と実績を並べる() {
        let env = Env::new();
        let c = env.ctx();
        let f = setup(&c, None, None);
        for n in ["Rust", "設計"] {
            skill::add(
                &c,
                &SkillInput {
                    name: n.into(),
                    parent: None,
                },
            )
            .unwrap();
        }
        task::edit(
            &c,
            &f.t1,
            &TaskInput {
                skills: vec!["Rust".into(), "設計".into()],
                estimate_min: Some(90),
                ..Default::default()
            },
        )
        .unwrap();
        let d = env.day_start();
        seg(
            &c,
            "A",
            d + 60 * MIN,
            d + 120 * MIN,
            SegmentKind::Work,
            Some(&f.t1),
        );
        seg(
            &c,
            "B",
            d + 120 * MIN,
            d + 150 * MIN,
            SegmentKind::Work,
            Some(&f.t2),
        );
        let s = kr_summary(&c, &f.kr, None).unwrap();
        let by = |n: &str| s.skills.iter().find(|x| x.name == n).map(|x| x.duration_ms);
        assert_eq!(by("Rust"), Some(60 * MIN));
        assert_eq!(by("設計"), Some(60 * MIN));
        assert_eq!(by("(なし)"), Some(30 * MIN));
        assert_eq!(s.time_ms, 90 * MIN);
        assert_eq!(s.estimate.estimate_min_total, 90);
        assert_eq!(s.estimate.estimated_tasks, 1);
        assert_eq!(s.estimate.actual_ms_of_estimated, 60 * MIN);
    }

    #[test]
    fn 付け替えると集計も移る() {
        let env = Env::new();
        let c = env.ctx();
        let f = setup(&c, None, None);
        let o2 = okr_svc::add_objective(
            &c,
            &ObjectiveInput {
                title: "O2".into(),
                from: None,
                to: None,
            },
        )
        .unwrap();
        let k2 = okr_svc::add_kr(
            &c,
            &KrInput {
                objective: o2.id,
                title: "K2".into(),
                target: None,
                skills: vec![],
            },
        )
        .unwrap();
        let d = env.day_start();
        seg(
            &c,
            "A",
            d + 60 * MIN,
            d + 120 * MIN,
            SegmentKind::Work,
            Some(&f.t1),
        );
        let r = okr_svc::link_tasks(&c, &k2.id[..20], &[f.t1[..20].to_string()]).unwrap();
        assert_eq!(
            r.linked[0].previous_kr.as_ref().map(|p| p.title.as_str()),
            Some("K")
        );
        assert_eq!(kr_summary(&c, &f.kr, None).unwrap().time_ms, 0);
        assert_eq!(kr_summary(&c, &k2.id, None).unwrap().time_ms, 60 * MIN);
        // 外す。KR に紐付いていないものは何もしない。
        let out = okr_svc::unlink_tasks(&c, &[f.t1.clone(), f.t1.clone()]).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(kr_summary(&c, &k2.id, None).unwrap().time_ms, 0);
        assert!(okr_svc::unlink_tasks(&c, std::slice::from_ref(&f.t1))
            .unwrap()
            .is_empty());
        // 見つからないタスクがあれば何も変えない。
        assert!(okr_svc::link_tasks(&c, &k2.id, &[f.t2.clone(), "ZZZZ".into()]).is_err());
        assert_eq!(kr_summary(&c, &k2.id, None).unwrap().tasks.len(), 0);
        assert!(okr_svc::link_tasks(&c, &k2.id, &[]).is_err());
    }

    #[test]
    fn 軸と最小分数の検証() {
        let env = Env::new();
        let c = env.ctx();
        let f = setup(&c, None, None);
        let bad = |e: KrEdit| okr_svc::edit_kr(&c, &f.kr, &e).is_err();
        assert!(bad(KrEdit {
            measure: Some("x".into()),
            ..Default::default()
        }));
        assert!(bad(KrEdit {
            day_min: Some(0),
            ..Default::default()
        }));
        let k = okr_svc::edit_kr(
            &c,
            &f.kr,
            &KrEdit {
                measure: Some("done".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!((k.measure.as_str(), k.day_min), ("done", 15));
    }

    /// 「Rust」（子: 「Rust 非同期」）を KR に付け、紐付けていないタスク `o1`（Rust）・`o2`（子）・`o3`（別スキル）を作る。
    fn setup_skill(c: &Ctx, f: &Fx) -> (String, String, String) {
        for (n, parent) in [("Rust", None), ("Rust 非同期", Some("Rust")), ("設計", None)] {
            skill::add(
                c,
                &SkillInput {
                    name: n.into(),
                    parent: parent.map(String::from),
                },
            )
            .unwrap();
        }
        okr_svc::edit_kr(
            c,
            &f.kr,
            &KrEdit {
                skills: vec!["Rust".into()],
                ..Default::default()
            },
        )
        .unwrap();
        let mk = |title: &str, sk: &str| {
            task::add(
                c,
                &TaskInput {
                    title: Some(title.into()),
                    skills: vec![sk.into()],
                    ..Default::default()
                },
            )
            .unwrap()
            .task
            .id
        };
        (mk("O1", "Rust"), mk("O2", "Rust 非同期"), mk("O3", "設計"))
    }

    #[test]
    fn 時間はスキル_子スキル含む_と紐付けたタスクの和集合で内訳が付く() {
        let env = Env::new();
        let c = env.ctx();
        let f = setup(&c, None, None);
        let (o1, o2, o3) = setup_skill(&c, &f);
        let d = env.day_start();
        seg(
            &c,
            "A",
            d + 60 * MIN,
            d + 120 * MIN,
            SegmentKind::Work,
            Some(&f.t1),
        ); // 紐付き 60
        seg(
            &c,
            "B",
            d + 120 * MIN,
            d + 150 * MIN,
            SegmentKind::Work,
            Some(&o1),
        ); // スキル 30
        seg(
            &c,
            "C",
            d + 150 * MIN,
            d + 160 * MIN,
            SegmentKind::Work,
            Some(&o2),
        ); // 子スキル 10
        seg(
            &c,
            "D",
            d + 160 * MIN,
            d + 190 * MIN,
            SegmentKind::Work,
            Some(&o3),
        ); // 別スキル: 数えない
        seg(&c, "E", d + 190 * MIN, d + 200 * MIN, SegmentKind::Work, None); // タスクなし: 数えない
        seg(
            &c,
            "F",
            d + 200 * MIN,
            d + 210 * MIN,
            SegmentKind::Idle,
            Some(&o1),
        ); // 無操作: 数えない
        let s = kr_summary(&c, &f.kr, None).unwrap();
        assert_eq!(s.kr_skills, vec!["Rust".to_string()]);
        assert_eq!(s.time_ms, 100 * MIN);
        assert_eq!((s.from_skill_ms, s.from_task_ms), (40 * MIN, 60 * MIN));
        assert_eq!(s.weekly.iter().map(|w| w.duration_ms).sum::<i64>(), s.time_ms);
        // 紐付けたタスクの時間はタスクごとに残る（スキル経由のタスクは紐付けていないので出ない）。
        assert_eq!(
            s.tasks.iter().find(|t| t.id == f.t1).unwrap().tracked_ms,
            60 * MIN
        );
        // スキルを外せば、紐付けたタスクだけになる。
        okr_svc::edit_kr(
            &c,
            &f.kr,
            &KrEdit {
                clear_skills: true,
                ..Default::default()
            },
        )
        .unwrap();
        let s = kr_summary(&c, &f.kr, None).unwrap();
        assert_eq!((s.time_ms, s.from_skill_ms, s.kr_skills.len()), (60 * MIN, 0, 0));
    }

    #[test]
    fn 両方に当てはまる区間は一度だけ数えスキルの内訳に入る() {
        let env = Env::new();
        let c = env.ctx();
        let f = setup(&c, None, None);
        setup_skill(&c, &f);
        task::edit(
            &c,
            &f.t1,
            &TaskInput {
                skills: vec!["Rust".into()],
                ..Default::default()
            },
        )
        .unwrap();
        let d = env.day_start();
        seg(
            &c,
            "A",
            d + 60 * MIN,
            d + 120 * MIN,
            SegmentKind::Work,
            Some(&f.t1),
        ); // 紐付き かつ スキル 60
        seg(
            &c,
            "B",
            d + 120 * MIN,
            d + 150 * MIN,
            SegmentKind::Work,
            Some(&f.t2),
        ); // 紐付きのみ 30
        let s = kr_summary(&c, &f.kr, None).unwrap();
        assert_eq!(s.time_ms, 90 * MIN);
        assert_eq!((s.from_skill_ms, s.from_task_ms), (60 * MIN, 30 * MIN));
    }

    #[test]
    fn 活動ルールで付くスキルの区間も数える() {
        let env = Env::new();
        let c = env.ctx();
        let f = setup(&c, None, None);
        setup_skill(&c, &f);
        c.conn
            .execute(
                "INSERT INTO activity_skill_rule (id, field, pattern, pattern_type, skill_id)                  SELECT 'R1', 'title', 'cargo', 'contains', id FROM skill WHERE name = 'Rust 非同期'",
                [],
            )
            .unwrap();
        let d = env.day_start();
        seg(&c, "A", d + 60 * MIN, d + 90 * MIN, SegmentKind::Work, None); // タイトル main.rs: 一致しない
        seg(&c, "B", d + 90 * MIN, d + 110 * MIN, SegmentKind::Work, None);
        c.conn
            .execute(
                "UPDATE activity_segment SET title = 'Cargo.toml' WHERE id = 'B'",
                [],
            )
            .unwrap();
        let s = kr_summary(&c, &f.kr, None).unwrap();
        assert_eq!(
            (s.time_ms, s.from_skill_ms, s.from_task_ms),
            (20 * MIN, 20 * MIN, 0)
        );
        assert_eq!(
            s.skills
                .iter()
                .find(|x| x.name == "Rust 非同期")
                .unwrap()
                .duration_ms,
            20 * MIN
        );
    }

    #[test]
    fn 取り組んだ日はスキルとタスクを合算した時間で数える() {
        let env = Env::new();
        let c = env.ctx();
        let f = setup(&c, None, None);
        let (o1, _, _) = setup_skill(&c, &f);
        let d = env.day_start();
        // day_min 15: 2 日前 = タスク 5 + スキル 10 で届く、1 日前 = 5 + 9 で届かない、当日 = スキルのみ 20 で届く。
        seg(
            &c,
            "A",
            d - 2 * DAY + 600 * MIN,
            d - 2 * DAY + 605 * MIN,
            SegmentKind::Work,
            Some(&f.t1),
        );
        seg(
            &c,
            "B",
            d - 2 * DAY + 640 * MIN,
            d - 2 * DAY + 650 * MIN,
            SegmentKind::Work,
            Some(&o1),
        );
        seg(
            &c,
            "C",
            d - DAY + 600 * MIN,
            d - DAY + 605 * MIN,
            SegmentKind::Work,
            Some(&f.t1),
        );
        seg(
            &c,
            "D",
            d - DAY + 640 * MIN,
            d - DAY + 649 * MIN,
            SegmentKind::Work,
            Some(&o1),
        );
        seg(
            &c,
            "E",
            d + 600 * MIN,
            d + 620 * MIN,
            SegmentKind::Work,
            Some(&o1),
        );
        let s = kr_summary(&c, &f.kr, None).unwrap();
        assert_eq!(s.active_days, 2);
        assert_eq!(s.time_ms, (15 + 14 + 20) * MIN);
    }

    #[test]
    fn ヒートマップの_kr_指標は_okr_show_と同じ区間の和集合で日別に一致する() {
        use crate::service::heatmap::{heatmap, HeatmapInput};
        let env = Env::new();
        let c = env.ctx();
        let f = setup(&c, None, None);
        let (o1, o2, o3) = setup_skill(&c, &f);
        let d = env.day_start();
        // 前日: 紐付き 10 + スキル 20、当日: 子スキル 30 + 別スキル（数えない）+ 紐付き 5。
        seg(
            &c,
            "A",
            d - DAY + 600 * MIN,
            d - DAY + 610 * MIN,
            SegmentKind::Work,
            Some(&f.t1),
        );
        seg(
            &c,
            "B",
            d - DAY + 620 * MIN,
            d - DAY + 640 * MIN,
            SegmentKind::Work,
            Some(&o1),
        );
        seg(
            &c,
            "C",
            d + 600 * MIN,
            d + 630 * MIN,
            SegmentKind::Work,
            Some(&o2),
        );
        seg(
            &c,
            "D",
            d + 640 * MIN,
            d + 660 * MIN,
            SegmentKind::Work,
            Some(&o3),
        );
        seg(
            &c,
            "E",
            d + 670 * MIN,
            d + 675 * MIN,
            SegmentKind::Work,
            Some(&f.t1),
        );
        let h = heatmap(
            &c,
            &HeatmapInput {
                by: Some(format!("kr:{}", f.kr)),
                ..Default::default()
            },
        )
        .unwrap();
        let v = |date: &str| h.days.iter().find(|x| x.date == date).unwrap().value;
        assert_eq!(v("2026-10-09"), 30 * MIN);
        assert_eq!(v("2026-10-10"), 35 * MIN);
        let s = kr_summary(&c, &f.kr, None).unwrap();
        assert_eq!(h.total, s.time_ms);
        assert_eq!(h.active_days, 2);
    }

    #[test]
    fn 完了数の対象は紐付けたタスクのまま() {
        let env = Env::new();
        let c = env.ctx();
        let f = setup(&c, None, None);
        let (o1, _, _) = setup_skill(&c, &f);
        task::done(&c, &o1).unwrap(); // スキルは付いているが紐付けていない
        task::done(&c, &f.t1).unwrap();
        let s = kr_summary(&c, &f.kr, None).unwrap();
        assert_eq!((s.done_count, s.task_count), (1, 2));
    }

    #[test]
    fn 存在しないスキル名は何も変えずに失敗し_削除でも付与が消える() {
        let env = Env::new();
        let c = env.ctx();
        let f = setup(&c, None, None);
        setup_skill(&c, &f);
        let bad = okr_svc::edit_kr(
            &c,
            &f.kr,
            &KrEdit {
                title: Some("変えない".into()),
                skills: vec!["設計".into(), "存在しない".into()],
                ..Default::default()
            },
        );
        assert!(bad.is_err());
        let s = kr_summary(&c, &f.kr, None).unwrap();
        assert_eq!(s.kr_skills, vec!["Rust".to_string()]);
        assert_eq!(s.key_result.title, "K");
        // スキルを削除すると kr_skill も消える。
        skill::delete(&c, "Rust", skill::ChildPolicy::Reparent, DeleteMode::FORCE).unwrap();
        assert!(kr_summary(&c, &f.kr, None).unwrap().kr_skills.is_empty());
        let count = |c: &Ctx| -> i64 {
            c.conn
                .query_row("SELECT COUNT(*) FROM kr_skill", [], |r| r.get(0))
                .unwrap()
        };
        assert_eq!(count(&c), 0);
        // KR を消しても kr_skill が残らない。
        okr_svc::edit_kr(
            &c,
            &f.kr,
            &KrEdit {
                skills: vec!["Rust 非同期".into()],
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(count(&c), 1);
        okr_svc::delete_kr(&c, &f.kr, DeleteMode::FORCE).unwrap();
        assert_eq!(count(&c), 0);
    }

    #[test]
    fn kr_の作成時にスキルを付けられ_不明なスキルなら作らない() {
        let env = Env::new();
        let c = env.ctx();
        let f = setup(&c, None, None);
        skill::add(
            &c,
            &SkillInput {
                name: "Rust".into(),
                parent: None,
            },
        )
        .unwrap();
        let o = okr_svc::list(&c).unwrap().objectives[0].objective.id.clone();
        let input = |skills: Vec<String>| KrInput {
            objective: o.clone(),
            title: "K2".into(),
            target: None,
            skills,
        };
        assert!(okr_svc::add_kr(&c, &input(vec!["存在しない".into()])).is_err());
        assert_eq!(okr_svc::list(&c).unwrap().objectives[0].key_results.len(), 1);
        let k = okr_svc::add_kr(&c, &input(vec!["Rust".into()])).unwrap();
        assert_eq!(
            kr_summary(&c, &k.id, None).unwrap().kr_skills,
            vec!["Rust".to_string()]
        );
        let _ = f;
    }

    fn idle_ids(env: &Env) -> Vec<String> {
        let tz = env.cfg.tz().unwrap();
        idle_krs_this_week(&env.conn, tz, env.ctx().now())
            .unwrap()
            .into_iter()
            .map(|k| k.id)
            .collect()
    }

    #[test]
    fn 今週時間が入っていない_kr_だけを拾う() {
        let env = Env::new();
        let c = env.ctx();
        let f = setup(&c, None, None);
        let d = env.day_start(); // 2026-10-10（土）00:00 JST。今週は 10/5（月）から
        assert_eq!(idle_ids(&env), vec![f.kr.clone()]);
        // 先週の作業は今週に数えない。
        seg(
            &c,
            "OLD",
            d - 6 * DAY,
            d - 6 * DAY + 30 * MIN,
            SegmentKind::Work,
            Some(&f.t1),
        );
        assert_eq!(idle_ids(&env), vec![f.kr.clone()]);
        // 休憩は時間に数えない。
        seg(
            &c,
            "BRK",
            d + 60 * MIN,
            d + 90 * MIN,
            SegmentKind::Break,
            Some(&f.t1),
        );
        assert_eq!(idle_ids(&env), vec![f.kr.clone()]);
        // 今週の作業があれば対象外。
        seg(
            &c,
            "NEW",
            d - 2 * DAY,
            d - 2 * DAY + 10 * MIN,
            SegmentKind::Work,
            Some(&f.t2),
        );
        assert!(idle_ids(&env).is_empty());
    }

    #[test]
    fn スキル経由の時間も今週の時間に数える() {
        let env = Env::new();
        let c = env.ctx();
        let f = setup(&c, None, None);
        let (skilled, _, _) = setup_skill(&c, &f);
        let d = env.day_start();
        // スキル付きのタスクも紐付けタスクもある KR は対象。時間が無い間は拾う。
        assert_eq!(idle_ids(&env), vec![f.kr.clone()]);
        seg(
            &c,
            "S",
            d + 60 * MIN,
            d + 90 * MIN,
            SegmentKind::Work,
            Some(&skilled),
        );
        assert!(idle_ids(&env).is_empty());
    }

    #[test]
    fn 対象外の_kr_は拾わない() {
        let env = Env::new();
        let c = env.ctx();
        let now = c.now();
        // 期間が終わった Objective。
        let past = setup(&c, Some(now - 60 * DAY), Some(now - DAY));
        // まだ始まっていない Objective。
        let future = setup(&c, Some(now + DAY), Some(now + 30 * DAY));
        assert!(idle_ids(&env).is_empty());
        // 期間内なら拾う（期間が無い Objective も同様）。
        let current = setup(&c, Some(now - DAY), Some(now + 30 * DAY));
        assert_eq!(idle_ids(&env), vec![current.kr.clone()]);
        // done の軸・完了した KR・やめた KR は対象外。
        for (measure, status) in [
            (Some("done"), None),
            (None, Some("done")),
            (None, Some("dropped")),
        ] {
            let f = setup(&c, None, None);
            okr_svc::edit_kr(
                &c,
                &f.kr,
                &KrEdit {
                    measure: measure.map(String::from),
                    status: status.map(String::from),
                    ..Default::default()
                },
            )
            .unwrap();
        }
        assert_eq!(idle_ids(&env), vec![current.kr.clone()]);
        // スキルも紐付けタスクも無い KR は対象外。
        let o = okr_svc::list(&c).unwrap().objectives[0].objective.id.clone();
        okr_svc::add_kr(
            &c,
            &KrInput {
                objective: o,
                title: "空".into(),
                target: None,
                skills: vec![],
            },
        )
        .unwrap();
        assert_eq!(idle_ids(&env), vec![current.kr.clone()]);
        let _ = (past, future);
    }
}
