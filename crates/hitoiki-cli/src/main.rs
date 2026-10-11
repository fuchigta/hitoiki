//! `hitoiki` CLI。ロジックは hitoiki-core の service 層にあり、ここは引数の解釈と出力整形だけを行う。

mod args;
mod ext;
mod native_host;
mod render;
mod table;

use args::*;
use clap::Parser;
use hitoiki_core::config::{self, Config};
use hitoiki_core::model::{ReflectionKind, TaskStatus};
use hitoiki_core::service::block_template as bt;
use hitoiki_core::service::{
    self, context, correction, log as log_svc, okr, plan, quick, record, reflect, report, review, skill,
    status, task, Ctx, RangeArgs,
};
use hitoiki_core::{db, paths, timefmt, Result, SystemClock};
use serde::Serialize;
use std::io::Read;
use std::process::ExitCode;

fn main() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(c) => c,
        Err(e) => {
            // --help は 0、引数エラーは 2（clap の既定）。
            let code = e.exit_code();
            let _ = e.print();
            return ExitCode::from(code as u8);
        }
    };
    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("エラー: {e}");
            ExitCode::from(e.exit_code() as u8)
        }
    }
}

/// JSON を `{"version": 1, ...}` の形で出力する。
fn print_json<T: Serialize>(v: &T) -> Result<()> {
    let mut value = serde_json::to_value(v).map_err(anyhow::Error::from)?;
    match value.as_object_mut() {
        Some(obj) => {
            obj.insert("version".into(), service::SCHEMA_VERSION.into());
        }
        None => {
            value = serde_json::json!({"version": service::SCHEMA_VERSION, "data": value});
        }
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&value).map_err(anyhow::Error::from)?
    );
    Ok(())
}

/// `--json` なら JSON、そうでなければ人間向けの文字列を出力する。
fn emit<T: Serialize>(json: bool, v: &T, human: impl FnOnce() -> String) -> Result<()> {
    if json {
        print_json(v)
    } else {
        println!("{}", human());
        Ok(())
    }
}

/// `-` なら標準入力から読む。
fn read_text(src: &str) -> Result<String> {
    if src == "-" {
        let mut s = String::new();
        std::io::stdin().read_to_string(&mut s)?;
        Ok(s)
    } else {
        Ok(src.to_string())
    }
}

fn range_args(r: &RangeOpts) -> RangeArgs {
    RangeArgs {
        date: r.date.clone(),
        from: r.from.clone(),
        to: r.to.clone(),
    }
}

fn template_input(
    name: String,
    description: Option<String>,
    weekdays: Option<&str>,
    blocks: &[String],
) -> Result<bt::TemplateInput> {
    Ok(bt::TemplateInput {
        name,
        description,
        weekdays: match weekdays {
            Some(w) => bt::parse_weekdays(w)?,
            None => vec![],
        },
        items: blocks
            .iter()
            .map(|b| bt::ItemInput::parse_spec(b))
            .collect::<Result<_>>()?,
    })
}

fn template_cmd(ctx: &Ctx, tz: chrono_tz::Tz, json: bool, cmd: TemplateCmd) -> Result<()> {
    match cmd {
        TemplateCmd::Add {
            name,
            description,
            weekdays,
            blocks,
        } => {
            let t = bt::add(
                ctx,
                &template_input(name, description, weekdays.as_deref(), &blocks)?,
            )?;
            ext::notify_data("blocks");
            emit(json, &t, || render::template_saved(&t, "作成"))
        }
        TemplateCmd::List => {
            let l = bt::list(ctx)?;
            emit(json, &l, || render::template_list(&l))
        }
        TemplateCmd::Show { name } => {
            let t = bt::show(ctx, &name)?;
            emit(json, &t, || render::template_show(&t))
        }
        TemplateCmd::Edit {
            name,
            new_name,
            description,
            weekdays,
            blocks,
        } => {
            let cur = bt::show(ctx, &name)?;
            let mut input = bt::TemplateInput {
                name: new_name.unwrap_or(cur.name),
                description: description.or(cur.description),
                weekdays: match weekdays {
                    Some(w) => bt::parse_weekdays(&w)?,
                    None => cur.weekdays,
                },
                items: cur
                    .blocks
                    .into_iter()
                    .map(|b| bt::ItemInput {
                        start: b.start,
                        end: b.end,
                        label: b.label,
                        focus: b.focus,
                        task: b.task_id,
                    })
                    .collect(),
            };
            if !blocks.is_empty() {
                input.items = blocks
                    .iter()
                    .map(|b| bt::ItemInput::parse_spec(b))
                    .collect::<Result<_>>()?;
            }
            let t = bt::edit(ctx, &cur.id, &input)?;
            ext::notify_data("blocks");
            emit(json, &t, || render::template_saved(&t, "更新"))
        }
        TemplateCmd::Delete { name } => {
            let t = bt::delete(ctx, &name)?;
            ext::notify_data("blocks");
            emit(json, &t, || format!("テンプレートを削除しました: {}", t.name))
        }
        TemplateCmd::Apply {
            name,
            date,
            overlap,
            dry_run,
        } => {
            let input = bt::ApplyInput {
                template: name,
                date,
                overlap: overlap.parse()?,
            };
            if dry_run {
                let p = bt::preview(ctx, &input)?;
                emit(json, &p, || render::template_preview(tz, &p))
            } else {
                let r = bt::apply(ctx, &input)?;
                ext::notify_data("blocks");
                emit(json, &r, || render::template_applied(tz, &r))
            }
        }
        TemplateCmd::Undo { date } => {
            let u = bt::undo(ctx, date.as_deref())?;
            ext::notify_data("blocks");
            emit(json, &u, || render::template_undone(&u))
        }
        TemplateCmd::SaveFrom {
            date,
            name,
            description,
            weekdays,
        } => {
            let t = bt::save_from(
                ctx,
                &bt::SaveFromInput {
                    name,
                    description,
                    date,
                    weekdays: match weekdays {
                        Some(w) => bt::parse_weekdays(&w)?,
                        None => vec![],
                    },
                },
            )?;
            ext::notify_data("blocks");
            emit(json, &t, || render::template_saved(&t, "作成"))
        }
        TemplateCmd::Suggest { date } => {
            let s = bt::suggest(ctx, date.as_deref())?;
            emit(json, &s, || render::template_suggest(&s))
        }
    }
}

fn run(cli: Cli) -> Result<()> {
    let json = cli.json;
    // DB を使わないコマンド
    match &cli.cmd {
        Cmd::Config(c) => return config_cmd(c, json),
        Cmd::Version { check } => {
            if *check {
                return ext::version_check(json);
            }
            println!("hitoiki {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        Cmd::Rebuild { date, from, to } => return ext::rebuild(from.clone(), to.clone(), date.clone(), json),
        Cmd::Daemon => return ext::daemon(),
        Cmd::Doctor { network } => return ext::doctor(*network, json),
        Cmd::Ui { screen } => return ext::ui(screen.clone(), json),
        Cmd::Ingest(IngestCmd::Context) => return ext::ingest_context(json),
        Cmd::NativeHost(_) | Cmd::Agent(_) => {}
        _ => {}
    }

    // 自前で DB を開くコマンド
    let cmd = match cli.cmd {
        Cmd::NativeHost(a) => return ext::native_host(a, json),
        Cmd::Agent(a) => return ext::agent(a, json),
        other => other,
    };

    let cfg = Config::load()?;
    let conn = db::open(&paths::db_file())?;
    let clock = SystemClock;
    let ctx = Ctx::new(&conn, &cfg, &clock);
    let tz = ctx.tz()?;

    match cmd {
        Cmd::Status => {
            let st = status::status(&ctx)?;
            emit(json, &st, || render::status(tz, &st))
        }
        Cmd::Task(t) => task_cmd(&ctx, t, json),
        Cmd::Log { range, raw } => {
            let l = log_svc::log(
                &ctx,
                &log_svc::LogInput {
                    range: range_args(&range),
                    raw,
                },
            )?;
            emit(json, &l, || render::log(tz, &l))
        }
        Cmd::Review(r) => match r {
            ReviewCmd::List { range, all } => {
                let l = review::list_with(&ctx, &range_args(&range), all)?;
                emit(json, &l, || render::review_list(tz, &l))
            }
            ReviewCmd::Set {
                mut segments,
                task,
                kind,
                activity,
                rule,
                from,
                to,
                no_task,
                rest,
                include_confirmed,
                include_meeting,
                include_idle,
                exclude,
                dry_run,
            } => {
                if let (Some(from), Some(to)) = (from, to) {
                    // 範囲指定の付け替え（実績の補正）。
                    let range = ctx.range(&RangeArgs {
                        date: None,
                        from: Some(from),
                        to: Some(to),
                    })?;
                    let flag = |b: bool| b.then_some(true);
                    let input = correction::CorrectionInput {
                        start_at: range.start,
                        end_at: range.end,
                        task,
                        no_task,
                        rest,
                        activity,
                        selected: None,
                        include_confirmed: flag(include_confirmed),
                        include_meeting: flag(include_meeting),
                        include_idle: flag(include_idle),
                        exclude,
                    };
                    if dry_run {
                        let p = correction::preview(&ctx, &input)?;
                        emit(json, &p, || render::correction_preview(tz, &p))
                    } else {
                        let r = correction::apply(&ctx, &input)?;
                        ext::notify_data("segments");
                        emit(json, &r, || render::correction_applied(tz, &r))
                    }
                } else if segments.len() == 1 {
                    let r = review::set(
                        &ctx,
                        &review::ReviewSetInput {
                            segment: segments.remove(0),
                            task,
                            kind,
                            activity,
                            rule,
                        },
                    )?;
                    ext::notify_data("segments");
                    emit(json, &r, || render::review_set(tz, &r))
                } else {
                    let r = review::set_many(
                        &ctx,
                        &review::ReviewBatchInput {
                            segments,
                            task,
                            kind,
                            activity,
                            rule,
                        },
                    )?;
                    ext::notify_data("segments");
                    emit(json, &r, || render::review_batch(tz, &r))
                }
            }
            ReviewCmd::Corrections { task, date } => {
                let l = correction::list(&ctx, &correction::CorrectionFilter { task, date })?;
                emit(json, &l, || render::corrections(tz, &l))
            }
            ReviewCmd::UndoCorrection { id } => {
                let judge = ext::judge_for(&ctx, None)?;
                let r = correction::undo(&ctx, &id, judge.as_deref())?;
                ext::notify_data("segments");
                emit(json, &r, || render::correction_undone(tz, &r))
            }
            ReviewCmd::Idle { mut args } => {
                let preset: review::IdlePreset = args.pop().unwrap_or_default().parse()?;
                let r = review::set_idle(&ctx, &args, preset)?;
                ext::notify_data("segments");
                emit(json, &r, || render::review_batch(tz, &r))
            }
            ReviewCmd::Suggest { accept, dismiss } => {
                let list = review::suggest_rules(&ctx)?;
                let pick = |n: usize| {
                    n.checked_sub(1).and_then(|i| list.get(i)).ok_or_else(|| {
                        hitoiki_core::Error::InvalidArgument(format!(
                            "提案 {n} はありません（1〜{}）",
                            list.len()
                        ))
                    })
                };
                if let Some(n) = accept {
                    let r = review::accept_suggestion(&ctx, &pick(n)?.rule)?;
                    ext::notify_data("segments");
                    emit(json, &r, || render::rule_line(&r))
                } else if let Some(n) = dismiss {
                    review::dismiss_suggestion(&ctx, &pick(n)?.rule)?;
                    emit(json, &serde_json::json!({ "dismissed": true }), || {
                        "この提案は一定期間表示しません。".to_string()
                    })
                } else {
                    emit(json, &list, || render::suggestions(&list))
                }
            }
        },
        Cmd::Block(b) => match b {
            BlockCmd::Add {
                start,
                end,
                task,
                label,
                focus,
            } => {
                let now = ctx.now();
                let block = plan::add(
                    &ctx,
                    &plan::BlockInput {
                        start: timefmt::parse_instant(tz, &start, now)?,
                        end: timefmt::parse_instant(tz, &end, now)?,
                        task,
                        label,
                        focus,
                    },
                )?;
                ext::notify_data("blocks");
                emit(json, &block, || render::block_added(tz, &block))
            }
            BlockCmd::List { range } => {
                let l = plan::list(&ctx, &range_args(&range))?;
                emit(json, &l, || render::blocks(tz, &l))
            }
            BlockCmd::Edit {
                id,
                start,
                end,
                task,
                label,
                focus,
                no_focus,
            } => {
                let now = ctx.now();
                let cur = plan::find(&ctx, &id)?;
                let b = plan::update(
                    &ctx,
                    &id,
                    &plan::BlockInput {
                        start: match &start {
                            Some(s) => timefmt::parse_instant(tz, s, now)?,
                            None => cur.start_at,
                        },
                        end: match &end {
                            Some(s) => timefmt::parse_instant(tz, s, now)?,
                            None => cur.end_at,
                        },
                        task: task.or(cur.task_id),
                        label: label.or(cur.label),
                        focus: if focus {
                            true
                        } else if no_focus {
                            false
                        } else {
                            cur.focus
                        },
                    },
                )?;
                ext::notify_data("blocks");
                emit(json, &b, || {
                    format!(
                        "ブロックを更新しました: {} {}-{}",
                        render::short(&b.id),
                        timefmt::fmt_hm(tz, b.start_at),
                        timefmt::fmt_hm(tz, b.end_at)
                    )
                })
            }
            BlockCmd::Delete { id } => {
                let b = plan::find(&ctx, &id)?;
                plan::delete(&ctx, &b.id)?;
                ext::notify_data("blocks");
                emit(json, &b, || {
                    format!("ブロックを削除しました: {}", render::short(&b.id))
                })
            }
            BlockCmd::Template(t) => template_cmd(&ctx, tz, json, t),
        },
        Cmd::Check {
            energy,
            mood,
            stress,
            erg,
            note,
        } => {
            let c = record::check(
                &ctx,
                &record::CheckInput {
                    energy,
                    mood,
                    stress,
                    erg,
                    note,
                },
            )?;
            ext::notify_data("checkin");
            emit(json, &c, || render::checkin(&c))
        }
        Cmd::Hassle { text, intensity } => {
            let text = read_text(&text)?;
            let h = record::hassle(&ctx, &record::HassleInput { text, intensity })?;
            ext::notify_data("hassle");
            emit(json, &h, || render::hassle(&h))
        }
        Cmd::Abc(AbcCmd::Add {
            a,
            b,
            c_emotion,
            c_intensity,
            from_hassle,
        }) => {
            let r = record::abc_add(
                &ctx,
                &record::AbcInput {
                    activating_event: a,
                    belief: b,
                    consequence_emotion: c_emotion,
                    consequence_intensity: c_intensity,
                    from_hassle,
                },
            )?;
            ext::notify_data("abc");
            emit(json, &r, || {
                format!(
                    "ABC を記録しました: {} (A: {})",
                    render::short(&r.id),
                    r.activating_event
                )
            })
        }
        Cmd::Abc(AbcCmd::Dispute { id }) => {
            let r = reflect::abc_dispute(&ctx, &id)?;
            emit(json, &r, || render::abc_dispute(&r))
        }
        Cmd::Context { kind, date } => {
            let c = context::context(&ctx, kind.parse()?, date.as_deref())?;
            // 機械可読が目的なので --json の有無にかかわらず JSON を出す。
            print_json(&c)
        }
        Cmd::Reflect(r) => reflect_cmd(&ctx, r, json),
        Cmd::Okr(o) => match o {
            OkrCmd::AddObjective { title, from, to } => {
                let now = ctx.now();
                let from = from.map(|s| timefmt::parse_instant(tz, &s, now)).transpose()?;
                let to = to.map(|s| service::parse_to(tz, &s, now)).transpose()?;
                let o = okr::add_objective(&ctx, &okr::ObjectiveInput { title, from, to })?;
                emit(json, &o, || render::objective_added(&o))
            }
            OkrCmd::AddKr {
                objective,
                title,
                target,
            } => {
                let k = okr::add_kr(
                    &ctx,
                    &okr::KrInput {
                        objective,
                        title,
                        target,
                    },
                )?;
                emit(json, &k, || render::kr_added(&k))
            }
            OkrCmd::List => {
                let l = okr::list(&ctx)?;
                emit(json, &l, || render::okr_list(&l))
            }
        },
        Cmd::Skill(s) => match s {
            SkillCmd::Add { name, parent } => {
                let s = skill::add(&ctx, &skill::SkillInput { name, parent })?;
                emit(json, &s, || {
                    format!("スキルを追加しました: {} {}", render::short(&s.id), s.name)
                })
            }
            SkillCmd::ImportPreset { name } => {
                let p = skill::import_preset(&ctx, name.as_deref())?;
                emit(json, &p, || render::preset_import(&p))
            }
            SkillCmd::List => {
                let l = skill::list(&ctx)?;
                emit(json, &l, || render::skills(&l))
            }
        },
        Cmd::Report {
            period,
            from,
            to,
            by,
            include_idle,
        } => {
            let r = report::report(
                &ctx,
                &report::ReportInput {
                    range: RangeArgs {
                        date: period,
                        from,
                        to,
                    },
                    by: by.parse()?,
                    include_idle,
                },
            )?;
            emit(json, &r, || render::report(tz, &r))
        }
        // DB を開く前に処理済み
        _ => unreachable!("handled before opening the database"),
    }
}

fn reflect_cmd(ctx: &Ctx, cmd: ReflectCmd, json: bool) -> Result<()> {
    let tz = ctx.tz()?;
    let generate = |kind: ReflectionKind, date: Option<String>| -> Result<()> {
        let r = reflect::reflect(ctx, kind, date.as_deref())?;
        emit(json, &r, || render::reflection(tz, &r))
    };
    match cmd {
        ReflectCmd::DayStart { date } => generate(ReflectionKind::DayStart, date),
        ReflectCmd::DayEnd { date } => generate(ReflectionKind::DayEnd, date),
        ReflectCmd::Weekly { date } => generate(ReflectionKind::Weekly, date),
        ReflectCmd::Answer { id, file } => {
            let text = match file.as_deref() {
                None | Some("-") => read_text("-")?,
                Some(path) => std::fs::read_to_string(path)?,
            };
            let r = reflect::answer(ctx, &id, &text)?;
            emit(json, &r, || {
                format!("回答を保存しました: {}", render::short(&r.id))
            })
        }
        ReflectCmd::List { kind, limit } => {
            let kind = kind.map(|k| k.parse::<ReflectionKind>()).transpose()?;
            let l = reflect::list(ctx, kind, limit)?;
            emit(json, &serde_json::json!({ "reflections": l }), || {
                render::reflection_list(tz, &l)
            })
        }
        ReflectCmd::Show { id } => {
            let r = reflect::show(ctx, &id)?;
            emit(json, &r, || render::reflection(tz, &r))
        }
    }
}

fn task_input(ctx: &Ctx, title: Option<String>, o: TaskOpts) -> Result<task::TaskInput> {
    let tz = ctx.tz()?;
    let now = ctx.now();
    let body = match (&o.body, &o.body_file) {
        (Some(b), _) => Some(read_text(b)?),
        (None, Some(f)) => Some(std::fs::read_to_string(f)?),
        _ => None,
    };
    let flag = |yes: bool, no: bool| -> Option<bool> {
        if yes {
            Some(true)
        } else if no {
            Some(false)
        } else {
            None
        }
    };
    Ok(task::TaskInput {
        title,
        body,
        urgent: flag(o.urgent, o.not_urgent),
        important: flag(o.important, o.not_important),
        estimate_min: o
            .estimate
            .as_deref()
            .map(timefmt::parse_duration_min)
            .transpose()?,
        key_result: o.kr,
        skills: o.skill,
        clear_skills: false,
        external_url: o.url,
        planned_start_at: o
            .plan_start
            .as_deref()
            .map(|s| timefmt::parse_instant(tz, s, now))
            .transpose()?,
        planned_end_at: o
            .plan_end
            .as_deref()
            .map(|s| timefmt::parse_instant(tz, s, now))
            .transpose()?,
        interrupt: o.interrupt,
    })
}

fn parse_statuses(s: &Option<String>) -> Result<Vec<TaskStatus>> {
    let Some(s) = s else { return Ok(vec![]) };
    let mut out = Vec::new();
    for part in s.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        if part == "all" {
            return Ok(TaskStatus::ALL.to_vec());
        }
        out.push(part.parse()?);
    }
    Ok(out)
}

/// `task switch` / `task back` の出力（JSON は `created` を含む）。
fn switch_emit(
    json: bool,
    r: &quick::SwitchResult,
    done_msg: &dyn Fn(&str, &task::TaskView) -> String,
) -> Result<()> {
    ext::notify_task(Some(&r.change.task.task.id));
    emit(json, r, || {
        let verb = if r.created {
            "作成して開始しました"
        } else {
            "開始しました"
        };
        let mut s = done_msg(verb, &r.change.task);
        for d in &r.change.demoted {
            s += &format!(
                "
{}",
                done_msg("todo に戻しました", d)
            );
        }
        s
    })
}

fn task_cmd(ctx: &Ctx, cmd: TaskCmd, json: bool) -> Result<()> {
    let tz = ctx.tz()?;
    let done_msg =
        |verb: &str, v: &task::TaskView| format!("{verb}: {} {}", render::short(&v.task.id), v.task.title);
    match cmd {
        TaskCmd::Add { title, opts } => {
            let input = task_input(ctx, Some(title), opts)?;
            let v = task::add(ctx, &input)?;
            ext::notify_task(Some(&v.task.id));
            emit(json, &v, || done_msg("追加しました", &v))
        }
        TaskCmd::List { status, kind } => {
            let l = task::list(
                ctx,
                &task::TaskFilter {
                    statuses: parse_statuses(&status)?,
                    kind,
                },
            )?;
            emit(json, &l, || render::tasks(&l))
        }
        TaskCmd::Show { id } => {
            let v = task::show(ctx, &id)?;
            emit(json, &v, || render::task_show(tz, &v))
        }
        TaskCmd::Edit { id, title, opts } => {
            let input = task_input(ctx, title, opts)?;
            let v = task::edit(ctx, &id, &input)?;
            ext::notify_task(Some(&v.task.id));
            emit(json, &v, || done_msg("更新しました", &v))
        }
        TaskCmd::Start { id } => {
            let c = task::start(ctx, &id)?;
            ext::notify_task(Some(&c.task.task.id));
            emit(json, &c, || {
                let mut s = done_msg("開始しました", &c.task);
                for d in &c.demoted {
                    s += &format!("\n{}", done_msg("todo に戻しました", d));
                }
                s
            })
        }
        TaskCmd::Switch {
            query,
            new,
            interrupt,
        } => {
            let r = quick::switch(
                ctx,
                &quick::SwitchInput {
                    query,
                    new,
                    interrupt,
                },
            )?;
            switch_emit(json, &r, &done_msg)
        }
        TaskCmd::Back => {
            let r = quick::back(ctx)?;
            switch_emit(json, &r, &done_msg)
        }
        TaskCmd::Done { id } => {
            let v = task::done(ctx, &id)?;
            ext::notify_task(Some(&v.task.id));
            emit(json, &v, || done_msg("完了しました", &v))
        }
        TaskCmd::Drop { id } => {
            let v = task::drop_task(ctx, &id)?;
            ext::notify_task(Some(&v.task.id));
            emit(json, &v, || done_msg("破棄しました", &v))
        }
        TaskCmd::Rule(RuleCmd::Add {
            id,
            field,
            pattern,
            pattern_type,
            priority,
        }) => {
            let r = task::rule_add(
                ctx,
                &task::RuleInput {
                    task: id,
                    field,
                    pattern,
                    pattern_type,
                    priority,
                },
            )?;
            emit(json, &r, || {
                format!(
                    "ルールを追加しました: {} {} {} ({})",
                    render::short(&r.id),
                    r.field,
                    r.pattern,
                    r.pattern_type
                )
            })
        }
        TaskCmd::Rule(RuleCmd::List { id }) => {
            let l = task::rule_list(ctx, id.as_deref())?;
            emit(json, &l, || render::rule_list(&l))
        }
    }
}

fn config_cmd(cmd: &ConfigCmd, json: bool) -> Result<()> {
    match cmd {
        ConfigCmd::Path => {
            let p = paths::config_file();
            emit(json, &serde_json::json!({"path": p}), || p.display().to_string())
        }
        ConfigCmd::Get { key } => {
            let cfg = Config::load()?;
            let v = cfg.get_value(key)?;
            let jv = serde_json::to_value(&v).map_err(anyhow::Error::from)?;
            emit(json, &serde_json::json!({"key": key, "value": jv}), || match &v {
                toml::Value::String(s) => s.clone(),
                other => other.to_string(),
            })
        }
        ConfigCmd::Set { key, value } => {
            let path = paths::config_file();
            config::set_value(&path, key, value)?;
            emit(json, &serde_json::json!({"key": key, "path": path}), || {
                format!("設定しました: {key}")
            })
        }
    }
}
