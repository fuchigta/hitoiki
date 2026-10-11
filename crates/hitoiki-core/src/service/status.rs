//! `hitoiki status`: 現在の状況の要約（docs/dev/architecture.md「CLI」）。

use super::report::{self, ReportBy, ReportInput, ReportRow};
use super::{range_view, segment_view, task_titles, Ctx, RangeArgs, RangeView, SegmentView};
use crate::model::SegmentStatus;
use crate::repo::{self, push};
use crate::service::task::{self as task_svc, TaskView};
use crate::{timefmt, Millis, Result};
use rusqlite::OptionalExtension;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct DoingView {
    pub task: TaskView,
    /// 現在の doing ログの開始時刻。
    pub since: Option<Millis>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TodaySummary {
    pub range: RangeView,
    /// idle / break を除く合計。
    pub total_ms: i64,
    pub idle_ms: i64,
    pub break_ms: i64,
    /// 無視した区間の合計（合計には含めない）。
    pub ignored_ms: i64,
    pub by_kind: Vec<ReportRow>,
    /// 時間の長い順の上位 5 件。
    pub top_tasks: Vec<ReportRow>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PushView {
    pub id: String,
    pub at: Millis,
    pub trigger: String,
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct Status {
    pub now: Millis,
    pub now_local: String,
    pub doing: Vec<DoingView>,
    /// 直近の区間（新しい順に最大 5 件）。
    pub recent_segments: Vec<SegmentView>,
    pub today: TodaySummary,
    /// 全期間の needs_review 件数のうち、`review.min_sec` 以上のもの（仕分け一覧の既定表示と同じ数え方）。
    pub needs_review_count: i64,
    /// 全期間の needs_review のうち、`review.min_sec` 未満で既定では隠れている件数。
    pub needs_review_short_count: i64,
    /// 当日の配信済みで未応答のプッシュ。
    pub unanswered_pushes: Vec<PushView>,
    /// 保存済みの確認結果で、現在より新しい版があればそのバージョン（`v` なし）。
    /// `general.update_check = true` のときだけ。ネットワークには出ない。
    pub update_available: Option<String>,
}

pub fn status(ctx: &Ctx) -> Result<Status> {
    let tz = ctx.tz()?;
    let now = ctx.now();

    let mut doing = Vec::new();
    for t in repo::task::list_doing(ctx.conn)? {
        let since = ctx
            .conn
            .query_row(
                "SELECT start_at FROM task_doing_log WHERE task_id = ?1 AND end_at IS NULL \
                 ORDER BY start_at DESC LIMIT 1",
                [&t.id],
                |r| r.get(0),
            )
            .optional()?;
        doing.push(DoingView {
            task: task_svc::view(ctx, t)?,
            since,
        });
    }

    let titles = task_titles(ctx.conn)?;
    let mut stmt = ctx.conn.prepare(
        "SELECT id FROM activity_segment WHERE start_at <= ?1 ORDER BY start_at DESC, id DESC LIMIT 5",
    )?;
    let ids: Vec<String> = stmt
        .query_map([now], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    let mut recent_segments = Vec::new();
    for id in ids {
        let s = repo::segment::get(ctx.conn, &id)?;
        recent_segments.push(segment_view(tz, &s, &titles));
    }

    let by_kind = report::report(
        ctx,
        &ReportInput {
            range: RangeArgs::default(),
            by: ReportBy::Kind,
            include_idle: false,
        },
    )?;
    let by_task = report::report(
        ctx,
        &ReportInput {
            range: RangeArgs::default(),
            by: ReportBy::Task,
            include_idle: false,
        },
    )?;
    let today_range = ctx.range(&RangeArgs::default())?;
    let today = TodaySummary {
        range: range_view(tz, today_range),
        total_ms: by_kind.total_ms,
        idle_ms: by_kind.excluded.idle_ms,
        break_ms: by_kind.excluded.break_ms,
        ignored_ms: by_kind.excluded.ignored_ms,
        by_kind: by_kind.rows,
        top_tasks: by_task.rows.into_iter().take(5).collect(),
    };

    let (needs_review_count, needs_review_short_count) = repo::segment::count_by_status_split(
        ctx.conn,
        SegmentStatus::NeedsReview,
        ctx.cfg.review.min_sec as i64 * 1000,
    )?;

    let unanswered_pushes = push::list_unanswered(ctx.conn, today_range.start, today_range.end)?
        .into_iter()
        .map(|p| PushView {
            payload: serde_json::from_str(&p.payload_json)
                .unwrap_or(serde_json::Value::String(p.payload_json)),
            id: p.id,
            at: p.at,
            trigger: p.trigger,
        })
        .collect();

    let update_available = if ctx.cfg.general.update_check == Some(true) {
        super::update::available(ctx.conn, env!("CARGO_PKG_VERSION"))?
    } else {
        None
    };

    Ok(Status {
        now,
        now_local: timefmt::fmt_rfc3339(tz, now),
        doing,
        recent_segments,
        today,
        needs_review_count,
        needs_review_short_count,
        unanswered_pushes,
        update_available,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::SegmentKind;
    use crate::service::task::{self, TaskInput};
    use crate::service::testutil::{insert_segment, Env};

    #[test]
    fn status_summary() {
        let env = Env::new();
        let ctx = env.ctx();
        let t = task::add(
            &ctx,
            &TaskInput {
                title: Some("A".into()),
                ..Default::default()
            },
        )
        .unwrap();
        task::start(&ctx, &t.task.id).unwrap();
        let d = env.day_start();
        let m = 60_000;
        insert_segment(
            ctx.conn,
            "S1",
            d,
            d + 30 * m,
            SegmentKind::Work,
            Some(&t.task.id),
            SegmentStatus::Confirmed,
        );
        insert_segment(
            ctx.conn,
            "S2",
            d + 30 * m,
            d + 40 * m,
            SegmentKind::Work,
            None,
            SegmentStatus::NeedsReview,
        );
        insert_segment(
            ctx.conn,
            "S3",
            d + 40 * m,
            d + 50 * m,
            SegmentKind::Idle,
            None,
            SegmentStatus::Auto,
        );
        // 配信済み未応答 / 応答済み / 前日
        let ins = |id: &str, at: i64, resp: Option<i64>| {
            ctx.conn
                .execute(
                    "INSERT INTO push_log (id, at, trigger, payload_json, delivered, responded_at) VALUES (?1,?2,'t','{}',1,?3)",
                    rusqlite::params![id, at, resp],
                )
                .unwrap();
        };
        ins("P1", d + m, None);
        ins("P2", d + m, Some(d + 2 * m));
        ins("P3", d - m, None);

        let s = status(&ctx).unwrap();
        assert_eq!(s.doing.len(), 1);
        assert!(s.doing[0].since.is_some());
        assert_eq!(s.recent_segments.len(), 3);
        assert_eq!(s.recent_segments[0].id, "S3");
        assert_eq!(s.today.total_ms, 40 * m);
        assert_eq!(s.today.idle_ms, 10 * m);
        assert_eq!(s.today.top_tasks[0].label, "A");
        assert_eq!(s.needs_review_count, 1);
        assert_eq!(s.needs_review_short_count, 0);
        assert_eq!(s.unanswered_pushes.len(), 1);
        assert_eq!(s.unanswered_pushes[0].id, "P1");
    }
}
