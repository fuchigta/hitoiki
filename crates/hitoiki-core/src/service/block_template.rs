//! ブロックのテンプレート（働き方のパターン。docs/dev/architecture.md「ブロックのテンプレート」）。
//!
//! テンプレートはローカル時刻（HH:MM）のブロックの集まりで、曜日を割り当てられる。
//! 適用すると、指定日の `time_block` を作る。適用は履歴に残り、直前の適用を取り消せる。

use super::plan::BlockView;
use super::{range_view, task_titles, Ctx, RangeView};
use crate::id::new_id;
use crate::model::TimeBlock;
use crate::repo::block_template::{self as repo_tpl, Application, Item, Template};
use crate::repo::{self, calendar, plan};
use crate::timefmt::{self, Range};
use crate::{Error, Millis, Result};
use chrono::{Datelike, NaiveDate};
use serde::{Deserialize, Serialize};

// ===== 入力・出力 =====

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ItemInput {
    /// ローカル時刻 `HH:MM`。
    pub start: String,
    /// ローカル時刻 `HH:MM`（`24:00` も可）。
    pub end: String,
    pub label: Option<String>,
    pub focus: bool,
    /// タスク ID（前方一致）。
    pub task: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct TemplateInput {
    pub name: String,
    pub description: Option<String>,
    /// 1=月 … 7=日。
    pub weekdays: Vec<u8>,
    pub items: Vec<ItemInput>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ItemView {
    pub id: String,
    pub start: String,
    pub end: String,
    pub start_min: u32,
    pub end_min: u32,
    pub label: Option<String>,
    pub focus: bool,
    pub task_id: Option<String>,
    pub task_title: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TemplateView {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub weekdays: Vec<u8>,
    pub created_at: Millis,
    pub updated_at: Millis,
    pub blocks: Vec<ItemView>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TemplateList {
    pub templates: Vec<TemplateView>,
}

/// 既存のブロック・カレンダー予定との重なり方。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Overlap {
    /// 重なるテンプレートのブロックは作らない。
    #[default]
    Skip,
    /// 重なっていても作る。
    Overlay,
}

impl std::str::FromStr for Overlap {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self> {
        match s {
            "skip" => Ok(Overlap::Skip),
            "overlay" => Ok(Overlap::Overlay),
            _ => Err(Error::InvalidArgument(format!(
                "重なりの扱いは skip か overlay です: {s}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ApplyInput {
    /// テンプレートの名前または ID（前方一致）。
    pub template: String,
    /// 適用する日（`today` / `YYYY-MM-DD`）。省略は今日。
    pub date: Option<String>,
    pub overlap: Overlap,
}

#[derive(Debug, Clone, Serialize)]
pub struct Conflict {
    /// `block`（既存のブロック）または `calendar`（busy のカレンダー予定）。
    pub kind: &'static str,
    pub title: String,
    pub start_at: Millis,
    pub end_at: Millis,
}

#[derive(Debug, Clone, Serialize)]
pub struct PreviewBlock {
    pub start_at: Millis,
    pub end_at: Millis,
    pub start: String,
    pub end: String,
    pub label: Option<String>,
    pub focus: bool,
    pub task_id: Option<String>,
    pub task_title: Option<String>,
    pub conflicts: Vec<Conflict>,
}

impl Preview {
    /// 重なりの扱いの表示名。
    pub fn overlap_label(&self) -> &'static str {
        match self.overlap {
            Overlap::Skip => "作らない",
            Overlap::Overlay => "重ねて作る",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Preview {
    pub template: String,
    pub date: String,
    pub range: RangeView,
    pub overlap: Overlap,
    pub blocks: Vec<PreviewBlock>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ApplyResult {
    pub application_id: String,
    pub template: String,
    pub date: String,
    pub created: Vec<BlockView>,
    /// 重なりのために作らなかったブロック。
    pub skipped: Vec<PreviewBlock>,
}

#[derive(Debug, Clone, Serialize)]
pub struct UndoResult {
    pub template: String,
    pub date: String,
    /// 実際に削除したブロックの数（すでに消されたものは含まない）。
    pub removed: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct Suggestion {
    pub date: String,
    /// 1=月 … 7=日。
    pub weekday: u8,
    pub templates: Vec<TemplateView>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct SaveFromInput {
    pub name: String,
    pub description: Option<String>,
    /// 元にする日（`today` / `YYYY-MM-DD`）。省略は今日。
    pub date: Option<String>,
    pub weekdays: Vec<u8>,
}

// ===== 時刻・曜日の解釈 =====

/// `HH:MM` を 0 時からの分にする。`allow_24` なら `24:00` も受け付ける。
pub fn parse_hm(s: &str, allow_24: bool) -> Result<u32> {
    let err = || Error::InvalidArgument(format!("時刻は HH:MM で指定してください: {s}"));
    let (h, m) = s.trim().split_once(':').ok_or_else(err)?;
    let h: u32 = h.parse().map_err(|_| err())?;
    let m: u32 = m.parse().map_err(|_| err())?;
    if m >= 60 {
        return Err(err());
    }
    let total = h * 60 + m;
    if total > 24 * 60 || (total == 24 * 60 && !allow_24) {
        return Err(err());
    }
    Ok(total)
}

pub fn fmt_hm(min: u32) -> String {
    format!("{:02}:{:02}", min / 60, min % 60)
}

impl ItemInput {
    /// CLI の `--block` の書式を解釈する: `09:00-10:30[,focus][,task=ID][,label=ラベル]`。
    /// ラベルは最後に書く（ラベル中のカンマはそのまま入る）。
    pub fn parse_spec(spec: &str) -> Result<Self> {
        let err = || {
            Error::InvalidArgument(format!(
                "ブロックの書式は HH:MM-HH:MM[,focus][,task=ID][,label=ラベル] です: {spec}"
            ))
        };
        let (range, mut rest) = match spec.split_once(',') {
            Some((r, rest)) => (r, Some(rest)),
            None => (spec, None),
        };
        let (start, end) = range.split_once('-').ok_or_else(err)?;
        let mut it = ItemInput {
            start: start.trim().into(),
            end: end.trim().into(),
            ..Default::default()
        };
        while let Some(r) = rest {
            if let Some(label) = r.strip_prefix("label=") {
                it.label = Some(label.to_string());
                break;
            }
            let (head, tail) = match r.split_once(',') {
                Some((h, t)) => (h, Some(t)),
                None => (r, None),
            };
            match head.trim() {
                "focus" => it.focus = true,
                h if h.starts_with("task=") => it.task = Some(h["task=".len()..].to_string()),
                _ => return Err(err()),
            }
            rest = tail;
        }
        Ok(it)
    }
}

/// 曜日の指定を解釈する。`mon,wed` / `月,水` / `1,3`（1=月 … 7=日）をカンマ区切りで。
pub fn parse_weekdays(s: &str) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    for part in s.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        let n = match part.to_ascii_lowercase().as_str() {
            "1" | "mon" | "月" => 1,
            "2" | "tue" | "火" => 2,
            "3" | "wed" | "水" => 3,
            "4" | "thu" | "木" => 4,
            "5" | "fri" | "金" => 5,
            "6" | "sat" | "土" => 6,
            "7" | "sun" | "日" => 7,
            _ => {
                return Err(Error::InvalidArgument(format!(
                    "曜日を解釈できません: {part}（mon〜sun、月〜日、1〜7）"
                )))
            }
        };
        out.push(n);
    }
    out.sort_unstable();
    out.dedup();
    Ok(out)
}

fn normalize_weekdays(w: &[u8]) -> Result<Vec<u8>> {
    if let Some(bad) = w.iter().find(|d| !(1..=7).contains(*d)) {
        return Err(Error::InvalidArgument(format!(
            "曜日は 1（月）〜7（日）で指定してください: {bad}"
        )));
    }
    let mut v = w.to_vec();
    v.sort_unstable();
    v.dedup();
    Ok(v)
}

fn clean(s: &Option<String>) -> Option<String> {
    s.as_ref().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

// ===== CRUD =====

fn view(ctx: &Ctx, t: Template) -> Result<TemplateView> {
    let titles = task_titles(ctx.conn)?;
    let blocks = repo_tpl::items(ctx.conn, &t.id)?
        .into_iter()
        .map(|i| ItemView {
            task_title: i.task_id.as_ref().and_then(|id| titles.get(id).cloned()),
            start: fmt_hm(i.start_min),
            end: fmt_hm(i.end_min),
            id: i.id,
            start_min: i.start_min,
            end_min: i.end_min,
            label: i.label,
            focus: i.focus,
            task_id: i.task_id,
        })
        .collect();
    Ok(TemplateView {
        id: t.id,
        name: t.name,
        description: t.description,
        weekdays: t.weekdays,
        created_at: t.created_at,
        updated_at: t.updated_at,
        blocks,
    })
}

/// 名前（完全一致）または ID（前方一致）でテンプレートを 1 件に解決する。
fn resolve(ctx: &Ctx, key: &str) -> Result<Template> {
    if let Some(t) = repo_tpl::find_by_name(ctx.conn, key)? {
        return Ok(t);
    }
    let id = repo::resolve_prefix(ctx.conn, "block_template", key).map_err(|e| match e {
        Error::NotFound(_) => Error::NotFound(format!("テンプレート {key}")),
        e => e,
    })?;
    repo_tpl::get(ctx.conn, &id)?.ok_or_else(|| Error::NotFound(format!("テンプレート {key}")))
}

/// 入力を検証して、保存する形（項目の行）にする。
fn build_items(ctx: &Ctx, template_id: &str, items: &[ItemInput]) -> Result<Vec<Item>> {
    let mut out = Vec::new();
    for it in items {
        let start_min = parse_hm(&it.start, false)?;
        let end_min = parse_hm(&it.end, true)?;
        if end_min <= start_min {
            return Err(Error::InvalidArgument(format!(
                "終了は開始より後にしてください: {}-{}",
                it.start, it.end
            )));
        }
        let task_id = match clean(&it.task) {
            Some(p) => Some(repo::task::resolve(ctx.conn, &p)?.id),
            None => None,
        };
        let label = clean(&it.label);
        if task_id.is_none() && label.is_none() {
            return Err(Error::InvalidArgument(format!(
                "ブロック {}-{} にはタスクかラベルが必要です",
                it.start, it.end
            )));
        }
        out.push(Item {
            id: new_id(),
            template_id: template_id.to_string(),
            start_min,
            end_min,
            label,
            focus: it.focus,
            task_id,
        });
    }
    Ok(out)
}

fn check_name(ctx: &Ctx, name: &str, own_id: Option<&str>) -> Result<String> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err(Error::InvalidArgument("テンプレートの名前が必要です".into()));
    }
    if let Some(other) = repo_tpl::find_by_name(ctx.conn, &name)? {
        if Some(other.id.as_str()) != own_id {
            return Err(Error::InvalidArgument(format!(
                "同じ名前のテンプレートがあります: {name}"
            )));
        }
    }
    Ok(name)
}

pub fn add(ctx: &Ctx, input: &TemplateInput) -> Result<TemplateView> {
    let name = check_name(ctx, &input.name, None)?;
    let weekdays = normalize_weekdays(&input.weekdays)?;
    let now = ctx.now();
    let t = Template {
        id: new_id(),
        name,
        description: clean(&input.description),
        weekdays,
        created_at: now,
        updated_at: now,
    };
    let items = build_items(ctx, &t.id, &input.items)?;
    let tx = ctx.conn.unchecked_transaction()?;
    repo_tpl::insert(&tx, &t)?;
    for i in &items {
        repo_tpl::insert_item(&tx, i)?;
    }
    tx.commit()?;
    view(ctx, t)
}

/// テンプレートを編集する。名前・説明・曜日・ブロックをまるごと置き換える。
pub fn edit(ctx: &Ctx, key: &str, input: &TemplateInput) -> Result<TemplateView> {
    let mut t = resolve(ctx, key)?;
    t.name = check_name(ctx, &input.name, Some(&t.id))?;
    t.description = clean(&input.description);
    t.weekdays = normalize_weekdays(&input.weekdays)?;
    t.updated_at = ctx.now();
    let items = build_items(ctx, &t.id, &input.items)?;
    let tx = ctx.conn.unchecked_transaction()?;
    repo_tpl::update(&tx, &t)?;
    repo_tpl::delete_items(&tx, &t.id)?;
    for i in &items {
        repo_tpl::insert_item(&tx, i)?;
    }
    tx.commit()?;
    view(ctx, t)
}

pub fn delete(ctx: &Ctx, key: &str) -> Result<TemplateView> {
    let t = resolve(ctx, key)?;
    let v = view(ctx, t)?;
    repo_tpl::delete(ctx.conn, &v.id)?;
    Ok(v)
}

pub fn list(ctx: &Ctx) -> Result<TemplateList> {
    let templates = repo_tpl::list(ctx.conn)?
        .into_iter()
        .map(|t| view(ctx, t))
        .collect::<Result<_>>()?;
    Ok(TemplateList { templates })
}

pub fn show(ctx: &Ctx, key: &str) -> Result<TemplateView> {
    let t = resolve(ctx, key)?;
    view(ctx, t)
}

fn date_of(ctx: &Ctx, date: &Option<String>) -> Result<NaiveDate> {
    timefmt::parse_date(ctx.tz()?, date.as_deref().unwrap_or("today"), ctx.now())
}

/// その日の曜日に割り当てられたテンプレート。
pub fn suggest(ctx: &Ctx, date: Option<&str>) -> Result<Suggestion> {
    let d = date_of(ctx, &date.map(String::from))?;
    let weekday = d.weekday().number_from_monday() as u8;
    let templates = repo_tpl::list(ctx.conn)?
        .into_iter()
        .filter(|t| t.weekdays.contains(&weekday))
        .map(|t| view(ctx, t))
        .collect::<Result<_>>()?;
    Ok(Suggestion {
        date: d.to_string(),
        weekday,
        templates,
    })
}

// ===== 適用 =====

/// テンプレートを日付に当てはめた結果と、既存のブロック・busy な予定との重なり。
fn plan_blocks(ctx: &Ctx, t: &Template, d: NaiveDate) -> Result<(Range, Vec<PreviewBlock>)> {
    let tz = ctx.tz()?;
    let day = timefmt::day_range(tz, d);
    let existing = plan::list(ctx.conn, day)?;
    let titles = task_titles(ctx.conn)?;
    let busy: Vec<_> = calendar::list(ctx.conn, day)?
        .into_iter()
        .filter(|e| e.is_busy && !e.all_day)
        .collect();
    let mut out = Vec::new();
    for i in repo_tpl::items(ctx.conn, &t.id)? {
        let s = timefmt::local_minute_to_ms(tz, d, i.start_min);
        let e = timefmt::local_minute_to_ms(tz, d, i.end_min);
        // DST の空白で潰れた（開始と終了が同じ時刻になった）ブロックは作れない。
        if e <= s {
            continue;
        }
        let r = Range::new(s, e);
        let mut conflicts = Vec::new();
        for b in &existing {
            if r.intersect(&Range::new(b.start_at, b.end_at)).is_some() {
                conflicts.push(Conflict {
                    kind: "block",
                    title: b
                        .label
                        .clone()
                        .or_else(|| b.task_id.as_ref().and_then(|id| titles.get(id).cloned()))
                        .unwrap_or_default(),
                    start_at: b.start_at,
                    end_at: b.end_at,
                });
            }
        }
        for c in &busy {
            if r.intersect(&Range::new(c.start_at, c.end_at)).is_some() {
                conflicts.push(Conflict {
                    kind: "calendar",
                    title: c.title.clone(),
                    start_at: c.start_at,
                    end_at: c.end_at,
                });
            }
        }
        out.push(PreviewBlock {
            start_at: s,
            end_at: e,
            start: timefmt::fmt_rfc3339(tz, s),
            end: timefmt::fmt_rfc3339(tz, e),
            label: i.label,
            focus: i.focus,
            task_title: i.task_id.as_ref().and_then(|id| titles.get(id).cloned()),
            task_id: i.task_id,
            conflicts,
        });
    }
    Ok((day, out))
}

/// 適用した場合に作られるブロックと重なりを返す（何も保存しない）。
pub fn preview(ctx: &Ctx, input: &ApplyInput) -> Result<Preview> {
    let t = resolve(ctx, &input.template)?;
    let d = date_of(ctx, &input.date)?;
    let (day, blocks) = plan_blocks(ctx, &t, d)?;
    Ok(Preview {
        template: t.name,
        date: d.to_string(),
        range: range_view(ctx.tz()?, day),
        overlap: input.overlap,
        blocks,
    })
}

/// 指定日にテンプレートを適用して `time_block` を作る。
pub fn apply(ctx: &Ctx, input: &ApplyInput) -> Result<ApplyResult> {
    let t = resolve(ctx, &input.template)?;
    let d = date_of(ctx, &input.date)?;
    let (_, planned) = plan_blocks(ctx, &t, d)?;
    let (skipped, keep): (Vec<_>, Vec<_>) = planned
        .into_iter()
        .partition(|p| input.overlap == Overlap::Skip && !p.conflicts.is_empty());

    let tx = ctx.conn.unchecked_transaction()?;
    let mut created = Vec::new();
    for p in keep {
        let b = TimeBlock {
            id: new_id(),
            start_at: p.start_at,
            end_at: p.end_at,
            task_id: p.task_id,
            label: p.label,
            focus: p.focus,
        };
        plan::insert(&tx, &b)?;
        created.push(BlockView {
            task_title: p.task_title,
            block: b,
        });
    }
    let app = Application {
        id: new_id(),
        template_name: t.name.clone(),
        date: d.to_string(),
        applied_at: ctx.now(),
        block_ids: created.iter().map(|b| b.block.id.clone()).collect(),
    };
    repo_tpl::insert_application(&tx, &app)?;
    tx.commit()?;
    Ok(ApplyResult {
        application_id: app.id,
        template: t.name,
        date: app.date,
        created,
        skipped,
    })
}

/// 直前の適用（`date` を指定すればその日に適用したもののうち直近）で作ったブロックを削除する。
pub fn undo(ctx: &Ctx, date: Option<&str>) -> Result<UndoResult> {
    let date = match date {
        Some(s) => Some(date_of(ctx, &Some(s.to_string()))?.to_string()),
        None => None,
    };
    let app = repo_tpl::last_application(ctx.conn, date.as_deref())?
        .ok_or_else(|| Error::NotFound("取り消せるテンプレートの適用".into()))?;
    let tx = ctx.conn.unchecked_transaction()?;
    let mut removed = 0;
    for id in &app.block_ids {
        if plan::delete(&tx, id)? {
            removed += 1;
        }
    }
    repo_tpl::mark_undone(&tx, &app.id, ctx.now())?;
    tx.commit()?;
    Ok(UndoResult {
        template: app.template_name,
        date: app.date,
        removed,
    })
}

// ===== 保存 =====

/// 指定日のブロックからテンプレートを作る（日をまたぐブロックはその日の範囲に切る）。
pub fn save_from(ctx: &Ctx, input: &SaveFromInput) -> Result<TemplateView> {
    let tz = ctx.tz()?;
    let d = date_of(ctx, &input.date)?;
    let day = timefmt::day_range(tz, d);
    let mut items = Vec::new();
    for b in plan::list(ctx.conn, day)? {
        let start_min = timefmt::ms_to_local_minute(tz, d, b.start_at.max(day.start)).min(24 * 60 - 1);
        let end_min = timefmt::ms_to_local_minute(tz, d, b.end_at.min(day.end));
        let end_min = end_min.max(start_min + 1);
        items.push(ItemInput {
            start: fmt_hm(start_min),
            end: fmt_hm(end_min),
            label: b.label.clone(),
            focus: b.focus,
            task: b.task_id.clone(),
        });
    }
    if items.is_empty() {
        return Err(Error::InvalidArgument(format!(
            "{d} にはブロックがありません。テンプレートにするものがありません"
        )));
    }
    add(
        ctx,
        &TemplateInput {
            name: input.name.clone(),
            description: input.description.clone(),
            weekdays: input.weekdays.clone(),
            items,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::CalendarEvent;
    use crate::service::plan::{self as plan_svc, BlockInput};
    use crate::service::testutil::Env;
    use crate::service::RangeArgs;

    fn item(start: &str, end: &str, label: &str, focus: bool) -> ItemInput {
        ItemInput {
            start: start.into(),
            end: end.into(),
            label: Some(label.into()),
            focus,
            task: None,
        }
    }

    fn tpl(name: &str, weekdays: &[u8], items: Vec<ItemInput>) -> TemplateInput {
        TemplateInput {
            name: name.into(),
            description: None,
            weekdays: weekdays.to_vec(),
            items,
        }
    }

    fn apply_in(name: &str, date: Option<&str>, overlap: Overlap) -> ApplyInput {
        ApplyInput {
            template: name.into(),
            date: date.map(Into::into),
            overlap,
        }
    }

    #[test]
    fn crud() {
        let env = Env::new();
        let ctx = env.ctx();
        assert!(list(&ctx).unwrap().templates.is_empty());
        let v = add(
            &ctx,
            &tpl(
                "水曜",
                &[3, 1, 3],
                vec![
                    item("13:00", "14:00", "定例", false),
                    item("09:00", "11:00", "集中", true),
                ],
            ),
        )
        .unwrap();
        assert_eq!(v.weekdays, vec![1, 3]);
        // 開始時刻順
        assert_eq!(v.blocks[0].start, "09:00");
        assert!(v.blocks[0].focus);
        // 名前の重複
        assert!(add(&ctx, &tpl("水曜", &[], vec![])).is_err());
        // 名前・ID 前方一致で引ける
        assert_eq!(show(&ctx, "水曜").unwrap().id, v.id);
        assert_eq!(show(&ctx, &v.id[..10]).unwrap().name, "水曜");
        assert!(matches!(show(&ctx, "ない"), Err(Error::NotFound(_))));
        // 編集（名前変更・ブロック置き換え・自分自身との名前重複は可）
        let e = edit(
            &ctx,
            "水曜",
            &tpl("水曜", &[3], vec![item("10:00", "11:00", "新", false)]),
        )
        .unwrap();
        assert_eq!(e.blocks.len(), 1);
        assert_eq!(e.weekdays, vec![3]);
        let e = edit(
            &ctx,
            &v.id,
            &tpl("週半ば", &[], vec![item("10:00", "24:00", "夜", false)]),
        )
        .unwrap();
        assert_eq!(e.name, "週半ば");
        assert_eq!(e.blocks[0].end, "24:00");
        assert_eq!(list(&ctx).unwrap().templates.len(), 1);
        // 削除
        delete(&ctx, "週半ば").unwrap();
        assert!(list(&ctx).unwrap().templates.is_empty());
        assert!(matches!(delete(&ctx, "週半ば"), Err(Error::NotFound(_))));
    }

    #[test]
    fn validation() {
        let env = Env::new();
        let ctx = env.ctx();
        assert!(add(&ctx, &tpl("", &[], vec![])).is_err());
        assert!(add(&ctx, &tpl("a", &[8], vec![])).is_err());
        assert!(add(&ctx, &tpl("a", &[], vec![item("10:00", "09:00", "x", false)])).is_err());
        assert!(add(&ctx, &tpl("a", &[], vec![item("25:00", "26:00", "x", false)])).is_err());
        assert!(add(&ctx, &tpl("a", &[], vec![item("24:00", "24:00", "x", false)])).is_err());
        assert!(add(&ctx, &tpl("a", &[], vec![item("9:00", "9:60", "x", false)])).is_err());
        let mut no_label = item("09:00", "10:00", "", false);
        no_label.label = None;
        assert!(add(&ctx, &tpl("a", &[], vec![no_label])).is_err());
        // 失敗したものは残らない
        assert!(list(&ctx).unwrap().templates.is_empty());
    }

    #[test]
    fn item_spec_parse() {
        let i = ItemInput::parse_spec("09:00-10:30").unwrap();
        assert_eq!(
            (i.start.as_str(), i.end.as_str(), i.focus),
            ("09:00", "10:30", false)
        );
        let i = ItemInput::parse_spec("14:00-15:00,focus,task=01ABC,label=内省, 振り返り").unwrap();
        assert!(i.focus);
        assert_eq!(i.task.as_deref(), Some("01ABC"));
        assert_eq!(i.label.as_deref(), Some("内省, 振り返り"));
        assert!(ItemInput::parse_spec("0900").is_err());
        assert!(ItemInput::parse_spec("09:00-10:00,bogus").is_err());
    }

    #[test]
    fn weekday_parse() {
        assert_eq!(parse_weekdays("fri,Mon,月").unwrap(), vec![1, 5]);
        assert_eq!(parse_weekdays("").unwrap(), Vec::<u8>::new());
        assert!(parse_weekdays("xyz").is_err());
    }

    #[test]
    fn apply_converts_local_time_to_utc() {
        let env = Env::new();
        let ctx = env.ctx();
        add(&ctx, &tpl("朝", &[], vec![item("09:00", "10:30", "朝会", true)])).unwrap();
        let r = apply(&ctx, &apply_in("朝", Some("2026-10-10"), Overlap::Skip)).unwrap();
        assert_eq!(r.created.len(), 1);
        let b = &r.created[0].block;
        // 2026-10-10 09:00 JST = 00:00 UTC
        assert_eq!(b.start_at, env.day_start() + 9 * 3_600_000);
        assert_eq!(b.end_at - b.start_at, 90 * 60_000);
        assert!(b.focus);
        // 別の日にも適用できる
        let r = apply(&ctx, &apply_in("朝", Some("2026-10-12"), Overlap::Skip)).unwrap();
        assert_eq!(r.date, "2026-10-12");
        assert_eq!(
            plan_svc::list(&ctx, &RangeArgs::default()).unwrap().blocks.len(),
            1
        );
    }

    #[test]
    fn apply_dst_new_york() {
        let mut env = Env::new();
        env.cfg.general.timezone = "America/New_York".into();
        let ctx = env.ctx();
        add(
            &ctx,
            &tpl(
                "dst",
                &[],
                vec![
                    item("01:00", "02:30", "前", false),
                    item("09:00", "10:00", "朝", false),
                ],
            ),
        )
        .unwrap();
        // 2026-03-08 は 02:00 -> 03:00 に飛ぶ日。
        let r = apply(&ctx, &apply_in("dst", Some("2026-03-08"), Overlap::Overlay)).unwrap();
        let by = |l: &str| {
            r.created
                .iter()
                .find(|b| b.block.label.as_deref() == Some(l))
                .unwrap()
        };
        let utc = |s: &str| {
            chrono::DateTime::parse_from_rfc3339(s)
                .unwrap()
                .timestamp_millis()
        };
        // 01:00 EST = 06:00Z、02:30（存在しない）は 03:00 EDT = 07:00Z に寄る。
        assert_eq!(by("前").block.start_at, utc("2026-03-08T06:00:00Z"));
        assert_eq!(by("前").block.end_at, utc("2026-03-08T07:00:00Z"));
        // 09:00 EDT = 13:00Z
        assert_eq!(by("朝").block.start_at, utc("2026-03-08T13:00:00Z"));
        // 秋（2026-11-01 に 01:00 が 2 回ある）は早い方。
        let r = apply(&ctx, &apply_in("dst", Some("2026-11-01"), Overlap::Overlay)).unwrap();
        let first = r
            .created
            .iter()
            .find(|b| b.block.label.as_deref() == Some("前"))
            .unwrap();
        assert_eq!(first.block.start_at, utc("2026-11-01T05:00:00Z")); // 01:00 EDT
    }

    #[test]
    fn dst_collapsed_item_is_dropped() {
        let mut env = Env::new();
        env.cfg.general.timezone = "America/New_York".into();
        let ctx = env.ctx();
        add(&ctx, &tpl("空白", &[], vec![item("02:15", "02:45", "x", false)])).unwrap();
        let r = apply(&ctx, &apply_in("空白", Some("2026-03-08"), Overlap::Overlay)).unwrap();
        assert!(r.created.is_empty());
    }

    #[test]
    fn overlap_skip_and_overlay() {
        let env = Env::new();
        let ctx = env.ctx();
        let d = env.day_start();
        // 既存ブロック 09:30-10:00
        plan_svc::add(
            &ctx,
            &BlockInput {
                start: d + 9 * 3_600_000 + 1_800_000,
                end: d + 10 * 3_600_000,
                task: None,
                label: Some("既存".into()),
                focus: false,
            },
        )
        .unwrap();
        // busy な予定 13:30-14:30、busy でない予定 15:00-16:00
        for (id, s, e, busy) in [
            ("c1", 13 * 60 + 30, 14 * 60 + 30, true),
            ("c2", 15 * 60, 16 * 60, false),
        ] {
            crate::repo::calendar::replace_source_range(
                &env.conn,
                id,
                Range::new(d, d + 86_400_000),
                &[CalendarEvent {
                    id: id.into(),
                    source: id.into(),
                    external_id: id.into(),
                    start_at: d + s * 60_000,
                    end_at: d + e * 60_000,
                    all_day: false,
                    title: format!("予定{id}"),
                    location: None,
                    is_busy: busy,
                    fetched_at: 0,
                }],
            )
            .unwrap();
        }
        add(
            &ctx,
            &tpl(
                "t",
                &[],
                vec![
                    item("09:00", "10:00", "A", false),
                    item("11:00", "12:00", "B", false),
                    item("14:00", "15:00", "C", false),
                    item("15:00", "16:00", "D", false),
                ],
            ),
        )
        .unwrap();
        let p = preview(&ctx, &apply_in("t", None, Overlap::Skip)).unwrap();
        assert_eq!(p.blocks.len(), 4);
        assert_eq!(p.blocks[0].conflicts[0].kind, "block");
        assert!(p.blocks[1].conflicts.is_empty());
        assert_eq!(p.blocks[2].conflicts[0].kind, "calendar");
        assert!(
            p.blocks[3].conflicts.is_empty(),
            "busy でない予定は重なりに数えない"
        );
        // preview は何も作らない
        assert_eq!(
            plan_svc::list(&ctx, &RangeArgs::default()).unwrap().blocks.len(),
            1
        );

        let r = apply(&ctx, &apply_in("t", None, Overlap::Skip)).unwrap();
        assert_eq!(r.created.len(), 2);
        assert_eq!(r.skipped.len(), 2);
        assert_eq!(
            plan_svc::list(&ctx, &RangeArgs::default()).unwrap().blocks.len(),
            3
        );
        undo(&ctx, None).unwrap();

        let r = apply(&ctx, &apply_in("t", None, Overlap::Overlay)).unwrap();
        assert_eq!(r.created.len(), 4);
        assert!(r.skipped.is_empty());
        assert_eq!(
            plan_svc::list(&ctx, &RangeArgs::default()).unwrap().blocks.len(),
            5
        );
    }

    #[test]
    fn undo_removes_only_last_application() {
        let env = Env::new();
        let ctx = env.ctx();
        add(&ctx, &tpl("a", &[], vec![item("09:00", "10:00", "A", false)])).unwrap();
        add(&ctx, &tpl("b", &[], vec![item("11:00", "12:00", "B", false)])).unwrap();
        assert!(matches!(undo(&ctx, None), Err(Error::NotFound(_))));
        apply(&ctx, &apply_in("a", None, Overlap::Skip)).unwrap();
        let rb = apply(&ctx, &apply_in("b", None, Overlap::Skip)).unwrap();
        // 手で足したブロックは巻き込まない
        plan_svc::add(
            &ctx,
            &BlockInput {
                start: env.day_start() + 20 * 3_600_000,
                end: env.day_start() + 21 * 3_600_000,
                task: None,
                label: Some("手動".into()),
                focus: false,
            },
        )
        .unwrap();
        // ユーザーが先に消したブロックは数えない
        plan_svc::delete(&ctx, &rb.created[0].block.id).unwrap();
        let u = undo(&ctx, None).unwrap();
        assert_eq!((u.template.as_str(), u.removed), ("b", 0));
        let u = undo(&ctx, Some("today")).unwrap();
        assert_eq!((u.template.as_str(), u.removed), ("a", 1));
        assert!(undo(&ctx, None).is_err());
        let l = plan_svc::list(&ctx, &RangeArgs::default()).unwrap();
        assert_eq!(l.blocks.len(), 1);
        assert_eq!(l.blocks[0].block.label.as_deref(), Some("手動"));
    }

    #[test]
    fn suggest_by_weekday() {
        let env = Env::new(); // 2026-10-10 は土曜
        let ctx = env.ctx();
        add(&ctx, &tpl("土", &[6, 7], vec![])).unwrap();
        add(&ctx, &tpl("水", &[3], vec![])).unwrap();
        let s = suggest(&ctx, None).unwrap();
        assert_eq!(s.weekday, 6);
        assert_eq!(s.templates.len(), 1);
        assert_eq!(s.templates[0].name, "土");
        let s = suggest(&ctx, Some("2026-10-14")).unwrap();
        assert_eq!(s.weekday, 3);
        assert_eq!(s.templates[0].name, "水");
        assert!(suggest(&ctx, Some("2026-10-12")).unwrap().templates.is_empty());
    }

    #[test]
    fn save_from_day() {
        let env = Env::new();
        let ctx = env.ctx();
        let d = env.day_start();
        assert!(save_from(
            &ctx,
            &SaveFromInput {
                name: "空".into(),
                ..Default::default()
            }
        )
        .is_err());
        plan_svc::add(
            &ctx,
            &BlockInput {
                start: d + 9 * 3_600_000,
                end: d + 10 * 3_600_000 + 1_800_000,
                task: None,
                label: Some("集中".into()),
                focus: true,
            },
        )
        .unwrap();
        // 前日から続くブロックはその日の範囲に切る
        plan_svc::add(
            &ctx,
            &BlockInput {
                start: d - 3_600_000,
                end: d + 3_600_000,
                task: None,
                label: Some("夜".into()),
                focus: false,
            },
        )
        .unwrap();
        let v = save_from(
            &ctx,
            &SaveFromInput {
                name: "今日".into(),
                description: Some("メモ".into()),
                date: None,
                weekdays: vec![6],
            },
        )
        .unwrap();
        assert_eq!(v.blocks.len(), 2);
        assert_eq!(
            (v.blocks[0].start.as_str(), v.blocks[0].end.as_str()),
            ("00:00", "01:00")
        );
        assert_eq!(
            (v.blocks[1].start.as_str(), v.blocks[1].end.as_str()),
            ("09:00", "10:30")
        );
        assert!(v.blocks[1].focus);
        assert_eq!(v.weekdays, vec![6]);
        // 保存したものを別の日に適用すると同じ配置になる
        let r = apply(&ctx, &apply_in("今日", Some("2026-10-11"), Overlap::Skip)).unwrap();
        assert_eq!(r.created.len(), 2);
        assert_eq!(r.created[1].block.start_at, d + 86_400_000 + 9 * 3_600_000);
    }
}
