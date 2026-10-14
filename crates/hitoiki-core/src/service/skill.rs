//! スキル（docs/dev/architecture.md「データモデル」）。

use super::impact::{DeleteMode, DeleteOutcome, Impact};
use super::Ctx;
use crate::id::new_id;
use crate::model::Skill;
use crate::repo::edit;
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
        Some(p) => Some(find(ctx, p)?.id),
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

// ===== 編集・削除 =====

/// スキルの編集。`None` は「変更しない」。
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(default)]
pub struct SkillEdit {
    pub name: Option<String>,
    /// 親スキル（ID の前方一致または名前）。
    pub parent: Option<String>,
    /// 親を外してトップレベルにする。
    pub clear_parent: bool,
    /// 空文字で説明を消す。
    pub description: Option<String>,
}

/// 子スキルの扱い。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChildPolicy {
    /// 子を削除するスキルの親に付け替える（親がなければトップレベルになる）。
    #[default]
    Reparent,
    /// 子孫のスキルも一緒に削除する。
    Delete,
}

impl std::str::FromStr for ChildPolicy {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self> {
        match s {
            "reparent" => Ok(Self::Reparent),
            "delete" => Ok(Self::Delete),
            _ => Err(Error::InvalidArgument(format!(
                "子スキルの扱いは reparent か delete です: {s}"
            ))),
        }
    }
}

/// ID の前方一致、または名前でスキルを探す。
pub fn find(ctx: &Ctx, key: &str) -> Result<Skill> {
    if let Some(s) = skill::find_by_name(ctx.conn, key)? {
        return Ok(s);
    }
    skill::resolve(ctx.conn, key)
}

/// `id` の子孫（自身を除く）の ID。
pub(crate) fn descendants(all: &[Skill], id: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut frontier = vec![id.to_string()];
    while let Some(cur) = frontier.pop() {
        for s in all
            .iter()
            .filter(|s| s.parent_id.as_deref() == Some(cur.as_str()))
        {
            if !out.contains(&s.id) {
                out.push(s.id.clone());
                frontier.push(s.id.clone());
            }
        }
    }
    out
}

pub fn edit(ctx: &Ctx, key: &str, input: &SkillEdit) -> Result<Skill> {
    let mut s = find(ctx, key)?;
    let all = skill::list(ctx.conn)?;
    if let Some(n) = &input.name {
        let n = n.trim();
        if n.is_empty() {
            return Err(Error::InvalidArgument("スキル名が空です".into()));
        }
        if let Some(other) = skill::find_by_name(ctx.conn, n)? {
            if other.id != s.id {
                return Err(Error::InvalidArgument(format!("スキル「{n}」は既に存在します")));
            }
        }
        s.name = n.to_string();
    }
    if input.clear_parent {
        s.parent_id = None;
    }
    if let Some(p) = &input.parent {
        let parent = find(ctx, p)?;
        if parent.id == s.id || descendants(&all, &s.id).contains(&parent.id) {
            return Err(Error::InvalidArgument(
                "自分自身か子孫のスキルを親にはできません".into(),
            ));
        }
        s.parent_id = Some(parent.id);
    }
    if let Some(d) = &input.description {
        let d = d.trim();
        s.description = (!d.is_empty()).then(|| d.to_string());
    }
    edit::update_skill(ctx.conn, &s)?;
    Ok(s)
}

/// スキルの削除の影響。`task_skill`（付与済みタスク）と活動ルールの参照は外れる。
pub fn impact(ctx: &Ctx, key: &str, children: ChildPolicy) -> Result<Impact> {
    let s = find(ctx, key)?;
    let all = skill::list(ctx.conn)?;
    let direct = edit::children_of(ctx.conn, &s.id)?.len() as i64;
    let mut targets = vec![s.id.clone()];
    let mut im = Impact::new("skill", &s.id, &s.name);
    match children {
        ChildPolicy::Reparent => {
            im = im.line("子スキル（親を付け替える）", direct);
        }
        ChildPolicy::Delete => {
            let d = descendants(&all, &s.id);
            im = im.line("子孫のスキル（一緒に削除）", d.len() as i64);
            targets.extend(d);
        }
    }
    let mut ts = 0;
    let mut rules = 0;
    let mut krs = 0;
    for t in &targets {
        ts += edit::count_task_skill(ctx.conn, t)?;
        krs += edit::count_kr_skill(ctx.conn, t)?;
        rules += edit::count_activity_skill_rules(ctx.conn, t)?;
    }
    im = im
        .line("スキルが付いたタスク（付与だけ外れる）", ts)
        .line("スキルを付けた成果の指標（付与だけ外れる）", krs)
        .line("活動からスキルを推定するルール（一緒に削除）", rules);
    if s.preset {
        im = im.note("同梱プリセットのスキルです。もう一度取り込むと復活します。");
    }
    Ok(im)
}

pub fn delete(ctx: &Ctx, key: &str, children: ChildPolicy, mode: DeleteMode) -> Result<DeleteOutcome> {
    let im = impact(ctx, key, children)?;
    let go = DeleteOutcome::gate(mode, &im)?;
    if go {
        let all = skill::list(ctx.conn)?;
        let me = all
            .iter()
            .find(|s| s.id == im.id)
            .ok_or_else(|| Error::NotFound(format!("スキル {}", im.name)))?
            .clone();
        let tx = ctx.conn.unchecked_transaction()?;
        match children {
            ChildPolicy::Reparent => {
                for c in edit::children_of(ctx.conn, &me.id)? {
                    edit::set_skill_parent(ctx.conn, &c, me.parent_id.as_deref())?;
                }
                edit::delete_skill_row(ctx.conn, &me.id)?;
            }
            ChildPolicy::Delete => {
                // 子孫から先に消す（親の参照を残さない）。
                let mut ids = descendants(&all, &me.id);
                ids.reverse();
                ids.push(me.id.clone());
                for id in ids {
                    edit::delete_skill_row(ctx.conn, &id)?;
                }
            }
        }
        tx.commit()?;
    }
    Ok(DeleteOutcome::new(mode, im, go))
}

#[cfg(test)]
mod edit_tests {
    use super::*;
    use crate::service::task::{self, TaskInput};
    use crate::service::testutil::Env;

    fn mk(ctx: &Ctx, name: &str, parent: Option<&str>) -> Skill {
        add(
            ctx,
            &SkillInput {
                name: name.into(),
                parent: parent.map(String::from),
            },
        )
        .unwrap()
    }

    #[test]
    fn 名前_親_説明を編集できる() {
        let env = Env::new();
        let ctx = env.ctx();
        let a = mk(&ctx, "A", None);
        let b = mk(&ctx, "B", Some("A"));
        let e = edit(
            &ctx,
            "B",
            &SkillEdit {
                name: Some("B2".into()),
                description: Some("メモ".into()),
                clear_parent: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            (e.name.as_str(), e.parent_id.clone(), e.description.as_deref()),
            ("B2", None, Some("メモ"))
        );
        let e = edit(
            &ctx,
            &b.id,
            &SkillEdit {
                parent: Some(a.id.clone()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(e.parent_id.as_deref(), Some(a.id.as_str()));
        // 重複名、空、循環
        let c = mk(&ctx, "C", Some("B2"));
        assert!(edit(
            &ctx,
            "B2",
            &SkillEdit {
                name: Some("A".into()),
                ..Default::default()
            }
        )
        .is_err());
        assert!(edit(
            &ctx,
            "B2",
            &SkillEdit {
                name: Some(" ".into()),
                ..Default::default()
            }
        )
        .is_err());
        assert!(edit(
            &ctx,
            "A",
            &SkillEdit {
                parent: Some(c.id.clone()),
                ..Default::default()
            }
        )
        .is_err());
        assert!(edit(
            &ctx,
            "A",
            &SkillEdit {
                parent: Some("A".into()),
                ..Default::default()
            }
        )
        .is_err());
        // 自分の名前への「変更」は許す
        assert!(edit(
            &ctx,
            "A",
            &SkillEdit {
                name: Some("A".into()),
                ..Default::default()
            }
        )
        .is_ok());
    }

    #[test]
    fn 削除で子を付け替える() {
        let env = Env::new();
        let ctx = env.ctx();
        let a = mk(&ctx, "A", None);
        mk(&ctx, "B", Some("A"));
        mk(&ctx, "C", Some("B"));
        let t = task::add(
            &ctx,
            &TaskInput {
                title: Some("T".into()),
                skills: vec!["B".into()],
                ..Default::default()
            },
        )
        .unwrap();
        let im = impact(&ctx, "B", ChildPolicy::Reparent).unwrap();
        assert_eq!(im.total(), 2);
        assert!(delete(&ctx, "B", ChildPolicy::Reparent, DeleteMode::default()).is_err());
        delete(&ctx, "B", ChildPolicy::Reparent, DeleteMode::FORCE).unwrap();
        let l = list(&ctx).unwrap();
        assert_eq!(l.skills.len(), 2);
        let c = l.skills.iter().find(|s| s.skill.name == "C").unwrap();
        assert_eq!(c.skill.parent_id.as_deref(), Some(a.id.as_str()));
        assert!(task::show(&ctx, &t.task.id).unwrap().task.skills.is_empty());
    }

    #[test]
    fn 削除で子孫も消す() {
        let env = Env::new();
        let ctx = env.ctx();
        mk(&ctx, "A", None);
        mk(&ctx, "B", Some("A"));
        mk(&ctx, "C", Some("B"));
        mk(&ctx, "D", None);
        let t = task::add(
            &ctx,
            &TaskInput {
                title: Some("T".into()),
                skills: vec!["C".into(), "D".into()],
                ..Default::default()
            },
        )
        .unwrap();
        ctx.conn
            .execute(
                "INSERT INTO activity_skill_rule (id, field, pattern, pattern_type, skill_id) \
                 SELECT 'R1', 'exe', 'x', 'contains', id FROM skill WHERE name = 'C'",
                [],
            )
            .unwrap();
        let im = impact(&ctx, "A", ChildPolicy::Delete).unwrap();
        assert_eq!(im.total(), 2 + 1 + 1);
        delete(&ctx, "A", ChildPolicy::Delete, DeleteMode::FORCE).unwrap();
        let names: Vec<_> = list(&ctx)
            .unwrap()
            .skills
            .into_iter()
            .map(|s| s.skill.name)
            .collect();
        assert_eq!(names, ["D"]);
        assert_eq!(task::show(&ctx, &t.task.id).unwrap().task.skills, ["D"]);
        let n: i64 = ctx
            .conn
            .query_row("SELECT COUNT(*) FROM activity_skill_rule", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn プリセットの取り込み済みスキルも編集_削除できる() {
        let env = Env::new();
        let ctx = env.ctx();
        import_preset(&ctx, None).unwrap();
        edit(
            &ctx,
            "Rust",
            &SkillEdit {
                name: Some("Rust 言語".into()),
                ..Default::default()
            },
        )
        .unwrap();
        let im = impact(&ctx, "プログラミング言語", ChildPolicy::Reparent).unwrap();
        assert!(im.note.is_some());
        delete(
            &ctx,
            "プログラミング言語",
            ChildPolicy::Reparent,
            DeleteMode::FORCE,
        )
        .unwrap();
        let rust = list(&ctx)
            .unwrap()
            .skills
            .into_iter()
            .find(|s| s.skill.name == "Rust 言語")
            .unwrap();
        assert!(rust.skill.parent_id.is_none());
        // 再取り込みで消したプリセットだけ戻る
        let r = import_preset(&ctx, None).unwrap();
        assert!(r.imported.contains(&"プログラミング言語".to_string()));
    }
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
