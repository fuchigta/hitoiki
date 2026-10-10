//! タスクとタスク帰属ルールのユースケース（12.2 task コマンド群）。

use super::Ctx;
use crate::id::new_id;
use crate::model::{MatchField, PatternType, Task, TaskKind, TaskMatchRule, TaskStatus};
use crate::repo::{self, okr, rule, skill};
use crate::{Error, Millis, Result};
use serde::{Deserialize, Serialize};

/// add / edit 共通の入力。edit では `None` / 空は「変更しない」。
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct TaskInput {
    pub title: Option<String>,
    pub body: Option<String>,
    pub urgent: Option<bool>,
    pub important: Option<bool>,
    pub estimate_min: Option<i64>,
    /// KR の ID（前方一致）。
    pub key_result: Option<String>,
    /// スキル名。edit では 1 つ以上指定されたとき置き換える。
    pub skills: Vec<String>,
    /// edit でスキルを全て外す（`skills` が空でも置き換える）。
    pub clear_skills: bool,
    pub external_url: Option<String>,
    pub planned_start_at: Option<Millis>,
    pub planned_end_at: Option<Millis>,
    /// 割り込みタスク（kind=interrupt かつ urgent=1、10 章）。
    pub interrupt: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct KeyResultRef {
    pub id: String,
    pub title: String,
    pub objective_id: String,
    pub objective_title: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TaskView {
    #[serde(flatten)]
    pub task: Task,
    pub skills: Vec<String>,
    pub key_result: Option<KeyResultRef>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TaskList {
    pub tasks: Vec<TaskView>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TaskShow {
    pub task: TaskView,
    pub rules: Vec<TaskMatchRule>,
    /// このタスクに帰属した区間の合計（全期間、ミリ秒）。
    pub tracked_ms: i64,
    pub tracked_segments: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct TaskChange {
    pub task: TaskView,
    /// 開始に伴い todo に戻した他のタスク（`task.single_doing`）。
    pub demoted: Vec<TaskView>,
}

#[derive(Debug, Clone, Default)]
pub struct TaskFilter {
    /// 空なら todo と doing。`all` 相当は全状態を列挙する。
    pub statuses: Vec<TaskStatus>,
    pub kind: Option<TaskKind>,
}

pub fn view(ctx: &Ctx, task: Task) -> Result<TaskView> {
    let skills = skill::task_skill_names(ctx.conn, &task.id)?;
    let key_result = match &task.key_result_id {
        Some(id) => {
            let kr = okr::get_kr(ctx.conn, id)?;
            let objective_title: String = ctx.conn.query_row(
                "SELECT title FROM objective WHERE id = ?1",
                [&kr.objective_id],
                |r| r.get(0),
            )?;
            Some(KeyResultRef {
                id: kr.id,
                title: kr.title,
                objective_id: kr.objective_id,
                objective_title,
            })
        }
        None => None,
    };
    Ok(TaskView {
        task,
        skills,
        key_result,
    })
}

fn skill_ids(ctx: &Ctx, names: &[String]) -> Result<Vec<String>> {
    names
        .iter()
        .map(|n| skill::get_by_name(ctx.conn, n).map(|s| s.id))
        .collect()
}

fn validate(t: &Task) -> Result<()> {
    if t.title.trim().is_empty() {
        return Err(Error::InvalidArgument("タイトルが空です".into()));
    }
    if let Some(e) = t.estimate_min {
        if e <= 0 {
            return Err(Error::InvalidArgument(
                "見積もりは正の値で指定してください".into(),
            ));
        }
    }
    if let (Some(s), Some(e)) = (t.planned_start_at, t.planned_end_at) {
        if e <= s {
            return Err(Error::InvalidArgument(
                "計画の終了は開始より後にしてください".into(),
            ));
        }
    }
    Ok(())
}

pub fn add(ctx: &Ctx, input: &TaskInput) -> Result<TaskView> {
    let title = input
        .title
        .clone()
        .ok_or_else(|| Error::InvalidArgument("タイトルが必要です".into()))?;
    let now = ctx.now();
    let kr_id = match &input.key_result {
        Some(p) => Some(okr::resolve_kr(ctx.conn, p)?.id),
        None => None,
    };
    let sids = skill_ids(ctx, &input.skills)?;
    let task = Task {
        id: new_id(),
        title: title.trim().to_string(),
        body_md: input.body.clone().unwrap_or_default(),
        status: TaskStatus::Todo,
        kind: if input.interrupt {
            TaskKind::Interrupt
        } else {
            TaskKind::Normal
        },
        urgent: if input.interrupt { Some(true) } else { input.urgent },
        important: input.important,
        estimate_min: input.estimate_min,
        planned_start_at: input.planned_start_at,
        planned_end_at: input.planned_end_at,
        key_result_id: kr_id,
        external_url: input.external_url.clone(),
        created_at: now,
        updated_at: now,
        done_at: None,
    };
    validate(&task)?;
    let tx = ctx.conn.unchecked_transaction()?;
    repo::task::insert(ctx.conn, &task)?;
    skill::set_task_skills(ctx.conn, &task.id, &sids)?;
    tx.commit()?;
    view(ctx, task)
}

pub fn list(ctx: &Ctx, filter: &TaskFilter) -> Result<TaskList> {
    let statuses = if filter.statuses.is_empty() {
        vec![TaskStatus::Todo, TaskStatus::Doing]
    } else {
        filter.statuses.clone()
    };
    let tasks = repo::task::list(ctx.conn, &statuses, filter.kind)?
        .into_iter()
        .map(|t| view(ctx, t))
        .collect::<Result<_>>()?;
    Ok(TaskList { tasks })
}

pub fn show(ctx: &Ctx, id: &str) -> Result<TaskShow> {
    let task = repo::task::resolve(ctx.conn, id)?;
    let rules = rule::list_task_rules(ctx.conn, Some(&task.id))?;
    let (tracked_ms, tracked_segments): (i64, i64) = ctx.conn.query_row(
        "SELECT COALESCE(SUM(end_at - start_at), 0), COUNT(*) FROM activity_segment WHERE task_id = ?1",
        [&task.id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    Ok(TaskShow {
        task: view(ctx, task)?,
        rules,
        tracked_ms,
        tracked_segments,
    })
}

pub fn edit(ctx: &Ctx, id: &str, input: &TaskInput) -> Result<TaskView> {
    let mut t = repo::task::resolve(ctx.conn, id)?;
    if let Some(title) = &input.title {
        t.title = title.trim().to_string();
    }
    if let Some(b) = &input.body {
        t.body_md = b.clone();
    }
    if input.interrupt {
        t.kind = TaskKind::Interrupt;
        t.urgent = Some(true);
    }
    if input.urgent.is_some() {
        t.urgent = input.urgent;
    }
    if input.important.is_some() {
        t.important = input.important;
    }
    if input.estimate_min.is_some() {
        t.estimate_min = input.estimate_min;
    }
    if let Some(p) = &input.key_result {
        t.key_result_id = Some(okr::resolve_kr(ctx.conn, p)?.id);
    }
    if input.external_url.is_some() {
        t.external_url = input.external_url.clone();
    }
    if input.planned_start_at.is_some() {
        t.planned_start_at = input.planned_start_at;
    }
    if input.planned_end_at.is_some() {
        t.planned_end_at = input.planned_end_at;
    }
    t.updated_at = ctx.now();
    validate(&t)?;
    let sids = skill_ids(ctx, &input.skills)?;
    let tx = ctx.conn.unchecked_transaction()?;
    repo::task::update(ctx.conn, &t)?;
    if !input.skills.is_empty() || input.clear_skills {
        skill::set_task_skills(ctx.conn, &t.id, &sids)?;
    }
    tx.commit()?;
    view(ctx, t)
}

/// doing にする。`task.single_doing` なら他の doing を todo に戻してログを閉じる。
pub fn start(ctx: &Ctx, id: &str) -> Result<TaskChange> {
    let mut t = repo::task::resolve(ctx.conn, id)?;
    let now = ctx.now();
    let tx = ctx.conn.unchecked_transaction()?;
    let mut demoted = Vec::new();
    if ctx.cfg.task.single_doing {
        for mut other in repo::task::list_doing(ctx.conn)? {
            if other.id == t.id {
                continue;
            }
            repo::task::close_doing_logs(ctx.conn, &other.id, now)?;
            other.status = TaskStatus::Todo;
            other.updated_at = now;
            repo::task::update(ctx.conn, &other)?;
            demoted.push(other);
        }
    }
    if t.status != TaskStatus::Doing {
        t.status = TaskStatus::Doing;
        t.done_at = None;
        t.updated_at = now;
        repo::task::update(ctx.conn, &t)?;
        repo::task::open_doing_log(ctx.conn, &t.id, now)?;
    }
    tx.commit()?;
    Ok(TaskChange {
        task: view(ctx, t)?,
        demoted: demoted.into_iter().map(|d| view(ctx, d)).collect::<Result<_>>()?,
    })
}

fn finish(ctx: &Ctx, id: &str, status: TaskStatus) -> Result<TaskView> {
    let mut t = repo::task::resolve(ctx.conn, id)?;
    let now = ctx.now();
    let tx = ctx.conn.unchecked_transaction()?;
    repo::task::close_doing_logs(ctx.conn, &t.id, now)?;
    t.status = status;
    t.done_at = Some(now);
    t.updated_at = now;
    repo::task::update(ctx.conn, &t)?;
    tx.commit()?;
    view(ctx, t)
}

pub fn done(ctx: &Ctx, id: &str) -> Result<TaskView> {
    finish(ctx, id, TaskStatus::Done)
}

pub fn drop_task(ctx: &Ctx, id: &str) -> Result<TaskView> {
    finish(ctx, id, TaskStatus::Dropped)
}

// ===== 帰属ルール =====

#[derive(Debug, Clone)]
pub struct RuleInput {
    /// タスク ID（前方一致）。
    pub task: String,
    pub field: MatchField,
    pub pattern: String,
    /// 省略時は glob。
    pub pattern_type: Option<PatternType>,
    pub priority: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct RuleList {
    pub rules: Vec<TaskMatchRule>,
}

/// cwd / repo の glob はパス区切りを `/` に正規化する（`\` は glob のエスケープになるため）。
pub(crate) fn normalize_pattern(field: MatchField, ty: PatternType, pattern: &str) -> String {
    if ty == PatternType::Glob && matches!(field, MatchField::Cwd | MatchField::Repo) {
        pattern.replace('\\', "/")
    } else {
        pattern.to_string()
    }
}

pub fn rule_add(ctx: &Ctx, input: &RuleInput) -> Result<TaskMatchRule> {
    let task = repo::task::resolve(ctx.conn, &input.task)?;
    let ty = input.pattern_type.unwrap_or(PatternType::Glob);
    let pattern = normalize_pattern(input.field, ty, &input.pattern);
    rule::validate_pattern(&pattern, ty)?;
    let r = TaskMatchRule {
        id: new_id(),
        task_id: task.id,
        field: input.field,
        pattern,
        pattern_type: ty,
        priority: input.priority,
        enabled: true,
    };
    rule::insert_task_rule(ctx.conn, &r)?;
    Ok(r)
}

pub fn rule_list(ctx: &Ctx, task: Option<&str>) -> Result<RuleList> {
    let tid = match task {
        Some(p) => Some(repo::task::resolve(ctx.conn, p)?.id),
        None => None,
    };
    Ok(RuleList {
        rules: rule::list_task_rules(ctx.conn, tid.as_deref())?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::testutil::Env;

    fn add_task(ctx: &Ctx, title: &str) -> TaskView {
        add(
            ctx,
            &TaskInput {
                title: Some(title.into()),
                ..Default::default()
            },
        )
        .unwrap()
    }

    #[test]
    fn add_interrupt_sets_urgent() {
        let env = Env::new();
        let ctx = env.ctx();
        let v = add(
            &ctx,
            &TaskInput {
                title: Some("障害対応".into()),
                interrupt: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(v.task.kind, TaskKind::Interrupt);
        assert_eq!(v.task.urgent, Some(true));
        assert!(add(&ctx, &TaskInput::default()).is_err());
    }

    #[test]
    fn start_single_doing_demotes_and_logs() {
        let env = Env::new();
        let ctx = env.ctx();
        let a = add_task(&ctx, "A");
        let b = add_task(&ctx, "B");
        start(&ctx, &a.task.id).unwrap();
        env.clock.advance(60_000);
        let ch = start(&ctx, &b.task.id).unwrap();
        assert_eq!(ch.demoted.len(), 1);
        assert_eq!(ch.demoted[0].task.id, a.task.id);
        let doing = repo::task::list_doing(ctx.conn).unwrap();
        assert_eq!(doing.len(), 1);
        assert_eq!(doing[0].id, b.task.id);
        let open: i64 = ctx
            .conn
            .query_row(
                "SELECT COUNT(*) FROM task_doing_log WHERE end_at IS NULL",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(open, 1);
        env.clock.advance(60_000);
        let d = done(&ctx, &b.task.id).unwrap();
        assert_eq!(d.task.status, TaskStatus::Done);
        assert_eq!(d.task.done_at, Some(ctx.now()));
        let open: i64 = ctx
            .conn
            .query_row(
                "SELECT COUNT(*) FROM task_doing_log WHERE end_at IS NULL",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(open, 0);
    }

    #[test]
    fn multi_doing_when_disabled() {
        let mut env = Env::new();
        env.cfg.task.single_doing = false;
        let ctx = env.ctx();
        let a = add_task(&ctx, "A");
        let b = add_task(&ctx, "B");
        start(&ctx, &a.task.id).unwrap();
        start(&ctx, &b.task.id).unwrap();
        assert_eq!(repo::task::list_doing(ctx.conn).unwrap().len(), 2);
    }

    #[test]
    fn list_defaults_and_drop() {
        let env = Env::new();
        let ctx = env.ctx();
        let a = add_task(&ctx, "A");
        add_task(&ctx, "B");
        drop_task(&ctx, &a.task.id).unwrap();
        assert_eq!(list(&ctx, &TaskFilter::default()).unwrap().tasks.len(), 1);
        let all = list(
            &ctx,
            &TaskFilter {
                statuses: TaskStatus::ALL.to_vec(),
                kind: None,
            },
        )
        .unwrap();
        assert_eq!(all.tasks.len(), 2);
    }

    #[test]
    fn unknown_skill_is_not_found_and_rule_defaults_glob() {
        let env = Env::new();
        let ctx = env.ctx();
        let err = add(
            &ctx,
            &TaskInput {
                title: Some("x".into()),
                skills: vec!["nope".into()],
                ..Default::default()
            },
        )
        .unwrap_err();
        assert_eq!(err.exit_code(), 3);
        let t = add_task(&ctx, "T");
        let r = rule_add(
            &ctx,
            &RuleInput {
                task: t.task.id.clone(),
                field: MatchField::Cwd,
                pattern: "C:\\src\\foo\\**".into(),
                pattern_type: None,
                priority: 0,
            },
        )
        .unwrap();
        assert_eq!(r.pattern_type, PatternType::Glob);
        assert_eq!(r.pattern, "C:/src/foo/**");
        assert_eq!(rule_list(&ctx, Some(&t.task.id)).unwrap().rules.len(), 1);
        assert!(rule_add(
            &ctx,
            &RuleInput {
                task: t.task.id,
                field: MatchField::Title,
                pattern: "(".into(),
                pattern_type: Some(PatternType::Regex),
                priority: 0
            }
        )
        .is_err());
    }
}
