//! AI 振り返り（11 章）: テンプレート展開、AI コマンドの起動、`reflection` への保存、ABC の掘り下げ。
//!
//! ツール本体は推論を持たない。`ai.command` を子プロセスとして起動し、標準入力にプロンプトを渡して
//! 標準出力を受け取る。

use super::context::{self, ContextJson};
use super::Ctx;
use crate::id::new_id;
use crate::model::{AbcRecord, Reflection, ReflectionKind};
use crate::repo::reflection as repo;
use crate::{paths, timefmt, Error, Result};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// 組み込みの既定テンプレート。
pub fn builtin_template(name: &str) -> Option<&'static str> {
    match name {
        "day_start" => Some(include_str!("../../templates/day_start.md")),
        "day_end" => Some(include_str!("../../templates/day_end.md")),
        "weekly" => Some(include_str!("../../templates/weekly.md")),
        "abc_dispute" => Some(include_str!("../../templates/abc_dispute.md")),
        _ => None,
    }
}

fn templates_dir(ctx: &Ctx) -> PathBuf {
    match &ctx.cfg.ai.templates_dir {
        Some(d) if !d.trim().is_empty() => expand_env(d),
        _ => paths::templates_dir(),
    }
}

/// `%APPDATA%` 形式と `~` の簡易展開。
fn expand_env(s: &str) -> PathBuf {
    let mut out = s.to_string();
    while let Some(start) = out.find('%') {
        let Some(len) = out[start + 1..].find('%') else {
            break;
        };
        let name = out[start + 1..start + 1 + len].to_string();
        let val = std::env::var(&name).unwrap_or_default();
        out.replace_range(start..start + len + 2, &val);
    }
    if let Some(rest) = out.strip_prefix("~/") {
        if let Some(h) = dirs::home_dir() {
            return h.join(rest);
        }
    }
    PathBuf::from(out)
}

/// ユーザー定義テンプレート（`templates_dir/<name>.md`）を優先し、無ければ組み込みを返す。
pub fn load_template(ctx: &Ctx, name: &str) -> Result<String> {
    let path = templates_dir(ctx).join(format!("{name}.md"));
    if path.is_file() {
        return Ok(std::fs::read_to_string(&path)?);
    }
    builtin_template(name)
        .map(str::to_string)
        .ok_or_else(|| Error::NotFound(format!("テンプレート {name}")))
}

/// `{{key}}` を置換する。未知のプレースホルダはそのまま残す。置換結果の中身は再展開しない。
pub fn render_template(tpl: &str, vars: &[(&str, &str)]) -> String {
    let mut out = String::with_capacity(tpl.len());
    let mut rest = tpl;
    while let Some(i) = rest.find("{{") {
        out.push_str(&rest[..i]);
        let after = &rest[i + 2..];
        match after.find("}}") {
            Some(j) => {
                let key = after[..j].trim();
                match vars.iter().find(|(k, _)| *k == key) {
                    Some((_, v)) => out.push_str(v),
                    None => out.push_str(&rest[i..i + 2 + j + 2]),
                }
                rest = &after[j + 2..];
            }
            None => {
                out.push_str(&rest[i..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

/// コマンド名を PATH 解決する。Windows では PATHEXT（`.cmd` 等）に加え `.ps1` も探す。
fn resolve_program(name: &str) -> Result<PathBuf> {
    match which::which(name) {
        Ok(p) => Ok(p),
        Err(e) => {
            if cfg!(windows) && Path::new(name).extension().is_none() {
                if let Ok(p) = which::which(format!("{name}.ps1")) {
                    return Ok(p);
                }
            }
            Err(Error::Other(anyhow::anyhow!(
                "AI コマンド `{name}` が見つかりません ({e})。設定 ai.command を確認してください"
            )))
        }
    }
}

fn build_command(argv: &[String]) -> Result<Command> {
    let prog = resolve_program(&argv[0])?;
    let is_ps1 = prog.extension().is_some_and(|e| e.eq_ignore_ascii_case("ps1"));
    let mut cmd = if is_ps1 {
        let mut c = Command::new("powershell");
        c.args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"]);
        c.arg(&prog);
        c
    } else {
        Command::new(&prog)
    };
    cmd.args(&argv[1..]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    Ok(cmd)
}

/// AI コマンドを起動して `prompt` を標準入力に渡し、標準出力を返す。
/// `timeout` を超えたら子プロセスを kill してエラーにする。非 0 終了・空出力もエラー。
pub fn run_ai(argv: &[String], prompt: &str, timeout: Duration) -> Result<String> {
    if argv.is_empty() {
        return Err(Error::InvalidArgument("ai.command が空です".into()));
    }
    let mut child = build_command(argv)?
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| Error::Other(anyhow::anyhow!("AI コマンドを起動できません: {e}")))?;

    // 標準入力は別スレッドで書く（出力パイプが詰まって双方待ちになるのを避ける）。
    let mut stdin = child.stdin.take().expect("piped");
    let data = prompt.as_bytes().to_vec();
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(&data);
    });
    let mut out_pipe = child.stdout.take().expect("piped");
    let mut err_pipe = child.stderr.take().expect("piped");
    let out_t = std::thread::spawn(move || {
        let mut b = Vec::new();
        let _ = out_pipe.read_to_end(&mut b);
        b
    });
    let err_t = std::thread::spawn(move || {
        let mut b = Vec::new();
        let _ = err_pipe.read_to_end(&mut b);
        b
    });

    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait()? {
            Some(s) => break Some(s),
            None if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            None => std::thread::sleep(Duration::from_millis(20)),
        }
    };
    let _ = writer.join();
    let stdout = out_t.join().unwrap_or_default();
    let stderr = err_t.join().unwrap_or_default();

    let Some(status) = status else {
        return Err(Error::Other(anyhow::anyhow!(
            "AI コマンドが {} 秒でタイムアウトしました（ai.timeout_sec）",
            timeout.as_secs()
        )));
    };
    if !status.success() {
        let tail: String = String::from_utf8_lossy(&stderr)
            .trim()
            .chars()
            .take(500)
            .collect();
        return Err(Error::Other(anyhow::anyhow!(
            "AI コマンドが失敗しました (終了コード {}): {tail}",
            status.code().map_or("不明".to_string(), |c| c.to_string())
        )));
    }
    let text = String::from_utf8_lossy(&stdout).trim().to_string();
    if text.is_empty() {
        return Err(Error::Other(anyhow::anyhow!("AI コマンドの出力が空でした")));
    }
    Ok(text)
}

fn run_configured(ctx: &Ctx, prompt: &str) -> Result<String> {
    run_ai(
        &ctx.cfg.ai.command,
        prompt,
        Duration::from_secs(ctx.cfg.ai.timeout_sec.max(1)),
    )
}

/// 指定した種別のプロンプトを組み立てる（AI は呼ばない）。
pub fn build_prompt(ctx: &Ctx, context: &ContextJson) -> Result<String> {
    let tpl = load_template(ctx, context.kind.as_str())?;
    let json = serde_json::to_string_pretty(context).map_err(anyhow::Error::from)?;
    Ok(render_template(
        &tpl,
        &[
            ("context", &json),
            ("date", &context.date),
            ("kind", context.kind.as_str()),
        ],
    ))
}

/// コンテキストを作り、AI で問いを生成して `reflection` に保存する。
pub fn reflect(ctx: &Ctx, kind: ReflectionKind, date: Option<&str>) -> Result<Reflection> {
    let c = context::context(ctx, kind, date)?;
    let prompt = build_prompt(ctx, &c)?;
    let questions = run_configured(ctx, &prompt)?;
    let r = Reflection {
        id: new_id(),
        at: ctx.now(),
        kind,
        context_json: serde_json::to_string(&c).map_err(anyhow::Error::from)?,
        questions_md: questions,
        answers_md: None,
        summary_md: None,
    };
    repo::insert(ctx.conn, &r)?;
    Ok(r)
}

/// 回答を保存する（既存の回答は置き換える）。
pub fn answer(ctx: &Ctx, id: &str, answers_md: &str) -> Result<Reflection> {
    if answers_md.trim().is_empty() {
        return Err(Error::InvalidArgument("回答が空です".into()));
    }
    let r = repo::resolve(ctx.conn, id)?;
    repo::set_answers(ctx.conn, &r.id, answers_md.trim_end())?;
    repo::get(ctx.conn, &r.id)
}

pub fn list(ctx: &Ctx, kind: Option<ReflectionKind>, limit: i64) -> Result<Vec<Reflection>> {
    repo::list(ctx.conn, kind, limit)
}

pub fn show(ctx: &Ctx, id: &str) -> Result<Reflection> {
    repo::resolve(ctx.conn, id)
}

/// ABC 記録の B（受け止め方）を掘り下げる問いを AI で生成し、`ai_dialog_md` に追記する。
pub fn abc_dispute(ctx: &Ctx, id: &str) -> Result<AbcRecord> {
    let tz = ctx.tz()?;
    let a = repo::resolve_abc(ctx.conn, id)?;
    let none = "（未記入）";
    let intensity = a
        .consequence_intensity
        .map_or(none.to_string(), |n| n.to_string());
    let tpl = load_template(ctx, "abc_dispute")?;
    let prompt = render_template(
        &tpl,
        &[
            ("activating_event", &a.activating_event),
            ("belief", a.belief.as_deref().unwrap_or(none)),
            ("emotion", a.consequence_emotion.as_deref().unwrap_or(none)),
            ("intensity", &intensity),
            ("dispute", a.dispute.as_deref().unwrap_or(none)),
            ("date", &timefmt::fmt_rfc3339(tz, a.at)),
        ],
    );
    let questions = run_configured(ctx, &prompt)?;
    let entry = format!(
        "### AI の問い ({})\n\n{}",
        timefmt::fmt_rfc3339(tz, ctx.now()),
        questions
    );
    let dialog = match a.ai_dialog_md.as_deref().filter(|s| !s.trim().is_empty()) {
        Some(prev) => format!("{}\n\n{}", prev.trim_end(), entry),
        None => entry,
    };
    repo::set_abc_dialog(ctx.conn, &a.id, &dialog)?;
    repo::get_abc(ctx.conn, &a.id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::record::{self, AbcInput};
    use crate::service::testutil::Env;

    /// `cargo test` が一緒にビルドする examples/echo_stdin のパス。
    fn echo_bin() -> Option<String> {
        let exe = std::env::current_exe().ok()?;
        let dir = exe.parent()?.parent()?.join("examples");
        let name = if cfg!(windows) {
            "echo_stdin.exe"
        } else {
            "echo_stdin"
        };
        let p = dir.join(name);
        p.is_file().then(|| p.to_string_lossy().into_owned())
    }

    fn argv(mode: Option<&str>) -> Vec<String> {
        let mut v = vec![echo_bin().expect("echo_stdin example is built by cargo test")];
        v.extend(mode.map(String::from));
        v
    }

    #[test]
    fn template_rendering() {
        let s = render_template(
            "a {{x}} b {{ y }} {{unknown}} {{x",
            &[("x", "1{{y}}"), ("y", "2")],
        );
        assert_eq!(s, "a 1{{y}} b 2 {{unknown}} {{x");
    }

    #[test]
    fn builtin_templates_have_placeholders() {
        for n in ["day_start", "day_end", "weekly"] {
            assert!(builtin_template(n).unwrap().contains("{{context}}"), "{n}");
        }
        assert!(builtin_template("abc_dispute").unwrap().contains("{{belief}}"));
    }

    #[test]
    fn user_template_overrides_builtin() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("day_end.md"), "独自 {{date}}").unwrap();
        let mut env = Env::new();
        env.cfg.ai.templates_dir = Some(dir.path().to_string_lossy().into_owned());
        let t = load_template(&env.ctx(), "day_end").unwrap();
        assert_eq!(t, "独自 {{date}}");
        // 無いものは組み込み
        assert!(load_template(&env.ctx(), "weekly")
            .unwrap()
            .contains("{{context}}"));
    }

    #[test]
    fn prompt_embeds_context() {
        let env = Env::new();
        let ctx = env.ctx();
        let c = context::context(&ctx, ReflectionKind::DayEnd, None).unwrap();
        let p = build_prompt(&ctx, &c).unwrap();
        assert!(!p.contains("{{context}}"));
        assert!(p.contains("\"version\": 1"));
        assert!(p.contains("2026-10-10"));
    }

    #[test]
    fn run_ai_echoes_stdin() {
        let out = run_ai(&argv(None), "こんにちは\n複数行", Duration::from_secs(20)).unwrap();
        assert_eq!(out, "こんにちは\n複数行");
    }

    #[test]
    fn run_ai_nonzero_exit() {
        let e = run_ai(&argv(Some("fail")), "x", Duration::from_secs(20)).unwrap_err();
        let m = e.to_string();
        assert!(m.contains("終了コード 3") && m.contains("失敗しました"), "{m}");
    }

    #[test]
    fn run_ai_timeout_kills() {
        let t = Instant::now();
        let e = run_ai(&argv(Some("sleep")), "x", Duration::from_millis(500)).unwrap_err();
        assert!(e.to_string().contains("タイムアウト"));
        assert!(t.elapsed() < Duration::from_secs(20));
    }

    #[test]
    fn run_ai_missing_command() {
        let e = run_ai(&["hitoiki-no-such-cmd-xyz".into()], "x", Duration::from_secs(5)).unwrap_err();
        assert!(e.to_string().contains("見つかりません"));
    }

    #[test]
    fn reflect_saves_and_answers() {
        let mut env = Env::new();
        env.cfg.ai.command = argv(None);
        let ctx = env.ctx();
        let r = reflect(&ctx, ReflectionKind::DayEnd, None).unwrap();
        // echo なのでプロンプトがそのまま問いとして保存される
        assert!(r.questions_md.contains("2026-10-10"));
        let saved: serde_json::Value = serde_json::from_str(&r.context_json).unwrap();
        assert_eq!(saved["kind"], "day_end");
        assert_eq!(list(&ctx, Some(ReflectionKind::DayEnd), 10).unwrap().len(), 1);
        assert!(list(&ctx, Some(ReflectionKind::Weekly), 10).unwrap().is_empty());

        let a = answer(&ctx, &r.id[..10], "考えたこと\n").unwrap();
        assert_eq!(a.answers_md.as_deref(), Some("考えたこと"));
        assert!(answer(&ctx, &r.id, "  ").is_err());
        assert_eq!(show(&ctx, &r.id).unwrap().id, r.id);
    }

    #[test]
    fn reflect_failure_saves_nothing() {
        let mut env = Env::new();
        env.cfg.ai.command = argv(Some("fail"));
        let ctx = env.ctx();
        assert!(reflect(&ctx, ReflectionKind::DayStart, None).is_err());
        assert!(list(&ctx, None, 10).unwrap().is_empty());
    }

    #[test]
    fn abc_dispute_appends_dialog() {
        let mut env = Env::new();
        env.cfg.ai.command = argv(None);
        let ctx = env.ctx();
        let abc = record::abc_add(
            &ctx,
            &AbcInput {
                activating_event: Some("レビューで指摘された".into()),
                belief: Some("自分は向いていない".into()),
                consequence_emotion: Some("落ち込み".into()),
                consequence_intensity: Some(4),
                from_hassle: None,
            },
        )
        .unwrap();
        let r1 = abc_dispute(&ctx, &abc.id[..10]).unwrap();
        let d1 = r1.ai_dialog_md.unwrap();
        assert!(d1.starts_with("### AI の問い"));
        assert!(d1.contains("自分は向いていない") && d1.contains("レビューで指摘された"));
        let r2 = abc_dispute(&ctx, &abc.id).unwrap();
        assert_eq!(r2.ai_dialog_md.unwrap().matches("### AI の問い").count(), 2);
        assert!(abc_dispute(&ctx, "ZZZZ").is_err());
    }
}
