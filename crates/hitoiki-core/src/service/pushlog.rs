//! 通知（プッシュ）の一覧と、あとからの回答・見送り（docs/dev/architecture.md「プッシュ通知」）。
//!
//! 応答の保存は [`crate::push::store::apply_response_with`] を通す（トーストの応答と同じ処理）。
//! 記録（checkin / hassle）の `at` は通知の時刻になる。

use super::{Ctx, RangeArgs};
use crate::model::TaskStatus;
use crate::push::store::{self, PushResponse, Scales};
use crate::push::{payload, PushPayload, PushTrigger};
use crate::repo;
use crate::service::review::IdlePreset;
use crate::timefmt::{self, Range};
use crate::{Error, Millis, Result};
use serde::Serialize;

/// 通知の状態。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PushState {
    /// まだ答えていない。
    Unanswered,
    /// 答えた（スヌーズ・「開く」を含む）。
    Answered,
    /// 「答えない」で見送った。
    Dismissed,
}

#[derive(Debug, Clone, Serialize)]
pub struct PushView {
    pub id: String,
    pub at: Millis,
    pub at_local: String,
    /// トリガー ID（`day_start` など）。
    pub trigger: String,
    /// トリガーの日本語表示名。
    pub trigger_label: String,
    pub title: String,
    pub body: String,
    pub state: PushState,
    /// 通知の中身（ボタン・入力欄・対象期間を含む）。読み取れなければ None。
    pub payload: Option<PushPayload>,
}

fn state_of(log: &crate::model::PushLog) -> PushState {
    if log.responded_at.is_some() {
        return PushState::Answered;
    }
    let dismissed = log
        .response_json
        .as_deref()
        .and_then(|j| serde_json::from_str::<PushResponse>(j).ok())
        .is_some_and(|r| r.action.as_deref() == Some("dismiss"));
    if dismissed {
        PushState::Dismissed
    } else {
        PushState::Unanswered
    }
}

fn view(tz: chrono_tz::Tz, log: crate::model::PushLog) -> PushView {
    let state = state_of(&log);
    let payload = serde_json::from_str::<PushPayload>(&log.payload_json).ok();
    let trigger_label = PushTrigger::label_of(&log.trigger);
    PushView {
        at_local: timefmt::fmt_rfc3339(tz, log.at),
        title: payload
            .as_ref()
            .map_or_else(|| trigger_label.clone(), |p| p.title.clone()),
        body: payload.as_ref().map_or_else(String::new, |p| p.body.clone()),
        id: log.id,
        at: log.at,
        trigger: log.trigger,
        trigger_label,
        state,
        payload,
    }
}

/// `push list` の出力。
#[derive(Debug, Clone, Serialize)]
pub struct PushList {
    pub range: super::RangeView,
    pub pushes: Vec<PushView>,
}

/// 範囲内で配信済みの通知（古い順）。`unanswered` なら未回答だけ。
pub fn list(ctx: &Ctx, range: &RangeArgs, unanswered: bool) -> Result<PushList> {
    let r = ctx.range(range)?;
    let tz = ctx.tz()?;
    let logs = repo::push::list_delivered(ctx.conn, r.start, r.end)?;
    Ok(PushList {
        range: super::range_view(tz, r),
        pushes: logs
            .into_iter()
            .map(|l| view(tz, l))
            .filter(|v| !unanswered || v.state == PushState::Unanswered)
            .collect(),
    })
}

/// 範囲内の未回答の通知（`hitoiki status` と今日画面用）。
pub fn list_unanswered(ctx: &Ctx, range: Range) -> Result<Vec<PushView>> {
    let tz = ctx.tz()?;
    Ok(repo::push::list_unanswered(ctx.conn, range.start, range.end)?
        .into_iter()
        .map(|l| view(tz, l))
        .collect())
}

/// `push answer` の入力。`dismiss` と無操作の分類（`idle`）は、ほかの値と同時に指定できない。
#[derive(Debug, Clone, Default)]
pub struct AnswerInput {
    pub energy: Option<i64>,
    pub mood: Option<i64>,
    pub stress: Option<i64>,
    pub hassle: Option<String>,
    pub idle: Option<IdlePreset>,
    /// タスク通知（urgent_task / important_task / block_start）と、idle_return の「{タスク名}をしていた」への答え。
    pub task: Option<TaskAnswer>,
    pub dismiss: bool,
}

/// タスク通知のボタンに当たる答え。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskAnswer {
    /// 対象のタスクを始める（`task_start`）。
    Start,
    /// あとで（`task_later`。記録だけ）。
    Later,
    /// 今週は見送る（`task_skip_week`。今週その通知の対象から外す）。
    SkipWeek,
    /// 今週は見送る（`kr_skip_week`。kr_idle。記録だけ）。
    KrSkipWeek,
    /// 無操作の期間を、通知が示したタスクの PC 外の作業として確定する（`idle_task`。idle_return）。
    IdleTask,
    /// 今回はやめる（`block_skip`。block_start。記録だけ）。
    BlockSkip,
}

impl TaskAnswer {
    pub fn action_id(&self) -> &'static str {
        match self {
            TaskAnswer::Start => payload::TASK_START,
            TaskAnswer::Later => payload::TASK_LATER,
            TaskAnswer::SkipWeek => payload::TASK_SKIP_WEEK,
            TaskAnswer::KrSkipWeek => payload::KR_SKIP_WEEK,
            TaskAnswer::IdleTask => payload::IDLE_TASK_ACTION,
            TaskAnswer::BlockSkip => payload::BLOCK_SKIP,
        }
    }
}

impl std::str::FromStr for TaskAnswer {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self> {
        match s {
            "start" => Ok(TaskAnswer::Start),
            "later" => Ok(TaskAnswer::Later),
            "skip_week" | "skip-week" => Ok(TaskAnswer::SkipWeek),
            "kr_skip_week" | "kr-skip-week" => Ok(TaskAnswer::KrSkipWeek),
            "idle_task" | "idle-task" => Ok(TaskAnswer::IdleTask),
            "block_skip" | "block-skip" | "skip" => Ok(TaskAnswer::BlockSkip),
            other => Err(Error::InvalidArgument(format!(
                "不明なタスクの答え: {other}（start / later / skip_week / kr_skip_week / idle_task / block_skip）"
            ))),
        }
    }
}

/// `task_start` の応答で始めるタスクを実際に始める（`service::task::start`）。
/// 完了・中止済みのタスクは始めずにエラーにする（通知を出したあとで状態が変わっていた場合）。
pub fn start_pushed_task(ctx: &Ctx, task_id: &str) -> Result<super::task::TaskChange> {
    let t = repo::task::get(ctx.conn, task_id)?;
    if !matches!(t.status, TaskStatus::Todo | TaskStatus::Doing) {
        return Err(Error::InvalidArgument(
            "このタスクはすでに完了または中止されています".into(),
        ));
    }
    super::task::start(ctx, task_id)
}

#[derive(Debug, Clone, Serialize)]
pub struct AnswerResult {
    pub id: String,
    pub state: PushState,
    pub checkin_id: Option<String>,
    pub hassle_id: Option<String>,
    /// 分類をつけた無操作区間の数。
    pub idle_labeled: usize,
    /// 始めたタスクの ID（`task_start` の答え）。
    pub started_task: Option<String>,
}

/// 通知に答える（または見送る）。回答済み・見送り済みの通知にも、あらためて答えられる。
pub fn answer(ctx: &Ctx, id: &str, input: &AnswerInput) -> Result<AnswerResult> {
    let id = repo::resolve_prefix(ctx.conn, "push_log", id)?;
    let log = store::get_log(ctx.conn, &id)?;
    if !log.delivered {
        return Err(Error::NotFound(format!("配信された通知 {id}")));
    }
    let has_text = input.hassle.as_deref().is_some_and(|t| !t.trim().is_empty());
    let has_scale = input.energy.is_some() || input.mood.is_some() || input.stress.is_some();
    if input.task.is_some() && (input.dismiss || has_text || has_scale || input.idle.is_some()) {
        return Err(Error::InvalidArgument(
            "--task はほかの回答と同時に指定できません".into(),
        ));
    }
    if input.dismiss && (has_text || has_scale || input.idle.is_some()) {
        return Err(Error::InvalidArgument(
            "--dismiss はほかの回答と同時に指定できません".into(),
        ));
    }
    if input.idle.is_some() && (has_text || has_scale) {
        return Err(Error::InvalidArgument(
            "--idle はほかの回答と同時に指定できません".into(),
        ));
    }
    if !input.dismiss && input.idle.is_none() && input.task.is_none() && !has_text && !has_scale {
        return Err(Error::InvalidArgument(
            "回答（--energy / --mood / --stress / --hassle / --idle / --task）か --dismiss を指定してください".into(),
        ));
    }
    let response = if let Some(t) = input.task {
        PushResponse {
            action: Some(t.action_id().into()),
            ..Default::default()
        }
    } else if input.dismiss {
        PushResponse {
            action: Some("dismiss".into()),
            ..Default::default()
        }
    } else if let Some(p) = input.idle {
        PushResponse {
            action: Some(format!(
                "{}{}",
                crate::push::payload::IDLE_ACTION_PREFIX,
                p.as_str()
            )),
            ..Default::default()
        }
    } else {
        PushResponse {
            text: input.hassle.clone(),
            ..Default::default()
        }
    };
    let scales = Scales {
        energy: input.energy,
        mood: input.mood,
        stress: input.stress,
    };
    let out = store::apply_response_with(ctx.conn, &id, response, scales, ctx.now(), ctx.tz()?)?;
    let started_task = match out.start_task {
        Some(task_id) => {
            start_pushed_task(ctx, &task_id)?;
            Some(task_id)
        }
        None => None,
    };
    let state = state_of(&store::get_log(ctx.conn, &id)?);
    Ok(AnswerResult {
        id,
        state,
        checkin_id: out.checkin_id,
        hassle_id: out.hassle_id,
        idle_labeled: out.idle_labeled,
        started_task,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::push::{payload, PushDecision};
    use crate::service::testutil::Env;

    fn deliver(env: &Env, p: PushPayload, at: Millis) -> String {
        store::record_decision(&env.conn, &PushDecision::Deliver(p), at)
            .unwrap()
            .unwrap()
    }

    #[test]
    fn 未回答の一覧にタイトルと日本語のトリガー名が出る() {
        let env = Env::new();
        let ctx = env.ctx();
        let d = env.day_start();
        let a = deliver(&env, payload::day_start(), d + 3_600_000);
        let b = deliver(
            &env,
            payload::idle_return(d + 1000, d + 20 * 60_000, None),
            d + 7_200_000,
        );
        let v = list(&ctx, &RangeArgs::default(), true).unwrap().pushes;
        assert_eq!(v.len(), 2);
        assert_eq!(
            (v[0].id.as_str(), v[0].trigger_label.as_str()),
            (a.as_str(), "始業")
        );
        assert_eq!(v[0].title, "おはようございます");
        assert_eq!(v[1].id, b);
        let p = v[1].payload.as_ref().unwrap();
        assert_eq!(p.idle_range, Some((d + 1000, d + 20 * 60_000)));
        assert_eq!(p.actions.len(), 4);
        let s = crate::service::status::status(&ctx).unwrap();
        assert_eq!(s.unanswered_pushes.len(), 2);
    }

    #[test]
    fn エネルギーの回答は通知の時刻でcheckinに保存される() {
        let env = Env::new();
        let ctx = env.ctx();
        let at = env.day_start() + 3_600_000;
        let id = deliver(&env, payload::day_start(), at);
        let r = answer(
            &ctx,
            &id[..12],
            &AnswerInput {
                energy: Some(4),
                mood: Some(3),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(r.state, PushState::Answered);
        let (e, m, a, p): (i64, i64, i64, String) = env
            .conn
            .query_row("SELECT energy, mood, at, push_id FROM checkin", [], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
            })
            .unwrap();
        assert_eq!((e, m, a, p), (4, 3, at, id));
        assert!(list(&ctx, &RangeArgs::default(), true).unwrap().pushes.is_empty());
        assert_eq!(
            list(&ctx, &RangeArgs::default(), false).unwrap().pushes[0].state,
            PushState::Answered
        );
    }

    #[test]
    fn ハッスルの回答はhassleに保存される() {
        let env = Env::new();
        let ctx = env.ctx();
        let at = env.day_start() + 3_600_000;
        let id = deliver(&env, payload::switch_spike(15, 21), at);
        let r = answer(
            &ctx,
            &id,
            &AnswerInput {
                stress: Some(4),
                hassle: Some("チャット".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(r.checkin_id.is_some() && r.hassle_id.is_some());
        let (t, i, a): (String, i64, i64) = env
            .conn
            .query_row("SELECT text, intensity, at FROM hassle", [], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .unwrap();
        assert_eq!((t.as_str(), i, a), ("チャット", 4, at));
    }

    #[test]
    fn 無操作の回答は期間にoverrideを作る() {
        let env = Env::new();
        let ctx = env.ctx();
        let d = env.day_start();
        let id = deliver(
            &env,
            payload::idle_return(d + 600_000, d + 1_800_000, None),
            d + 1_900_000,
        );
        let r = answer(
            &ctx,
            &id,
            &AnswerInput {
                idle: Some(IdlePreset::Meeting),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(r.state, PushState::Answered);
        let n: i64 = env
            .conn
            .query_row(
                "SELECT COUNT(*) FROM segment_override WHERE start_at = ?1 AND end_at = ?2",
                [d + 600_000, d + 1_800_000],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 1);
        // 無操作の期間を持たない通知には idle を付けられない。
        let other = deliver(&env, payload::day_start(), d + 2_000_000);
        let e = answer(
            &ctx,
            &other,
            &AnswerInput {
                idle: Some(IdlePreset::Break),
                ..Default::default()
            },
        )
        .unwrap_err();
        assert_eq!(e.exit_code(), 2);
    }

    #[test]
    fn 見送りは一覧から消え応答率では無応答のまま() {
        let env = Env::new();
        let ctx = env.ctx();
        let d = env.day_start();
        let id = deliver(&env, payload::day_start(), d + 3_600_000);
        let r = answer(
            &ctx,
            &id,
            &AnswerInput {
                dismiss: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(r.state, PushState::Dismissed);
        assert!(list(&ctx, &RangeArgs::default(), true).unwrap().pushes.is_empty());
        let all = list(&ctx, &RangeArgs::default(), false).unwrap().pushes;
        assert_eq!(all[0].state, PushState::Dismissed);
        assert!(crate::service::status::status(&ctx)
            .unwrap()
            .unanswered_pushes
            .is_empty());
        let log = store::get_log(&env.conn, &id).unwrap();
        assert!(log.responded_at.is_none(), "応答率の算出では無応答として数える");
        // 見送ったあとでも答えられる。
        let r = answer(
            &ctx,
            &id,
            &AnswerInput {
                energy: Some(2),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(r.state, PushState::Answered);
    }

    fn urgent_task_push(env: &Env, at: Millis) -> (String, String) {
        let t = crate::service::task::add(
            &env.ctx(),
            &crate::service::task::TaskInput {
                title: Some("請求書".into()),
                urgent: Some(true),
                ..Default::default()
            },
        )
        .unwrap();
        let id = deliver(env, payload::urgent_task(&t.task.id, &t.task.title, 0), at);
        (id, t.task.id)
    }

    #[test]
    fn タスク通知の始めるは対象のタスクをdoingにする() {
        let env = Env::new();
        let ctx = env.ctx();
        let (id, task_id) = urgent_task_push(&env, env.day_start() + 3_600_000);
        let r = answer(
            &ctx,
            &id,
            &AnswerInput {
                task: Some(TaskAnswer::Start),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(r.state, PushState::Answered);
        assert_eq!(r.started_task.as_deref(), Some(task_id.as_str()));
        let t = repo::task::get(&env.conn, &task_id).unwrap();
        assert_eq!(t.status, TaskStatus::Doing);
        assert_eq!(repo::task::list_doing(&env.conn).unwrap().len(), 1);
    }

    #[test]
    fn タスク通知のあとでと今週は見送るは記録だけ() {
        let env = Env::new();
        let ctx = env.ctx();
        for (a, name) in [
            (TaskAnswer::Later, "task_later"),
            (TaskAnswer::SkipWeek, "task_skip_week"),
        ] {
            let (id, task_id) = urgent_task_push(&env, env.day_start() + 3_600_000);
            let r = answer(
                &ctx,
                &id,
                &AnswerInput {
                    task: Some(a),
                    ..Default::default()
                },
            )
            .unwrap();
            assert!(r.started_task.is_none());
            assert_eq!(r.state, PushState::Answered);
            assert!(store::get_log(&env.conn, &id)
                .unwrap()
                .response_json
                .unwrap()
                .contains(name));
            assert_eq!(
                repo::task::get(&env.conn, &task_id).unwrap().status,
                TaskStatus::Todo
            );
        }
        // ほかの回答とは同時に指定できない。
        let (id, _) = urgent_task_push(&env, env.day_start() + 3_700_000);
        let e = answer(
            &ctx,
            &id,
            &AnswerInput {
                task: Some(TaskAnswer::Start),
                dismiss: true,
                ..Default::default()
            },
        )
        .unwrap_err();
        assert_eq!(e.exit_code(), 2);
        assert_eq!("skip-week".parse::<TaskAnswer>().unwrap(), TaskAnswer::SkipWeek);
        assert!("x".parse::<TaskAnswer>().is_err());
    }

    #[test]
    fn 完了済みのタスクは通知から始められない() {
        let env = Env::new();
        let ctx = env.ctx();
        let (id, task_id) = urgent_task_push(&env, env.day_start() + 3_600_000);
        crate::service::task::done(&ctx, &task_id).unwrap();
        let e = answer(
            &ctx,
            &id,
            &AnswerInput {
                task: Some(TaskAnswer::Start),
                ..Default::default()
            },
        )
        .unwrap_err();
        assert_eq!(e.exit_code(), 2);
        assert_eq!(
            repo::task::get(&env.conn, &task_id).unwrap().status,
            TaskStatus::Done
        );
    }

    #[test]
    fn 引数の検査() {
        let env = Env::new();
        let ctx = env.ctx();
        let id = deliver(&env, payload::day_start(), env.day_start() + 1000);
        let code = |i: AnswerInput| answer(&ctx, &id, &i).unwrap_err().exit_code();
        assert_eq!(code(AnswerInput::default()), 2);
        assert_eq!(
            code(AnswerInput {
                energy: Some(9),
                ..Default::default()
            }),
            2
        );
        assert_eq!(
            code(AnswerInput {
                dismiss: true,
                energy: Some(3),
                ..Default::default()
            }),
            2
        );
        let none = answer(
            &ctx,
            "ZZZZ",
            &AnswerInput {
                dismiss: true,
                ..Default::default()
            },
        );
        assert_eq!(none.unwrap_err().exit_code(), 3);
    }
}
