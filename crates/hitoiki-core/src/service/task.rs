//! タスクとタスク帰属ルールのユースケース（docs/dev/architecture.md「CLI」）。

use super::Ctx;
use crate::id::new_id;
use crate::model::{MatchField, PatternType, Task, TaskKind, TaskMatchRule, TaskStatus};
use crate::pipeline::SegmentJudge;
use crate::repo::{self, goal, rule, skill};
use crate::timefmt::Range;
use crate::{Error, Millis, Result};
use rusqlite::params;
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
    /// 成果の指標の ID（前方一致）。
    pub key_result: Option<String>,
    /// edit で成果の指標との紐付けを外す（`key_result` と同時に指定したときは外す方が優先）。
    pub clear_key_result: bool,
    /// スキル名。edit では 1 つ以上指定されたとき置き換える。
    pub skills: Vec<String>,
    /// edit でスキルを全て外す（`skills` が空でも置き換える）。
    pub clear_skills: bool,
    pub external_url: Option<String>,
    pub planned_start_at: Option<Millis>,
    pub planned_end_at: Option<Millis>,
    /// 割り込みタスク（kind=interrupt かつ urgent=1）。
    pub interrupt: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct KeyResultRef {
    pub id: String,
    pub title: String,
    pub goal_id: String,
    pub goal_title: String,
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
            let key_result = goal::get_key_result(ctx.conn, id)?;
            let goal_title: String = ctx.conn.query_row(
                "SELECT title FROM goal WHERE id = ?1",
                [&key_result.goal_id],
                |r| r.get(0),
            )?;
            Some(KeyResultRef {
                id: key_result.id,
                title: key_result.title,
                goal_id: key_result.goal_id,
                goal_title,
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
    let key_result_id = match &input.key_result {
        Some(p) => Some(goal::resolve_key_result(ctx.conn, p)?.id),
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
        key_result_id,
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
    if input.clear_key_result {
        t.key_result_id = None;
    } else if let Some(p) = &input.key_result {
        t.key_result_id = Some(goal::resolve_key_result(ctx.conn, p)?.id);
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

// ===== 削除 =====

/// タスクの削除が及ぼす影響（`delete_preview` の結果。削除後は削除した内容として返す）。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DeletePreview {
    pub task_id: String,
    pub title: String,
    pub status: TaskStatus,
    /// このタスクに計上済みの区間の数と合計（ミリ秒）。削除後は未割り当てに戻る。
    pub segments: i64,
    pub tracked_ms: i64,
    /// このタスクを指す確定済みの割り当て（`segment_override`）の数。削除される。
    pub overrides: i64,
    /// このタスクへの補正（`segment_correction`）の数。補正ごと削除される。
    pub corrections: i64,
    /// 帰属ルールの数（削除される）。
    pub rules: i64,
    /// スキルの紐付けの数（削除される）。
    pub skills: i64,
    /// このタスクを指す計画ブロックの数（ブロックは残り、タスクの指定だけが外れる）。
    pub blocks: i64,
    /// このタスクを指すテンプレートの項目の数（項目は残り、タスクの指定だけが外れる）。
    pub template_items: i64,
    /// doing の履歴の数（削除される）。
    pub doing_logs: i64,
    /// 実行中（doing）だったか。
    pub is_doing: bool,
    /// 計上済み時間・確定済みの割り当て・補正のどれかがあり、実行に `force` が要るか。
    pub needs_force: bool,
    /// 影響の日本語の説明（確認の文面用）。
    pub summary: String,
}

fn summarize(p: &DeletePreview) -> String {
    let mut parts = Vec::new();
    if p.segments > 0 || p.overrides > 0 {
        parts.push(format!(
            "計上済み {}（{} 区間）と確定した割り当て {} 件が未割り当てに戻ります",
            super::correction::fmt_jp_duration(p.tracked_ms),
            p.segments,
            p.overrides
        ));
    }
    if p.corrections > 0 {
        parts.push(format!("補正 {} 件も削除されます", p.corrections));
    }
    if p.rules > 0 {
        parts.push(format!("帰属ルール {} 件も削除されます", p.rules));
    }
    if p.blocks + p.template_items > 0 {
        parts.push(format!(
            "計画ブロック {} 件・テンプレート項目 {} 件はタスクの指定だけが外れます",
            p.blocks, p.template_items
        ));
    }
    if p.is_doing {
        parts.push("実行中（doing）は解除されます".to_string());
    }
    if parts.is_empty() {
        "他への影響はありません。".to_string()
    } else {
        format!("{}。", parts.join("。"))
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct DeleteResult {
    pub deleted: DeletePreview,
    /// 再計算した範囲（区間も確定した割り当ても無ければ `None`）。
    pub range: Option<super::RangeView>,
}

fn count(conn: &rusqlite::Connection, sql: &str, id: &str) -> Result<i64> {
    Ok(conn.query_row(sql, [id], |r| r.get(0))?)
}

/// 影響の見積もり。再計算する期間（計上済み区間・確定済みの割り当て・補正の最小〜最大）も返す。
fn impact(ctx: &Ctx, t: &Task) -> Result<(DeletePreview, Option<Range>)> {
    let c = ctx.conn;
    let id = t.id.as_str();
    type Agg = (i64, i64, Option<Millis>, Option<Millis>);
    let (segments, tracked_ms, s_lo, s_hi): Agg = c.query_row(
        "SELECT COUNT(*), COALESCE(SUM(end_at - start_at), 0), MIN(start_at), MAX(end_at) \
         FROM activity_segment WHERE task_id = ?1",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    )?;
    // 補正が作った override は task_id が付け替え先と同じだが、念のため correction_id でも拾う。
    let (overrides, _, o_lo, o_hi): Agg = c.query_row(
        "SELECT COUNT(*), 0, MIN(start_at), MAX(end_at) FROM segment_override \
         WHERE task_id = ?1 OR correction_id IN (SELECT id FROM segment_correction WHERE task_id = ?1)",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    )?;
    let (corrections, _, c_lo, c_hi): Agg = c.query_row(
        "SELECT COUNT(*), 0, MIN(start_at), MAX(end_at) FROM segment_correction WHERE task_id = ?1",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    )?;
    let lo = [s_lo, o_lo, c_lo].into_iter().flatten().min();
    let hi = [s_hi, o_hi, c_hi].into_iter().flatten().max();
    let range = match (lo, hi) {
        (Some(a), Some(b)) if b > a => Some(Range::new(a, b)),
        _ => None,
    };
    let mut preview = DeletePreview {
        task_id: t.id.clone(),
        title: t.title.clone(),
        status: t.status,
        segments,
        tracked_ms,
        overrides,
        corrections,
        rules: count(c, "SELECT COUNT(*) FROM task_match_rule WHERE task_id = ?1", id)?,
        skills: count(c, "SELECT COUNT(*) FROM task_skill WHERE task_id = ?1", id)?,
        blocks: count(c, "SELECT COUNT(*) FROM time_block WHERE task_id = ?1", id)?,
        template_items: count(
            c,
            "SELECT COUNT(*) FROM block_template_item WHERE task_id = ?1",
            id,
        )?,
        doing_logs: count(c, "SELECT COUNT(*) FROM task_doing_log WHERE task_id = ?1", id)?,
        is_doing: t.status == TaskStatus::Doing,
        needs_force: segments > 0 || overrides > 0 || corrections > 0,
        summary: String::new(),
    };
    preview.summary = summarize(&preview);
    Ok((preview, range))
}

/// 削除の影響を見積もる（何も変更しない）。
pub fn delete_preview(ctx: &Ctx, id: &str) -> Result<DeletePreview> {
    let t = repo::task::resolve(ctx.conn, id)?;
    Ok(impact(ctx, &t)?.0)
}

/// 誤入力のタスクを削除する（「やめる」は記録として残す。削除は元から無かったことにする）。
///
/// 1 トランザクションで次を行い、その後に影響した期間を再計算して区間を未割り当てへ戻す。
/// - タスク本体・帰属ルール・スキルの紐付け・doing の履歴を削除する。
/// - このタスクへの `segment_override` を削除する。補正（`segment_correction`）は付け替え先が 1 つのタスクなので、
///   このタスクへの補正は補正ごと（補正が作った override も含めて）削除する。他のタスクへの補正には触れない。
/// - `time_block` / `block_template_item` の `task_id` は NULL にして残す（ラベルが空ならタスク名を写す）。
///
/// 計上済み時間・確定済みの割り当て・補正があるとき、`force` が偽ならエラー（引数不正）で影響を知らせる。
pub fn delete(ctx: &Ctx, id: &str, force: bool, judge: Option<&dyn SegmentJudge>) -> Result<DeleteResult> {
    let t = repo::task::resolve(ctx.conn, id)?;
    let (preview, range) = impact(ctx, &t)?;
    if preview.needs_force && !force {
        return Err(Error::InvalidArgument(format!(
            "タスク「{}」には記録済みの内容があります。{}削除するには --force を指定してください",
            t.title, preview.summary
        )));
    }
    let tid = t.id.as_str();
    let c = ctx.conn;
    let tx = c.unchecked_transaction()?;
    // 補正が作った override → このタスクへの override → 補正 の順。
    c.execute(
        "DELETE FROM segment_override WHERE correction_id IN (SELECT id FROM segment_correction WHERE task_id = ?1)",
        [tid],
    )?;
    c.execute("DELETE FROM segment_override WHERE task_id = ?1", [tid])?;
    c.execute("DELETE FROM segment_correction WHERE task_id = ?1", [tid])?;
    // 再計算までの間も参照が残らないよう、派生層は先に未割り当てにしておく。
    c.execute(
        "UPDATE activity_segment SET task_id = NULL, \
         status = CASE WHEN status = 'confirmed' THEN 'needs_review' ELSE status END WHERE task_id = ?1",
        [tid],
    )?;
    c.execute(
        "UPDATE time_block SET label = CASE WHEN label IS NULL OR TRIM(label) = '' THEN ?2 ELSE label END, \
         task_id = NULL WHERE task_id = ?1",
        params![tid, t.title],
    )?;
    c.execute(
        "UPDATE block_template_item SET label = CASE WHEN label IS NULL OR TRIM(label) = '' THEN ?2 ELSE label END, \
         task_id = NULL WHERE task_id = ?1",
        params![tid, t.title],
    )?;
    c.execute("DELETE FROM task_doing_log WHERE task_id = ?1", [tid])?;
    c.execute("DELETE FROM task_match_rule WHERE task_id = ?1", [tid])?;
    c.execute("DELETE FROM task_skill WHERE task_id = ?1", [tid])?;
    c.execute("DELETE FROM task WHERE id = ?1", [tid])?;
    tx.commit()?;
    let range = match range {
        Some(r) => {
            let stats = crate::rebuild::rebuild_with_judge(c, ctx.cfg, r, ctx.now(), judge)?;
            Some(super::range_view(ctx.tz()?, stats.range))
        }
        None => None,
    };
    Ok(DeleteResult {
        deleted: preview,
        range,
    })
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
    // ---- 削除 ----

    const MIN: Millis = 60_000;

    fn n(ctx: &Ctx, sql: &str) -> i64 {
        ctx.conn.query_row(sql, [], |r| r.get(0)).unwrap()
    }

    /// 0〜30 分に Code.exe の窓を置き、exe のルールでタスクに帰属させる。
    fn tracked_task(env: &Env) -> (String, Millis) {
        let ctx = env.ctx();
        let t = add_task(&ctx, "誤入力").task.id;
        let m0 = ctx.now() - 120 * MIN;
        ctx.conn
            .execute(
                "INSERT INTO window_span (id, start_at, end_at, exe_name, title, excluded)                  VALUES ('w1', ?1, ?2, 'Code.exe', 'main.rs', 0)",
                rusqlite::params![m0, m0 + 30 * MIN],
            )
            .unwrap();
        rule_add(
            &ctx,
            &RuleInput {
                task: t.clone(),
                field: MatchField::Exe,
                pattern: "Code.exe".into(),
                pattern_type: Some(PatternType::Contains),
                priority: 0,
            },
        )
        .unwrap();
        crate::rebuild::rebuild(ctx.conn, ctx.cfg, Range::new(m0, m0 + 60 * MIN), ctx.now()).unwrap();
        (t, m0)
    }

    fn segs(ctx: &Ctx, m0: Millis) -> Vec<crate::model::ActivitySegment> {
        repo::segment::list(ctx.conn, Range::new(m0, m0 + 60 * MIN)).unwrap()
    }

    #[test]
    fn 削除は参照を全て片付けて区間を未割り当てに戻す() {
        let env = Env::new();
        let (t, m0) = tracked_task(&env);
        let ctx = env.ctx();
        let other = add_task(&ctx, "別").task.id;
        assert_eq!(segs(&ctx, m0)[0].task_id.as_deref(), Some(t.as_str()));
        // 補正（このタスクへ 0〜10 分）と、別タスクへの補正（10〜20 分）。
        let apply_to = |task: &str, s: i64, e: i64| {
            crate::service::correction::apply(
                &ctx,
                &crate::service::correction::CorrectionInput {
                    start_at: m0 + s * MIN,
                    end_at: m0 + e * MIN,
                    task: Some(task.into()),
                    include_confirmed: Some(true),
                    ..Default::default()
                },
            )
            .unwrap()
        };
        apply_to(&t, 0, 10);
        let other_corr = apply_to(&other, 10, 20);
        // スキル・ブロック・テンプレート項目・doing。
        ctx.conn
            .execute("INSERT INTO skill (id, name) VALUES ('sk1', 'rust')", [])
            .unwrap();
        skill::set_task_skills(ctx.conn, &t, &["sk1".to_string()]).unwrap();
        ctx.conn
            .execute_batch(&format!(
                "INSERT INTO time_block (id, start_at, end_at, task_id, label) VALUES ('b1', 1, 2, '{t}', NULL),                    ('b2', 3, 4, '{t}', '集中'), ('b3', 5, 6, '{other}', NULL);                  INSERT INTO block_template (id, name, created_at, updated_at) VALUES ('tp', 'T', 0, 0);                  INSERT INTO block_template_item (id, template_id, start_min, end_min, label, task_id) VALUES                    ('i1', 'tp', 0, 60, '', '{t}'), ('i2', 'tp', 60, 120, '午後', '{t}');"
            ))
            .unwrap();
        start(&ctx, &t).unwrap();

        let p = delete_preview(&ctx, &t).unwrap();
        assert_eq!((p.segments, p.tracked_ms), (2, 20 * MIN));
        assert_eq!((p.overrides, p.corrections, p.rules, p.skills), (1, 1, 1, 1));
        assert_eq!((p.blocks, p.template_items, p.doing_logs), (2, 2, 1));
        assert!(p.is_doing && p.needs_force);
        assert!(p
            .summary
            .contains("計上済み 20分（2 区間）と確定した割り当て 1 件が未割り当てに戻ります"));
        assert!(p.summary.contains("帰属ルール 1 件も削除されます"));
        // プレビューは何も変えない。
        assert_eq!(n(&ctx, "SELECT COUNT(*) FROM task"), 2);

        // --force なしは引数エラー（終了コード 2）で、何も消えない。
        let err = delete(&ctx, &t, false, None).unwrap_err();
        assert_eq!(err.exit_code(), 2);
        assert!(err.to_string().contains("--force"));
        assert_eq!(n(&ctx, "SELECT COUNT(*) FROM task"), 2);

        let r = delete(&ctx, &t, true, None).unwrap();
        assert_eq!(r.deleted.segments, 2);
        assert!(r.range.is_some());
        // 各テーブルに参照が残らない。
        for q in [
            "SELECT COUNT(*) FROM task_match_rule",
            "SELECT COUNT(*) FROM task_skill",
            "SELECT COUNT(*) FROM task_doing_log",
            "SELECT COUNT(*) FROM time_block WHERE task_id = (SELECT id FROM task WHERE title = '誤入力')",
        ] {
            assert_eq!(n(&ctx, q), 0, "{q}");
        }
        assert_eq!(n(&ctx, "SELECT COUNT(*) FROM task WHERE title = '誤入力'"), 0);
        assert!(repo::task::list_doing(ctx.conn).unwrap().is_empty());
        // 別タスクの補正とその override は残る。
        assert_eq!(n(&ctx, "SELECT COUNT(*) FROM segment_correction"), 1);
        assert_eq!(
            n(
                &ctx,
                &format!(
                    "SELECT COUNT(*) FROM segment_override WHERE correction_id = '{}'",
                    other_corr.correction.id
                )
            ),
            1
        );
        assert_eq!(
            n(
                &ctx,
                &format!("SELECT COUNT(*) FROM segment_override WHERE task_id = '{t}'")
            ),
            0
        );
        // ブロックとテンプレート項目は残り、task_id は NULL。ラベルが空ならタスク名を写す。
        let lbl = |tbl: &str, id: &str| -> (Option<String>, Option<String>) {
            ctx.conn
                .query_row(
                    &format!("SELECT label, task_id FROM {tbl} WHERE id = '{id}'"),
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .unwrap()
        };
        assert_eq!(lbl("time_block", "b1"), (Some("誤入力".into()), None));
        assert_eq!(lbl("time_block", "b2"), (Some("集中".into()), None));
        assert_eq!(lbl("time_block", "b3").1.as_deref(), Some(other.as_str()));
        assert_eq!(lbl("block_template_item", "i1"), (Some("誤入力".into()), None));
        assert_eq!(lbl("block_template_item", "i2"), (Some("午後".into()), None));
        // 再計算後、0〜30 分の区間はタスクなし（要確認）に戻る。別タスクへの補正（10〜20 分）は維持される。
        let after = segs(&ctx, m0);
        assert!(after.iter().all(|s| s.task_id.as_deref() != Some(t.as_str())));
        let kept: Vec<_> = after
            .iter()
            .filter(|s| s.task_id.as_deref() == Some(other.as_str()))
            .collect();
        assert_eq!(kept.len(), 1);
        let rest: Vec<_> = after.iter().filter(|s| s.task_id.is_none()).collect();
        assert!(!rest.is_empty());
        assert!(rest
            .iter()
            .all(|s| s.status != crate::model::SegmentStatus::Confirmed));
        assert_eq!(rest.iter().map(|s| s.end_at - s.start_at).sum::<i64>(), 20 * MIN);
    }

    #[test]
    fn 記録の無いタスクは_force_なしで削除できる() {
        let env = Env::new();
        let ctx = env.ctx();
        let t = add_task(&ctx, "A").task.id;
        let p = delete_preview(&ctx, &t).unwrap();
        assert!(!p.needs_force);
        assert_eq!(p.summary, "他への影響はありません。");
        let r = delete(&ctx, &t, false, None).unwrap();
        assert!(r.range.is_none());
        assert_eq!(delete(&ctx, &t, false, None).unwrap_err().exit_code(), 3);
    }
}
