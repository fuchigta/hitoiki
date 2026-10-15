//! 目標（docs/dev/architecture.md「データモデル」）。

use super::impact::{DeleteMode, DeleteOutcome, Impact};
use super::Ctx;
use crate::id::new_id;
use crate::model::{Goal, KeyResult};
use crate::repo::{edit, goal};
use crate::{Error, Millis, Result};
use serde::Serialize;

#[derive(Debug, Clone)]
pub struct GoalInput {
    pub title: String,
    pub from: Option<Millis>,
    pub to: Option<Millis>,
}

#[derive(Debug, Clone)]
pub struct KeyResultInput {
    /// 目標の ID（前方一致）。
    pub goal: String,
    pub title: String,
    pub target: Option<String>,
    /// 成果の指標のスキル（既存のスキル名）。成果の指標の時間はこのスキルが付いたタスク・活動の時間が主になる。
    pub skills: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GoalView {
    #[serde(flatten)]
    pub goal: Goal,
    pub key_results: Vec<KeyResult>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GoalList {
    pub goals: Vec<GoalView>,
}

pub fn add_goal(ctx: &Ctx, input: &GoalInput) -> Result<Goal> {
    if input.title.trim().is_empty() {
        return Err(Error::InvalidArgument("タイトルが空です".into()));
    }
    if let (Some(f), Some(t)) = (input.from, input.to) {
        if t <= f {
            return Err(Error::InvalidArgument(
                "期間の終了は開始より後にしてください".into(),
            ));
        }
    }
    let o = Goal {
        id: new_id(),
        title: input.title.trim().to_string(),
        description_md: None,
        period_start: input.from,
        period_end: input.to,
        status: "active".into(),
    };
    goal::insert_goal(ctx.conn, &o)?;
    Ok(o)
}

pub fn add_key_result(ctx: &Ctx, input: &KeyResultInput) -> Result<KeyResult> {
    if input.title.trim().is_empty() {
        return Err(Error::InvalidArgument("タイトルが空です".into()));
    }
    let goal_id = goal::resolve_goal(ctx.conn, &input.goal)?;
    let sids = skill_ids(ctx, &input.skills)?;
    let k = KeyResult {
        id: new_id(),
        goal_id,
        title: input.title.trim().to_string(),
        target: input.target.clone(),
        current: None,
        status: "active".into(),
        measure: DEFAULT_MEASURE.into(),
        active_day_min: DEFAULT_DAY_MIN,
    };
    let tx = ctx.conn.unchecked_transaction()?;
    goal::insert_key_result(ctx.conn, &k)?;
    crate::repo::skill::set_key_result_skills(ctx.conn, &k.id, &sids)?;
    tx.commit()?;
    Ok(k)
}

/// スキル名を ID にする。存在しない名前があれば何も変えずに失敗する。
fn skill_ids(ctx: &Ctx, names: &[String]) -> Result<Vec<String>> {
    names
        .iter()
        .map(|n| crate::repo::skill::get_by_name(ctx.conn, n).map(|s| s.id))
        .collect()
}

/// 成果の指標の数え方。`time`（時間）/ `active_days`（取り組んだ日数）/ `done_tasks`（完了したタスク数）。
pub const MEASURES: [&str; 3] = ["time", "active_days", "done_tasks"];
pub const DEFAULT_MEASURE: &str = "time";
/// 数え方が `active_days` のとき「取り組んだ日」とみなす 1 日の最小分数の既定。
pub const DEFAULT_DAY_MIN: i64 = 15;

fn check_measure(s: &str) -> Result<()> {
    if MEASURES.contains(&s) {
        Ok(())
    } else {
        Err(Error::InvalidArgument(format!(
            "数え方は {} のいずれかにしてください",
            MEASURES.join(" / ")
        )))
    }
}

/// ID（前方一致）、なければタイトルの完全一致で成果の指標を探す。
pub fn find_key_result(ctx: &Ctx, key: &str) -> Result<KeyResult> {
    match goal::resolve_key_result(ctx.conn, key) {
        Err(Error::NotFound(_)) => {}
        other => return other,
    }
    let hits: Vec<_> = goal::list_key_results(ctx.conn)?
        .into_iter()
        .filter(|k| k.title == key)
        .collect();
    match hits.len() {
        0 => Err(Error::NotFound(format!("成果の指標 {key}"))),
        1 => Ok(hits.into_iter().next().expect("one")),
        _ => Err(Error::Ambiguous(format!(
            "{key}: {}",
            hits.iter().map(|k| k.id.as_str()).collect::<Vec<_>>().join(", ")
        ))),
    }
}

/// 紐付けた（付け替えた）タスク 1 件。
#[derive(Debug, Clone, Serialize)]
pub struct LinkedTask {
    pub id: String,
    pub title: String,
    /// 付け替え前の成果の指標（無かった・同じ成果の指標のままなら null）。
    pub previous_key_result: Option<KeyResultBrief>,
}

#[derive(Debug, Clone, Serialize)]
pub struct KeyResultBrief {
    pub id: String,
    pub title: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct LinkResult {
    pub key_result: KeyResult,
    pub linked: Vec<LinkedTask>,
}

fn resolve_tasks(ctx: &Ctx, tasks: &[String]) -> Result<Vec<crate::model::Task>> {
    if tasks.is_empty() {
        return Err(Error::InvalidArgument("タスクを 1 つ以上指定してください".into()));
    }
    let mut out: Vec<crate::model::Task> = Vec::new();
    for key in tasks {
        let t = crate::repo::task::resolve(ctx.conn, key)?;
        if !out.iter().any(|o| o.id == t.id) {
            out.push(t);
        }
    }
    Ok(out)
}

/// タスクを成果の指標に紐付ける。別の成果の指標に紐付いていたものは付け替える。
/// 先にすべてのタスクを解決し、1 つでも見つからなければ何も変えない。
pub fn link_tasks(ctx: &Ctx, key_result: &str, tasks: &[String]) -> Result<LinkResult> {
    let k = find_key_result(ctx, key_result)?;
    let found = resolve_tasks(ctx, tasks)?;
    let now = ctx.now();
    let tx = ctx.conn.unchecked_transaction()?;
    let mut linked = Vec::new();
    for t in found {
        let previous_key_result = match t.key_result_id.as_deref() {
            Some(prev) if prev != k.id => goal::get_key_result(ctx.conn, prev).ok().map(|p| KeyResultBrief {
                id: p.id,
                title: p.title,
            }),
            _ => None,
        };
        crate::repo::task::set_key_result(ctx.conn, &t.id, Some(&k.id), now)?;
        linked.push(LinkedTask {
            id: t.id,
            title: t.title,
            previous_key_result,
        });
    }
    tx.commit()?;
    Ok(LinkResult {
        key_result: k,
        linked,
    })
}

/// タスクの成果の指標との紐付けを外す（成果の指標に紐付いていないタスクは何もしない）。外したタスクを返す。
pub fn unlink_tasks(ctx: &Ctx, tasks: &[String]) -> Result<Vec<LinkedTask>> {
    let found = resolve_tasks(ctx, tasks)?;
    let now = ctx.now();
    let tx = ctx.conn.unchecked_transaction()?;
    let mut out = Vec::new();
    for t in found {
        let Some(prev) = t.key_result_id.as_deref() else {
            continue;
        };
        let previous_key_result = goal::get_key_result(ctx.conn, prev).ok().map(|p| KeyResultBrief {
            id: p.id,
            title: p.title,
        });
        crate::repo::task::set_key_result(ctx.conn, &t.id, None, now)?;
        out.push(LinkedTask {
            id: t.id,
            title: t.title,
            previous_key_result,
        });
    }
    tx.commit()?;
    Ok(out)
}

pub use super::key_result_summary::{idle_key_results_this_week, key_result_summary, KeyResultSummary};

pub fn list(ctx: &Ctx) -> Result<GoalList> {
    let key_results = goal::list_key_results(ctx.conn)?;
    let goals = goal::list_goals(ctx.conn)?
        .into_iter()
        .map(|o| {
            let key_results = key_results
                .iter()
                .filter(|k| k.goal_id == o.id)
                .cloned()
                .collect();
            GoalView { goal: o, key_results }
        })
        .collect();
    Ok(GoalList { goals })
}

/// 目標 1 件と、その下の成果の指標。ID（前方一致）で選ぶ。
pub fn show(ctx: &Ctx, key: &str) -> Result<GoalView> {
    let id = goal::resolve_goal(ctx.conn, key)?;
    list(ctx)?
        .goals
        .into_iter()
        .find(|g| g.goal.id == id)
        .ok_or_else(|| Error::NotFound(format!("目標が見つかりません: {key}")))
}

// ===== 編集・削除 =====

/// 目標 / 成果の指標の状態。
pub const STATUSES: [&str; 3] = ["active", "achieved", "dropped"];

fn check_status(s: &str) -> Result<()> {
    if STATUSES.contains(&s) {
        Ok(())
    } else {
        Err(Error::InvalidArgument(format!(
            "状態は {} のいずれかにしてください",
            STATUSES.join(" / ")
        )))
    }
}

/// 目標の編集。`None` は「変更しない」。
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(default)]
pub struct GoalEdit {
    pub title: Option<String>,
    /// 空文字で説明を消す。
    pub description: Option<String>,
    pub from: Option<Millis>,
    pub to: Option<Millis>,
    /// 期間（開始・終了）を先に消してから `from` / `to` を適用する。
    pub clear_period: bool,
    pub status: Option<String>,
}

/// 成果の指標の編集。`None` は「変更しない」（`target` / `current` は空文字で消す）。
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(default)]
pub struct KeyResultEdit {
    pub title: Option<String>,
    pub target: Option<String>,
    pub current: Option<String>,
    /// 現在値を変えたときに履歴（`key_result_progress`）へ残すメモ。
    pub current_note: Option<String>,
    pub status: Option<String>,
    /// 付け替え先の目標（ID の前方一致）。
    pub goal: Option<String>,
    /// 数え方（`time` / `active_days` / `done_tasks`）。
    pub measure: Option<String>,
    /// 数え方が `active_days` のとき「取り組んだ日」とみなす 1 日の最小分数（1 以上）。
    pub active_day_min: Option<i64>,
    /// 成果の指標のスキル（名前）。空でなければ今のスキルをこれで置き換える。
    pub skills: Vec<String>,
    /// 成果の指標のスキルを全て外す（`skills` が空でも置き換える）。
    pub clear_skills: bool,
}

fn blank_to_none(s: &str) -> Option<String> {
    let t = s.trim();
    (!t.is_empty()).then(|| t.to_string())
}

pub fn edit_goal(ctx: &Ctx, id: &str, input: &GoalEdit) -> Result<Goal> {
    let mut o = edit::get_goal(ctx.conn, &goal::resolve_goal(ctx.conn, id)?)?;
    if let Some(t) = &input.title {
        if t.trim().is_empty() {
            return Err(Error::InvalidArgument("タイトルが空です".into()));
        }
        o.title = t.trim().to_string();
    }
    if let Some(d) = &input.description {
        o.description_md = blank_to_none(d);
    }
    if input.clear_period {
        o.period_start = None;
        o.period_end = None;
    }
    if input.from.is_some() {
        o.period_start = input.from;
    }
    if input.to.is_some() {
        o.period_end = input.to;
    }
    if let (Some(f), Some(t)) = (o.period_start, o.period_end) {
        if t <= f {
            return Err(Error::InvalidArgument(
                "期間の終了は開始より後にしてください".into(),
            ));
        }
    }
    if let Some(s) = &input.status {
        check_status(s)?;
        o.status = s.clone();
    }
    crate::repo::edit::update_goal(ctx.conn, &o)?;
    Ok(o)
}

pub fn edit_key_result(ctx: &Ctx, id: &str, input: &KeyResultEdit) -> Result<KeyResult> {
    let mut k = goal::resolve_key_result(ctx.conn, id)?;
    let old_current = k.current.clone();
    if let Some(t) = &input.title {
        if t.trim().is_empty() {
            return Err(Error::InvalidArgument("タイトルが空です".into()));
        }
        k.title = t.trim().to_string();
    }
    if let Some(t) = &input.target {
        k.target = blank_to_none(t);
    }
    if let Some(c) = &input.current {
        k.current = blank_to_none(c);
    }
    if let Some(s) = &input.status {
        check_status(s)?;
        k.status = s.clone();
    }
    if let Some(o) = &input.goal {
        k.goal_id = goal::resolve_goal(ctx.conn, o)?;
    }
    if let Some(m) = &input.measure {
        check_measure(m)?;
        k.measure = m.clone();
    }
    if let Some(d) = input.active_day_min {
        if d < 1 {
            return Err(Error::InvalidArgument(
                "「取り組んだ日」の最小分数は 1 以上にしてください".into(),
            ));
        }
        k.active_day_min = d;
    }
    // 存在しないスキル名は何も変えずに失敗させる。
    let sids = skill_ids(ctx, &input.skills)?;
    let tx = ctx.conn.unchecked_transaction()?;
    crate::repo::edit::update_key_result(ctx.conn, &k)?;
    if !sids.is_empty() || input.clear_skills {
        crate::repo::skill::set_key_result_skills(ctx.conn, &k.id, &sids)?;
    }
    // 現在値が変わったときは履歴を 1 行残す。
    if k.current != old_current {
        super::key_result_progress::insert_entry(
            ctx,
            &k.id,
            k.current.as_deref(),
            input.current_note.as_deref(),
        )?;
    }
    tx.commit()?;
    Ok(k)
}

/// 目標の削除の影響。配下の成果の指標は一緒に消え、成果の指標に紐付いたタスクは残して参照だけ外す。
pub fn goal_impact(ctx: &Ctx, id: &str) -> Result<Impact> {
    let oid = goal::resolve_goal(ctx.conn, id)?;
    let o = edit::get_goal(ctx.conn, &oid)?;
    Ok(Impact::new("goal", &oid, &o.title)
        .line(
            "配下の成果の指標（一緒に削除）",
            edit::count_key_results_of(ctx.conn, &oid)?,
        )
        .line(
            "成果の指標に紐付いたタスク（タスクは残り、成果の指標との紐付けだけ外れる）",
            edit::count_tasks_of_goal(ctx.conn, &oid)?,
        ))
}

pub fn delete_goal(ctx: &Ctx, id: &str, mode: DeleteMode) -> Result<DeleteOutcome> {
    let impact = goal_impact(ctx, id)?;
    let go = DeleteOutcome::gate(mode, &impact)?;
    if go {
        let tx = ctx.conn.unchecked_transaction()?;
        edit::delete_goal(ctx.conn, &impact.id)?;
        tx.commit()?;
    }
    Ok(DeleteOutcome::new(mode, impact, go))
}

/// 成果の指標の削除の影響。紐付いたタスクは残して参照だけ外す。
pub fn key_result_impact(ctx: &Ctx, id: &str) -> Result<Impact> {
    let k = goal::resolve_key_result(ctx.conn, id)?;
    Ok(Impact::new("key_result", &k.id, &k.title).line(
        "紐付いたタスク（タスクは残り、成果の指標との紐付けだけ外れる）",
        edit::count_tasks_of_key_result(ctx.conn, &k.id)?,
    ))
}

pub fn delete_key_result(ctx: &Ctx, id: &str, mode: DeleteMode) -> Result<DeleteOutcome> {
    let impact = key_result_impact(ctx, id)?;
    let go = DeleteOutcome::gate(mode, &impact)?;
    if go {
        let tx = ctx.conn.unchecked_transaction()?;
        edit::delete_key_result(ctx.conn, &impact.id)?;
        tx.commit()?;
    }
    Ok(DeleteOutcome::new(mode, impact, go))
}

#[cfg(test)]
mod edit_tests {
    use super::*;
    use crate::service::task::{self, TaskInput};
    use crate::service::testutil::Env;

    fn setup(ctx: &Ctx) -> (Goal, KeyResult, String) {
        let o = add_goal(
            ctx,
            &GoalInput {
                title: "O".into(),
                from: Some(1),
                to: Some(100),
            },
        )
        .unwrap();
        let k = add_key_result(
            ctx,
            &KeyResultInput {
                goal: o.id.clone(),
                title: "K".into(),
                target: Some("10".into()),
                skills: vec![],
            },
        )
        .unwrap();
        let t = task::add(
            ctx,
            &TaskInput {
                title: Some("T".into()),
                key_result: Some(k.id.clone()),
                ..Default::default()
            },
        )
        .unwrap();
        (o, k, t.task.id)
    }

    #[test]
    fn goal_を編集できる() {
        let env = Env::new();
        let ctx = env.ctx();
        let (o, _, _) = setup(&ctx);
        let e = edit_goal(
            &ctx,
            &o.id[..12],
            &GoalEdit {
                title: Some(" O2 ".into()),
                description: Some("説明".into()),
                status: Some("achieved".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            (e.title.as_str(), e.description_md.as_deref(), e.status.as_str()),
            ("O2", Some("説明"), "achieved")
        );
        assert_eq!((e.period_start, e.period_end), (Some(1), Some(100)));
        let e = edit_goal(
            &ctx,
            &o.id,
            &GoalEdit {
                clear_period: true,
                description: Some("".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            (e.period_start, e.period_end, e.description_md),
            (None, None, None)
        );
        // 不正な入力
        let bad = |edit: GoalEdit| edit_goal(&ctx, &o.id, &edit).is_err();
        assert!(bad(GoalEdit {
            title: Some(" ".into()),
            ..Default::default()
        }));
        assert!(bad(GoalEdit {
            from: Some(10),
            to: Some(5),
            ..Default::default()
        }));
        assert!(bad(GoalEdit {
            status: Some("x".into()),
            ..Default::default()
        }));
        assert_eq!(list(&ctx).unwrap().goals[0].goal.title, "O2");
    }

    #[test]
    fn key_result_を編集して付け替えられる() {
        let env = Env::new();
        let ctx = env.ctx();
        let (o, k, _) = setup(&ctx);
        let o2 = add_goal(
            &ctx,
            &GoalInput {
                title: "O2".into(),
                from: None,
                to: None,
            },
        )
        .unwrap();
        let e = edit_key_result(
            &ctx,
            &k.id,
            &KeyResultEdit {
                title: Some("K2".into()),
                target: Some("".into()),
                current: Some("3".into()),
                status: Some("dropped".into()),
                goal: Some(o2.id.clone()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(e.title, "K2");
        assert_eq!((e.target, e.current.as_deref()), (None, Some("3")));
        assert_eq!(e.goal_id, o2.id);
        assert_eq!(count_after_move(&ctx, &o.id, &o2.id), (0, 1));
        assert!(edit_key_result(
            &ctx,
            &k.id,
            &KeyResultEdit {
                goal: Some("ZZZ".into()),
                ..Default::default()
            }
        )
        .is_err());
    }

    fn count_after_move(ctx: &Ctx, a: &str, b: &str) -> (i64, i64) {
        (
            edit::count_key_results_of(ctx.conn, a).unwrap(),
            edit::count_key_results_of(ctx.conn, b).unwrap(),
        )
    }

    #[test]
    fn key_result_の削除はタスクの参照を外して残す() {
        let env = Env::new();
        let ctx = env.ctx();
        let (_, k, tid) = setup(&ctx);
        let im = key_result_impact(&ctx, &k.id).unwrap();
        assert_eq!(im.total(), 1);
        // force なしは失敗し、何も変わらない
        assert!(delete_key_result(&ctx, &k.id, DeleteMode::default()).is_err());
        let d = delete_key_result(&ctx, &k.id, DeleteMode::DRY_RUN).unwrap();
        assert!(d.dry_run && !d.deleted);
        assert_eq!(list(&ctx).unwrap().goals[0].key_results.len(), 1);
        let d = delete_key_result(&ctx, &k.id, DeleteMode::FORCE).unwrap();
        assert!(d.deleted);
        assert!(list(&ctx).unwrap().goals[0].key_results.is_empty());
        let t = task::show(&ctx, &tid).unwrap();
        assert!(t.task.task.key_result_id.is_none());
        assert!(t.task.key_result.is_none());
        assert!(delete_key_result(&ctx, &k.id, DeleteMode::FORCE).is_err());
    }

    #[test]
    fn goal_の削除は配下の_key_result_も消してタスクを残す() {
        let env = Env::new();
        let ctx = env.ctx();
        let (o, _, tid) = setup(&ctx);
        let im = goal_impact(&ctx, &o.id).unwrap();
        assert_eq!(im.lines.len(), 2);
        assert_eq!(im.total(), 2);
        assert!(delete_goal(&ctx, &o.id, DeleteMode::default()).is_err());
        delete_goal(&ctx, &o.id, DeleteMode::FORCE).unwrap();
        assert!(list(&ctx).unwrap().goals.is_empty());
        assert!(task::show(&ctx, &tid).unwrap().task.task.key_result_id.is_none());
        // 影響のない目標は force なしで消える
        let o2 = add_goal(
            &ctx,
            &GoalInput {
                title: "空".into(),
                from: None,
                to: None,
            },
        )
        .unwrap();
        assert!(delete_goal(&ctx, &o2.id, DeleteMode::default()).unwrap().deleted);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::testutil::Env;

    #[test]
    fn add_and_list() {
        let env = Env::new();
        let ctx = env.ctx();
        let o = add_goal(
            &ctx,
            &GoalInput {
                title: "O".into(),
                from: Some(1),
                to: Some(2),
            },
        )
        .unwrap();
        add_key_result(
            &ctx,
            &KeyResultInput {
                goal: o.id[..12].to_string(),
                title: "K".into(),
                target: Some("80%".into()),
                skills: vec![],
            },
        )
        .unwrap();
        let l = list(&ctx).unwrap();
        assert_eq!(l.goals.len(), 1);
        assert_eq!(l.goals[0].key_results[0].target.as_deref(), Some("80%"));
        assert!(add_key_result(
            &ctx,
            &KeyResultInput {
                goal: "ZZZ".into(),
                title: "K".into(),
                target: None,
                skills: vec![],
            }
        )
        .is_err());
        assert!(add_goal(
            &ctx,
            &GoalInput {
                title: "x".into(),
                from: Some(5),
                to: Some(1)
            }
        )
        .is_err());
    }
}
