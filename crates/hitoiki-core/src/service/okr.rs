//! OKR（docs/dev/architecture.md「データモデル」）。

use super::impact::{DeleteMode, DeleteOutcome, Impact};
use super::Ctx;
use crate::id::new_id;
use crate::model::{KeyResult, Objective};
use crate::repo::{edit, okr};
use crate::{Error, Millis, Result};
use serde::Serialize;

#[derive(Debug, Clone)]
pub struct ObjectiveInput {
    pub title: String,
    pub from: Option<Millis>,
    pub to: Option<Millis>,
}

#[derive(Debug, Clone)]
pub struct KrInput {
    /// Objective の ID（前方一致）。
    pub objective: String,
    pub title: String,
    pub target: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ObjectiveView {
    #[serde(flatten)]
    pub objective: Objective,
    pub key_results: Vec<KeyResult>,
}

#[derive(Debug, Clone, Serialize)]
pub struct OkrList {
    pub objectives: Vec<ObjectiveView>,
}

pub fn add_objective(ctx: &Ctx, input: &ObjectiveInput) -> Result<Objective> {
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
    let o = Objective {
        id: new_id(),
        title: input.title.trim().to_string(),
        description_md: None,
        period_start: input.from,
        period_end: input.to,
        status: "active".into(),
    };
    okr::insert_objective(ctx.conn, &o)?;
    Ok(o)
}

pub fn add_kr(ctx: &Ctx, input: &KrInput) -> Result<KeyResult> {
    if input.title.trim().is_empty() {
        return Err(Error::InvalidArgument("タイトルが空です".into()));
    }
    let objective_id = okr::resolve_objective(ctx.conn, &input.objective)?;
    let k = KeyResult {
        id: new_id(),
        objective_id,
        title: input.title.trim().to_string(),
        target: input.target.clone(),
        current: None,
        status: "active".into(),
        measure: DEFAULT_MEASURE.into(),
        day_min: DEFAULT_DAY_MIN,
    };
    okr::insert_kr(ctx.conn, &k)?;
    Ok(k)
}

/// KR の軸。`time`（時間）/ `days`（取り組んだ日の回数）/ `done`（完了数）。
pub const MEASURES: [&str; 3] = ["time", "days", "done"];
pub const DEFAULT_MEASURE: &str = "time";
/// 回数の軸で「取り組んだ日」とみなす 1 日の最小分数の既定。
pub const DEFAULT_DAY_MIN: i64 = 15;

fn check_measure(s: &str) -> Result<()> {
    if MEASURES.contains(&s) {
        Ok(())
    } else {
        Err(Error::InvalidArgument(format!(
            "軸は {} のいずれかにしてください",
            MEASURES.join(" / ")
        )))
    }
}

/// ID（前方一致）、なければタイトルの完全一致で KR を探す。
pub fn find_kr(ctx: &Ctx, key: &str) -> Result<KeyResult> {
    match okr::resolve_kr(ctx.conn, key) {
        Err(Error::NotFound(_)) => {}
        other => return other,
    }
    let hits: Vec<_> = okr::list_krs(ctx.conn)?
        .into_iter()
        .filter(|k| k.title == key)
        .collect();
    match hits.len() {
        0 => Err(Error::NotFound(format!("KR {key}"))),
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
    /// 付け替え前の KR（無かった・同じ KR のままなら null）。
    pub previous_kr: Option<KrBrief>,
}

#[derive(Debug, Clone, Serialize)]
pub struct KrBrief {
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

/// タスクを KR に紐付ける。別の KR に紐付いていたものは付け替える。
/// 先にすべてのタスクを解決し、1 つでも見つからなければ何も変えない。
pub fn link_tasks(ctx: &Ctx, kr: &str, tasks: &[String]) -> Result<LinkResult> {
    let k = find_kr(ctx, kr)?;
    let found = resolve_tasks(ctx, tasks)?;
    let now = ctx.now();
    let tx = ctx.conn.unchecked_transaction()?;
    let mut linked = Vec::new();
    for t in found {
        let previous_kr = match t.key_result_id.as_deref() {
            Some(prev) if prev != k.id => okr::get_kr(ctx.conn, prev).ok().map(|p| KrBrief {
                id: p.id,
                title: p.title,
            }),
            _ => None,
        };
        crate::repo::task::set_key_result(ctx.conn, &t.id, Some(&k.id), now)?;
        linked.push(LinkedTask {
            id: t.id,
            title: t.title,
            previous_kr,
        });
    }
    tx.commit()?;
    Ok(LinkResult {
        key_result: k,
        linked,
    })
}

/// タスクの KR との紐付けを外す（KR に紐付いていないタスクは何もしない）。外したタスクを返す。
pub fn unlink_tasks(ctx: &Ctx, tasks: &[String]) -> Result<Vec<LinkedTask>> {
    let found = resolve_tasks(ctx, tasks)?;
    let now = ctx.now();
    let tx = ctx.conn.unchecked_transaction()?;
    let mut out = Vec::new();
    for t in found {
        let Some(prev) = t.key_result_id.as_deref() else {
            continue;
        };
        let previous_kr = okr::get_kr(ctx.conn, prev).ok().map(|p| KrBrief {
            id: p.id,
            title: p.title,
        });
        crate::repo::task::set_key_result(ctx.conn, &t.id, None, now)?;
        out.push(LinkedTask {
            id: t.id,
            title: t.title,
            previous_kr,
        });
    }
    tx.commit()?;
    Ok(out)
}

pub use super::okr_summary::{kr_summary, KrSummary};

pub fn list(ctx: &Ctx) -> Result<OkrList> {
    let krs = okr::list_krs(ctx.conn)?;
    let objectives = okr::list_objectives(ctx.conn)?
        .into_iter()
        .map(|o| {
            let key_results = krs.iter().filter(|k| k.objective_id == o.id).cloned().collect();
            ObjectiveView {
                objective: o,
                key_results,
            }
        })
        .collect();
    Ok(OkrList { objectives })
}

// ===== 編集・削除 =====

/// Objective / KR の状態。
pub const STATUSES: [&str; 3] = ["active", "done", "dropped"];

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

/// Objective の編集。`None` は「変更しない」。
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(default)]
pub struct ObjectiveEdit {
    pub title: Option<String>,
    /// 空文字で説明を消す。
    pub description: Option<String>,
    pub from: Option<Millis>,
    pub to: Option<Millis>,
    /// 期間（開始・終了）を先に消してから `from` / `to` を適用する。
    pub clear_period: bool,
    pub status: Option<String>,
}

/// KR の編集。`None` は「変更しない」（`target` / `current` は空文字で消す）。
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(default)]
pub struct KrEdit {
    pub title: Option<String>,
    pub target: Option<String>,
    pub current: Option<String>,
    /// 現在値を変えたときに履歴（`kr_progress`）へ残すメモ。
    pub current_note: Option<String>,
    pub status: Option<String>,
    /// 付け替え先の Objective（ID の前方一致）。
    pub objective: Option<String>,
    /// 軸（`time` / `days` / `done`）。
    pub measure: Option<String>,
    /// 回数の軸で「取り組んだ日」とみなす 1 日の最小分数（1 以上）。
    pub day_min: Option<i64>,
}

fn blank_to_none(s: &str) -> Option<String> {
    let t = s.trim();
    (!t.is_empty()).then(|| t.to_string())
}

pub fn edit_objective(ctx: &Ctx, id: &str, input: &ObjectiveEdit) -> Result<Objective> {
    let mut o = edit::get_objective(ctx.conn, &okr::resolve_objective(ctx.conn, id)?)?;
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
    crate::repo::edit::update_objective(ctx.conn, &o)?;
    Ok(o)
}

pub fn edit_kr(ctx: &Ctx, id: &str, input: &KrEdit) -> Result<KeyResult> {
    let mut k = okr::resolve_kr(ctx.conn, id)?;
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
    if let Some(o) = &input.objective {
        k.objective_id = okr::resolve_objective(ctx.conn, o)?;
    }
    if let Some(m) = &input.measure {
        check_measure(m)?;
        k.measure = m.clone();
    }
    if let Some(d) = input.day_min {
        if d < 1 {
            return Err(Error::InvalidArgument(
                "「取り組んだ日」の最小分数は 1 以上にしてください".into(),
            ));
        }
        k.day_min = d;
    }
    let tx = ctx.conn.unchecked_transaction()?;
    crate::repo::edit::update_kr(ctx.conn, &k)?;
    // 現在値が変わったときは履歴を 1 行残す。
    if k.current != old_current {
        super::okr_progress::insert_entry(ctx, &k.id, k.current.as_deref(), input.current_note.as_deref())?;
    }
    tx.commit()?;
    Ok(k)
}

/// Objective の削除の影響。配下の KR は一緒に消え、KR に紐付いたタスクは残して参照だけ外す。
pub fn objective_impact(ctx: &Ctx, id: &str) -> Result<Impact> {
    let oid = okr::resolve_objective(ctx.conn, id)?;
    let o = edit::get_objective(ctx.conn, &oid)?;
    Ok(Impact::new("objective", &oid, &o.title)
        .line("配下の KR（一緒に削除）", edit::count_krs_of(ctx.conn, &oid)?)
        .line(
            "KR に紐付いたタスク（タスクは残り、KR との紐付けだけ外れる）",
            edit::count_tasks_of_objective(ctx.conn, &oid)?,
        ))
}

pub fn delete_objective(ctx: &Ctx, id: &str, mode: DeleteMode) -> Result<DeleteOutcome> {
    let impact = objective_impact(ctx, id)?;
    let go = DeleteOutcome::gate(mode, &impact)?;
    if go {
        let tx = ctx.conn.unchecked_transaction()?;
        edit::delete_objective(ctx.conn, &impact.id)?;
        tx.commit()?;
    }
    Ok(DeleteOutcome::new(mode, impact, go))
}

/// KR の削除の影響。紐付いたタスクは残して参照だけ外す。
pub fn kr_impact(ctx: &Ctx, id: &str) -> Result<Impact> {
    let k = okr::resolve_kr(ctx.conn, id)?;
    Ok(Impact::new("key_result", &k.id, &k.title).line(
        "紐付いたタスク（タスクは残り、KR との紐付けだけ外れる）",
        edit::count_tasks_of_kr(ctx.conn, &k.id)?,
    ))
}

pub fn delete_kr(ctx: &Ctx, id: &str, mode: DeleteMode) -> Result<DeleteOutcome> {
    let impact = kr_impact(ctx, id)?;
    let go = DeleteOutcome::gate(mode, &impact)?;
    if go {
        let tx = ctx.conn.unchecked_transaction()?;
        edit::delete_kr(ctx.conn, &impact.id)?;
        tx.commit()?;
    }
    Ok(DeleteOutcome::new(mode, impact, go))
}

#[cfg(test)]
mod edit_tests {
    use super::*;
    use crate::service::task::{self, TaskInput};
    use crate::service::testutil::Env;

    fn setup(ctx: &Ctx) -> (Objective, KeyResult, String) {
        let o = add_objective(
            ctx,
            &ObjectiveInput {
                title: "O".into(),
                from: Some(1),
                to: Some(100),
            },
        )
        .unwrap();
        let k = add_kr(
            ctx,
            &KrInput {
                objective: o.id.clone(),
                title: "K".into(),
                target: Some("10".into()),
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
    fn objective_を編集できる() {
        let env = Env::new();
        let ctx = env.ctx();
        let (o, _, _) = setup(&ctx);
        let e = edit_objective(
            &ctx,
            &o.id[..12],
            &ObjectiveEdit {
                title: Some(" O2 ".into()),
                description: Some("説明".into()),
                status: Some("done".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            (e.title.as_str(), e.description_md.as_deref(), e.status.as_str()),
            ("O2", Some("説明"), "done")
        );
        assert_eq!((e.period_start, e.period_end), (Some(1), Some(100)));
        let e = edit_objective(
            &ctx,
            &o.id,
            &ObjectiveEdit {
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
        let bad = |edit: ObjectiveEdit| edit_objective(&ctx, &o.id, &edit).is_err();
        assert!(bad(ObjectiveEdit {
            title: Some(" ".into()),
            ..Default::default()
        }));
        assert!(bad(ObjectiveEdit {
            from: Some(10),
            to: Some(5),
            ..Default::default()
        }));
        assert!(bad(ObjectiveEdit {
            status: Some("x".into()),
            ..Default::default()
        }));
        assert_eq!(list(&ctx).unwrap().objectives[0].objective.title, "O2");
    }

    #[test]
    fn kr_を編集して付け替えられる() {
        let env = Env::new();
        let ctx = env.ctx();
        let (o, k, _) = setup(&ctx);
        let o2 = add_objective(
            &ctx,
            &ObjectiveInput {
                title: "O2".into(),
                from: None,
                to: None,
            },
        )
        .unwrap();
        let e = edit_kr(
            &ctx,
            &k.id,
            &KrEdit {
                title: Some("K2".into()),
                target: Some("".into()),
                current: Some("3".into()),
                status: Some("dropped".into()),
                objective: Some(o2.id.clone()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(e.title, "K2");
        assert_eq!((e.target, e.current.as_deref()), (None, Some("3")));
        assert_eq!(e.objective_id, o2.id);
        assert_eq!(count_after_move(&ctx, &o.id, &o2.id), (0, 1));
        assert!(edit_kr(
            &ctx,
            &k.id,
            &KrEdit {
                objective: Some("ZZZ".into()),
                ..Default::default()
            }
        )
        .is_err());
    }

    fn count_after_move(ctx: &Ctx, a: &str, b: &str) -> (i64, i64) {
        (
            edit::count_krs_of(ctx.conn, a).unwrap(),
            edit::count_krs_of(ctx.conn, b).unwrap(),
        )
    }

    #[test]
    fn kr_の削除はタスクの参照を外して残す() {
        let env = Env::new();
        let ctx = env.ctx();
        let (_, k, tid) = setup(&ctx);
        let im = kr_impact(&ctx, &k.id).unwrap();
        assert_eq!(im.total(), 1);
        // force なしは失敗し、何も変わらない
        assert!(delete_kr(&ctx, &k.id, DeleteMode::default()).is_err());
        let d = delete_kr(&ctx, &k.id, DeleteMode::DRY_RUN).unwrap();
        assert!(d.dry_run && !d.deleted);
        assert_eq!(list(&ctx).unwrap().objectives[0].key_results.len(), 1);
        let d = delete_kr(&ctx, &k.id, DeleteMode::FORCE).unwrap();
        assert!(d.deleted);
        assert!(list(&ctx).unwrap().objectives[0].key_results.is_empty());
        let t = task::show(&ctx, &tid).unwrap();
        assert!(t.task.task.key_result_id.is_none());
        assert!(t.task.key_result.is_none());
        assert!(delete_kr(&ctx, &k.id, DeleteMode::FORCE).is_err());
    }

    #[test]
    fn objective_の削除は配下の_kr_も消してタスクを残す() {
        let env = Env::new();
        let ctx = env.ctx();
        let (o, _, tid) = setup(&ctx);
        let im = objective_impact(&ctx, &o.id).unwrap();
        assert_eq!(im.lines.len(), 2);
        assert_eq!(im.total(), 2);
        assert!(delete_objective(&ctx, &o.id, DeleteMode::default()).is_err());
        delete_objective(&ctx, &o.id, DeleteMode::FORCE).unwrap();
        assert!(list(&ctx).unwrap().objectives.is_empty());
        assert!(task::show(&ctx, &tid).unwrap().task.task.key_result_id.is_none());
        // 影響のない Objective は force なしで消える
        let o2 = add_objective(
            &ctx,
            &ObjectiveInput {
                title: "空".into(),
                from: None,
                to: None,
            },
        )
        .unwrap();
        assert!(
            delete_objective(&ctx, &o2.id, DeleteMode::default())
                .unwrap()
                .deleted
        );
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
        let o = add_objective(
            &ctx,
            &ObjectiveInput {
                title: "O".into(),
                from: Some(1),
                to: Some(2),
            },
        )
        .unwrap();
        add_kr(
            &ctx,
            &KrInput {
                objective: o.id[..12].to_string(),
                title: "K".into(),
                target: Some("80%".into()),
            },
        )
        .unwrap();
        let l = list(&ctx).unwrap();
        assert_eq!(l.objectives.len(), 1);
        assert_eq!(l.objectives[0].key_results[0].target.as_deref(), Some("80%"));
        assert!(add_kr(
            &ctx,
            &KrInput {
                objective: "ZZZ".into(),
                title: "K".into(),
                target: None
            }
        )
        .is_err());
        assert!(add_objective(
            &ctx,
            &ObjectiveInput {
                title: "x".into(),
                from: Some(5),
                to: Some(1)
            }
        )
        .is_err());
    }
}
