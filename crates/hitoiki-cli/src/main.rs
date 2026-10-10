//! `hitoiki` CLI。ロジックは hitoiki-core の service 層にあり、ここは引数の解釈と出力整形だけを行う。

mod args;
mod render;
mod table;

use args::*;
use clap::Parser;
use hitoiki_core::config::{self, Config};
use hitoiki_core::model::{ReflectionKind, TaskStatus};
use hitoiki_core::service::{
    self, context, log as log_svc, okr, plan, record, reflect, report, review, skill, status, task, Ctx,
    RangeArgs,
};
use hitoiki_core::{db, paths, timefmt, Error, Result, SystemClock};
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

fn unimplemented_cmd(name: &str) -> Result<()> {
    Err(Error::Other(anyhow::anyhow!("`{name}` は未実装です")))
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

fn run(cli: Cli) -> Result<()> {
    let json = cli.json;
    // DB を使わないコマンド
    match &cli.cmd {
        Cmd::Config(c) => return config_cmd(c, json),
        Cmd::Version { check } => {
            if *check {
                return unimplemented_cmd("version --check");
            }
            println!("hitoiki {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        Cmd::Rebuild { .. } => return unimplemented_cmd("rebuild"),
        Cmd::Daemon => return unimplemented_cmd("daemon"),
        Cmd::Doctor => return unimplemented_cmd("doctor"),
        Cmd::Ui { .. } => return unimplemented_cmd("ui"),
        Cmd::Ingest(_) => return unimplemented_cmd("ingest context"),
        Cmd::NativeHost { .. } => return unimplemented_cmd("native-host"),
        Cmd::Agent(_) => return unimplemented_cmd("agent"),
        _ => {}
    }

    let cfg = Config::load()?;
    let conn = db::open(&paths::db_file())?;
    let clock = SystemClock;
    let ctx = Ctx::new(&conn, &cfg, &clock);
    let tz = ctx.tz()?;

    match cli.cmd {
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
            ReviewCmd::List { range } => {
                let l = review::list(&ctx, &range_args(&range))?;
                emit(json, &l, || render::review_list(tz, &l))
            }
            ReviewCmd::Set {
                segment,
                task,
                kind,
                activity,
                rule,
            } => {
                let r = review::set(
                    &ctx,
                    &review::ReviewSetInput {
                        segment,
                        task,
                        kind,
                        activity,
                        rule,
                    },
                )?;
                emit(json, &r, || render::review_set(tz, &r))
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
                emit(json, &block, || render::block_added(tz, &block))
            }
            BlockCmd::List { range } => {
                let l = plan::list(&ctx, &range_args(&range))?;
                emit(json, &l, || render::blocks(tz, &l))
            }
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
            emit(json, &c, || render::checkin(&c))
        }
        Cmd::Hassle { text, intensity } => {
            let text = read_text(&text)?;
            let h = record::hassle(&ctx, &record::HassleInput { text, intensity })?;
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

fn task_cmd(ctx: &Ctx, cmd: TaskCmd, json: bool) -> Result<()> {
    let tz = ctx.tz()?;
    let done_msg =
        |verb: &str, v: &task::TaskView| format!("{verb}: {} {}", render::short(&v.task.id), v.task.title);
    match cmd {
        TaskCmd::Add { title, opts } => {
            let input = task_input(ctx, Some(title), opts)?;
            let v = task::add(ctx, &input)?;
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
            emit(json, &v, || done_msg("更新しました", &v))
        }
        TaskCmd::Start { id } => {
            let c = task::start(ctx, &id)?;
            emit(json, &c, || {
                let mut s = done_msg("開始しました", &c.task);
                for d in &c.demoted {
                    s += &format!("\n{}", done_msg("todo に戻しました", d));
                }
                s
            })
        }
        TaskCmd::Done { id } => {
            let v = task::done(ctx, &id)?;
            emit(json, &v, || done_msg("完了しました", &v))
        }
        TaskCmd::Drop { id } => {
            let v = task::drop_task(ctx, &id)?;
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
