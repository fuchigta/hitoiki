//! クイック入力（割り込みなどで、タスクを素早く作成・選択して開始する）のユースケース。
//! CLI の `task switch` / `task back` と GUI のクイック入力ウインドウが共通で呼ぶ。

use super::task::{self, TaskChange, TaskInput, TaskView};
use super::Ctx;
use crate::model::{Task, TaskStatus};
use crate::repo;
use crate::{Error, Result};
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};

/// `switch` の入力。
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct SwitchInput {
    /// タスクの ID（前方一致）またはタイトル。
    pub query: String,
    /// 既存のタスクを探さず、`query` をタイトルとして必ず新規作成する。
    pub new: bool,
    /// 新規作成するタスクを割り込み（kind=interrupt, urgent）にする。`new` のときだけ有効。
    pub interrupt: bool,
}

/// `switch` / `back` の結果。
#[derive(Debug, Clone, Serialize)]
pub struct SwitchResult {
    #[serde(flatten)]
    pub change: TaskChange,
    /// 新規に作成したタスクか。
    pub created: bool,
}

/// 直前まで doing だったタスク（いまの doing を除く、最後に doing を終えた未完了のタスク）。
///
/// `task_doing_log` の閉じたログのうち終了が最も新しいものを選ぶ。完了・破棄したタスクは戻り先にしない。
pub fn previous_doing(ctx: &Ctx) -> Result<Option<TaskView>> {
    let id: Option<String> = ctx
        .conn
        .query_row(
            "SELECT l.task_id FROM task_doing_log l JOIN task t ON t.id = l.task_id \
             WHERE l.end_at IS NOT NULL AND t.status = 'todo' \
             ORDER BY l.end_at DESC, l.start_at DESC, l.id DESC LIMIT 1",
            [],
            |r| r.get(0),
        )
        .optional()?;
    match id {
        Some(id) => Ok(Some(task::view(ctx, repo::task::get(ctx.conn, &id)?)?)),
        None => Ok(None),
    }
}

/// 未完了（todo / doing）のタスクから `query` に対応する 1 件を探す。
///
/// 順に、ID の完全一致、タイトルの完全一致（大文字小文字無視）、ID の前方一致（4 文字以上）、
/// タイトルの部分一致。複数に当たれば曖昧（終了コード 4）、無ければ対象なし（3）。
fn find_open(ctx: &Ctx, query: &str) -> Result<Task> {
    let open = repo::task::list(ctx.conn, &[TaskStatus::Todo, TaskStatus::Doing], None)?;
    let q = query.trim();
    let upper = q.to_ascii_uppercase();
    if let Some(t) = open.iter().find(|t| t.id == upper) {
        return Ok(t.clone());
    }
    let pick = |hits: Vec<&Task>| -> Result<Option<Task>> {
        match hits.len() {
            0 => Ok(None),
            1 => Ok(Some(hits[0].clone())),
            _ => Err(Error::Ambiguous(format!(
                "{q}: {}",
                hits.iter()
                    .map(|t| format!("{} {}", &t.id[..t.id.len().min(8)], t.title))
                    .collect::<Vec<_>>()
                    .join(", ")
            ))),
        }
    };
    let lower = q.to_lowercase();
    if let Some(t) = pick(open.iter().filter(|t| t.title.to_lowercase() == lower).collect())? {
        return Ok(t);
    }
    if q.len() >= 4 && q.chars().all(|c| c.is_ascii_alphanumeric()) {
        if let Some(t) = pick(open.iter().filter(|t| t.id.starts_with(&upper)).collect())? {
            return Ok(t);
        }
    }
    if let Some(t) = pick(
        open.iter()
            .filter(|t| t.title.to_lowercase().contains(&lower))
            .collect(),
    )? {
        return Ok(t);
    }
    Err(Error::NotFound(format!("未完了のタスク「{q}」")))
}

/// タスクを作成または選択して、すぐに開始（doing に切り替え）する。
pub fn switch(ctx: &Ctx, input: &SwitchInput) -> Result<SwitchResult> {
    let q = input.query.trim();
    if q.is_empty() {
        return Err(Error::InvalidArgument("タスクのタイトルか ID が必要です".into()));
    }
    if input.interrupt && !input.new {
        return Err(Error::InvalidArgument(
            "interrupt は新規作成（new）のときだけ指定できます".into(),
        ));
    }
    let (id, created) = if input.new {
        let v = task::add(
            ctx,
            &TaskInput {
                title: Some(q.to_string()),
                interrupt: input.interrupt,
                ..Default::default()
            },
        )?;
        (v.task.id, true)
    } else {
        (find_open(ctx, q)?.id, false)
    };
    let change = task::start(ctx, &id)?;
    Ok(SwitchResult { change, created })
}

/// 直前の doing に戻る。
pub fn back(ctx: &Ctx) -> Result<SwitchResult> {
    let prev = previous_doing(ctx)?.ok_or_else(|| Error::NotFound("戻れる直前のタスク".into()))?;
    let change = task::start(ctx, &prev.task.id)?;
    Ok(SwitchResult {
        change,
        created: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::TaskKind;
    use crate::service::testutil::Env;

    fn add(ctx: &Ctx, title: &str) -> TaskView {
        task::add(
            ctx,
            &TaskInput {
                title: Some(title.into()),
                ..Default::default()
            },
        )
        .unwrap()
    }

    fn sw(query: &str, new: bool, interrupt: bool) -> SwitchInput {
        SwitchInput {
            query: query.into(),
            new,
            interrupt,
        }
    }

    #[test]
    fn 直前のdoingは無ければ_none() {
        let env = Env::new();
        let ctx = env.ctx();
        assert!(previous_doing(&ctx).unwrap().is_none());
        let a = add(&ctx, "A");
        task::start(&ctx, &a.task.id).unwrap();
        // 開始しただけ（まだ誰も doing を終えていない）。
        assert!(previous_doing(&ctx).unwrap().is_none());
    }

    #[test]
    fn 新規作成して開始し直前のdoingに戻れる() {
        let env = Env::new();
        let ctx = env.ctx();
        let a = add(&ctx, "設計");
        task::start(&ctx, &a.task.id).unwrap();
        env.clock.advance(60_000);

        let r = switch(&ctx, &sw("障害対応", true, true)).unwrap();
        assert!(r.created);
        assert_eq!(r.change.task.task.status, TaskStatus::Doing);
        assert_eq!(r.change.task.task.kind, TaskKind::Interrupt);
        assert_eq!(r.change.task.task.urgent, Some(true));
        assert_eq!(r.change.demoted[0].task.id, a.task.id);

        // 戻り先は割り込み前の「設計」。
        assert_eq!(previous_doing(&ctx).unwrap().unwrap().task.id, a.task.id);
        env.clock.advance(60_000);
        let b = back(&ctx).unwrap();
        assert!(!b.created);
        assert_eq!(b.change.task.task.id, a.task.id);
        assert_eq!(b.change.task.task.status, TaskStatus::Doing);
        assert_eq!(b.change.demoted[0].task.title, "障害対応");
        // もう一度戻ると割り込みタスクに行き来できる。
        env.clock.advance(60_000);
        assert_eq!(back(&ctx).unwrap().change.task.task.title, "障害対応");
    }

    #[test]
    fn 完了したタスクには戻らない() {
        let env = Env::new();
        let ctx = env.ctx();
        let a = add(&ctx, "A");
        let b = add(&ctx, "B");
        task::start(&ctx, &a.task.id).unwrap();
        env.clock.advance(1000);
        task::start(&ctx, &b.task.id).unwrap();
        env.clock.advance(1000);
        task::done(&ctx, &a.task.id).unwrap();
        task::done(&ctx, &b.task.id).unwrap();
        assert!(previous_doing(&ctx).unwrap().is_none());
        assert_eq!(back(&ctx).unwrap_err().exit_code(), 3);
    }

    #[test]
    fn 既存のタスクをタイトルやidで選んで開始する() {
        let env = Env::new();
        let ctx = env.ctx();
        let a = add(&ctx, "レビュー依頼の対応");
        add(&ctx, "設計メモ");
        add(&ctx, "設計レビュー");
        // 完全一致（大文字小文字無視）。
        let r = switch(&ctx, &sw("設計メモ", false, false)).unwrap();
        assert!(!r.created);
        assert_eq!(r.change.task.task.title, "設計メモ");
        // 部分一致が 1 件。
        let r = switch(&ctx, &sw("依頼", false, false)).unwrap();
        assert_eq!(r.change.task.task.id, a.task.id);
        // ID の前方一致。
        let r = switch(&ctx, &sw(&a.task.id[..18], false, false)).unwrap();
        assert_eq!(r.change.task.task.id, a.task.id);
        // 部分一致が複数 → 曖昧。
        assert_eq!(
            switch(&ctx, &sw("設計", false, false)).unwrap_err().exit_code(),
            4
        );
        // 見つからない → 対象なし（作成はしない）。
        assert_eq!(
            switch(&ctx, &sw("ない", false, false)).unwrap_err().exit_code(),
            3
        );
        let all = task::list(&ctx, &task::TaskFilter::default()).unwrap();
        assert_eq!(all.tasks.len(), 3);
    }

    #[test]
    fn 完了済みは選べず_new_は既存と同名でも作る() {
        let env = Env::new();
        let ctx = env.ctx();
        let a = add(&ctx, "A");
        task::done(&ctx, &a.task.id).unwrap();
        assert_eq!(switch(&ctx, &sw("A", false, false)).unwrap_err().exit_code(), 3);
        add(&ctx, "メール");
        let r = switch(&ctx, &sw("メール", true, false)).unwrap();
        assert!(r.created);
        assert_eq!(r.change.task.task.kind, TaskKind::Normal);
        assert_eq!(
            task::list(&ctx, &task::TaskFilter::default())
                .unwrap()
                .tasks
                .len(),
            2
        );
    }

    #[test]
    fn 不正な入力は引数エラー() {
        let env = Env::new();
        let ctx = env.ctx();
        assert_eq!(switch(&ctx, &sw("  ", true, false)).unwrap_err().exit_code(), 2);
        // 既存の選択に割り込みは指定できない。
        assert_eq!(switch(&ctx, &sw("x", false, true)).unwrap_err().exit_code(), 2);
    }
}
