//! AI 振り返り（docs/dev/architecture.md「AI 振り返り」）: テンプレート展開、AI コマンドの起動、`reflection` への保存、ABC の掘り下げ。
//!
//! ツール本体は推論を持たない。`ai.command` を子プロセスとして起動し、標準入力にプロンプトを渡して
//! 標準出力を受け取る。

use super::context::{self, ContextJson};
use super::record::{self, AbcInput, CheckInput, HassleInput};
use super::structured::{self, QuestionKind, StructuredOutput};
use super::Ctx;
use crate::id::new_id;
use crate::model::{AbcRecord, ErgTag, Reflection, ReflectionKind};
use crate::repo::reflection as repo;
use crate::{paths, timefmt, Error, Result};
use serde::{Deserialize, Serialize};
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
        "day_start_structured" => Some(include_str!("../../templates/day_start_structured.md")),
        "day_end_structured" => Some(include_str!("../../templates/day_end_structured.md")),
        "weekly_structured" => Some(include_str!("../../templates/weekly_structured.md")),
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

/// 指定した種別のプロンプトを組み立てる（AI は呼ばない）。従来の Markdown 用。
pub fn build_prompt(ctx: &Ctx, context: &ContextJson) -> Result<String> {
    build_prompt_named(ctx, context, context.kind.as_str())
}

/// 構造化出力用のプロンプト（テンプレート `<kind>_structured`）。
pub fn build_structured_prompt(ctx: &Ctx, context: &ContextJson) -> Result<String> {
    build_prompt_named(ctx, context, &format!("{}_structured", context.kind.as_str()))
}

fn build_prompt_named(ctx: &Ctx, context: &ContextJson, template: &str) -> Result<String> {
    let tpl = load_template(ctx, template)?;
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

/// 構造化出力で問いを生成する。コマンドの失敗・JSON 不正・スキーマ違反はすべてエラーで返す。
fn generate_structured(ctx: &Ctx, c: &ContextJson) -> Result<StructuredOutput> {
    let prompt = build_structured_prompt(ctx, c)?;
    let schema = structured::schema_json();
    // `_file` は実行が終わるまで保持する（Drop で一時スキーマファイルを消す）。
    let (extra, _file) = structured::build_args(&ctx.cfg.ai, &schema)?;
    let mut argv = ctx.cfg.ai.command.clone();
    argv.extend(extra);
    let out = run_ai(&argv, &prompt, Duration::from_secs(ctx.cfg.ai.timeout_sec.max(1)))?;
    structured::parse_output(&out)
}

/// コンテキストを作り、AI で問いを生成して `reflection` に保存する。
///
/// `ai.structured` が有効なら構造化出力を試み、失敗したら（エラーはログに残して）従来の Markdown の問いに
/// フォールバックする。
pub fn reflect(ctx: &Ctx, kind: ReflectionKind, date: Option<&str>) -> Result<Reflection> {
    let c = context::context(ctx, kind, date)?;
    let mut structured_out = None;
    if structured::enabled(&ctx.cfg.ai) {
        match generate_structured(ctx, &c) {
            Ok(o) => structured_out = Some(o),
            Err(e) => tracing::warn!(
                error = %e,
                "構造化出力に失敗したので Markdown の問いにフォールバックします"
            ),
        }
    }
    let (questions_md, structured_json, summary_md) = match &structured_out {
        Some(o) => (
            o.to_markdown(),
            Some(serde_json::to_string(o).map_err(anyhow::Error::from)?),
            o.summary.clone(),
        ),
        None => {
            let prompt = build_prompt(ctx, &c)?;
            (run_configured(ctx, &prompt)?, None, None)
        }
    };
    let r = Reflection {
        id: new_id(),
        at: ctx.now(),
        kind,
        context_json: serde_json::to_string(&c).map_err(anyhow::Error::from)?,
        questions_md,
        answers_md: None,
        summary_md,
        structured_json,
        answers_json: None,
    };
    repo::insert(ctx.conn, &r)?;
    Ok(r)
}

/// 構造化出力を取り出す（従来の Markdown のみの振り返りは `None`）。
pub fn structured_of(r: &Reflection) -> Result<Option<StructuredOutput>> {
    r.structured_json
        .as_deref()
        .map(|j| {
            serde_json::from_str(j)
                .map_err(|e| Error::Other(anyhow::anyhow!("structured_json を読めません: {e}")))
        })
        .transpose()
}

/// 構造化された問い 1 件への回答。使うフィールドは問いの種類で決まる。
/// - checkin: `energy` / `mood` / `stress`（1〜5）、`erg`、`note`
/// - hassle: `text`、`intensity`（1〜5、省略時 3）
/// - abc: `activating_event`（A）、`belief`（B）、`emotion` / `emotion_intensity`（C）、`dispute`（反論）
/// - free: `text`
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AnswerItem {
    /// 問いの番号（0 始まり。`structured_json` の `questions` の添字）。
    pub index: usize,
    pub energy: Option<i64>,
    pub mood: Option<i64>,
    pub stress: Option<i64>,
    pub erg: Option<ErgTag>,
    pub note: Option<String>,
    pub text: Option<String>,
    pub intensity: Option<i64>,
    pub activating_event: Option<String>,
    pub belief: Option<String>,
    pub emotion: Option<String>,
    pub emotion_intensity: Option<i64>,
    pub dispute: Option<String>,
}

/// `answer_structured` の入力（`hitoiki reflect answer` に渡す JSON もこの形）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AnswersInput {
    pub answers: Vec<AnswerItem>,
}

/// 保存済みの回答 1 件。どの問いに答え、どのレコードを作ったかの対応。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnsweredItem {
    pub index: usize,
    pub kind: QuestionKind,
    /// checkin / hassle / abc の保存先（`checkin.id` / `hassle.id` / `abc_record.id`）。free は無し。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub record_id: Option<String>,
    /// free の回答本文。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

/// `reflection.answers_json` の中身。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AnswersRecord {
    pub items: Vec<AnsweredItem>,
}

fn nonblank(s: &Option<String>) -> Option<String> {
    s.as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
}

/// 構造化された問いへの回答を、種類ごとの保存先（`checkin` / `hassle` / `abc_record`、自由記述は
/// `answers_md`）に保存する。空の回答は読み飛ばす。回答済みの問いには答え直せない。
/// 1 件でも不正なら何も保存しない。
pub fn answer_structured(ctx: &Ctx, id: &str, input: &AnswersInput) -> Result<Reflection> {
    let r = repo::resolve(ctx.conn, id)?;
    let out = structured_of(&r)?.ok_or_else(|| {
        Error::InvalidArgument("この振り返りは構造化されていません。Markdown で回答してください".into())
    })?;
    let mut record: AnswersRecord = match r.answers_json.as_deref() {
        Some(j) => serde_json::from_str(j)
            .map_err(|e| Error::Other(anyhow::anyhow!("answers_json を読めません: {e}")))?,
        None => AnswersRecord::default(),
    };

    let mut seen = Vec::new();
    for a in &input.answers {
        if a.index >= out.questions.len() {
            return Err(Error::InvalidArgument(format!(
                "問い {} はありません（0〜{}）",
                a.index,
                out.questions.len() - 1
            )));
        }
        if seen.contains(&a.index) {
            return Err(Error::InvalidArgument(format!(
                "問い {} の回答が重複しています",
                a.index
            )));
        }
        if record.items.iter().any(|i| i.index == a.index) {
            return Err(Error::InvalidArgument(format!("問い {} は回答済みです", a.index)));
        }
        seen.push(a.index);
    }

    let tx = ctx.conn.unchecked_transaction()?;
    let mut added = 0;
    for a in &input.answers {
        let q = &out.questions[a.index];
        let mut item = AnsweredItem {
            index: a.index,
            kind: q.kind,
            record_id: None,
            text: None,
        };
        match q.kind {
            QuestionKind::Checkin => {
                let note = nonblank(&a.note);
                if a.energy.is_none()
                    && a.mood.is_none()
                    && a.stress.is_none()
                    && a.erg.is_none()
                    && note.is_none()
                {
                    continue;
                }
                let c = record::check(
                    ctx,
                    &CheckInput {
                        energy: a.energy,
                        mood: a.mood,
                        stress: a.stress,
                        erg: a.erg,
                        note,
                    },
                )?;
                item.record_id = Some(c.id);
            }
            QuestionKind::Hassle => {
                let Some(text) = nonblank(&a.text) else { continue };
                let h = record::hassle(
                    ctx,
                    &HassleInput {
                        text,
                        intensity: a.intensity,
                    },
                )?;
                item.record_id = Some(h.id);
            }
            QuestionKind::Abc => {
                let (event, belief, emotion, dispute) = (
                    nonblank(&a.activating_event),
                    nonblank(&a.belief),
                    nonblank(&a.emotion),
                    nonblank(&a.dispute),
                );
                if event.is_none()
                    && belief.is_none()
                    && emotion.is_none()
                    && dispute.is_none()
                    && a.emotion_intensity.is_none()
                {
                    continue;
                }
                let abc = record::abc_add(
                    ctx,
                    &AbcInput {
                        activating_event: event,
                        belief,
                        consequence_emotion: emotion,
                        consequence_intensity: a.emotion_intensity,
                        from_hassle: None,
                    },
                )?;
                if let Some(d) = dispute {
                    repo::set_abc_dispute(ctx.conn, &abc.id, &d)?;
                }
                item.record_id = Some(abc.id);
            }
            QuestionKind::Free => {
                let Some(text) = nonblank(&a.text) else { continue };
                item.text = Some(text);
            }
        }
        record.items.push(item);
        added += 1;
    }
    if added == 0 {
        return Err(Error::InvalidArgument("回答が空です".into()));
    }
    record.items.sort_by_key(|i| i.index);

    let free: Vec<String> = record
        .items
        .iter()
        .filter_map(|i| {
            let text = i.text.as_deref()?;
            Some(format!("### {}\n\n{}", out.questions[i.index].prompt, text))
        })
        .collect();
    // 自由記述が無いときは、Markdown で先に書かれた回答があればそのまま残す。
    let answers_md = if free.is_empty() {
        r.answers_md.clone()
    } else {
        Some(free.join("\n\n"))
    };
    let json = serde_json::to_string(&record).map_err(anyhow::Error::from)?;
    repo::set_structured_answers(ctx.conn, &r.id, answers_md.as_deref(), &json)?;
    tx.commit()?;
    repo::get(ctx.conn, &r.id)
}

/// CLI 用: `{"answers":[...]}` の JSON なら構造化された回答として、それ以外は従来どおり Markdown の回答として保存する。
pub fn answer_text(ctx: &Ctx, id: &str, text: &str) -> Result<Reflection> {
    let t = text.trim();
    if t.starts_with('{') {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(t) {
            if v.get("answers").is_some() {
                let input: AnswersInput = serde_json::from_value(v)
                    .map_err(|e| Error::InvalidArgument(format!("回答の JSON が不正です: {e}")))?;
                return answer_structured(ctx, id, &input);
            }
        }
    }
    answer(ctx, id, text)
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

    fn structured_env(mode: &str) -> Env {
        let mut env = Env::new();
        env.cfg.ai.command = argv(Some(mode));
        env.cfg.ai.structured = crate::config::AiStructured::On;
        env.cfg.ai.structured_args = vec!["--structured".into(), "{schema}".into()];
        env
    }

    #[test]
    fn structured_prompt_has_context_and_kind_rules() {
        let env = Env::new();
        let ctx = env.ctx();
        for (k, forbidden) in [
            (ReflectionKind::DayStart, "`hassle` と `abc` は使わない"),
            (ReflectionKind::DayEnd, "`abc` は使わない"),
            (ReflectionKind::Weekly, "`checkin` は使わない"),
        ] {
            let c = context::context(&ctx, k, None).unwrap();
            let p = build_structured_prompt(&ctx, &c).unwrap();
            assert!(!p.contains("{{context}}") && p.contains("\"version\": 1"), "{k}");
            assert!(p.contains(forbidden), "{k}");
        }
    }

    #[test]
    fn structured_reflect_saves_json_and_markdown() {
        let env = structured_env("structured");
        let ctx = env.ctx();
        let r = reflect(&ctx, ReflectionKind::DayEnd, None).unwrap();
        // 複数 JSON が連結された出力でも読み取れる
        let s = structured_of(&r).unwrap().expect("構造化されている");
        assert_eq!(s.questions.len(), 4);
        assert_eq!(r.summary_md.as_deref(), Some("今日の要約"));
        assert!(r.questions_md.contains("1. いまの調子は？"));
        assert!(r.questions_md.contains("出来事の候補: レビューで指摘された"));
        assert!(!r.answered());
        let saved = show(&ctx, &r.id).unwrap();
        assert_eq!(saved.structured_json, r.structured_json);
    }

    #[test]
    fn structured_failures_fall_back_to_markdown() {
        for mode in ["badjson", "badschema"] {
            let env = structured_env(mode);
            let ctx = env.ctx();
            let r = reflect(&ctx, ReflectionKind::DayEnd, None).unwrap();
            assert!(r.structured_json.is_none(), "{mode}");
            assert_eq!(r.questions_md, "## 問い\n\n1. 従来の Markdown の問い", "{mode}");
        }
        // 構造化の引数でコマンドが失敗しても、引数なしで再実行した結果にフォールバックする
        let mut env = Env::new();
        env.cfg.ai.command = argv(Some("structured"));
        env.cfg.ai.structured = crate::config::AiStructured::Off;
        let r = reflect(&env.ctx(), ReflectionKind::DayEnd, None).unwrap();
        assert!(r.structured_json.is_none());
        // off ならプロンプトがそのまま（echo ではなく Markdown 応答）保存される
        assert_eq!(r.questions_md, "## 問い\n\n1. 従来の Markdown の問い");
    }

    #[test]
    fn answer_structured_routes_by_kind() {
        let env = structured_env("structured");
        let ctx = env.ctx();
        let r = reflect(&ctx, ReflectionKind::DayEnd, None).unwrap();
        let input: AnswersInput = serde_json::from_str(
            r#"{"answers":[
              {"index":0,"energy":3,"mood":4,"erg":"growth","note":"普通"},
              {"index":1,"text":"会議が長かった","intensity":4},
              {"index":2,"activating_event":"レビューで指摘された","belief":"向いていない","emotion":"落ち込み","emotion_intensity":4,"dispute":"一部の指摘だけ"},
              {"index":3,"text":"まあまあ"}
            ]}"#,
        )
        .unwrap();
        let a = answer_structured(&ctx, &r.id[..10], &input).unwrap();
        assert!(a.answered());
        let rec: AnswersRecord = serde_json::from_str(a.answers_json.as_ref().unwrap()).unwrap();
        assert_eq!(rec.items.len(), 4);
        assert_eq!(
            a.answers_md.as_deref(),
            Some("### ひとことで言うと？\n\nまあまあ")
        );

        // 保存先: checkin / hassle / abc_record
        let conn = ctx.conn;
        let cs = crate::repo::record::list_checkins(conn, 0, i64::MAX).unwrap();
        assert_eq!(cs.len(), 1);
        assert_eq!(
            (cs[0].energy, cs[0].mood, cs[0].erg_tag),
            (Some(3), Some(4), Some(ErgTag::Growth))
        );
        assert_eq!(rec.items[0].record_id.as_deref(), Some(cs[0].id.as_str()));
        let hs = crate::repo::record::list_hassles(conn, 0, i64::MAX).unwrap();
        assert_eq!((hs.len(), hs[0].intensity), (1, 4));
        assert_eq!(rec.items[1].record_id.as_deref(), Some(hs[0].id.as_str()));
        let abcs = crate::repo::record::list_abc(conn, 0, i64::MAX).unwrap();
        assert_eq!(abcs.len(), 1);
        assert_eq!(abcs[0].belief.as_deref(), Some("向いていない"));
        assert_eq!(abcs[0].consequence_intensity, Some(4));
        assert_eq!(abcs[0].dispute.as_deref(), Some("一部の指摘だけ"));
        assert_eq!(rec.items[2].record_id.as_deref(), Some(abcs[0].id.as_str()));
        assert_eq!(rec.items[3].record_id, None);

        // 回答済みの問いには答え直せない
        let again = AnswersInput {
            answers: vec![AnswerItem {
                index: 0,
                energy: Some(1),
                ..Default::default()
            }],
        };
        assert!(answer_structured(&ctx, &r.id, &again).is_err());
    }

    #[test]
    fn answer_structured_validates_without_partial_save() {
        let env = structured_env("structured");
        let ctx = env.ctx();
        let r = reflect(&ctx, ReflectionKind::DayEnd, None).unwrap();
        let item = |index, f: &dyn Fn(&mut AnswerItem)| {
            let mut a = AnswerItem {
                index,
                ..Default::default()
            };
            f(&mut a);
            a
        };
        // 範囲外の番号、重複、空
        for answers in [
            vec![item(9, &|a| a.text = Some("x".into()))],
            vec![
                item(3, &|a| a.text = Some("x".into())),
                item(3, &|a| a.text = Some("y".into())),
            ],
            vec![item(0, &|_| {}), item(3, &|a| a.text = Some("  ".into()))],
        ] {
            assert!(answer_structured(&ctx, &r.id, &AnswersInput { answers }).is_err());
        }
        // 後ろの項目が不正（強度 9）なら、前の checkin も保存されない
        let bad = AnswersInput {
            answers: vec![
                item(0, &|a| a.energy = Some(3)),
                item(1, &|a| {
                    a.text = Some("x".into());
                    a.intensity = Some(9);
                }),
            ],
        };
        assert!(answer_structured(&ctx, &r.id, &bad).is_err());
        assert!(crate::repo::record::list_checkins(ctx.conn, 0, i64::MAX)
            .unwrap()
            .is_empty());
        assert!(!show(&ctx, &r.id).unwrap().answered());
        // Markdown のみの振り返りには使えない
        let mut env2 = Env::new();
        env2.cfg.ai.command = argv(None);
        let md = reflect(&env2.ctx(), ReflectionKind::DayEnd, None).unwrap();
        let ok = AnswersInput {
            answers: vec![item(0, &|a| a.text = Some("x".into()))],
        };
        assert!(answer_structured(&env2.ctx(), &md.id, &ok).is_err());
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
