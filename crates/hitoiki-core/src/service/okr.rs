//! OKR（docs/architecture.md「データモデル」）。

use super::Ctx;
use crate::id::new_id;
use crate::model::{KeyResult, Objective};
use crate::repo::okr;
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
    };
    okr::insert_kr(ctx.conn, &k)?;
    Ok(k)
}

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
