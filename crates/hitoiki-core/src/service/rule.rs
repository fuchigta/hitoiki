//! タスクの割り当てルール（`task_assign_rule`）と活動ルール（`activity_rule`）の編集・削除・有効/無効。
//!
//! ルールを変えると派生層（区間の割り当て・活動）が変わりうる。再計算は呼び出し側
//! （CLI はデーモンへの通知、アプリは `request_rebuild`）が行い、過去の日は `hitoiki rebuild` で作り直す。

use super::task::normalize_pattern;
use super::Ctx;
use crate::id::new_id;
use crate::model::{ActivityRule, MatchField, PatternType, TaskAssignRule};
use crate::repo::{self, edit, rule};
use crate::{Error, Result};
use serde::{Deserialize, Serialize};

/// タスクの割り当てルールの編集。`None` は「変更しない」。
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct TaskRuleEdit {
    pub field: Option<MatchField>,
    pub pattern: Option<String>,
    pub pattern_type: Option<PatternType>,
    pub priority: Option<i64>,
    pub enabled: Option<bool>,
}

/// 活動ルールの追加。
#[derive(Debug, Clone, Deserialize)]
pub struct ActivityRuleInput {
    pub field: MatchField,
    pub pattern: String,
    /// 省略時は glob。
    #[serde(default)]
    pub pattern_type: Option<PatternType>,
    pub activity: String,
    #[serde(default)]
    pub priority: i64,
}

/// 活動ルールの編集。`None` は「変更しない」。
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ActivityRuleEdit {
    pub field: Option<MatchField>,
    pub pattern: Option<String>,
    pub pattern_type: Option<PatternType>,
    pub activity: Option<String>,
    pub priority: Option<i64>,
    pub enabled: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ActivityRuleList {
    pub rules: Vec<ActivityRule>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RuleDeleted<T> {
    pub deleted: bool,
    pub dry_run: bool,
    pub rule: T,
}

fn check_label(label: &str) -> Result<String> {
    let l = label.trim();
    if l.is_empty() {
        return Err(Error::InvalidArgument("活動が空です".into()));
    }
    Ok(l.to_string())
}

pub fn task_rule_get(ctx: &Ctx, id: &str) -> Result<TaskAssignRule> {
    edit::get_task_rule(ctx.conn, &repo::resolve_prefix(ctx.conn, "task_assign_rule", id)?)
}

pub fn task_rule_edit(ctx: &Ctx, id: &str, input: &TaskRuleEdit) -> Result<TaskAssignRule> {
    let mut r = task_rule_get(ctx, id)?;
    if let Some(f) = input.field {
        r.field = f;
    }
    if let Some(t) = input.pattern_type {
        r.pattern_type = t;
    }
    if let Some(p) = &input.pattern {
        r.pattern = p.clone();
    }
    r.pattern = normalize_pattern(r.field, r.pattern_type, &r.pattern);
    rule::validate_pattern(&r.pattern, r.pattern_type)?;
    if let Some(p) = input.priority {
        r.priority = p;
    }
    if let Some(e) = input.enabled {
        r.enabled = e;
    }
    edit::update_task_rule(ctx.conn, &r)?;
    Ok(r)
}

pub fn task_rule_delete(ctx: &Ctx, id: &str, dry_run: bool) -> Result<RuleDeleted<TaskAssignRule>> {
    let r = task_rule_get(ctx, id)?;
    if !dry_run {
        edit::delete_task_rule(ctx.conn, &r.id)?;
    }
    Ok(RuleDeleted {
        deleted: !dry_run,
        dry_run,
        rule: r,
    })
}

pub fn activity_rule_add(ctx: &Ctx, input: &ActivityRuleInput) -> Result<ActivityRule> {
    let ty = input.pattern_type.unwrap_or(PatternType::Glob);
    let pattern = normalize_pattern(input.field, ty, &input.pattern);
    rule::validate_pattern(&pattern, ty)?;
    let r = ActivityRule {
        id: new_id(),
        field: input.field,
        pattern,
        pattern_type: ty,
        activity: check_label(&input.activity)?,
        priority: input.priority,
        enabled: true,
    };
    rule::insert_activity_rule(ctx.conn, &r)?;
    Ok(r)
}

pub fn activity_rule_list(ctx: &Ctx) -> Result<ActivityRuleList> {
    Ok(ActivityRuleList {
        rules: rule::list_activity_rules(ctx.conn)?,
    })
}

pub fn activity_rule_get(ctx: &Ctx, id: &str) -> Result<ActivityRule> {
    edit::get_activity_rule(ctx.conn, &repo::resolve_prefix(ctx.conn, "activity_rule", id)?)
}

pub fn activity_rule_edit(ctx: &Ctx, id: &str, input: &ActivityRuleEdit) -> Result<ActivityRule> {
    let mut r = activity_rule_get(ctx, id)?;
    if let Some(f) = input.field {
        r.field = f;
    }
    if let Some(t) = input.pattern_type {
        r.pattern_type = t;
    }
    if let Some(p) = &input.pattern {
        r.pattern = p.clone();
    }
    r.pattern = normalize_pattern(r.field, r.pattern_type, &r.pattern);
    rule::validate_pattern(&r.pattern, r.pattern_type)?;
    if let Some(l) = &input.activity {
        r.activity = check_label(l)?;
    }
    if let Some(p) = input.priority {
        r.priority = p;
    }
    if let Some(e) = input.enabled {
        r.enabled = e;
    }
    edit::update_activity_rule(ctx.conn, &r)?;
    Ok(r)
}

pub fn activity_rule_delete(ctx: &Ctx, id: &str, dry_run: bool) -> Result<RuleDeleted<ActivityRule>> {
    let r = activity_rule_get(ctx, id)?;
    if !dry_run {
        edit::delete_activity_rule(ctx.conn, &r.id)?;
    }
    Ok(RuleDeleted {
        deleted: !dry_run,
        dry_run,
        rule: r,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::task::{self, RuleInput, TaskInput};
    use crate::service::testutil::Env;

    fn rule_for_task(ctx: &Ctx) -> TaskAssignRule {
        let t = task::add(
            ctx,
            &TaskInput {
                title: Some("T".into()),
                ..Default::default()
            },
        )
        .unwrap();
        task::task_rule_add(
            ctx,
            &RuleInput {
                task: t.task.id,
                field: MatchField::Exe,
                pattern: "code*".into(),
                pattern_type: None,
                priority: 0,
            },
        )
        .unwrap()
    }

    #[test]
    fn タスクの割り当てルールの編集と無効化と削除() {
        let env = Env::new();
        let ctx = env.ctx();
        let r = rule_for_task(&ctx);
        let e = task_rule_edit(
            &ctx,
            &r.id,
            &TaskRuleEdit {
                field: Some(MatchField::Cwd),
                pattern: Some("C:\\work\\*".into()),
                priority: Some(5),
                ..Default::default()
            },
        )
        .unwrap();
        // cwd の glob は区切りを正規化する
        assert_eq!(
            (e.field, e.pattern.as_str(), e.priority),
            (MatchField::Cwd, "C:/work/*", 5)
        );
        let e = task_rule_edit(
            &ctx,
            &r.id,
            &TaskRuleEdit {
                enabled: Some(false),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(!e.enabled);
        assert!(!task::task_rule_list(&ctx, None).unwrap().rules[0].enabled);
        // 不正な正規表現は拒否し、元の値を保つ
        assert!(task_rule_edit(
            &ctx,
            &r.id,
            &TaskRuleEdit {
                pattern_type: Some(PatternType::Regex),
                pattern: Some("(".into()),
                ..Default::default()
            }
        )
        .is_err());
        assert_eq!(
            task_rule_get(&ctx, &r.id).unwrap().pattern_type,
            PatternType::Glob
        );
        assert!(task_rule_edit(&ctx, "ZZZ", &TaskRuleEdit::default()).is_err());
        let d = task_rule_delete(&ctx, &r.id, true).unwrap();
        assert!(!d.deleted);
        assert_eq!(task::task_rule_list(&ctx, None).unwrap().rules.len(), 1);
        assert!(task_rule_delete(&ctx, &r.id, false).unwrap().deleted);
        assert!(task::task_rule_list(&ctx, None).unwrap().rules.is_empty());
        assert!(task_rule_delete(&ctx, &r.id, false).is_err());
    }

    #[test]
    fn 活動ルールの追加_編集_削除() {
        let env = Env::new();
        let ctx = env.ctx();
        let r = activity_rule_add(
            &ctx,
            &ActivityRuleInput {
                field: MatchField::Exe,
                pattern: "slack*".into(),
                pattern_type: None,
                activity: " コミュニケーション ".into(),
                priority: 1,
            },
        )
        .unwrap();
        assert_eq!(r.activity, "コミュニケーション");
        assert!(activity_rule_add(
            &ctx,
            &ActivityRuleInput {
                field: MatchField::Exe,
                pattern: "".into(),
                pattern_type: None,
                activity: "x".into(),
                priority: 0,
            }
        )
        .is_err());
        let e = activity_rule_edit(
            &ctx,
            &r.id,
            &ActivityRuleEdit {
                activity: Some("会議".into()),
                enabled: Some(false),
                pattern_type: Some(PatternType::Contains),
                pattern: Some("zoom".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            (e.activity.as_str(), e.enabled, e.pattern.as_str()),
            ("会議", false, "zoom")
        );
        let l = activity_rule_list(&ctx).unwrap();
        assert_eq!(l.rules.len(), 1);
        assert!(activity_rule_edit(
            &ctx,
            &r.id,
            &ActivityRuleEdit {
                activity: Some("".into()),
                ..Default::default()
            }
        )
        .is_err());
        assert!(activity_rule_delete(&ctx, &r.id, false).unwrap().deleted);
        assert!(activity_rule_list(&ctx).unwrap().rules.is_empty());
    }
}
