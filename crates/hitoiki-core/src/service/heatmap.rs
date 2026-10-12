//! 活動量ヒートマップ（日別の集計と濃淡の段階）。
//!
//! 時間の指標は `activity_segment`（`segment_override` 適用後の派生層）から、記録の指標は
//! `checkin` / `hassle` / `abc_record` / `reflection` の `at` から、ローカル日付ごとに集計する。
//! 日付の区切りは設定のタイムゾーンの 0 時。週は月曜始まり。
//!
//! 濃淡の段階（0〜4）: 値が 0 の日は 0。値のある日だけを小さい順に並べ、
//! 「その値以下の日」が全体に占める割合 p を 4 等分する（段階 = ceil(4p)）。同じ値は同じ段階。
//! 段階は相対的な濃淡であり、多い少ないの評価ではない。

use super::skill::descendants;
use super::Ctx;
use crate::model::SegmentKind;
use crate::repo::{self, segment::kind_label};
use crate::timefmt::{self, Range};
use crate::{Error, Result};
use chrono::{Datelike, Duration, NaiveDate};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};

/// 既定の表示週数（今日を含む週が最後の列）。
pub const DEFAULT_WEEKS: i64 = 53;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct HeatmapInput {
    /// `time`（既定）/ `kind:<種別>` / `task:<ID・タイトル>` / `skill:<名前・ID>` / `records[:checkin|hassle|abc|reflection]`。
    pub by: Option<String>,
    /// 開始日（`YYYY-MM-DD` / `today` / `yesterday`）。既定は 53 週前の週の月曜。
    pub from: Option<String>,
    /// 終了日（含む）。既定は今日。
    pub to: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct HeatmapDay {
    /// ローカル日付 `YYYY-MM-DD`。
    pub date: String,
    /// `unit` が `ms` ならミリ秒、`count` なら件数。
    pub value: i64,
    /// 濃淡の段階（0〜4）。
    pub level: u8,
}

#[derive(Debug, Clone, Serialize)]
pub struct Heatmap {
    pub from: String,
    pub to: String,
    /// `from` の曜日（月曜 = 0 … 日曜 = 6）。
    pub from_weekday: u8,
    /// 常に `monday`。
    pub week_start: &'static str,
    /// 正規化した指標（例 `kind:work`）。
    pub by: String,
    pub label: String,
    /// `ms` または `count`。
    pub unit: &'static str,
    pub days: Vec<HeatmapDay>,
    pub total: i64,
    /// 値のあった日数。
    pub active_days: usize,
    pub max: i64,
    /// 段階 1〜3 の上限（q1, q2, q3）。値のある日が無ければ空。
    pub thresholds: Vec<i64>,
}

enum Metric {
    Time,
    Kind(SegmentKind),
    Task { id: String },
    Skill { ids: HashSet<String> },
    Records(Option<&'static str>),
}

/// (指定名, テーブル, 表示名)
const RECORD_KINDS: [(&str, &str, &str); 4] = [
    ("checkin", "checkin", "チェックイン"),
    ("hassle", "hassle", "ハッスル"),
    ("abc", "abc_record", "ABC"),
    ("reflection", "reflection", "振り返り"),
];

/// ID（前方一致）、なければタイトルの完全一致でタスクを探す。
fn find_task(ctx: &Ctx, key: &str) -> Result<crate::model::Task> {
    match repo::task::resolve(ctx.conn, key) {
        Err(Error::NotFound(_)) => {}
        other => return other,
    }
    let hits: Vec<_> = repo::task::list(ctx.conn, &[], None)?
        .into_iter()
        .filter(|t| t.title == key)
        .collect();
    match hits.len() {
        0 => Err(Error::NotFound(format!("タスク {key}"))),
        1 => Ok(hits.into_iter().next().expect("one")),
        _ => Err(Error::Ambiguous(format!(
            "{key}: {}",
            hits.iter().map(|t| t.id.as_str()).collect::<Vec<_>>().join(", ")
        ))),
    }
}

fn parse_metric(ctx: &Ctx, by: &str) -> Result<(Metric, String, String, &'static str)> {
    let (head, arg) = match by.split_once(':') {
        Some((h, a)) => (h, Some(a)),
        None => (by, None),
    };
    let bad = || {
        Error::InvalidArgument(format!(
            "--by の値が不正です: {by} (候補: time, kind:<種別>, task:<タスク>, skill:<スキル>, records[:checkin|hassle|abc|reflection])"
        ))
    };
    Ok(match (head, arg) {
        ("time", None) => (Metric::Time, "time".into(), "作業の時間".into(), "ms"),
        ("kind", Some(k)) => {
            let kind: SegmentKind = k
                .parse()
                .map_err(|_| Error::InvalidArgument(format!("区間の種別が不正です: {k}")))?;
            (
                Metric::Kind(kind),
                format!("kind:{kind}"),
                format!("種別: {}", kind_label(kind)),
                "ms",
            )
        }
        ("task", Some(k)) if !k.is_empty() => {
            let t = find_task(ctx, k)?;
            (
                Metric::Task { id: t.id.clone() },
                format!("task:{}", t.id),
                format!("タスク: {}", t.title),
                "ms",
            )
        }
        ("skill", Some(k)) if !k.is_empty() => {
            // 子スキルの時間も含める（階層の親は子を包むため）。複数該当しても 1 回だけ数える。
            let s = super::skill::find(ctx, k)?;
            let all = repo::skill::list(ctx.conn)?;
            let mut ids: HashSet<String> = descendants(&all, &s.id).into_iter().collect();
            ids.insert(s.id.clone());
            (
                Metric::Skill { ids },
                format!("skill:{}", s.name),
                format!("スキル: {}（子スキルを含む）", s.name),
                "ms",
            )
        }
        ("records", None) => (
            Metric::Records(None),
            "records".into(),
            "記録の数".into(),
            "count",
        ),
        ("records", Some(k)) => {
            let (name, _, label) = RECORD_KINDS.iter().find(|(n, _, _)| *n == k).ok_or_else(bad)?;
            (
                Metric::Records(Some(name)),
                format!("records:{name}"),
                format!("記録の数: {label}"),
                "count",
            )
        }
        _ => return Err(bad()),
    })
}

/// 値から段階（0〜4）を決める。`sorted` は値のある日の値（昇順）。
/// 値以下の日が全体に占める割合 p を 4 等分する: 段階 = ceil(4 * p)。同じ値は同じ段階になる。
pub fn level_of(value: i64, sorted: &[i64]) -> u8 {
    if value <= 0 || sorted.is_empty() {
        return 0;
    }
    let le = sorted.partition_point(|v| *v <= value);
    ((4 * le).div_ceil(sorted.len())).clamp(1, 4) as u8
}

/// 段階 1〜3 に入る値の上限（それぞれの段階で最大の値。その段階に値が無ければ手前の上限）。
fn thresholds(sorted: &[i64]) -> Vec<i64> {
    let mut out = Vec::new();
    let mut prev = 0;
    for lv in 1..=3u8 {
        let top = sorted
            .iter()
            .copied()
            .filter(|v| level_of(*v, sorted) <= lv)
            .max()
            .unwrap_or(prev);
        out.push(top);
        prev = top;
    }
    out
}

fn resolve_dates(ctx: &Ctx, tz: Tz, input: &HeatmapInput) -> Result<(NaiveDate, NaiveDate)> {
    let now = ctx.now();
    let today = timefmt::local_date_of(tz, now);
    let to = match &input.to {
        Some(t) => timefmt::parse_date(tz, t, now)?,
        None => today,
    };
    let from = match &input.from {
        Some(f) => timefmt::parse_date(tz, f, now)?,
        None => {
            let monday = to - Duration::days(to.weekday().num_days_from_monday() as i64);
            monday - Duration::weeks(DEFAULT_WEEKS - 1)
        }
    };
    if to < from {
        return Err(Error::InvalidArgument(
            "範囲の終了は開始以降の日付にしてください".into(),
        ));
    }
    Ok((from, to))
}

pub fn heatmap(ctx: &Ctx, input: &HeatmapInput) -> Result<Heatmap> {
    let tz = ctx.tz()?;
    let (from, to) = resolve_dates(ctx, tz, input)?;
    let (metric, by, label, unit) = parse_metric(ctx, input.by.as_deref().unwrap_or("time"))?;
    let range = Range::new(timefmt::day_range(tz, from).start, timefmt::day_range(tz, to).end);

    let mut per_day: BTreeMap<NaiveDate, i64> = BTreeMap::new();
    match &metric {
        Metric::Records(kind) => {
            for (name, table, _) in RECORD_KINDS {
                if kind.is_some_and(|k| k != name) {
                    continue;
                }
                let mut stmt = ctx
                    .conn
                    .prepare(&format!("SELECT at FROM {table} WHERE at >= ?1 AND at < ?2"))?;
                let ats: Vec<i64> = stmt
                    .query_map([range.start, range.end], |r| r.get(0))?
                    .collect::<rusqlite::Result<_>>()?;
                for at in ats {
                    *per_day.entry(timefmt::local_date_of(tz, at)).or_default() += 1;
                }
            }
        }
        _ => {
            let task_ids: Option<HashSet<String>> = match &metric {
                Metric::Skill { ids } => {
                    let mut stmt = ctx.conn.prepare("SELECT task_id, skill_id FROM task_skill")?;
                    let rows: Vec<(String, String)> = stmt
                        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                        .collect::<rusqlite::Result<_>>()?;
                    Some(
                        rows.into_iter()
                            .filter(|(_, s)| ids.contains(s))
                            .map(|(t, _)| t)
                            .collect(),
                    )
                }
                _ => None,
            };
            for s in repo::segment::list(ctx.conn, range)? {
                let counted = match &metric {
                    Metric::Kind(k) => s.kind == *k,
                    // 時間・タスク・スキルは「今日の合計」と同じ数え方（無操作・休憩・無視を除く）。
                    _ => !matches!(
                        s.kind,
                        SegmentKind::Idle | SegmentKind::Break | SegmentKind::Ignored
                    ),
                };
                if !counted {
                    continue;
                }
                let matches_target = match &metric {
                    Metric::Task { id } => s.task_id.as_deref() == Some(id.as_str()),
                    Metric::Skill { .. } => s
                        .task_id
                        .as_ref()
                        .zip(task_ids.as_ref())
                        .is_some_and(|(t, set)| set.contains(t)),
                    _ => true,
                };
                if !matches_target {
                    continue;
                }
                // 日付の境界で分割して加算する。
                let Some(clipped) = Range::new(s.start_at, s.end_at).intersect(&range) else {
                    continue;
                };
                let mut d = timefmt::local_date_of(tz, clipped.start);
                loop {
                    let dr = timefmt::day_range(tz, d);
                    if dr.start >= clipped.end {
                        break;
                    }
                    if let Some(part) = dr.intersect(&clipped) {
                        *per_day.entry(d).or_default() += part.len();
                    }
                    d += Duration::days(1);
                }
            }
        }
    }

    let mut values: Vec<i64> = per_day.values().copied().filter(|v| *v > 0).collect();
    values.sort_unstable();
    let mut days = Vec::new();
    let mut d = from;
    while d <= to {
        let value = per_day.get(&d).copied().unwrap_or(0);
        days.push(HeatmapDay {
            date: d.format("%Y-%m-%d").to_string(),
            value,
            level: level_of(value, &values),
        });
        d += Duration::days(1);
    }
    Ok(Heatmap {
        from: from.format("%Y-%m-%d").to_string(),
        to: to.format("%Y-%m-%d").to_string(),
        from_weekday: from.weekday().num_days_from_monday() as u8,
        week_start: "monday",
        by,
        label,
        unit,
        total: values.iter().sum(),
        active_days: values.len(),
        max: values.last().copied().unwrap_or(0),
        thresholds: if values.is_empty() {
            vec![]
        } else {
            thresholds(&values)
        },
        days,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::SegmentStatus;
    use crate::service::skill::{self, SkillInput};
    use crate::service::task::{self, TaskInput};
    use crate::service::testutil::{insert_segment, Env};

    const MIN: i64 = 60_000;
    const DAY: i64 = 24 * 60 * MIN;

    fn input(by: &str) -> HeatmapInput {
        HeatmapInput {
            by: Some(by.into()),
            ..Default::default()
        }
    }

    fn day<'a>(h: &'a Heatmap, date: &str) -> &'a HeatmapDay {
        h.days.iter().find(|d| d.date == date).unwrap()
    }

    #[test]
    fn default_range_is_53_weeks_aligned_to_monday() {
        let env = Env::new();
        let h = heatmap(&env.ctx(), &HeatmapInput::default()).unwrap();
        // 2026-10-10 は土曜。今日までで、開始は 52 週前の月曜（2025-10-06）。
        assert_eq!(h.to, "2026-10-10");
        assert_eq!(h.from, "2025-10-06");
        assert_eq!(h.days.len(), 52 * 7 + 6);
        assert_eq!(h.total, 0);
        assert!(h.days.iter().all(|d| d.level == 0));
        assert!(h.thresholds.is_empty());
    }

    #[test]
    fn splits_at_local_midnight_and_excludes_idle() {
        let env = Env::new();
        let c = env.ctx();
        let d = env.day_start(); // 2026-10-10 00:00 JST
        let st = SegmentStatus::Auto;
        // 前日 23:00 から当日 01:00 までの作業は日付で 60 分ずつに分かれる。
        insert_segment(
            c.conn,
            "A",
            d - 60 * MIN,
            d + 60 * MIN,
            SegmentKind::Work,
            None,
            st,
        );
        insert_segment(
            c.conn,
            "B",
            d + 60 * MIN,
            d + 120 * MIN,
            SegmentKind::Idle,
            None,
            st,
        );
        insert_segment(
            c.conn,
            "C",
            d + 120 * MIN,
            d + 150 * MIN,
            SegmentKind::Ignored,
            None,
            st,
        );
        insert_segment(
            c.conn,
            "D",
            d + 150 * MIN,
            d + 160 * MIN,
            SegmentKind::Meeting,
            None,
            st,
        );
        let h = heatmap(&c, &input("time")).unwrap();
        assert_eq!(day(&h, "2026-10-09").value, 60 * MIN);
        assert_eq!(day(&h, "2026-10-10").value, 70 * MIN);
        assert_eq!(h.total, 130 * MIN);
        assert_eq!(h.active_days, 2);
        // 今日画面の合計と一致する。
        let s = crate::service::status::status(&c).unwrap();
        assert_eq!(day(&h, "2026-10-10").value, s.today.total_ms);
    }

    #[test]
    fn levels_by_quartile() {
        let v = [1, 2, 3, 4, 5, 6, 7, 8];
        assert_eq!(
            [0, 1, 2, 3, 4, 5, 6, 7, 8].map(|x| level_of(x, &v)),
            [0, 1, 1, 2, 2, 3, 3, 4, 4]
        );
        assert_eq!(thresholds(&v), [2, 4, 6]);
        // 2 件なら小さい方が 2、大きい方が 4。
        assert_eq!([60, 120].map(|x| level_of(x, &[60, 120])), [2, 4]);
        // 同じ値は同じ段階（ここでは最も濃い）。
        assert_eq!(level_of(5, &[5, 5, 5]), 4);
    }

    #[test]
    fn levels_assigned_in_output() {
        let env = Env::new();
        let c = env.ctx();
        let d = env.day_start();
        for i in 0..4 {
            let s = d - i * DAY;
            insert_segment(
                c.conn,
                &format!("S{i}"),
                s,
                s + (i + 1) * 10 * MIN,
                SegmentKind::Work,
                None,
                SegmentStatus::Auto,
            );
        }
        let h = heatmap(&c, &input("time")).unwrap();
        assert_eq!(h.thresholds.len(), 3);
        assert_eq!(day(&h, "2026-10-10").level, 1); // 10 分
        assert_eq!(day(&h, "2026-10-07").level, 4); // 40 分
        assert_eq!(day(&h, "2026-10-06").level, 0);
    }

    #[test]
    fn kind_task_skill_metrics() {
        let env = Env::new();
        let c = env.ctx();
        let d = env.day_start();
        let st = SegmentStatus::Confirmed;
        skill::add(
            &c,
            &SkillInput {
                name: "プログラミング".into(),
                parent: None,
            },
        )
        .unwrap();
        skill::add(
            &c,
            &SkillInput {
                name: "Rust".into(),
                parent: Some("プログラミング".into()),
            },
        )
        .unwrap();
        let a = task::add(
            &c,
            &TaskInput {
                title: Some("実装".into()),
                skills: vec!["Rust".into()],
                ..Default::default()
            },
        )
        .unwrap()
        .task;
        let b = task::add(
            &c,
            &TaskInput {
                title: Some("雑務".into()),
                ..Default::default()
            },
        )
        .unwrap()
        .task;
        insert_segment(c.conn, "S1", d, d + 60 * MIN, SegmentKind::Work, Some(&a.id), st);
        insert_segment(
            c.conn,
            "S2",
            d + 60 * MIN,
            d + 90 * MIN,
            SegmentKind::AgentDialog,
            Some(&a.id),
            st,
        );
        insert_segment(
            c.conn,
            "S3",
            d + 90 * MIN,
            d + 100 * MIN,
            SegmentKind::Work,
            Some(&b.id),
            st,
        );
        insert_segment(
            c.conn,
            "S4",
            d + 100 * MIN,
            d + 110 * MIN,
            SegmentKind::Idle,
            Some(&a.id),
            st,
        );

        let k = heatmap(&c, &input("kind:agent_dialog")).unwrap();
        assert_eq!(k.total, 30 * MIN);
        let t = heatmap(&c, &input("task:実装")).unwrap();
        assert_eq!(t.total, 90 * MIN);
        let t2 = heatmap(&c, &input(&format!("task:{}", &a.id[..12]))).unwrap();
        assert_eq!(t2.total, 90 * MIN);
        // 親スキルは子スキルのタスクの時間を含む。
        let s = heatmap(&c, &input("skill:プログラミング")).unwrap();
        assert_eq!(s.total, 90 * MIN);
        let s = heatmap(&c, &input("skill:Rust")).unwrap();
        assert_eq!(s.total, 90 * MIN);

        assert!(matches!(
            heatmap(&c, &input("task:存在しない")),
            Err(Error::NotFound(_))
        ));
        assert!(matches!(
            heatmap(&c, &input("skill:無い")),
            Err(Error::NotFound(_))
        ));
        assert!(matches!(
            heatmap(&c, &input("kind:bogus")),
            Err(Error::InvalidArgument(_))
        ));
        assert!(matches!(
            heatmap(&c, &input("records:x")),
            Err(Error::InvalidArgument(_))
        ));
        assert!(matches!(
            heatmap(&c, &input("bogus")),
            Err(Error::InvalidArgument(_))
        ));
    }

    #[test]
    fn follows_derived_layer_after_reassignment() {
        // 派生層（activity_segment）の値をそのまま使うので、付け替え後のタスクで数えられる。
        let env = Env::new();
        let c = env.ctx();
        let d = env.day_start();
        let t = task::add(
            &c,
            &TaskInput {
                title: Some("X".into()),
                ..Default::default()
            },
        )
        .unwrap()
        .task;
        insert_segment(
            c.conn,
            "S1",
            d,
            d + 30 * MIN,
            SegmentKind::Work,
            None,
            SegmentStatus::Auto,
        );
        assert_eq!(heatmap(&c, &input(&format!("task:{}", t.id))).unwrap().total, 0);
        c.conn
            .execute(
                "UPDATE activity_segment SET task_id = ?1, status='confirmed' WHERE id='S1'",
                [&t.id],
            )
            .unwrap();
        assert_eq!(
            heatmap(&c, &input(&format!("task:{}", t.id))).unwrap().total,
            30 * MIN
        );
    }

    #[test]
    fn records_count_per_local_day() {
        let env = Env::new();
        let c = env.ctx();
        let d = env.day_start();
        // 当日 00:30 JST（UTC では前日）のチェックインは、ローカル日付の当日に数える。
        c.conn
            .execute("INSERT INTO checkin (id, at) VALUES ('C1', ?1)", [d + 30 * MIN])
            .unwrap();
        c.conn
            .execute("INSERT INTO checkin (id, at) VALUES ('C2', ?1)", [d - 30 * MIN])
            .unwrap();
        c.conn
            .execute(
                "INSERT INTO hassle (id, at, text) VALUES ('H1', ?1, 'x')",
                [d + 60 * MIN],
            )
            .unwrap();
        c.conn
            .execute(
                "INSERT INTO reflection (id, at, kind, context_json, questions_md) VALUES ('R1', ?1, 'day_end', '{}', '')",
                [d + 90 * MIN],
            )
            .unwrap();
        let all = heatmap(&c, &input("records")).unwrap();
        assert_eq!(all.unit, "count");
        assert_eq!(day(&all, "2026-10-10").value, 3);
        assert_eq!(day(&all, "2026-10-09").value, 1);
        let ck = heatmap(&c, &input("records:checkin")).unwrap();
        assert_eq!(ck.total, 2);
        assert_eq!(day(&ck, "2026-10-10").value, 1);
        let rf = heatmap(&c, &input("records:reflection")).unwrap();
        assert_eq!(rf.total, 1);
    }

    #[test]
    fn explicit_range_and_errors() {
        let env = Env::new();
        let h = heatmap(
            &env.ctx(),
            &HeatmapInput {
                from: Some("2026-10-01".into()),
                to: Some("2026-10-03".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(h.days.len(), 3);
        let e = heatmap(
            &env.ctx(),
            &HeatmapInput {
                from: Some("2026-10-03".into()),
                to: Some("2026-10-01".into()),
                ..Default::default()
            },
        );
        assert!(matches!(e, Err(Error::InvalidArgument(_))));
    }
}
