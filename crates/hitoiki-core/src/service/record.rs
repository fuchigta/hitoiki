//! 手入力の記録: 調子、気になったこと、捉え直し（docs/dev/architecture.md「データモデル」、docs/dev/architecture.md「CLI」）。

use super::impact::{DeleteMode, DeleteOutcome, Impact};
use super::{check_1_5, Ctx, RangeArgs};
use crate::id::new_id;
use crate::model::{Concern, Condition, ErgRecordKind, ErgTag, Reframe};
use crate::repo::{edit, erg, record};
use crate::{Error, Millis, Result};
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

/// 削除の呼び出し元（ログ用）。プロセスの起動時に `set_caller("cli")` / `set_caller("app")` で一度だけ設定する。
static CALLER: OnceLock<&'static str> = OnceLock::new();

pub fn set_caller(caller: &'static str) {
    let _ = CALLER.set(caller);
}

/// 記録の削除をログに残す。種類・ID・呼び出し元だけで、本文・メモは出さない
/// （記録が消えた原因を後から調べられるようにするため）。
fn log_deleted(entity: &str, id: &str) {
    tracing::info!(
        entity,
        id,
        caller = CALLER.get().copied().unwrap_or("unknown"),
        "記録を削除しました"
    );
}

/// 振り返りの回答から参照されているときの警告を足す。
fn with_reflection_refs(ctx: &Ctx, mut im: Impact) -> Result<Impact> {
    for label in super::reflect::referencing_reflections(ctx, &im.id)? {
        im = im.warning(format!("振り返り（{label}）の回答から参照されています"));
    }
    Ok(im)
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ConditionInput {
    pub energy: Option<i64>,
    pub mood: Option<i64>,
    pub stress: Option<i64>,
    pub erg: Option<ErgTag>,
    pub note: Option<String>,
}

pub fn condition(ctx: &Ctx, input: &ConditionInput) -> Result<Condition> {
    check_1_5("energy (-e)", input.energy)?;
    check_1_5("mood (-m)", input.mood)?;
    check_1_5("stress (-s)", input.stress)?;
    if input.energy.is_none()
        && input.mood.is_none()
        && input.stress.is_none()
        && input.erg.is_none()
        && input.note.is_none()
    {
        return Err(Error::InvalidArgument(
            "-e / -m / -s / --erg / --note のいずれかを指定してください".into(),
        ));
    }
    let c = Condition {
        id: new_id(),
        at: ctx.now(),
        energy: input.energy,
        mood: input.mood,
        stress: input.stress,
        erg_tag: input.erg,
        note: input.note.clone(),
        nudge_id: None,
    };
    // `answer_structured` などの外側のトランザクションの中でも呼ばれるので、ここでは張らない。
    record::insert_condition(ctx.conn, &c)?;
    if let Some(t) = input.erg {
        // ユーザーが付けたタグ（出所 user）。AI は上書きしない。
        erg::set_user(ctx.conn, ErgRecordKind::Condition, &c.id, t.into(), c.at)?;
    }
    Ok(c)
}

#[derive(Debug, Clone, Deserialize)]
pub struct ConcernInput {
    pub text: String,
    /// 1〜5。省略時は 3。
    pub intensity: Option<i64>,
}

pub fn concern(ctx: &Ctx, input: &ConcernInput) -> Result<Concern> {
    check_1_5("強度 (-i)", input.intensity)?;
    if input.text.trim().is_empty() {
        return Err(Error::InvalidArgument("気になったことの内容が空です".into()));
    }
    let h = Concern {
        id: new_id(),
        at: ctx.now(),
        text: input.text.trim().to_string(),
        intensity: input.intensity.unwrap_or(3),
        segment_id: None,
        nudge_id: None,
    };
    record::insert_concern(ctx.conn, &h)?;
    Ok(h)
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ReframeInput {
    /// A: 出来事。`from_concern` 指定時は省略可（気になったこと本文で補完）。
    pub event: Option<String>,
    pub belief: Option<String>,
    pub emotion: Option<String>,
    /// 1〜5。
    pub emotion_intensity: Option<i64>,
    /// 気になったことの ID（前方一致）。
    pub from_concern: Option<String>,
}

pub fn reframe_add(ctx: &Ctx, input: &ReframeInput) -> Result<Reframe> {
    check_1_5("感情の強度 (--emotion-intensity)", input.emotion_intensity)?;
    let concern = match &input.from_concern {
        Some(p) => Some(record::resolve_concern(ctx.conn, p)?),
        None => None,
    };
    let a = input
        .event
        .clone()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| concern.as_ref().map(|h| h.text.clone()))
        .ok_or_else(|| Error::InvalidArgument("--event か --from-concern が必要です".into()))?;
    let r = Reframe {
        id: new_id(),
        at: ctx.now(),
        event: a,
        belief: input.belief.clone(),
        emotion: input.emotion.clone(),
        emotion_intensity: input.emotion_intensity,
        alternative: None,
        deepening_md: None,
        concern_id: concern.map(|h| h.id),
    };
    record::insert_reframe(ctx.conn, &r)?;
    Ok(r)
}

/// 期間付きの一覧（調子・気になったこと・捉え直し共通）。
#[derive(Debug, Clone, Serialize)]
pub struct Listing<T> {
    pub start_at: Millis,
    pub end_at: Millis,
    pub items: Vec<T>,
}

fn listing<T>(
    ctx: &Ctx,
    args: &RangeArgs,
    f: impl FnOnce(&Ctx, Millis, Millis) -> Result<Vec<T>>,
) -> Result<Listing<T>> {
    let r = ctx.range(args)?;
    Ok(Listing {
        start_at: r.start,
        end_at: r.end,
        items: f(ctx, r.start, r.end)?,
    })
}

/// 範囲内の調子（時刻順）。
pub fn list_conditions(ctx: &Ctx, args: &RangeArgs) -> Result<Listing<Condition>> {
    listing(ctx, args, |c, s, e| record::list_conditions(c.conn, s, e))
}

/// 範囲内の気になったこと（時刻順）。
pub fn list_concerns(ctx: &Ctx, args: &RangeArgs) -> Result<Listing<Concern>> {
    listing(ctx, args, |c, s, e| record::list_concerns(c.conn, s, e))
}

/// 範囲内の捉え直し（時刻順）。
pub fn list_reframe(ctx: &Ctx, args: &RangeArgs) -> Result<Listing<Reframe>> {
    listing(ctx, args, |c, s, e| record::list_reframe(c.conn, s, e))
}

// ===== 編集・削除 =====

/// 調子の編集。記録した内容を丸ごと置き換える（指定のない項目は消える）。時刻は変えない。
pub fn condition_edit(ctx: &Ctx, id: &str, input: &ConditionInput) -> Result<Condition> {
    check_1_5("energy (-e)", input.energy)?;
    check_1_5("mood (-m)", input.mood)?;
    check_1_5("stress (-s)", input.stress)?;
    let note = input
        .note
        .as_ref()
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty());
    if input.energy.is_none()
        && input.mood.is_none()
        && input.stress.is_none()
        && input.erg.is_none()
        && note.is_none()
    {
        return Err(Error::InvalidArgument(
            "エネルギー・気分・ストレス・ERG・メモのいずれかが必要です（消す場合は削除してください）".into(),
        ));
    }
    let mut c = edit::get_condition(ctx.conn, &crate::repo::resolve_prefix(ctx.conn, "condition", id)?)?;
    let text_changed = c.note != note;
    c.energy = input.energy;
    c.mood = input.mood;
    c.stress = input.stress;
    c.note = note;
    let tx = ctx.conn.unchecked_transaction()?;
    edit::update_condition(ctx.conn, &c)?;
    if text_changed {
        // メモを直したので、AI 由来の ERG は捨てて判定をやり直せるようにする（ユーザーが付けたものは残す）。
        erg::delete_auto(ctx.conn, ErgRecordKind::Condition, &c.id)?;
    }
    if let Some(t) = input.erg {
        erg::set_user(ctx.conn, ErgRecordKind::Condition, &c.id, t.into(), ctx.now())?;
    }
    c.erg_tag = erg::get(ctx.conn, ErgRecordKind::Condition, &c.id)?.and_then(|l| l.confirmed_tag());
    tx.commit()?;
    Ok(c)
}

pub fn condition_get(ctx: &Ctx, id: &str) -> Result<Condition> {
    edit::get_condition(ctx.conn, &crate::repo::resolve_prefix(ctx.conn, "condition", id)?)
}

pub fn condition_delete(ctx: &Ctx, id: &str, mode: DeleteMode) -> Result<DeleteOutcome> {
    let c = condition_get(ctx, id)?;
    let im = with_reflection_refs(ctx, Impact::new("condition", &c.id, "調子"))?;
    let go = DeleteOutcome::gate(mode, &im)?;
    if go {
        edit::delete_condition(ctx.conn, &c.id)?;
        log_deleted("condition", &c.id);
    }
    Ok(DeleteOutcome::new(mode, im, go))
}

pub fn concern_get(ctx: &Ctx, id: &str) -> Result<Concern> {
    record::resolve_concern(ctx.conn, id)
}

/// 気になったことの編集。`intensity` が `None` なら強度は変えない。
pub fn concern_edit(ctx: &Ctx, id: &str, input: &ConcernInput) -> Result<Concern> {
    check_1_5("強度 (-i)", input.intensity)?;
    if input.text.trim().is_empty() {
        return Err(Error::InvalidArgument("気になったことの内容が空です".into()));
    }
    let mut h = concern_get(ctx, id)?;
    let text = input.text.trim().to_string();
    let text_changed = h.text != text;
    h.text = text;
    if let Some(i) = input.intensity {
        h.intensity = i;
    }
    let tx = ctx.conn.unchecked_transaction()?;
    edit::update_concern(ctx.conn, &h)?;
    if text_changed {
        erg::delete_auto(ctx.conn, ErgRecordKind::Concern, &h.id)?;
    }
    tx.commit()?;
    Ok(h)
}

/// 気になったことの削除。元にした捉え直しは残し、気になったこととの紐付けだけ外す。
pub fn concern_delete(ctx: &Ctx, id: &str, mode: DeleteMode) -> Result<DeleteOutcome> {
    let h = concern_get(ctx, id)?;
    let im = Impact::new("concern", &h.id, &h.text).line(
        "この気になったことから作った捉え直し（記録は残り、気になったこととの紐付けだけ外れる）",
        edit::count_reframe_of_concern(ctx.conn, &h.id)?,
    );
    let im = with_reflection_refs(ctx, im)?;
    let go = DeleteOutcome::gate(mode, &im)?;
    if go {
        let tx = ctx.conn.unchecked_transaction()?;
        edit::delete_concern(ctx.conn, &h.id)?;
        tx.commit()?;
        log_deleted("concern", &h.id);
    }
    Ok(DeleteOutcome::new(mode, im, go))
}

/// 捉え直しの編集内容。記録した内容を丸ごと置き換える（`None` は消す）。時刻と AI の問いは変えない。
#[derive(Debug, Clone, Deserialize)]
pub struct ReframeEditInput {
    pub event: String,
    #[serde(default)]
    pub belief: Option<String>,
    #[serde(default)]
    pub emotion: Option<String>,
    #[serde(default)]
    pub emotion_intensity: Option<i64>,
    #[serde(default)]
    pub alternative: Option<String>,
}

pub fn reframe_get(ctx: &Ctx, id: &str) -> Result<Reframe> {
    edit::get_reframe(ctx.conn, &crate::repo::resolve_prefix(ctx.conn, "reframe", id)?)
}

pub fn reframe_edit(ctx: &Ctx, id: &str, input: &ReframeEditInput) -> Result<Reframe> {
    check_1_5("感情の強度 (--emotion-intensity)", input.emotion_intensity)?;
    if input.event.trim().is_empty() {
        return Err(Error::InvalidArgument("A（出来事）が空です".into()));
    }
    let blank = |s: &Option<String>| s.as_ref().map(|x| x.trim().to_string()).filter(|x| !x.is_empty());
    let mut a = reframe_get(ctx, id)?;
    let before = (
        a.event.clone(),
        a.belief.clone(),
        a.emotion.clone(),
        a.alternative.clone(),
    );
    a.event = input.event.trim().to_string();
    a.belief = blank(&input.belief);
    a.emotion = blank(&input.emotion);
    a.emotion_intensity = input.emotion_intensity;
    a.alternative = blank(&input.alternative);
    let text_changed = before
        != (
            a.event.clone(),
            a.belief.clone(),
            a.emotion.clone(),
            a.alternative.clone(),
        );
    let tx = ctx.conn.unchecked_transaction()?;
    edit::update_reframe(ctx.conn, &a)?;
    if text_changed {
        erg::delete_auto(ctx.conn, ErgRecordKind::Reframe, &a.id)?;
    }
    tx.commit()?;
    Ok(a)
}

pub fn reframe_delete(ctx: &Ctx, id: &str, mode: DeleteMode) -> Result<DeleteOutcome> {
    let a = reframe_get(ctx, id)?;
    let im = with_reflection_refs(ctx, Impact::new("reframe", &a.id, &a.event))?;
    let go = DeleteOutcome::gate(mode, &im)?;
    if go {
        edit::delete_reframe(ctx.conn, &a.id)?;
        log_deleted("reframe", &a.id);
    }
    Ok(DeleteOutcome::new(mode, im, go))
}

#[cfg(test)]
mod edit_tests {
    use super::*;
    use crate::service::testutil::Env;

    #[test]
    fn 調子の編集と削除() {
        let env = Env::new();
        let ctx = env.ctx();
        let c = condition(
            &ctx,
            &ConditionInput {
                energy: Some(2),
                mood: Some(3),
                note: Some("眠い".into()),
                ..Default::default()
            },
        )
        .unwrap();
        let e = condition_edit(
            &ctx,
            &c.id,
            &ConditionInput {
                energy: Some(4),
                erg: Some(ErgTag::Growth),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            (e.energy, e.mood, e.note.clone(), e.erg_tag),
            (Some(4), None, None, Some(ErgTag::Growth))
        );
        assert_eq!(e.at, c.at);
        let listed = list_conditions(&ctx, &RangeArgs::default()).unwrap().items;
        assert_eq!(listed[0].energy, Some(4));
        assert!(condition_edit(&ctx, &c.id, &ConditionInput::default()).is_err());
        assert!(condition_edit(
            &ctx,
            &c.id,
            &ConditionInput {
                stress: Some(9),
                ..Default::default()
            }
        )
        .is_err());
        assert!(condition_delete(&ctx, &c.id, DeleteMode::DRY_RUN).is_ok());
        assert_eq!(
            list_conditions(&ctx, &RangeArgs::default()).unwrap().items.len(),
            1
        );
        assert!(
            condition_delete(&ctx, &c.id, DeleteMode::default())
                .unwrap()
                .deleted
        );
        assert!(list_conditions(&ctx, &RangeArgs::default())
            .unwrap()
            .items
            .is_empty());
        assert!(condition_delete(&ctx, &c.id, DeleteMode::default()).is_err());
    }

    #[test]
    fn 気になったことの削除は_reframe_の紐付けだけ外す() {
        let env = Env::new();
        let ctx = env.ctx();
        let h = concern(
            &ctx,
            &ConcernInput {
                text: "長い会議".into(),
                intensity: Some(4),
            },
        )
        .unwrap();
        let e = concern_edit(
            &ctx,
            &h.id,
            &ConcernInput {
                text: " 長すぎる会議 ".into(),
                intensity: None,
            },
        )
        .unwrap();
        assert_eq!((e.text.as_str(), e.intensity), ("長すぎる会議", 4));
        assert!(concern_edit(
            &ctx,
            &h.id,
            &ConcernInput {
                text: " ".into(),
                intensity: None
            }
        )
        .is_err());
        let a = reframe_add(
            &ctx,
            &ReframeInput {
                from_concern: Some(h.id.clone()),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(concern_delete(&ctx, &h.id, DeleteMode::default()).is_err());
        let d = concern_delete(&ctx, &h.id, DeleteMode::FORCE).unwrap();
        assert!(d.deleted);
        assert_eq!(d.impact.total(), 1);
        let a2 = reframe_get(&ctx, &a.id).unwrap();
        assert!(a2.concern_id.is_none());
        assert_eq!(a2.event, "長すぎる会議");
        assert!(list_concerns(&ctx, &RangeArgs::default())
            .unwrap()
            .items
            .is_empty());
    }

    #[test]
    fn reframe_の編集と削除() {
        let env = Env::new();
        let ctx = env.ctx();
        let a = reframe_add(
            &ctx,
            &ReframeInput {
                event: Some("A".into()),
                belief: Some("B".into()),
                emotion_intensity: Some(2),
                ..Default::default()
            },
        )
        .unwrap();
        let e = reframe_edit(
            &ctx,
            &a.id,
            &ReframeEditInput {
                event: "A2".into(),
                belief: None,
                emotion: Some("不安".into()),
                emotion_intensity: Some(4),
                alternative: Some("本当に？".into()),
            },
        )
        .unwrap();
        assert_eq!(
            (e.belief.clone(), e.emotion_intensity, e.alternative.as_deref()),
            (None, Some(4), Some("本当に？"))
        );
        assert!(reframe_edit(
            &ctx,
            &a.id,
            &ReframeEditInput {
                event: " ".into(),
                belief: None,
                emotion: None,
                emotion_intensity: None,
                alternative: None
            }
        )
        .is_err());
        assert!(
            reframe_delete(&ctx, &a.id, DeleteMode::default())
                .unwrap()
                .deleted
        );
        assert!(list_reframe(&ctx, &RangeArgs::default())
            .unwrap()
            .items
            .is_empty());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::testutil::Env;

    #[test]
    fn condition_validates_range() {
        let env = Env::new();
        let ctx = env.ctx();
        assert!(condition(
            &ctx,
            &ConditionInput {
                energy: Some(6),
                ..Default::default()
            }
        )
        .is_err());
        assert!(condition(
            &ctx,
            &ConditionInput {
                mood: Some(0),
                ..Default::default()
            }
        )
        .is_err());
        assert!(condition(&ctx, &ConditionInput::default()).is_err());
        let c = condition(
            &ctx,
            &ConditionInput {
                energy: Some(4),
                erg: Some(ErgTag::Growth),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(c.energy, Some(4));
        assert_eq!(c.at, ctx.now());
    }

    #[test]
    fn concern_default_and_reframe_from_concern() {
        let env = Env::new();
        let ctx = env.ctx();
        let h = concern(
            &ctx,
            &ConcernInput {
                text: "会議が長い".into(),
                intensity: None,
            },
        )
        .unwrap();
        assert_eq!(h.intensity, 3);
        assert!(concern(
            &ctx,
            &ConcernInput {
                text: "x".into(),
                intensity: Some(9)
            }
        )
        .is_err());
        let a = reframe_add(
            &ctx,
            &ReframeInput {
                from_concern: Some(h.id.clone()),
                belief: Some("無駄だ".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(a.event, "会議が長い");
        assert_eq!(a.concern_id.as_deref(), Some(h.id.as_str()));
        assert!(reframe_add(&ctx, &ReframeInput::default()).is_err());
    }

    #[test]
    fn 範囲の一覧() {
        let env = Env::new();
        let ctx = env.ctx();
        condition(
            &ctx,
            &ConditionInput {
                energy: Some(3),
                ..Default::default()
            },
        )
        .unwrap();
        concern(
            &ctx,
            &ConcernInput {
                text: "x".into(),
                intensity: None,
            },
        )
        .unwrap();
        reframe_add(
            &ctx,
            &ReframeInput {
                event: Some("a".into()),
                ..Default::default()
            },
        )
        .unwrap();
        let a = RangeArgs::default();
        assert_eq!(list_conditions(&ctx, &a).unwrap().items.len(), 1);
        assert_eq!(list_concerns(&ctx, &a).unwrap().items.len(), 1);
        assert_eq!(list_reframe(&ctx, &a).unwrap().items.len(), 1);
    }
}
