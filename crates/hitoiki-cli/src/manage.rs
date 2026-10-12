//! 編集・削除系コマンド（OKR・スキル・ルール・記録・カレンダーソース・活動ラベル）。
//! ロジックは core の service 層にあり、ここは引数の解釈と出力整形だけを行う。

use crate::args::*;
use crate::table::table;
use crate::{emit, ext, range_args, render};
use hitoiki_core::config::Config;
use hitoiki_core::service::impact::{DeleteMode, DeleteOutcome};
use hitoiki_core::service::{okr, okr_progress, parse_to, record, rule, settings, skill, Ctx};
use hitoiki_core::timefmt::{self, to_local};
use hitoiki_core::{paths, Error, Result};

fn mode(o: &DeleteOpts) -> DeleteMode {
    DeleteMode {
        dry_run: o.dry_run,
        force: o.force,
    }
}

/// 削除結果の表示。dry_run なら影響だけ見せる。
fn delete_report(json: bool, what: &str, o: &DeleteOutcome) -> Result<()> {
    emit(json, o, || {
        let mut s = String::new();
        if o.dry_run {
            s += &format!("{what}「{}」を削除すると:", o.impact.name);
            if o.impact.lines.is_empty() {
                s += " 影響はありません";
            }
            for l in &o.impact.lines {
                s += &format!("\n  - {} {} 件", l.label, l.count);
            }
            s += "\n（--dry-run のため削除していません）";
        } else {
            s += &format!("{what}を削除しました: {}", o.impact.name);
            for l in &o.impact.lines {
                s += &format!("\n  - {} {} 件", l.label, l.count);
            }
        }
        if let Some(n) = &o.impact.note {
            s += &format!("\n  ※ {n}");
        }
        s
    })
}

/// 0 は「消す」。
fn scale(v: Option<i64>, cur: Option<i64>) -> Option<i64> {
    match v {
        Some(0) => None,
        Some(n) => Some(n),
        None => cur,
    }
}

pub fn check_sub(ctx: &Ctx, json: bool, cmd: CheckCmd) -> Result<()> {
    let tz = ctx.tz()?;
    match cmd {
        CheckCmd::List { range } => {
            let l = record::list_checkins(ctx, &range_args(&range))?;
            emit(json, &l, || {
                let rows: Vec<Vec<String>> = l
                    .items
                    .iter()
                    .map(|c| {
                        let f = |n: Option<i64>| n.map(|v| v.to_string()).unwrap_or_else(|| "-".into());
                        vec![
                            render::short(&c.id),
                            to_local(tz, c.at).format("%m-%d %H:%M").to_string(),
                            f(c.energy),
                            f(c.mood),
                            f(c.stress),
                            c.note.clone().unwrap_or_default(),
                        ]
                    })
                    .collect();
                table(&["ID", "時刻", "エネルギー", "気分", "ストレス", "メモ"], &rows)
            })
        }
        CheckCmd::Edit {
            id,
            energy,
            mood,
            stress,
            erg,
            note,
        } => {
            let cur = record::check_get(ctx, &id)?;
            let input = record::CheckInput {
                energy: scale(energy, cur.energy),
                mood: scale(mood, cur.mood),
                stress: scale(stress, cur.stress),
                // 指定したときだけユーザーの確定にする（AI の判定を勝手にユーザーのものにしない）。
                erg,
                note: note.or(cur.note),
            };
            let c = record::check_edit(ctx, &cur.id, &input)?;
            ext::notify_data("checkin");
            emit(json, &c, || {
                format!("チェックインを更新しました: {}", render::short(&c.id))
            })
        }
        CheckCmd::Delete { id, opts } => {
            let o = record::check_delete(ctx, &id, mode(&opts))?;
            ext::notify_data("checkin");
            delete_report(json, "チェックイン", &o)
        }
    }
}

pub fn hassle_sub(ctx: &Ctx, json: bool, cmd: HassleCmd) -> Result<()> {
    let tz = ctx.tz()?;
    match cmd {
        HassleCmd::List { range } => {
            let l = record::list_hassles(ctx, &range_args(&range))?;
            emit(json, &l, || {
                let rows: Vec<Vec<String>> = l
                    .items
                    .iter()
                    .map(|h| {
                        vec![
                            render::short(&h.id),
                            to_local(tz, h.at).format("%m-%d %H:%M").to_string(),
                            h.intensity.to_string(),
                            h.text.clone(),
                        ]
                    })
                    .collect();
                table(&["ID", "時刻", "強度", "内容"], &rows)
            })
        }
        HassleCmd::Edit { id, text, intensity } => {
            let cur = record::hassle_get(ctx, &id)?;
            let h = record::hassle_edit(
                ctx,
                &cur.id,
                &record::HassleInput {
                    text: text.unwrap_or(cur.text),
                    intensity,
                },
            )?;
            ext::notify_data("hassle");
            emit(json, &h, || {
                format!("ハッスルを更新しました: {}", render::short(&h.id))
            })
        }
        HassleCmd::Delete { id, opts } => {
            let o = record::hassle_delete(ctx, &id, mode(&opts))?;
            ext::notify_data("hassle");
            delete_report(json, "ハッスル", &o)
        }
    }
}

pub fn abc_sub(ctx: &Ctx, json: bool, cmd: AbcCmd) -> Result<()> {
    let tz = ctx.tz()?;
    match cmd {
        AbcCmd::List { range } => {
            let l = record::list_abc(ctx, &range_args(&range))?;
            emit(json, &l, || {
                let rows: Vec<Vec<String>> = l
                    .items
                    .iter()
                    .map(|a| {
                        vec![
                            render::short(&a.id),
                            to_local(tz, a.at).format("%m-%d %H:%M").to_string(),
                            a.activating_event.clone(),
                            a.belief.clone().unwrap_or_default(),
                            a.consequence_emotion.clone().unwrap_or_default(),
                        ]
                    })
                    .collect();
                table(&["ID", "時刻", "A", "B", "C"], &rows)
            })
        }
        AbcCmd::Edit {
            id,
            a,
            b,
            c_emotion,
            c_intensity,
            dispute,
        } => {
            let cur = record::abc_get(ctx, &id)?;
            let r = record::abc_edit(
                ctx,
                &cur.id,
                &record::AbcEditInput {
                    activating_event: a.unwrap_or(cur.activating_event),
                    belief: b.or(cur.belief),
                    consequence_emotion: c_emotion.or(cur.consequence_emotion),
                    consequence_intensity: scale(c_intensity, cur.consequence_intensity),
                    dispute: dispute.or(cur.dispute),
                },
            )?;
            ext::notify_data("abc");
            emit(json, &r, || {
                format!("ABC を更新しました: {}", render::short(&r.id))
            })
        }
        AbcCmd::Delete { id, opts } => {
            let o = record::abc_delete(ctx, &id, mode(&opts))?;
            ext::notify_data("abc");
            delete_report(json, "ABC 記録", &o)
        }
        AbcCmd::Add { .. } | AbcCmd::Dispute { .. } => unreachable!("handled in main"),
    }
}

pub fn okr_sub(ctx: &Ctx, json: bool, cmd: OkrCmd) -> Result<()> {
    let tz = ctx.tz()?;
    let now = ctx.now();
    match cmd {
        OkrCmd::EditObjective {
            id,
            title,
            description,
            from,
            to,
            clear_period,
            status,
        } => {
            let o = okr::edit_objective(
                ctx,
                &id,
                &okr::ObjectiveEdit {
                    title,
                    description,
                    from: from.map(|s| timefmt::parse_instant(tz, &s, now)).transpose()?,
                    to: to.map(|s| parse_to(tz, &s, now)).transpose()?,
                    clear_period,
                    status,
                },
            )?;
            emit(json, &o, || {
                format!("Objective を更新しました: {} {}", render::short(&o.id), o.title)
            })
        }
        OkrCmd::DeleteObjective { id, opts } => {
            let o = okr::delete_objective(ctx, &id, mode(&opts))?;
            delete_report(json, "Objective", &o)
        }
        OkrCmd::EditKr {
            id,
            title,
            target,
            current,
            note,
            status,
            objective,
            measure,
            day_min,
        } => {
            let k = okr::edit_kr(
                ctx,
                &id,
                &okr::KrEdit {
                    title,
                    target,
                    current,
                    current_note: note,
                    status,
                    objective,
                    measure,
                    day_min,
                },
            )?;
            emit(json, &k, || {
                format!("KR を更新しました: {} {}", render::short(&k.id), k.title)
            })
        }
        OkrCmd::DeleteKr { id, opts } => {
            let o = okr::delete_kr(ctx, &id, mode(&opts))?;
            delete_report(json, "KR", &o)
        }
        OkrCmd::Link { kr, tasks } => {
            let r = okr::link_tasks(ctx, &kr, &tasks)?;
            ext::notify_task(None);
            emit(json, &r, || render::kr_linked(&r))
        }
        OkrCmd::Unlink { tasks } => {
            let r = okr::unlink_tasks(ctx, &tasks)?;
            ext::notify_task(None);
            emit(json, &serde_json::json!({ "unlinked": r }), || {
                render::kr_unlinked(&r)
            })
        }
        OkrCmd::Progress { kr, value, note } => {
            let r = okr_progress::record(ctx, &kr, &value, note.as_deref())?;
            emit(json, &r, || render::kr_progress_recorded(tz, &r))
        }
        OkrCmd::History { kr, range } => {
            let args = range_args(&range);
            let given = (args.date.is_some() || args.from.is_some() || args.to.is_some())
                .then(|| ctx.range(&args))
                .transpose()?;
            let h = okr_progress::kr_history(ctx, &kr, given)?;
            emit(json, &h, || render::kr_history(tz, &h))
        }
        OkrCmd::Show { kr, range } => {
            let args = range_args(&range);
            let given = (args.date.is_some() || args.from.is_some() || args.to.is_some())
                .then(|| ctx.range(&args))
                .transpose()?;
            let s = okr::kr_summary(ctx, &kr, given)?;
            emit(json, &s, || render::kr_summary(&s))
        }
        _ => unreachable!("handled in main"),
    }
}

pub fn skill_sub(ctx: &Ctx, json: bool, cmd: SkillCmd) -> Result<()> {
    match cmd {
        SkillCmd::Edit {
            skill: key,
            name,
            parent,
            no_parent,
            description,
        } => {
            let s = skill::edit(
                ctx,
                &key,
                &skill::SkillEdit {
                    name,
                    parent,
                    clear_parent: no_parent,
                    description,
                },
            )?;
            emit(json, &s, || {
                format!("スキルを更新しました: {} {}", render::short(&s.id), s.name)
            })
        }
        SkillCmd::Delete {
            skill: key,
            children,
            opts,
        } => {
            let o = skill::delete(ctx, &key, children.parse()?, mode(&opts))?;
            delete_report(json, "スキル", &o)
        }
        _ => unreachable!("handled in main"),
    }
}

fn enabled(enable: bool, disable: bool) -> Option<bool> {
    if enable {
        Some(true)
    } else if disable {
        Some(false)
    } else {
        None
    }
}

pub fn task_rule_sub(ctx: &Ctx, json: bool, cmd: RuleCmd) -> Result<()> {
    match cmd {
        RuleCmd::Edit {
            rule: id,
            field,
            pattern,
            pattern_type,
            priority,
            enable,
            disable,
        } => {
            let r = rule::task_rule_edit(
                ctx,
                &id,
                &rule::TaskRuleEdit {
                    field,
                    pattern,
                    pattern_type,
                    priority,
                    enabled: enabled(enable, disable),
                },
            )?;
            ext::notify_data("segments");
            emit(json, &r, || {
                format!(
                    "ルールを更新しました: {} {} {} ({}){}",
                    render::short(&r.id),
                    r.field,
                    r.pattern,
                    r.pattern_type,
                    if r.enabled { "" } else { " [無効]" }
                )
            })
        }
        RuleCmd::Delete { rule: id, dry_run } => {
            let d = rule::task_rule_delete(ctx, &id, dry_run)?;
            ext::notify_data("segments");
            emit(json, &d, || {
                format!(
                    "ルールを{}: {} {} {}",
                    if dry_run {
                        "削除対象です（--dry-run のため削除していません）"
                    } else {
                        "削除しました"
                    },
                    render::short(&d.rule.id),
                    d.rule.field,
                    d.rule.pattern
                )
            })
        }
        RuleCmd::Add { .. } | RuleCmd::List { .. } => unreachable!("handled in main"),
    }
}

fn activity_rules_table(rules: &[hitoiki_core::model::ActivityRule]) -> String {
    if rules.is_empty() {
        return "活動ルールはありません。".into();
    }
    let rows: Vec<Vec<String>> = rules
        .iter()
        .map(|r| {
            vec![
                render::short(&r.id),
                r.field.to_string(),
                r.pattern_type.to_string(),
                r.pattern.clone(),
                r.activity_label.clone(),
                r.priority.to_string(),
                if r.enabled { "有効" } else { "無効" }.into(),
            ]
        })
        .collect();
    table(
        &["ID", "対象", "種類", "パターン", "活動", "優先度", "状態"],
        &rows,
    )
}

pub fn activity_rule_sub(ctx: &Ctx, json: bool, cmd: ActivityRuleCmd) -> Result<()> {
    match cmd {
        ActivityRuleCmd::List => {
            let l = rule::activity_rule_list(ctx)?;
            emit(json, &l, || activity_rules_table(&l.rules))
        }
        ActivityRuleCmd::Add {
            field,
            pattern,
            pattern_type,
            label,
            priority,
        } => {
            let r = rule::activity_rule_add(
                ctx,
                &rule::ActivityRuleInput {
                    field,
                    pattern,
                    pattern_type,
                    activity_label: label,
                    priority,
                },
            )?;
            ext::notify_data("segments");
            emit(json, &r, || {
                format!("活動ルールを追加しました: {}", render::short(&r.id))
            })
        }
        ActivityRuleCmd::Edit {
            rule: id,
            field,
            pattern,
            pattern_type,
            label,
            priority,
            enable,
            disable,
        } => {
            let r = rule::activity_rule_edit(
                ctx,
                &id,
                &rule::ActivityRuleEdit {
                    field,
                    pattern,
                    pattern_type,
                    activity_label: label,
                    priority,
                    enabled: enabled(enable, disable),
                },
            )?;
            ext::notify_data("segments");
            emit(json, &r, || {
                format!("活動ルールを更新しました: {}", render::short(&r.id))
            })
        }
        ActivityRuleCmd::Delete { rule: id, dry_run } => {
            let d = rule::activity_rule_delete(ctx, &id, dry_run)?;
            ext::notify_data("segments");
            emit(json, &d, || {
                format!(
                    "活動ルールを{}: {}",
                    if dry_run {
                        "削除対象です（--dry-run のため削除していません）"
                    } else {
                        "削除しました"
                    },
                    render::short(&d.rule.id)
                )
            })
        }
    }
}

pub fn label_sub(ctx: &Ctx, json: bool, cmd: LabelCmd) -> Result<()> {
    let path = paths::config_file();
    let show = |l: &settings::LabelList| l.labels.join("\n");
    match cmd {
        LabelCmd::List => {
            let l = settings::labels_list(&Config::load()?);
            emit(json, &l, || show(&l))
        }
        LabelCmd::Add { label } => {
            let l = settings::labels_add(&path, &label)?;
            emit(json, &l, || show(&l))
        }
        LabelCmd::Rename { from, to } => {
            let l = settings::labels_rename(ctx.conn, &path, &from, &to)?;
            ext::notify_data("segments");
            emit(json, &l, || show(&l))
        }
        LabelCmd::Remove { label } => {
            let l = settings::labels_remove(&path, &label)?;
            emit(json, &l, || show(&l))
        }
    }
}

pub fn calendar_sub(ctx: &Ctx, json: bool, cmd: CalendarCmd) -> Result<()> {
    let path = paths::config_file();
    match cmd {
        CalendarCmd::List => {
            let l = settings::calendar_list(&Config::load()?);
            emit(json, &l, || {
                if l.sources.is_empty() {
                    return "カレンダーソースはありません。".into();
                }
                let rows: Vec<Vec<String>> = l
                    .sources
                    .iter()
                    .map(|s| vec![s.name.clone(), s.kind.clone(), s.host.clone()])
                    .collect();
                table(&["名前", "種類", "ホスト"], &rows)
            })
        }
        CalendarCmd::Add { name, url } => {
            let v = settings::calendar_add(&path, &name, &url)?;
            emit(json, &v, || format!("カレンダーソースを追加しました: {}", v.name))
        }
        CalendarCmd::Edit { name, new_name, url } => {
            if new_name.is_none() && url.is_none() {
                return Err(Error::InvalidArgument(
                    "--new-name か --url を指定してください".into(),
                ));
            }
            let v = settings::calendar_edit(ctx.conn, &path, &name, new_name.as_deref(), url.as_deref())?;
            emit(json, &v, || format!("カレンダーソースを更新しました: {}", v.name))
        }
        CalendarCmd::Delete { name, opts } => {
            let o = settings::calendar_delete(ctx.conn, &path, &name, mode(&opts))?;
            delete_report(json, "カレンダーソース", &o)
        }
    }
}
