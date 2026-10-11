//! タイムブロッキング（docs/dev/architecture.md「データモデル」、docs/dev/architecture.md「CLI」）。

use super::{range_view, task_titles, Ctx, RangeArgs, RangeView};
use crate::id::new_id;
use crate::model::TimeBlock;
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
    pub block: TimeBlock,
    pub task_title: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BlockList {
    pub range: RangeView,
    pub blocks: Vec<BlockView>,
}

pub fn add(ctx: &Ctx, input: &BlockInput) -> Result<TimeBlock> {
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
    let b = TimeBlock {
        id: new_id(),
        start_at: input.start,
        end_at: input.end,
        task_id,
        label: input.label.clone(),
        focus: input.focus,
    };
    plan::insert(ctx.conn, &b)?;
    Ok(b)
}

/// ブロックを削除する。存在しなければ `NotFound`。
pub fn delete(ctx: &Ctx, id: &str) -> Result<()> {
    if plan::delete(ctx.conn, id)? {
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
