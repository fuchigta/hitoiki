//! スキル（10 章）。

use super::Ctx;
use crate::id::new_id;
use crate::model::Skill;
use crate::repo::skill::{self, PRESETS};
use crate::{Error, Result};
use serde::Serialize;

#[derive(Debug, Clone)]
pub struct SkillInput {
    pub name: String,
    /// 親スキルの ID（前方一致）。
    pub parent: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SkillView {
    #[serde(flatten)]
    pub skill: Skill,
    pub parent_name: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SkillList {
    pub skills: Vec<SkillView>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PresetImport {
    pub imported: Vec<String>,
    /// 既に存在したため取り込まなかったもの。
    pub skipped: Vec<String>,
}

pub fn add(ctx: &Ctx, input: &SkillInput) -> Result<Skill> {
    let name = input.name.trim();
    if name.is_empty() {
        return Err(Error::InvalidArgument("スキル名が空です".into()));
    }
    if skill::find_by_name(ctx.conn, name)?.is_some() {
        return Err(Error::InvalidArgument(format!(
            "スキル「{name}」は既に存在します"
        )));
    }
    let parent_id = match &input.parent {
        Some(p) => Some(skill::resolve(ctx.conn, p)?.id),
        None => None,
    };
    let s = Skill {
        id: new_id(),
        name: name.to_string(),
        parent_id,
        description: None,
        preset: false,
    };
    skill::insert(ctx.conn, &s)?;
    Ok(s)
}

pub fn list(ctx: &Ctx) -> Result<SkillList> {
    let all = skill::list(ctx.conn)?;
    let skills = all
        .iter()
        .cloned()
        .map(|s| {
            let parent_name = s
                .parent_id
                .as_ref()
                .and_then(|p| all.iter().find(|x| &x.id == p))
                .map(|x| x.name.clone());
            SkillView {
                skill: s,
                parent_name,
            }
        })
        .collect();
    Ok(SkillList { skills })
}

/// プリセットの取り込み。`name` 省略（または `default`）で全部、指定時はそのスキルと親だけ。
/// 既存の同名スキルは変更しない（冪等）。
pub fn import_preset(ctx: &Ctx, name: Option<&str>) -> Result<PresetImport> {
    let wanted: Vec<&(&str, Option<&str>)> = match name {
        None | Some("default") => PRESETS.iter().collect(),
        Some(n) => {
            let Some(entry) = PRESETS.iter().find(|(pn, _)| *pn == n) else {
                return Err(Error::NotFound(format!(
                    "プリセット「{n}」(候補: {})",
                    PRESETS.iter().map(|(n, _)| *n).collect::<Vec<_>>().join(", ")
                )));
            };
            PRESETS
                .iter()
                .filter(|(pn, _)| *pn == n || Some(*pn) == entry.1)
                .collect()
        }
    };
    let tx = ctx.conn.unchecked_transaction()?;
    let mut out = PresetImport {
        imported: vec![],
        skipped: vec![],
    };
    for (pname, parent) in wanted {
        if skill::find_by_name(ctx.conn, pname)?.is_some() {
            out.skipped.push((*pname).to_string());
            continue;
        }
        let parent_id = match parent {
            Some(p) => skill::find_by_name(ctx.conn, p)?.map(|s| s.id),
            None => None,
        };
        skill::insert(
            ctx.conn,
            &Skill {
                id: new_id(),
                name: (*pname).to_string(),
                parent_id,
                description: None,
                preset: true,
            },
        )?;
        out.imported.push((*pname).to_string());
    }
    tx.commit()?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::testutil::Env;

    #[test]
    fn preset_is_idempotent_with_parents() {
        let env = Env::new();
        let ctx = env.ctx();
        let r = import_preset(&ctx, None).unwrap();
        assert_eq!(r.imported.len(), PRESETS.len());
        let r2 = import_preset(&ctx, None).unwrap();
        assert!(r2.imported.is_empty());
        let l = list(&ctx).unwrap();
        let rust = l.skills.iter().find(|s| s.skill.name == "Rust").unwrap();
        assert_eq!(rust.parent_name.as_deref(), Some("プログラミング言語"));
        assert!(rust.skill.preset);
    }

    #[test]
    fn single_preset_brings_parent() {
        let env = Env::new();
        let ctx = env.ctx();
        let r = import_preset(&ctx, Some("Go")).unwrap();
        assert_eq!(r.imported, ["プログラミング言語", "Go"]);
        assert!(import_preset(&ctx, Some("nope")).is_err());
    }

    #[test]
    fn add_rejects_duplicates() {
        let env = Env::new();
        let ctx = env.ctx();
        add(
            &ctx,
            &SkillInput {
                name: "X".into(),
                parent: None,
            },
        )
        .unwrap();
        assert!(add(
            &ctx,
            &SkillInput {
                name: "X".into(),
                parent: None
            }
        )
        .is_err());
    }
}
