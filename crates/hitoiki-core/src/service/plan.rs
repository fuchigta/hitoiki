//! 計画ブロック（docs/dev/architecture.md「データモデル」、docs/dev/architecture.md「CLI」）。

use super::{range_view, task_titles, Ctx, RangeArgs, RangeView};
use crate::id::new_id;
use crate::model::PlanBlock;
use crate::repo::{self, plan};
use crate::{Error, Millis, Result};
use serde::Serialize;

#[derive(Debug, Clone)]
pub struct BlockInput {
    pub start: Millis,
    pub end: Millis,
    /// タスク ID（前方一致）。
    pub task: Option<String>,
    pub label: Option<String>,
    pub focus: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct BlockView {
    #[serde(flatten)]
    pub block: PlanBlock,
    pub task_title: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BlockList {
    pub range: RangeView,
    pub blocks: Vec<BlockView>,
}

/// 入力を検証して `PlanBlock` を組み立てる（保存はしない）。
fn build(ctx: &Ctx, id: String, input: &BlockInput) -> Result<PlanBlock> {
    if input.end <= input.start {
        return Err(Error::InvalidArgument("終了は開始より後にしてください".into()));
    }
    let task_id = match &input.task {
        Some(p) => Some(repo::task::resolve(ctx.conn, p)?.id),
        None => None,
    };
    if task_id.is_none() && input.label.is_none() {
        return Err(Error::InvalidArgument(
            "--task か --label のいずれかが必要です".into(),
        ));
    }
    Ok(PlanBlock {
        id,
        start_at: input.start,
        end_at: input.end,
        task_id,
        label: input.label.clone(),
        focus: input.focus,
    })
}

pub fn add(ctx: &Ctx, input: &BlockInput) -> Result<PlanBlock> {
    let b = build(ctx, new_id(), input)?;
    plan::insert(ctx.conn, &b)?;
    Ok(b)
}

/// ブロックの内容（時間・タスク・ラベル・集中）をまるごと置き換える。`id` は前方一致可。
pub fn update(ctx: &Ctx, id: &str, input: &BlockInput) -> Result<PlanBlock> {
    let cur = plan::resolve(ctx.conn, id)?;
    let b = build(ctx, cur.id, input)?;
    plan::update(ctx.conn, &b)?;
    Ok(b)
}

/// ID（前方一致可）でブロックを 1 件取得する。
pub fn find(ctx: &Ctx, id: &str) -> Result<PlanBlock> {
    plan::resolve(ctx.conn, id)
}

/// ブロックを削除する。存在しなければ `NotFound`。`id` は前方一致可。
pub fn delete(ctx: &Ctx, id: &str) -> Result<()> {
    let b = plan::resolve(ctx.conn, id)?;
    if plan::delete(ctx.conn, &b.id)? {
        Ok(())
    } else {
        Err(Error::NotFound(format!("ブロック {id}")))
    }
}

pub fn list(ctx: &Ctx, args: &RangeArgs) -> Result<BlockList> {
    let tz = ctx.tz()?;
    let range = ctx.range(args)?;
    let titles = task_titles(ctx.conn)?;
    let blocks = plan::list(ctx.conn, range)?
        .into_iter()
        .map(|b| BlockView {
            task_title: b.task_id.as_ref().and_then(|i| titles.get(i).cloned()),
            block: b,
        })
        .collect();
    Ok(BlockList {
        range: range_view(tz, range),
        blocks,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::testutil::Env;

    #[test]
    fn delete_block() {
        let env = Env::new();
        let ctx = env.ctx();
        let d = env.day_start();
        let b = add(
            &ctx,
            &BlockInput {
                start: d + 3_600_000,
                end: d + 7_200_000,
                task: None,
                label: Some("x".into()),
                focus: false,
            },
        )
        .unwrap();
        delete(&ctx, &b.id).unwrap();
        assert!(list(&ctx, &RangeArgs::default()).unwrap().blocks.is_empty());
        assert!(matches!(delete(&ctx, &b.id), Err(Error::NotFound(_))));
    }

    #[test]
    fn update_block() {
        let env = Env::new();
        let ctx = env.ctx();
        let d = env.day_start();
        let b = add(
            &ctx,
            &BlockInput {
                start: d + 3_600_000,
                end: d + 7_200_000,
                task: None,
                label: Some("x".into()),
                focus: false,
            },
        )
        .unwrap();
        let u = update(
            &ctx,
            &b.id[..8],
            &BlockInput {
                start: d + 7_200_000,
                end: d + 10_800_000,
                task: None,
                label: Some("y".into()),
                focus: true,
            },
        )
        .unwrap();
        assert_eq!(u.id, b.id);
        let l = list(&ctx, &RangeArgs::default()).unwrap();
        assert_eq!(l.blocks.len(), 1);
        assert_eq!(l.blocks[0].block.start_at, d + 7_200_000);
        assert!(l.blocks[0].block.focus);
        assert!(update(
            &ctx,
            &b.id,
            &BlockInput {
                start: 5,
                end: 5,
                task: None,
                label: Some("a".into()),
                focus: false
            }
        )
        .is_err());
        assert!(matches!(
            update(
                &ctx,
                "ZZZZ",
                &BlockInput {
                    start: 1,
                    end: 5,
                    task: None,
                    label: Some("a".into()),
                    focus: false
                }
            ),
            Err(Error::NotFound(_))
        ));
    }

    #[test]
    fn add_and_list_today() {
        let env = Env::new();
        let ctx = env.ctx();
        let d = env.day_start();
        add(
            &ctx,
            &BlockInput {
                start: d + 3_600_000,
                end: d + 7_200_000,
                task: None,
                label: Some("集中".into()),
                focus: true,
            },
        )
        .unwrap();
        // 翌日のブロックは今日の一覧に出ない
        add(
            &ctx,
            &BlockInput {
                start: d + 90_000_000,
                end: d + 93_000_000,
                task: None,
                label: Some("x".into()),
                focus: false,
            },
        )
        .unwrap();
        let l = list(&ctx, &RangeArgs::default()).unwrap();
        assert_eq!(l.blocks.len(), 1);
        assert!(l.blocks[0].block.focus);
        assert!(add(
            &ctx,
            &BlockInput {
                start: 5,
                end: 5,
                task: None,
                label: Some("a".into()),
                focus: false
            }
        )
        .is_err());
        assert!(add(
            &ctx,
            &BlockInput {
                start: 1,
                end: 5,
                task: None,
                label: None,
                focus: false
            }
        )
        .is_err());
    }
}
