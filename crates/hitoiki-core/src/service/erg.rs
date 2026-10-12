//! ERG（存在・関係・成長）の AI 判定（docs/dev/architecture.md「ERG の判定」）。
//!
//! ユーザーに ERG を選んでもらうのではなく、記録の内容から AI が判定し、ユーザーが確認・修正する。
//! 保存先は `erg_label`（記録の種類と ID ごとに最大 1 件。出所 `user` / `ai` / `suggested`）。
//!
//! - 対象（`subjects`）: 文章を持つ記録。チェックインのメモ、ハッスル、ABC（A・B・C・反論）、
//!   振り返りの回答（`answers_md`）。文章が無い記録は対象外。
//! - AI の応答は確信度が閾値（`ai.erg_auto_threshold`）以上なら採用（`ai`）、未満・確信度なしなら
//!   確認待ち（`suggested`。提案タグと理由だけ保存し、確定しない）。`none`（どれにも関わらない）も同様。
//! - `user` のラベルは AI で上書きしない。判定済み（確認待ちを含む）の記録は再判定しない。
//! - AI に渡すのは記録の文章だけ（IDは `r1`, `r2` … の連番に置き換える）。文章やプロンプトはログに出さない。

use super::reflect::{load_template, render_template, run_ai};
use super::{range_view, structured, Ctx, RangeArgs, RangeView};
use crate::model::{ErgChoice, ErgLabel, ErgRecordKind, ErgSource, ReflectionKind};
use crate::repo::{self, edit, erg as repo_erg, record, reflection};
use crate::timefmt::{self, Range};
use crate::{Error, Millis, Result};
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::time::Duration;

/// 3 つの観点の短い説明（画面・AI へのコンテキスト共通）。
pub const LEGEND: &str = "存在 = 体や生活の安心、関係 = 人とのつながり、成長 = 手応えや学び";
/// AI が使えないときのメッセージ。
pub const NOT_CONFIGURED: &str = "AI が設定されていないため判定しません";

/// 1 件あたりに AI へ渡す文字数の上限。
const MAX_TEXT_CHARS: usize = 600;
/// 1 回の AI 呼び出しにまとめる記録の数。
const BATCH_SIZE: usize = 30;
/// 理由の長さの上限。
const MAX_REASON_CHARS: usize = 120;

fn table_of(kind: ErgRecordKind) -> &'static str {
    match kind {
        ErgRecordKind::Checkin => "checkin",
        ErgRecordKind::Hassle => "hassle",
        ErgRecordKind::Abc => "abc_record",
        ErgRecordKind::Reflection => "reflection",
    }
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

fn nonblank(s: &Option<String>) -> Option<&str> {
    s.as_deref().map(str::trim).filter(|s| !s.is_empty())
}

// ===== 対象の記録 =====

/// ERG のラベルを付けられる記録 1 件。
#[derive(Debug, Clone)]
pub struct Subject {
    pub kind: ErgRecordKind,
    pub id: String,
    pub at: Millis,
    /// 判定に使う文章。無ければ `None`（ラベルが既にある記録だけが対象に含まれる）。
    pub text: Option<String>,
    pub label: Option<ErgLabel>,
}

/// 期間内の記録。文章があるか、ラベルが既にあるものだけを返す。
pub fn subjects(ctx: &Ctx, range: Range) -> Result<Vec<Subject>> {
    let labels: HashMap<(ErgRecordKind, String), ErgLabel> =
        repo_erg::list_range(ctx.conn, range.start, range.end)?
            .into_iter()
            .map(|l| ((l.record_kind, l.record_id.clone()), l))
            .collect();
    let mut out = Vec::new();
    let mut push = |kind: ErgRecordKind, id: String, at: Millis, text: Option<String>| {
        let label = labels.get(&(kind, id.clone())).cloned();
        if text.is_some() || label.is_some() {
            out.push(Subject {
                kind,
                id,
                at,
                text,
                label,
            });
        }
    };
    for c in record::list_checkins(ctx.conn, range.start, range.end)? {
        let text = nonblank(&c.note).map(String::from);
        push(ErgRecordKind::Checkin, c.id, c.at, text);
    }
    for h in record::list_hassles(ctx.conn, range.start, range.end)? {
        let text = Some(h.text.trim().to_string()).filter(|s| !s.is_empty());
        push(ErgRecordKind::Hassle, h.id, h.at, text);
    }
    for a in record::list_abc(ctx.conn, range.start, range.end)? {
        let text = abc_text(&a);
        push(ErgRecordKind::Abc, a.id, a.at, text);
    }
    for r in reflection::list_range(ctx.conn, range.start, range.end)? {
        let text = nonblank(&r.answers_md).map(String::from);
        push(ErgRecordKind::Reflection, r.id, r.at, text);
    }
    out.sort_by(|a, b| a.at.cmp(&b.at).then_with(|| a.id.cmp(&b.id)));
    Ok(out)
}

fn abc_text(a: &crate::model::AbcRecord) -> Option<String> {
    let mut parts = Vec::new();
    if !a.activating_event.trim().is_empty() {
        parts.push(format!("出来事: {}", a.activating_event.trim()));
    }
    if let Some(b) = nonblank(&a.belief) {
        parts.push(format!("受け止め方: {b}"));
    }
    if let Some(c) = nonblank(&a.consequence_emotion) {
        parts.push(format!("感情: {c}"));
    }
    if let Some(d) = nonblank(&a.dispute) {
        parts.push(format!("別の捉え方: {d}"));
    }
    (!parts.is_empty()).then(|| parts.join("\n"))
}

/// 記録の文章を取り出す（ラベルが付いた記録の表示用）。記録が無ければ `None`。
fn text_of(ctx: &Ctx, kind: ErgRecordKind, id: &str) -> Result<Option<(Millis, Option<String>)>> {
    let found = match kind {
        ErgRecordKind::Checkin => {
            edit::get_checkin(ctx.conn, id).map(|c| (c.at, nonblank(&c.note).map(String::from)))
        }
        ErgRecordKind::Hassle => record::get_hassle(ctx.conn, id).map(|h| (h.at, Some(h.text))),
        ErgRecordKind::Abc => edit::get_abc(ctx.conn, id).map(|a| {
            let t = abc_text(&a);
            (a.at, t)
        }),
        ErgRecordKind::Reflection => {
            reflection::get(ctx.conn, id).map(|r| (r.at, nonblank(&r.answers_md).map(String::from)))
        }
    };
    match found {
        Ok(v) => Ok(Some(v)),
        Err(Error::NotFound(_)) => Ok(None),
        Err(e) => Err(e),
    }
}

// ===== 分布 =====

/// 期間内の記録の ERG の分布（件数）。事実の集計だけで、評価は含まない。
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct ErgDistribution {
    pub existence: usize,
    pub relatedness: usize,
    pub growth: usize,
    /// どれにも関わらないと判定（確定）された記録。
    pub none: usize,
    /// 確認待ち（AI の提案のみで未確定）の記録。
    pub pending: usize,
    /// 未判定の記録（文章はあるがラベルが無い）。
    pub unlabeled: usize,
    /// 確定した件数（existence + relatedness + growth + none）のうち、AI の判定を採用したもの。
    pub ai_decided: usize,
    /// 対象の記録の総数。
    pub total: usize,
    /// 3 つの観点の短い説明。
    pub legend: &'static str,
}

pub fn distribution(ctx: &Ctx, range: Range) -> Result<ErgDistribution> {
    Ok(distribution_of(&subjects(ctx, range)?))
}

fn distribution_of(subjects: &[Subject]) -> ErgDistribution {
    let mut d = ErgDistribution {
        legend: LEGEND,
        ..Default::default()
    };
    for s in subjects {
        d.total += 1;
        match &s.label {
            None => d.unlabeled += 1,
            Some(l) if l.source == ErgSource::Suggested => d.pending += 1,
            Some(l) => {
                match l.tag {
                    ErgChoice::Existence => d.existence += 1,
                    ErgChoice::Relatedness => d.relatedness += 1,
                    ErgChoice::Growth => d.growth += 1,
                    ErgChoice::None => d.none += 1,
                }
                if l.source == ErgSource::Ai {
                    d.ai_decided += 1;
                }
            }
        }
    }
    d
}

// ===== AI の応答 =====

/// AI が 1 件について返した判定。
#[derive(Debug, Clone, PartialEq)]
pub struct Verdict {
    pub id: String,
    pub tag: ErgChoice,
    /// 0〜1。欠落・範囲外・数値でない場合は `None`（確認待ちとして扱う）。
    pub confidence: Option<f64>,
    pub reason: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Parsed {
    pub verdicts: Vec<Verdict>,
    /// 読み捨てた要素の数（`id` や `tag` が不正・未知のタグなど）。
    pub invalid: usize,
}

/// 判定用の構造化出力の JSON Schema。
pub fn schema_json() -> String {
    serde_json::json!({
        "type": "object",
        "properties": {
            "labels": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "id": { "type": "string" },
                        "tag": { "type": "string", "enum": ["existence", "relatedness", "growth", "none"] },
                        "confidence": { "type": "number", "minimum": 0, "maximum": 1 },
                        "reason": { "type": "string" }
                    },
                    "required": ["id", "tag", "confidence", "reason"]
                }
            }
        },
        "required": ["labels"]
    })
    .to_string()
}

/// コードフェンスや前置きに包まれた JSON オブジェクトを探す（構造化出力を使わないコマンド向け）。
fn find_braced_json(s: &str) -> Option<Value> {
    let start = s.find('{')?;
    let end = s.rfind('}')?;
    if end <= start {
        return None;
    }
    serde_json::from_str(&s[start..=end]).ok()
}

/// AI コマンドの標準出力から判定を読む。JSON として読めない・`labels` が無い場合はエラー。
/// 要素ごとに検査し、`id` が文字列でない・未知のタグの要素は読み捨てる（`invalid` に数える）。
/// 確信度が 0〜1 の数値でない場合は `None` にする（採用せず確認待ちになる）。
pub fn parse_response(stdout: &str) -> Result<Parsed> {
    let value = match structured::extract_value(stdout, "labels") {
        Ok(v) => structured::unwrap_json_string(&v)?,
        Err(e) => find_braced_json(stdout).ok_or(e)?,
    };
    let labels = value
        .get("labels")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::Other(anyhow::anyhow!("AI の応答に labels がありません")))?;
    let mut parsed = Parsed::default();
    for item in labels {
        let id = item
            .get("id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let tag = item
            .get("tag")
            .and_then(Value::as_str)
            .and_then(|t| t.trim().parse::<ErgChoice>().ok());
        let (Some(id), Some(tag)) = (id, tag) else {
            parsed.invalid += 1;
            continue;
        };
        let confidence = item
            .get("confidence")
            .and_then(Value::as_f64)
            .filter(|c| c.is_finite() && (0.0..=1.0).contains(c));
        let reason = item
            .get("reason")
            .and_then(Value::as_str)
            .map(|r| truncate(r.trim(), MAX_REASON_CHARS))
            .unwrap_or_default();
        parsed.verdicts.push(Verdict {
            id: id.to_string(),
            tag,
            confidence,
            reason,
        });
    }
    Ok(parsed)
}

// ===== 判定の実行 =====

/// 設定された AI コマンドがあるか。
pub fn ai_configured(ctx: &Ctx) -> bool {
    ctx.cfg.ai.command.first().is_some_and(|c| !c.trim().is_empty())
}

/// 採用する確信度の下限（0〜1。不正な値は 0.8）。
pub fn threshold(ctx: &Ctx) -> f64 {
    let t = ctx.cfg.ai.erg_auto_threshold;
    if t.is_finite() && (0.0..=1.0).contains(&t) {
        t
    } else {
        0.8
    }
}

/// AI を呼ぶ関数（プロンプトを渡して標準出力を受け取る）。テストで差し替える。
pub type Runner<'a> = &'a dyn Fn(&str) -> Result<String>;

/// 設定された AI コマンドで実行する。構造化出力が使えるなら付け、失敗したら付けずにもう一度試す。
fn run_configured(ctx: &Ctx, prompt: &str) -> Result<String> {
    let ai = &ctx.cfg.ai;
    let timeout = Duration::from_secs(ai.timeout_sec.max(1));
    if structured::enabled(ai) {
        if let Ok((extra, _file)) = structured::build_args(ai, &schema_json()) {
            if !extra.is_empty() {
                let mut argv = ai.command.clone();
                argv.extend(extra);
                if let Ok(out) = run_ai(&argv, prompt, timeout) {
                    if parse_response(&out).is_ok() {
                        return Ok(out);
                    }
                }
            }
        }
    }
    run_ai(&ai.command, prompt, timeout)
}

/// 判定の対象になる記録（文章があり、ラベルが無いもの）。
pub fn candidates(ctx: &Ctx, range: Range) -> Result<Vec<Subject>> {
    Ok(subjects(ctx, range)?
        .into_iter()
        .filter(|s| s.label.is_none() && s.text.is_some())
        .collect())
}

fn kind_label(kind: ErgRecordKind) -> &'static str {
    match kind {
        ErgRecordKind::Checkin => "チェックインのメモ",
        ErgRecordKind::Hassle => "ハッスル（引っかかったこと）",
        ErgRecordKind::Abc => "ABC の記録",
        ErgRecordKind::Reflection => "振り返りの回答",
    }
}

/// AI へ渡すプロンプトを作る（AI は呼ばない）。記録の ID は `r1`, `r2` … に置き換える。
pub fn build_prompt(ctx: &Ctx, batch: &[Subject]) -> Result<String> {
    let records: Vec<Value> = batch
        .iter()
        .enumerate()
        .map(|(i, s)| {
            serde_json::json!({
                "id": format!("r{}", i + 1),
                "kind": kind_label(s.kind),
                "text": truncate(s.text.as_deref().unwrap_or_default(), MAX_TEXT_CHARS),
            })
        })
        .collect();
    let json = serde_json::to_string_pretty(&records).map_err(anyhow::Error::from)?;
    let tpl = load_template(ctx, "erg_classify")?;
    Ok(render_template(&tpl, &[("records", &json)]))
}

/// 判定した記録 1 件の結果。
#[derive(Debug, Clone, Serialize)]
pub struct ClassifiedItem {
    pub record_kind: ErgRecordKind,
    pub record_id: String,
    pub at: Millis,
    /// 記録の冒頭（確認用。ログには出さない）。
    pub excerpt: String,
    pub tag: ErgChoice,
    pub confidence: Option<f64>,
    pub reason: String,
    /// `ai`（採用）か `suggested`（確認待ち）。
    pub source: ErgSource,
}

#[derive(Debug, Clone, Serialize)]
pub struct ClassifyOutcome {
    pub range: RangeView,
    pub dry_run: bool,
    /// 判定しなかった理由（AI が未設定など）。
    pub skipped: Option<String>,
    /// 判定の対象にした記録の数。
    pub targets: usize,
    /// AI の判定を採用した数。
    pub adopted: usize,
    /// 確認待ちにした数。
    pub pending: usize,
    /// AI が返さなかった・読めなかったため未判定のままの数。
    pub unread: usize,
    pub items: Vec<ClassifiedItem>,
}

/// 指定範囲の未判定の記録を、設定された AI で判定して保存する（`dry_run` なら保存しない）。
pub fn classify(ctx: &Ctx, args: &RangeArgs, dry_run: bool) -> Result<ClassifyOutcome> {
    let range = ctx.range(args)?;
    classify_range(ctx, range, dry_run, &|p| run_configured(ctx, p))
}

/// `classify` の本体。AI の呼び出しを `runner` で差し替えられる。
/// AI が未設定なら何もせず、`skipped` に理由を入れて返す（エラーにしない）。
pub fn classify_range(ctx: &Ctx, range: Range, dry_run: bool, runner: Runner) -> Result<ClassifyOutcome> {
    let tz = ctx.tz()?;
    let mut out = ClassifyOutcome {
        range: range_view(tz, range),
        dry_run,
        skipped: None,
        targets: 0,
        adopted: 0,
        pending: 0,
        unread: 0,
        items: vec![],
    };
    if !ai_configured(ctx) {
        out.skipped = Some(NOT_CONFIGURED.to_string());
        return Ok(out);
    }
    let cands = candidates(ctx, range)?;
    out.targets = cands.len();
    let threshold = threshold(ctx);
    for batch in cands.chunks(BATCH_SIZE) {
        let prompt = build_prompt(ctx, batch)?;
        let stdout = runner(&prompt)?;
        let parsed = parse_response(&stdout)?;
        let mut done = vec![false; batch.len()];
        for v in parsed.verdicts {
            let Some(i) =
                v.id.strip_prefix('r')
                    .and_then(|n| n.parse::<usize>().ok())
                    .and_then(|n| n.checked_sub(1))
                    .filter(|i| *i < batch.len())
            else {
                continue;
            };
            if done[i] {
                continue;
            }
            let s = &batch[i];
            let adopt = v.confidence.is_some_and(|c| c >= threshold);
            let label = ErgLabel {
                record_kind: s.kind,
                record_id: s.id.clone(),
                tag: v.tag,
                source: if adopt {
                    ErgSource::Ai
                } else {
                    ErgSource::Suggested
                },
                confidence: v.confidence,
                reason: Some(v.reason.clone()).filter(|r| !r.is_empty()),
                at: ctx.now(),
            };
            // 保存中にユーザーが付けていた場合は更新されない（ユーザー確定が最優先）。
            if !dry_run && !repo_erg::set_auto(ctx.conn, &label)? {
                continue;
            }
            done[i] = true;
            if adopt {
                out.adopted += 1;
            } else {
                out.pending += 1;
            }
            out.items.push(ClassifiedItem {
                record_kind: s.kind,
                record_id: s.id.clone(),
                at: s.at,
                excerpt: truncate(s.text.as_deref().unwrap_or_default(), 60),
                tag: label.tag,
                confidence: label.confidence,
                reason: v.reason,
                source: label.source,
            });
        }
        out.unread += done.iter().filter(|d| !**d).count();
    }
    Ok(out)
}

/// 終業・週次の振り返りを生成する前に、その期間の未判定分を判定する。
/// `ai.erg_classify` が false・AI が未設定・失敗のいずれでも振り返りは続けるので、エラーは返さない
/// （失敗はログに残す。記録の文章やプロンプトは出さない）。
pub fn classify_before_reflection(ctx: &Ctx, kind: ReflectionKind, date: Option<&str>) {
    if kind == ReflectionKind::DayStart || !ctx.cfg.ai.erg_classify || !ai_configured(ctx) {
        return;
    }
    let run = || -> Result<ClassifyOutcome> {
        let tz = ctx.tz()?;
        let d = timefmt::parse_date(tz, date.unwrap_or("today"), ctx.now())?;
        let range = if kind == ReflectionKind::Weekly {
            timefmt::week_range(tz, d)
        } else {
            timefmt::day_range(tz, d)
        };
        classify_range(ctx, range, false, &|p| run_configured(ctx, p))
    };
    match run() {
        Ok(o) => tracing::info!(
            targets = o.targets,
            adopted = o.adopted,
            pending = o.pending,
            "ERG を判定しました"
        ),
        Err(e) => tracing::warn!(error = %e, "ERG の判定に失敗したので、振り返りはそのまま続けます"),
    }
}

// ===== ユーザーによる確認・修正 =====

/// 記録の指定（ID の前方一致。種類をまたいで探す）を 1 件に解決する。
pub fn resolve_record(ctx: &Ctx, prefix: &str) -> Result<(ErgRecordKind, String)> {
    let mut found = Vec::new();
    for kind in ErgRecordKind::ALL {
        match repo::resolve_prefix(ctx.conn, table_of(*kind), prefix) {
            Ok(id) => found.push((*kind, id)),
            Err(Error::NotFound(_)) => {}
            Err(e) => return Err(e),
        }
    }
    match found.len() {
        0 => Err(Error::NotFound(format!("記録 {prefix}"))),
        1 => Ok(found.remove(0)),
        _ => Err(Error::Ambiguous(format!(
            "{prefix}: {}",
            found
                .iter()
                .map(|(k, id)| format!("{k}:{id}"))
                .collect::<Vec<_>>()
                .join(", ")
        ))),
    }
}

/// ユーザーがタグを付ける・直す（出所 `user`）。`none` はどれにも関わらないという確定。
pub fn set_for(ctx: &Ctx, kind: ErgRecordKind, id: &str, choice: ErgChoice) -> Result<ErgLabel> {
    let id = repo::resolve_prefix(ctx.conn, table_of(kind), id)?;
    repo_erg::set_user(ctx.conn, kind, &id, choice, ctx.now())?;
    repo_erg::get(ctx.conn, kind, &id)?.ok_or_else(|| Error::NotFound(format!("ERG ラベル {id}")))
}

/// 記録の指定（前方一致）でタグを付ける・直す。
pub fn set(ctx: &Ctx, reference: &str, choice: ErgChoice) -> Result<ErgLabel> {
    let (kind, id) = resolve_record(ctx, reference)?;
    set_for(ctx, kind, &id, choice)
}

/// 確認待ちの提案をそのまま確定する（「これでよい」）。
pub fn confirm_for(ctx: &Ctx, kind: ErgRecordKind, id: &str) -> Result<ErgLabel> {
    let id = repo::resolve_prefix(ctx.conn, table_of(kind), id)?;
    let l = repo_erg::get(ctx.conn, kind, &id)?
        .filter(|l| l.source == ErgSource::Suggested)
        .ok_or_else(|| Error::InvalidArgument(format!("{id} は確認待ちではありません")))?;
    set_for(ctx, kind, &id, l.tag)
}

pub fn confirm(ctx: &Ctx, reference: &str) -> Result<ErgLabel> {
    let (kind, id) = resolve_record(ctx, reference)?;
    confirm_for(ctx, kind, &id)
}

/// ラベルと記録の文章（確認用）。
#[derive(Debug, Clone, Serialize)]
pub struct ErgItem {
    #[serde(flatten)]
    pub label: ErgLabel,
    /// 記録の日時（Unix ミリ秒）。
    pub record_at: Millis,
    /// 記録の冒頭。
    pub excerpt: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ErgReview {
    pub items: Vec<ErgItem>,
}

/// 確認待ち（AI の提案で未確定）の一覧。
pub fn review(ctx: &Ctx) -> Result<ErgReview> {
    let mut items = Vec::new();
    for l in repo_erg::list_pending(ctx.conn)? {
        // 記録が消えているものは出さない（削除時に消えるが念のため）。
        let Some((at, text)) = text_of(ctx, l.record_kind, &l.record_id)? else {
            continue;
        };
        items.push(ErgItem {
            record_at: at,
            excerpt: truncate(text.as_deref().unwrap_or_default(), 80),
            label: l,
        });
    }
    Ok(ErgReview { items })
}

/// 期間内の記録に付いたラベルと分布（記録画面・振り返り画面向け）。
#[derive(Debug, Clone, Serialize)]
pub struct ErgOverview {
    pub range: RangeView,
    pub distribution: ErgDistribution,
    pub labels: Vec<ErgLabel>,
    /// 確認待ちの数（全期間）。
    pub pending_total: usize,
}

pub fn overview(ctx: &Ctx, args: &RangeArgs) -> Result<ErgOverview> {
    let tz = ctx.tz()?;
    let range = ctx.range(args)?;
    let subs = subjects(ctx, range)?;
    Ok(ErgOverview {
        range: range_view(tz, range),
        distribution: distribution_of(&subs),
        labels: subs.into_iter().filter_map(|s| s.label).collect(),
        pending_total: repo_erg::list_pending(ctx.conn)?.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ErgTag;
    use crate::service::record::{self, AbcInput, CheckInput, HassleInput};
    use crate::service::testutil::Env;

    fn json_ok(labels: &str) -> String {
        format!(r#"{{"labels":{labels}}}"#)
    }

    fn env_with_ai() -> Env {
        let mut env = Env::new();
        env.cfg.ai.command = vec!["fake-ai".into()];
        env
    }

    fn today(env: &Env) -> Range {
        let tz = env.cfg.tz().unwrap();
        timefmt::day_range(tz, timefmt::local_date_of(tz, crate::service::testutil::now()))
    }

    fn hassle(ctx: &Ctx, text: &str) -> String {
        record::hassle(
            ctx,
            &HassleInput {
                text: text.into(),
                intensity: None,
            },
        )
        .unwrap()
        .id
    }

    #[test]
    fn parse_valid_and_envelope() {
        let p = parse_response(&json_ok(
            r#"[{"id":"r1","tag":"growth","confidence":0.9,"reason":"学び"},{"id":"r2","tag":"none","confidence":0.5,"reason":""}]"#,
        ))
        .unwrap();
        assert_eq!(p.invalid, 0);
        assert_eq!(p.verdicts[0].tag, ErgChoice::Growth);
        assert_eq!(p.verdicts[0].confidence, Some(0.9));
        assert_eq!(p.verdicts[1].tag, ErgChoice::None);
        // claude の構造化出力の封筒
        let env = serde_json::json!({"type":"result","is_error":false,
            "structured_output": serde_json::from_str::<Value>(&json_ok(r#"[{"id":"r1","tag":"existence","confidence":1,"reason":"x"}]"#)).unwrap()});
        assert_eq!(parse_response(&env.to_string()).unwrap().verdicts.len(), 1);
        // コードフェンス・前置き付き
        let fenced = format!(
            "結果です。\n```json\n{}\n```\n",
            json_ok(r#"[{"id":"r1","tag":"relatedness","confidence":0.8,"reason":"y"}]"#)
        );
        assert_eq!(
            parse_response(&fenced).unwrap().verdicts[0].tag,
            ErgChoice::Relatedness
        );
    }

    #[test]
    fn parse_rejects_bad_input_and_skips_bad_items() {
        assert!(parse_response("これは JSON ではありません").is_err());
        assert!(parse_response(r#"{"other":1}"#).is_err());
        assert!(parse_response(r#"{"labels":"x"}"#).is_err());
        // 未知のタグ・id なしは読み捨てる
        let p = parse_response(&json_ok(
            r#"[{"id":"r1","tag":"power","confidence":0.9,"reason":"x"},{"tag":"growth"},{"id":"r3","tag":"growth","confidence":0.9,"reason":"ok"}]"#,
        ))
        .unwrap();
        assert_eq!((p.verdicts.len(), p.invalid), (1, 2));
        // 範囲外・数値でない確信度は None（採用しない）
        let p = parse_response(&json_ok(
            r#"[{"id":"r1","tag":"growth","confidence":1.5,"reason":""},{"id":"r2","tag":"growth","confidence":-0.1},{"id":"r3","tag":"growth","confidence":"high"},{"id":"r4","tag":"growth"}]"#,
        ))
        .unwrap();
        assert!(p.verdicts.iter().all(|v| v.confidence.is_none()));
        assert_eq!(p.verdicts.len(), 4);
    }

    #[test]
    fn threshold_decides_adopt_or_pending() {
        let env = env_with_ai();
        let ctx = env.ctx();
        let a = hassle(&ctx, "新しい言語を触って手応えがあった");
        let b = hassle(&ctx, "同僚に相談できた気がする");
        let c = hassle(&ctx, "メモ");
        let d = hassle(&ctx, "確信度なし");
        let runner = |_: &str| -> Result<String> {
            Ok(json_ok(
                r#"[{"id":"r1","tag":"growth","confidence":0.8,"reason":"学び"},
                    {"id":"r2","tag":"relatedness","confidence":0.79,"reason":"人"},
                    {"id":"r3","tag":"none","confidence":0.95,"reason":"事務"},
                    {"id":"r4","tag":"existence","reason":"確信度なし"}]"#,
            ))
        };
        let o = classify_range(&ctx, today(&env), false, &runner).unwrap();
        assert_eq!((o.targets, o.adopted, o.pending, o.unread), (4, 2, 2, 0));
        let get = |id: &str| {
            repo_erg::get(ctx.conn, ErgRecordKind::Hassle, id)
                .unwrap()
                .unwrap()
        };
        // 時刻が同じなので ID 順だが、ここでは ID ごとに確認する
        let labels: Vec<ErgLabel> = [&a, &b, &c, &d].iter().map(|i| get(i)).collect();
        let by_tag = |t: ErgChoice| labels.iter().find(|l| l.tag == t).unwrap();
        assert_eq!(by_tag(ErgChoice::Growth).source, ErgSource::Ai);
        assert_eq!(by_tag(ErgChoice::None).source, ErgSource::Ai);
        assert_eq!(by_tag(ErgChoice::Relatedness).source, ErgSource::Suggested);
        let ex = by_tag(ErgChoice::Existence);
        assert_eq!((ex.source, ex.confidence), (ErgSource::Suggested, None));
        assert!(by_tag(ErgChoice::Relatedness).reason.is_some());
        // 判定済みは再判定しない
        let o2 = classify_range(&ctx, today(&env), false, &|_| panic!("呼ばれない")).unwrap();
        assert_eq!(o2.targets, 0);
        // 確認待ちは一覧に出て、確定すると user になる
        let r = review(&ctx).unwrap();
        assert_eq!(r.items.len(), 2);
        let pending_id = r.items[0].label.record_id.clone();
        let want = r.items[0].label.tag;
        let l = confirm_for(&ctx, ErgRecordKind::Hassle, &pending_id).unwrap();
        assert_eq!((l.source, l.tag, l.confidence), (ErgSource::User, want, None));
        assert!(confirm_for(&ctx, ErgRecordKind::Hassle, &pending_id).is_err());
        assert_eq!(review(&ctx).unwrap().items.len(), 1);
    }

    #[test]
    fn user_label_is_never_overwritten() {
        let env = env_with_ai();
        let ctx = env.ctx();
        let id = hassle(&ctx, "朝から頭が重い");
        let l = set(&ctx, &id[..10], ErgChoice::Existence).unwrap();
        assert_eq!(l.source, ErgSource::User);
        // 文章はあってもラベルがあるので対象にならない
        assert!(candidates(&ctx, today(&env)).unwrap().is_empty());
        // 競合（AI の保存より前にユーザーが付けていた）でも更新されない
        let ai = ErgLabel {
            record_kind: ErgRecordKind::Hassle,
            record_id: id.clone(),
            tag: ErgChoice::Growth,
            source: ErgSource::Ai,
            confidence: Some(0.99),
            reason: None,
            at: 1,
        };
        assert!(!repo_erg::set_auto(ctx.conn, &ai).unwrap());
        let got = repo_erg::get(ctx.conn, ErgRecordKind::Hassle, &id)
            .unwrap()
            .unwrap();
        assert_eq!((got.tag, got.source), (ErgChoice::Existence, ErgSource::User));
        // AI の判定はユーザーが直せる（none も確定として扱う）
        let id2 = hassle(&ctx, "別のこと");
        let runner = |_: &str| -> Result<String> {
            Ok(json_ok(
                r#"[{"id":"r1","tag":"growth","confidence":0.9,"reason":"x"}]"#,
            ))
        };
        classify_range(&ctx, today(&env), false, &runner).unwrap();
        let fixed = set_for(&ctx, ErgRecordKind::Hassle, &id2, ErgChoice::None).unwrap();
        assert_eq!((fixed.source, fixed.tag), (ErgSource::User, ErgChoice::None));
    }

    #[test]
    fn not_configured_skips_without_error() {
        let mut env = Env::new();
        env.cfg.ai.command = vec![];
        let ctx = env.ctx();
        hassle(&ctx, "何か");
        let o = classify_range(&ctx, today(&env), false, &|_| panic!("呼ばれない")).unwrap();
        assert_eq!(o.skipped.as_deref(), Some(NOT_CONFIGURED));
        assert_eq!(o.targets, 0);
        assert!(repo_erg::list_pending(ctx.conn).unwrap().is_empty());
        // 振り返りの前処理も何もしない
        classify_before_reflection(&ctx, ReflectionKind::DayEnd, None);
    }

    #[test]
    fn dry_run_saves_nothing_and_unread_is_counted() {
        let env = env_with_ai();
        let ctx = env.ctx();
        hassle(&ctx, "a");
        hassle(&ctx, "b");
        let runner = |_: &str| -> Result<String> {
            Ok(json_ok(
                r#"[{"id":"r1","tag":"growth","confidence":0.9,"reason":"x"},{"id":"r9","tag":"growth","confidence":0.9}]"#,
            ))
        };
        let o = classify_range(&ctx, today(&env), true, &runner).unwrap();
        assert_eq!((o.adopted, o.unread, o.items.len()), (1, 1, 1));
        assert!(repo_erg::list_range(ctx.conn, 0, i64::MAX).unwrap().is_empty());
        // AI の失敗は呼び出し元へ返る（途中まで保存済みのものは残る）
        let fail = |_: &str| -> Result<String> { Err(Error::Other(anyhow::anyhow!("失敗"))) };
        assert!(classify_range(&ctx, today(&env), false, &fail).is_err());
        let unreadable = |_: &str| -> Result<String> { Ok("読めない".into()) };
        assert!(classify_range(&ctx, today(&env), false, &unreadable).is_err());
    }

    #[test]
    fn subjects_cover_each_text_kind_and_distribution_counts() {
        let env = env_with_ai();
        let ctx = env.ctx();
        // チェックイン: メモ無しは対象外、メモ有りは対象、ERG をユーザーが付けたものはメモ無しでも数える
        record::check(
            &ctx,
            &CheckInput {
                energy: Some(3),
                ..Default::default()
            },
        )
        .unwrap();
        record::check(
            &ctx,
            &CheckInput {
                note: Some("よく眠れた".into()),
                ..Default::default()
            },
        )
        .unwrap();
        record::check(
            &ctx,
            &CheckInput {
                energy: Some(4),
                erg: Some(ErgTag::Growth),
                ..Default::default()
            },
        )
        .unwrap();
        hassle(&ctx, "会議が長い");
        let abc = record::abc_add(
            &ctx,
            &AbcInput {
                activating_event: Some("指摘された".into()),
                belief: Some("向いていない".into()),
                ..Default::default()
            },
        )
        .unwrap();
        let cands = candidates(&ctx, today(&env)).unwrap();
        assert_eq!(cands.len(), 3);
        let abc_text = cands
            .iter()
            .find(|c| c.id == abc.id)
            .unwrap()
            .text
            .clone()
            .unwrap();
        assert!(abc_text.contains("出来事: 指摘された") && abc_text.contains("受け止め方: 向いていない"));
        // 振り返りの回答も対象
        let refl = crate::model::Reflection {
            id: "01HREFL0000000000000000000".into(),
            at: crate::service::testutil::now(),
            kind: ReflectionKind::DayEnd,
            context_json: "{}".into(),
            questions_md: "q".into(),
            answers_md: Some("今日は人と話せてよかった".into()),
            summary_md: None,
            structured_json: None,
            answers_json: None,
        };
        reflection::insert(ctx.conn, &refl).unwrap();
        assert_eq!(candidates(&ctx, today(&env)).unwrap().len(), 4);

        let d = distribution(&ctx, today(&env)).unwrap();
        assert_eq!((d.growth, d.unlabeled, d.pending, d.total), (1, 4, 0, 5));
        assert_eq!(d.legend, LEGEND);
        // 判定後の分布
        let runner = |_: &str| -> Result<String> {
            Ok(json_ok(
                r#"[{"id":"r1","tag":"existence","confidence":0.9,"reason":"x"},
                    {"id":"r2","tag":"none","confidence":0.9,"reason":"x"},
                    {"id":"r3","tag":"relatedness","confidence":0.3,"reason":"x"},
                    {"id":"r4","tag":"relatedness","confidence":0.9,"reason":"x"}]"#,
            ))
        };
        classify_range(&ctx, today(&env), false, &runner).unwrap();
        let d = distribution(&ctx, today(&env)).unwrap();
        assert_eq!(
            (
                d.existence,
                d.relatedness,
                d.growth,
                d.none,
                d.pending,
                d.unlabeled,
                d.ai_decided,
                d.total
            ),
            (1, 1, 1, 1, 1, 0, 3, 5)
        );
        let ov = overview(&ctx, &RangeArgs::default()).unwrap();
        assert_eq!((ov.labels.len(), ov.pending_total), (5, 1));
    }

    #[test]
    fn prompt_uses_aliases_not_ids_and_truncates() {
        let env = env_with_ai();
        let ctx = env.ctx();
        let id = hassle(&ctx, &"あ".repeat(MAX_TEXT_CHARS + 50));
        let cands = candidates(&ctx, today(&env)).unwrap();
        let p = build_prompt(&ctx, &cands).unwrap();
        assert!(p.contains("\"id\": \"r1\"") && !p.contains(&id) && !p.contains("{{records}}"));
        assert!(p.contains('…') && !p.contains(&"あ".repeat(MAX_TEXT_CHARS + 1)));
    }

    #[test]
    fn editing_text_resets_ai_label_but_not_user_label() {
        let env = env_with_ai();
        let ctx = env.ctx();
        let id = hassle(&ctx, "元の文章");
        let runner = |_: &str| -> Result<String> {
            Ok(json_ok(
                r#"[{"id":"r1","tag":"growth","confidence":0.9,"reason":"x"}]"#,
            ))
        };
        classify_range(&ctx, today(&env), false, &runner).unwrap();
        record::hassle_edit(
            &ctx,
            &id,
            &HassleInput {
                text: "直した文章".into(),
                intensity: None,
            },
        )
        .unwrap();
        assert!(repo_erg::get(ctx.conn, ErgRecordKind::Hassle, &id)
            .unwrap()
            .is_none());
        set(&ctx, &id, ErgChoice::Growth).unwrap();
        record::hassle_edit(
            &ctx,
            &id,
            &HassleInput {
                text: "また直した".into(),
                intensity: None,
            },
        )
        .unwrap();
        assert!(repo_erg::get(ctx.conn, ErgRecordKind::Hassle, &id)
            .unwrap()
            .is_some());
        // 記録を消すとラベルも消える
        record::hassle_delete(&ctx, &id, crate::service::impact::DeleteMode::FORCE).unwrap();
        assert!(repo_erg::get(ctx.conn, ErgRecordKind::Hassle, &id)
            .unwrap()
            .is_none());
    }

    #[test]
    fn resolve_record_errors() {
        let env = env_with_ai();
        let ctx = env.ctx();
        assert!(matches!(
            set(&ctx, "ZZZZ", ErgChoice::None),
            Err(Error::NotFound(_))
        ));
        assert!(matches!(
            set(&ctx, "", ErgChoice::None),
            Err(Error::InvalidArgument(_))
        ));
    }
}
