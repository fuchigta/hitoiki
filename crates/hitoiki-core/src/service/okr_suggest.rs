//! KR への紐づけの提案（docs/dev/architecture.md「KR への紐づけの提案」）。
//!
//! スキルも紐付けタスクも無い KR について、紐付けるとよさそうなタスクとスキルの候補を出す。自動では紐付けない。
//! ユーザーが「付ける」を選んだときだけタスクの紐付け・KR のスキルの追加をし、「付けない」を選んだ候補は
//! `kr_suggest_reject` に残して二度と出さない。
//!
//! - 手元の候補: Objective と KR の名前（タイトル・説明・目標）と、タスク名・スキル名の近さ。ASCII の語の一致と、
//!   日本語は文字 2 つ組（助詞を含むものを除く）の重なりで見る。外部には何も送らない。
//! - AI の候補: `ai.command` が設定されているときだけ聞いて合わせる。**送るのは KR / Objective のタイトルと、
//!   タスク名・スキル名だけ**（本文・説明・時間の記録は送らない。ID は `t1` / `s1` の連番に置き換える）。
//!   失敗しても手元の候補は出す。

use super::erg::ai_configured;
use super::okr::{find_kr, KrBrief};
use super::reflect::run_ai;

use super::Ctx;
use crate::model::{Skill, Task, TaskStatus};
use crate::repo::{self, edit};
use crate::{Error, Result};
use rusqlite::params;
use serde::Serialize;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::time::Duration;

/// 候補の種類。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateKind {
    Task,
    Skill,
}

impl CandidateKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            CandidateKind::Task => "task",
            CandidateKind::Skill => "skill",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            CandidateKind::Task => "タスク",
            CandidateKind::Skill => "スキル",
        }
    }
}

/// 候補 1 件。
#[derive(Debug, Clone, Serialize)]
pub struct Candidate {
    pub kind: CandidateKind,
    pub id: String,
    pub name: String,
    /// 0〜1。手元の候補は名前の近さ、AI の候補は AI が答えた確信度（無ければ 0.5）。
    pub score: f64,
    /// `local`（手元）/ `ai` / `both`（両方）。
    pub source: &'static str,
}

/// AI への問い合わせの結果。
#[derive(Debug, Clone, Default, Serialize)]
pub struct AiOutcome {
    /// AI の候補を合わせたか。
    pub used: bool,
    /// 使わなかった・失敗したときの理由（日本語）。
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Suggestions {
    pub key_result: KrBrief,
    /// 提案の対象か。スキルか紐付けタスクが既にある KR は対象外（候補は空）。
    pub eligible: bool,
    pub candidates: Vec<Candidate>,
    pub ai: AiOutcome,
}

/// 採否の対象の指定。`task:<ID>` / `skill:<ID または名前>`（ID は前方一致）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidateRef {
    pub kind: CandidateKind,
    pub key: String,
}

impl std::str::FromStr for CandidateRef {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self> {
        let bad = || {
            Error::InvalidArgument(format!(
                "候補は task:<タスクの ID> または skill:<スキルの ID または名前> で指定してください: {s}"
            ))
        };
        let (k, key) = s.split_once(':').ok_or_else(bad)?;
        let kind = match k.trim() {
            "task" => CandidateKind::Task,
            "skill" => CandidateKind::Skill,
            _ => return Err(bad()),
        };
        let key = key.trim();
        if key.is_empty() {
            return Err(bad());
        }
        Ok(Self {
            kind,
            key: key.to_string(),
        })
    }
}

/// 手元の候補の下限（名前の近さ。0〜1）。
const LOCAL_MIN_SCORE: f64 = 0.3;
/// 種類ごとの手元の候補の上限。
const LOCAL_PER_KIND: usize = 5;
/// 提案の総数の上限。
const MAX_CANDIDATES: usize = 8;
/// AI に渡す候補の数の上限（種類ごと）と、名前の長さの上限。
const AI_POOL_PER_KIND: usize = 100;
const AI_NAME_CHARS: usize = 80;
/// AI の候補を採る確信度の下限。
const AI_MIN_CONFIDENCE: f64 = 0.5;

// ===== 名前の近さ =====

const PARTICLES: &str = "はがをにへでともやの";
const STOP_BIGRAMS: [&str; 8] = ["する", "した", "して", "ます", "です", "こと", "もの", "ため"];

/// 文字列の特徴（ASCII の語、日本語などは文字 2 つ組）。助詞を含む 2 つ組と定型の 2 つ組は除く。
fn features(s: &str) -> HashSet<String> {
    let lower = s.to_lowercase();
    let mut out = HashSet::new();
    // 英数字の連なりを、ASCII とそれ以外の連なりに分ける。
    let mut run: Vec<char> = Vec::new();
    let mut run_ascii = true;
    let flush = |run: &mut Vec<char>, ascii: bool, out: &mut HashSet<String>| {
        if ascii {
            if run.len() >= 2 {
                out.insert(run.iter().collect());
            }
        } else {
            for w in run.windows(2) {
                let bg: String = w.iter().collect();
                if w.iter().any(|c| PARTICLES.contains(*c)) || STOP_BIGRAMS.contains(&bg.as_str()) {
                    continue;
                }
                out.insert(bg);
            }
        }
        run.clear();
    };
    for c in lower.chars() {
        if !c.is_alphanumeric() {
            flush(&mut run, run_ascii, &mut out);
            continue;
        }
        if c.is_ascii() != run_ascii {
            flush(&mut run, run_ascii, &mut out);
            run_ascii = c.is_ascii();
        }
        run.push(c);
    }
    flush(&mut run, run_ascii, &mut out);
    out
}

/// 記号と空白を除いた小文字（部分一致の判定用）。
fn squash(s: &str) -> String {
    s.to_lowercase().chars().filter(|c| c.is_alphanumeric()).collect()
}

/// 名前 `name` と文章 `text` の近さ（0〜1）。一方が他方を含めば 1、そうでなければ共通の特徴の割合（小さい方に対する）。
fn closeness(name: &str, text: &str) -> f64 {
    let (n, t) = (squash(name), squash(text));
    if n.chars().count() >= 2 && (t.contains(&n) || (t.chars().count() >= 2 && n.contains(&t))) {
        return 1.0;
    }
    let (fa, fb) = (features(name), features(text));
    let min = fa.len().min(fb.len());
    if min == 0 {
        return 0.0;
    }
    fa.intersection(&fb).count() as f64 / min as f64
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        let mut t: String = s.chars().take(n).collect();
        t.push('…');
        t
    }
}

// ===== 候補の材料 =====

struct Pool {
    tasks: Vec<Task>,
    skills: Vec<Skill>,
}

fn rejected(ctx: &Ctx, kr_id: &str) -> Result<HashSet<(String, String)>> {
    let mut stmt = ctx
        .conn
        .prepare("SELECT kind, target_id FROM kr_suggest_reject WHERE kr_id = ?1")?;
    let rows = stmt.query_map([kr_id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// 提案の対象になる KR か（スキルも紐付けタスク（やめたものを除く）も無い）。
fn eligible(ctx: &Ctx, kr_id: &str) -> Result<bool> {
    let no_skill = repo::skill::kr_skills(ctx.conn, kr_id)?.is_empty();
    let no_task = repo::task::list_of_kr(ctx.conn, kr_id)?
        .iter()
        .all(|t| t.status == TaskStatus::Dropped);
    Ok(no_skill && no_task)
}

/// 候補にできるタスク（どの KR にも紐付いていない未完了のもの）とスキル。「付けない」にしたものは除く。
fn pool(ctx: &Ctx, kr_id: &str) -> Result<Pool> {
    let rej = rejected(ctx, kr_id)?;
    let tasks = repo::task::list(ctx.conn, &[TaskStatus::Todo, TaskStatus::Doing], None)?
        .into_iter()
        .filter(|t| t.key_result_id.is_none() && !rej.contains(&("task".into(), t.id.clone())))
        .collect();
    let skills = repo::skill::list(ctx.conn)?
        .into_iter()
        .filter(|s| !rej.contains(&("skill".into(), s.id.clone())))
        .collect();
    Ok(Pool { tasks, skills })
}

/// 手元の候補。KR の文章（タイトル・目標）と Objective の文章（タイトル・説明）のそれぞれとの近さのうち、
/// 大きい方（Objective との近さは 0.8 倍）をスコアにする。
fn local_candidates(kr_text: &str, obj_text: &str, pool: &Pool) -> Vec<Candidate> {
    let score = |name: &str| closeness(name, kr_text).max(0.8 * closeness(name, obj_text));
    let top = |kind: CandidateKind, items: Vec<(&str, &str)>| {
        let mut v: Vec<Candidate> = items
            .into_iter()
            .map(|(id, name)| (id, name, score(name)))
            .filter(|(_, _, s)| *s >= LOCAL_MIN_SCORE)
            .map(|(id, name, s)| Candidate {
                kind,
                id: id.to_string(),
                name: name.to_string(),
                score: s,
                source: "local",
            })
            .collect();
        v.sort_by(|a, b| b.score.total_cmp(&a.score).then_with(|| a.name.cmp(&b.name)));
        v.truncate(LOCAL_PER_KIND);
        v
    };
    let mut out = top(
        CandidateKind::Task,
        pool.tasks
            .iter()
            .map(|t| (t.id.as_str(), t.title.as_str()))
            .collect(),
    );
    out.extend(top(
        CandidateKind::Skill,
        pool.skills
            .iter()
            .map(|s| (s.id.as_str(), s.name.as_str()))
            .collect(),
    ));
    out
}

// ===== AI =====

/// AI を呼ぶ関数（プロンプトを渡して標準出力を受け取る）。テストで差し替える。
pub type Runner<'a> = &'a dyn Fn(&str) -> Result<String>;

/// AI に聞く。返り値は (候補, 確信度)。ID は連番 `t1` / `s1` から実際の ID に戻す。
fn ask_ai(
    run: Runner,
    objective_title: &str,
    kr_title: &str,
    pool: &Pool,
) -> Result<Vec<(CandidateKind, String, f64)>> {
    let tasks: Vec<&Task> = pool.tasks.iter().take(AI_POOL_PER_KIND).collect();
    let skills: Vec<&Skill> = pool.skills.iter().take(AI_POOL_PER_KIND).collect();
    if tasks.is_empty() && skills.is_empty() {
        return Ok(vec![]);
    }
    let mut prompt = String::from(
        "次の KR に関係しそうなタスクとスキルを、下の候補の中から選んでください。\n\
         名前の意味が KR に近いものだけを選び、関係が薄いものは含めないでください（最大 5 件）。\n\n",
    );
    prompt.push_str(&format!(
        "Objective: {}\nKR: {}\n\nタスク候補:\n",
        truncate(objective_title, AI_NAME_CHARS),
        truncate(kr_title, AI_NAME_CHARS)
    ));
    let mut ids: HashMap<String, (CandidateKind, String)> = HashMap::new();
    for (i, t) in tasks.iter().enumerate() {
        let key = format!("t{}", i + 1);
        prompt.push_str(&format!("{key}: {}\n", truncate(&t.title, AI_NAME_CHARS)));
        ids.insert(key, (CandidateKind::Task, t.id.clone()));
    }
    prompt.push_str("\nスキル候補:\n");
    for (i, s) in skills.iter().enumerate() {
        let key = format!("s{}", i + 1);
        prompt.push_str(&format!("{key}: {}\n", truncate(&s.name, AI_NAME_CHARS)));
        ids.insert(key, (CandidateKind::Skill, s.id.clone()));
    }
    prompt.push_str(
        "\n出力は次の形式の JSON だけにしてください。confidence は 0〜1 です。\n\
         {\"suggestions\":[{\"id\":\"t1\",\"confidence\":0.8}]}\n",
    );
    let out = run(&prompt)?;
    let found = parse_response(&out)?;
    Ok(found
        .into_iter()
        .filter_map(|(key, conf)| ids.get(&key).map(|(k, id)| (*k, id.clone(), conf)))
        .collect())
}

/// AI の標準出力から `suggestions` を読む（コードフェンスや前置きに包まれていてもよい）。
/// 確信度が 0〜1 の数値でなければ 0.5。`AI_MIN_CONFIDENCE` 未満は捨てる。
fn parse_response(stdout: &str) -> Result<Vec<(String, f64)>> {
    let value: Value = serde_json::from_str(stdout.trim())
        .ok()
        .or_else(|| {
            let (s, e) = (stdout.find('{')?, stdout.rfind('}')?);
            (e > s)
                .then(|| serde_json::from_str(&stdout[s..=e]).ok())
                .flatten()
        })
        .ok_or_else(|| Error::Other(anyhow::anyhow!("AI の応答を JSON として読めません")))?;
    let items = value
        .get("suggestions")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::Other(anyhow::anyhow!("AI の応答に suggestions がありません")))?;
    Ok(items
        .iter()
        .filter_map(|it| {
            let id = it.get("id")?.as_str()?.trim().to_string();
            let conf = it
                .get("confidence")
                .and_then(Value::as_f64)
                .filter(|c| c.is_finite() && (0.0..=1.0).contains(c))
                .unwrap_or(0.5);
            (conf >= AI_MIN_CONFIDENCE).then_some((id, conf))
        })
        .collect())
}

// ===== 提案 =====

/// KR（ID の前方一致またはタイトル）への紐づけの候補。`use_ai` が true で `ai.command` が設定されていれば AI にも聞く。
pub fn suggest(ctx: &Ctx, kr: &str, use_ai: bool) -> Result<Suggestions> {
    if use_ai && ai_configured(ctx) {
        let ai = &ctx.cfg.ai;
        let timeout = Duration::from_secs(ai.timeout_sec.max(1));
        let run = |prompt: &str| run_ai(&ai.command, prompt, timeout);
        suggest_with(ctx, kr, Some(&run))
    } else {
        suggest_with(ctx, kr, None)
    }
}

/// [`suggest`] の AI を呼ぶ関数を差し替えられる版。`None` なら手元の候補だけ。
pub fn suggest_with(ctx: &Ctx, kr: &str, run: Option<Runner>) -> Result<Suggestions> {
    let k = find_kr(ctx, kr)?;
    let brief = KrBrief {
        id: k.id.clone(),
        title: k.title.clone(),
    };
    if !eligible(ctx, &k.id)? {
        return Ok(Suggestions {
            key_result: brief,
            eligible: false,
            candidates: vec![],
            ai: AiOutcome::default(),
        });
    }
    let o = edit::get_objective(ctx.conn, &k.objective_id)?;
    let pool = pool(ctx, &k.id)?;
    let kr_text = format!("{} {}", k.title, k.target.as_deref().unwrap_or(""));
    let obj_text = format!("{} {}", o.title, o.description_md.as_deref().unwrap_or(""));

    let mut merged: Vec<Candidate> = local_candidates(&kr_text, &obj_text, &pool);
    let mut ai = AiOutcome::default();
    if let Some(run) = run {
        match ask_ai(run, &o.title, &k.title, &pool) {
            Ok(found) => {
                ai.used = true;
                for (kind, id, conf) in found {
                    if let Some(c) = merged.iter_mut().find(|c| c.kind == kind && c.id == id) {
                        c.source = "both";
                        c.score = c.score.max(conf);
                        continue;
                    }
                    let name = match kind {
                        CandidateKind::Task => {
                            pool.tasks.iter().find(|t| t.id == id).map(|t| t.title.clone())
                        }
                        CandidateKind::Skill => {
                            pool.skills.iter().find(|s| s.id == id).map(|s| s.name.clone())
                        }
                    };
                    if let Some(name) = name {
                        merged.push(Candidate {
                            kind,
                            id,
                            name,
                            score: conf,
                            source: "ai",
                        });
                    }
                }
            }
            Err(e) => {
                // 文章や応答の中身はログに出さない。
                tracing::warn!(error = %e, "KR の紐づけの提案: AI に問い合わせられませんでした");
                ai.note = Some("AI に問い合わせられなかったため、手元の候補だけを表示します".into());
            }
        }
    }
    // 両方が挙げたものを先に、次にスコア順。
    merged.sort_by(|a, b| {
        ((b.source == "both") as u8)
            .cmp(&((a.source == "both") as u8))
            .then_with(|| b.score.total_cmp(&a.score))
            .then_with(|| a.name.cmp(&b.name))
    });
    merged.truncate(MAX_CANDIDATES);
    Ok(Suggestions {
        key_result: brief,
        eligible: true,
        candidates: merged,
        ai,
    })
}

// ===== 採否 =====

/// 解決した候補（ID と表示名）。
struct Resolved {
    kind: CandidateKind,
    id: String,
    name: String,
}

fn resolve_refs(ctx: &Ctx, refs: &[CandidateRef]) -> Result<Vec<Resolved>> {
    if refs.is_empty() {
        return Err(Error::InvalidArgument("候補を 1 つ以上指定してください".into()));
    }
    let mut out: Vec<Resolved> = Vec::new();
    for r in refs {
        let (id, name) = match r.kind {
            CandidateKind::Task => {
                let t = repo::task::resolve(ctx.conn, &r.key)?;
                (t.id, t.title)
            }
            CandidateKind::Skill => {
                let s = match repo::skill::find_by_name(ctx.conn, &r.key)? {
                    Some(s) => s,
                    None => repo::skill::resolve(ctx.conn, &r.key)?,
                };
                (s.id, s.name)
            }
        };
        if !out.iter().any(|o| o.kind == r.kind && o.id == id) {
            out.push(Resolved {
                kind: r.kind,
                id,
                name,
            });
        }
    }
    Ok(out)
}

#[derive(Debug, Clone, Serialize)]
pub struct DecisionItem {
    pub kind: CandidateKind,
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DecisionResult {
    pub key_result: KrBrief,
    pub items: Vec<DecisionItem>,
}

fn items_of(r: Vec<Resolved>) -> Vec<DecisionItem> {
    r.into_iter()
        .map(|r| DecisionItem {
            kind: r.kind,
            id: r.id,
            name: r.name,
        })
        .collect()
}

/// 「付ける」: タスクは KR に紐付け（`okr link` と同じ）、スキルは KR のスキルに加える（今のスキルは残す）。
/// すべての候補を先に解決し、1 つでも見つからなければ何も変えない。
pub fn accept(ctx: &Ctx, kr: &str, refs: &[CandidateRef]) -> Result<DecisionResult> {
    let k = find_kr(ctx, kr)?;
    let resolved = resolve_refs(ctx, refs)?;
    // タスクの紐付け（`link_tasks`）は自前でトランザクションを張るので、ここでは張らない（候補は解決済み）。
    let task_ids: Vec<String> = resolved
        .iter()
        .filter(|r| r.kind == CandidateKind::Task)
        .map(|r| r.id.clone())
        .collect();
    if !task_ids.is_empty() {
        super::okr::link_tasks(ctx, &k.id, &task_ids)?;
    }
    let new_skills: Vec<&Resolved> = resolved
        .iter()
        .filter(|r| r.kind == CandidateKind::Skill)
        .collect();
    if !new_skills.is_empty() {
        let mut ids: Vec<String> = repo::skill::kr_skills(ctx.conn, &k.id)?
            .into_iter()
            .map(|s| s.id)
            .collect();
        for s in new_skills {
            if !ids.contains(&s.id) {
                ids.push(s.id.clone());
            }
        }
        repo::skill::set_kr_skills(ctx.conn, &k.id, &ids)?;
    }
    Ok(DecisionResult {
        key_result: KrBrief {
            id: k.id,
            title: k.title,
        },
        items: items_of(resolved),
    })
}

/// 「付けない」: 候補を記録し、以後この KR には提案しない。
pub fn reject(ctx: &Ctx, kr: &str, refs: &[CandidateRef]) -> Result<DecisionResult> {
    let k = find_kr(ctx, kr)?;
    let resolved = resolve_refs(ctx, refs)?;
    let now = ctx.now();
    let tx = ctx.conn.unchecked_transaction()?;
    for r in &resolved {
        ctx.conn.execute(
            "INSERT OR REPLACE INTO kr_suggest_reject (kr_id, kind, target_id, at) VALUES (?1, ?2, ?3, ?4)",
            params![k.id, r.kind.as_str(), r.id, now],
        )?;
    }
    tx.commit()?;
    Ok(DecisionResult {
        key_result: KrBrief {
            id: k.id,
            title: k.title,
        },
        items: items_of(resolved),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::okr::{self as okr_svc, KrInput, ObjectiveInput};
    use crate::service::skill::{self, SkillInput};
    use crate::service::task::{self, TaskInput};
    use crate::service::testutil::Env;

    fn setup(ctx: &Ctx, obj: &str, kr: &str) -> String {
        let o = okr_svc::add_objective(
            ctx,
            &ObjectiveInput {
                title: obj.into(),
                from: None,
                to: None,
            },
        )
        .unwrap();
        okr_svc::add_kr(
            ctx,
            &KrInput {
                objective: o.id,
                title: kr.into(),
                target: None,
                skills: vec![],
            },
        )
        .unwrap()
        .id
    }

    fn add_task(ctx: &Ctx, title: &str) -> String {
        task::add(
            ctx,
            &TaskInput {
                title: Some(title.into()),
                ..Default::default()
            },
        )
        .unwrap()
        .task
        .id
    }

    fn add_skill(ctx: &Ctx, name: &str) -> String {
        skill::add(
            ctx,
            &SkillInput {
                name: name.into(),
                parent: None,
            },
        )
        .unwrap()
        .id
    }

    #[test]
    fn 日本語の名前の近さで候補が出る() {
        assert!(closeness("英語の教材を読む", "英語を学ぶ") >= 0.5);
        assert!(closeness("TOEIC 英語の問題集", "英語の勉強") >= 0.5);
        assert_eq!(closeness("経費精算", "新機能の設計"), 0.0);
        // 助詞だけの重なりは近いとみなさない。
        assert!(closeness("朝の散歩", "夜の読書") < 0.5);
        // 部分一致は 1。
        assert_eq!(closeness("会議", "週次会議の準備"), 1.0);
        assert!(closeness("rust learning", "Rust を学ぶ") >= 0.5);
    }

    #[test]
    fn 手元の候補を近さの順に出し_無関係なものは出さない() {
        let env = Env::new();
        let ctx = env.ctx();
        let kr = setup(&ctx, "英語力を上げる", "英語の勉強を続ける");
        let t1 = add_task(&ctx, "英語の教材を読む");
        add_task(&ctx, "経費精算");
        let s1 = add_skill(&ctx, "英語");
        add_skill(&ctx, "料理");
        let s = suggest_with(&ctx, &kr, None).unwrap();
        assert!(s.eligible);
        assert!(!s.ai.used);
        let got: Vec<(&str, &str)> = s
            .candidates
            .iter()
            .map(|c| (c.kind.as_str(), c.id.as_str()))
            .collect();
        assert!(got.contains(&("task", t1.as_str())));
        assert!(got.contains(&("skill", s1.as_str())));
        assert_eq!(s.candidates.len(), 2);
        assert!(s.candidates.iter().all(|c| c.source == "local"));
    }

    #[test]
    fn スキルか紐付けタスクがある_kr_は対象外() {
        let env = Env::new();
        let ctx = env.ctx();
        let kr = setup(&ctx, "英語力", "英語の勉強");
        add_task(&ctx, "英語の教材");
        let t = add_task(&ctx, "別のタスク");
        okr_svc::link_tasks(&ctx, &kr, &[t]).unwrap();
        let s = suggest_with(&ctx, &kr, None).unwrap();
        assert!(!s.eligible && s.candidates.is_empty());
    }

    #[test]
    fn 他の_kr_に紐付いたタスクは候補にしない() {
        let env = Env::new();
        let ctx = env.ctx();
        let kr = setup(&ctx, "英語力", "英語の勉強");
        let other = setup(&ctx, "別", "別の KR");
        let t = add_task(&ctx, "英語の教材");
        okr_svc::link_tasks(&ctx, &other, &[t]).unwrap();
        assert!(suggest_with(&ctx, &kr, None).unwrap().candidates.is_empty());
    }

    #[test]
    fn 付けるでタスクを紐付けスキルを加え_付けないは再び出ない() {
        let env = Env::new();
        let ctx = env.ctx();
        let kr = setup(&ctx, "英語力", "英語の勉強");
        let t1 = add_task(&ctx, "英語の教材を読む");
        let t2 = add_task(&ctx, "英語のリスニング");
        let s1 = add_skill(&ctx, "英語");
        let r = reject(&ctx, &kr, &[format!("task:{t2}").parse().unwrap()]).unwrap();
        assert_eq!(r.items.len(), 1);
        let s = suggest_with(&ctx, &kr, None).unwrap();
        assert!(s.candidates.iter().all(|c| c.id != t2));
        assert!(s.candidates.iter().any(|c| c.id == t1));

        let refs: Vec<CandidateRef> = vec![
            format!("task:{t1}").parse().unwrap(),
            "skill:英語".parse().unwrap(),
        ];
        let a = accept(&ctx, &kr, &refs).unwrap();
        assert_eq!(a.items.len(), 2);
        assert_eq!(
            repo::task::get(ctx.conn, &t1).unwrap().key_result_id.as_deref(),
            Some(kr.as_str())
        );
        let ids: Vec<String> = repo::skill::kr_skills(ctx.conn, &kr)
            .unwrap()
            .into_iter()
            .map(|s| s.id)
            .collect();
        assert_eq!(ids, vec![s1]);
        // 紐付けたので対象外になる。
        assert!(!suggest_with(&ctx, &kr, None).unwrap().eligible);
    }

    #[test]
    fn 見つからない候補があれば何も変えない() {
        let env = Env::new();
        let ctx = env.ctx();
        let kr = setup(&ctx, "英語力", "英語の勉強");
        let t1 = add_task(&ctx, "英語の教材を読む");
        let refs: Vec<CandidateRef> = vec![
            format!("task:{t1}").parse().unwrap(),
            "skill:存在しない".parse().unwrap(),
        ];
        assert!(accept(&ctx, &kr, &refs).is_err());
        assert!(repo::task::get(ctx.conn, &t1).unwrap().key_result_id.is_none());
        assert!("foo:x".parse::<CandidateRef>().is_err());
        assert!("task".parse::<CandidateRef>().is_err());
    }

    #[test]
    fn ai_の候補を合わせ_プールに無い_id_は捨てる() {
        let env = Env::new();
        let ctx = env.ctx();
        let kr = setup(&ctx, "健康", "運動を習慣にする");
        let t1 = add_task(&ctx, "ジムの入会手続き");
        let seen = std::cell::RefCell::new(String::new());
        let run = |prompt: &str| -> Result<String> {
            *seen.borrow_mut() = prompt.to_string();
            // t1 は唯一のタスク候補。存在しない t9 と低確信度の s1 は捨てられる。
            Ok("```json\n{\"suggestions\":[{\"id\":\"t1\",\"confidence\":0.9},{\"id\":\"t9\",\"confidence\":0.9},{\"id\":\"s1\",\"confidence\":0.1}]}\n```".into())
        };
        let s = suggest_with(&ctx, &kr, Some(&run)).unwrap();
        assert!(s.ai.used);
        assert_eq!(s.candidates.len(), 1);
        assert_eq!(s.candidates[0].id, t1);
        assert_eq!(s.candidates[0].source, "ai");
        // 送ったのは名前だけで、実際の ID は含まない。
        let p = seen.borrow();
        assert!(p.contains("運動を習慣にする") && p.contains("ジムの入会手続き"));
        assert!(!p.contains(&t1));
    }

    #[test]
    fn ai_が失敗しても手元の候補は出る() {
        let env = Env::new();
        let ctx = env.ctx();
        let kr = setup(&ctx, "英語力", "英語の勉強");
        add_task(&ctx, "英語の教材を読む");
        let run = |_: &str| -> Result<String> { Err(Error::Other(anyhow::anyhow!("boom"))) };
        let s = suggest_with(&ctx, &kr, Some(&run)).unwrap();
        assert!(!s.ai.used);
        assert!(s.ai.note.is_some());
        assert_eq!(s.candidates.len(), 1);
        let run = |_: &str| -> Result<String> { Ok("関係ありません".into()) };
        assert!(!suggest_with(&ctx, &kr, Some(&run)).unwrap().ai.used);
    }

    #[test]
    fn 両方が挙げた候補は_both_になる() {
        let env = Env::new();
        let ctx = env.ctx();
        let kr = setup(&ctx, "英語力", "英語の勉強");
        let t1 = add_task(&ctx, "英語の教材を読む");
        let run = |_: &str| -> Result<String> { Ok("{\"suggestions\":[{\"id\":\"t1\"}]}".into()) };
        let s = suggest_with(&ctx, &kr, Some(&run)).unwrap();
        assert_eq!(s.candidates.len(), 1);
        assert_eq!(
            (s.candidates[0].id.as_str(), s.candidates[0].source),
            (t1.as_str(), "both")
        );
    }
}
