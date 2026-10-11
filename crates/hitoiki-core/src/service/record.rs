//! 手入力の記録: チェックイン、デイリーハッスル、ABC（docs/dev/architecture.md「データモデル」、docs/dev/architecture.md「CLI」）。

use super::impact::{DeleteMode, DeleteOutcome, Impact};
use super::{check_1_5, Ctx, RangeArgs};
use crate::id::new_id;
use crate::model::{AbcRecord, Checkin, ErgTag, Hassle};
use crate::repo::{edit, record};
use crate::{Error, Millis, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct CheckInput {
    pub energy: Option<i64>,
    pub mood: Option<i64>,
    pub stress: Option<i64>,
    pub erg: Option<ErgTag>,
    pub note: Option<String>,
}

pub fn check(ctx: &Ctx, input: &CheckInput) -> Result<Checkin> {
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
    let c = Checkin {
        id: new_id(),
        at: ctx.now(),
        energy: input.energy,
        mood: input.mood,
        stress: input.stress,
        erg_tag: input.erg,
        note: input.note.clone(),
        push_id: None,
    };
    record::insert_checkin(ctx.conn, &c)?;
    Ok(c)
}

#[derive(Debug, Clone, Deserialize)]
pub struct HassleInput {
    pub text: String,
    /// 1〜5。省略時は 3。
    pub intensity: Option<i64>,
}

pub fn hassle(ctx: &Ctx, input: &HassleInput) -> Result<Hassle> {
    check_1_5("強度 (-i)", input.intensity)?;
    if input.text.trim().is_empty() {
        return Err(Error::InvalidArgument("ハッスルの内容が空です".into()));
    }
    let h = Hassle {
        id: new_id(),
        at: ctx.now(),
        text: input.text.trim().to_string(),
        intensity: input.intensity.unwrap_or(3),
        segment_id: None,
        push_id: None,
    };
    record::insert_hassle(ctx.conn, &h)?;
    Ok(h)
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct AbcInput {
    /// A: 出来事。`from_hassle` 指定時は省略可（ハッスル本文で補完）。
    pub activating_event: Option<String>,
    pub belief: Option<String>,
    pub consequence_emotion: Option<String>,
    /// 1〜5。
    pub consequence_intensity: Option<i64>,
    /// ハッスルの ID（前方一致）。
    pub from_hassle: Option<String>,
}

pub fn abc_add(ctx: &Ctx, input: &AbcInput) -> Result<AbcRecord> {
    check_1_5("感情の強度 (--c-intensity)", input.consequence_intensity)?;
    let hassle = match &input.from_hassle {
        Some(p) => Some(record::resolve_hassle(ctx.conn, p)?),
        None => None,
    };
    let a = input
        .activating_event
        .clone()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| hassle.as_ref().map(|h| h.text.clone()))
        .ok_or_else(|| Error::InvalidArgument("--a か --from-hassle が必要です".into()))?;
    let r = AbcRecord {
        id: new_id(),
        at: ctx.now(),
        activating_event: a,
        belief: input.belief.clone(),
        consequence_emotion: input.consequence_emotion.clone(),
        consequence_intensity: input.consequence_intensity,
        dispute: None,
        ai_dialog_md: None,
        hassle_id: hassle.map(|h| h.id),
    };
    record::insert_abc(ctx.conn, &r)?;
    Ok(r)
}

/// 期間付きの一覧（チェックイン・ハッスル・ABC 共通）。
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

/// 範囲内のチェックイン（時刻順）。
pub fn list_checkins(ctx: &Ctx, args: &RangeArgs) -> Result<Listing<Checkin>> {
    listing(ctx, args, |c, s, e| record::list_checkins(c.conn, s, e))
}

/// 範囲内のハッスル（時刻順）。
pub fn list_hassles(ctx: &Ctx, args: &RangeArgs) -> Result<Listing<Hassle>> {
    listing(ctx, args, |c, s, e| record::list_hassles(c.conn, s, e))
}

/// 範囲内の ABC 記録（時刻順）。
pub fn list_abc(ctx: &Ctx, args: &RangeArgs) -> Result<Listing<AbcRecord>> {
    listing(ctx, args, |c, s, e| record::list_abc(c.conn, s, e))
}

// ===== 編集・削除 =====

/// チェックインの編集。記録した内容を丸ごと置き換える（指定のない項目は消える）。時刻は変えない。
pub fn check_edit(ctx: &Ctx, id: &str, input: &CheckInput) -> Result<Checkin> {
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
    let mut c = edit::get_checkin(ctx.conn, &crate::repo::resolve_prefix(ctx.conn, "checkin", id)?)?;
    c.energy = input.energy;
    c.mood = input.mood;
    c.stress = input.stress;
    c.erg_tag = input.erg;
    c.note = note;
    edit::update_checkin(ctx.conn, &c)?;
    Ok(c)
}

pub fn check_get(ctx: &Ctx, id: &str) -> Result<Checkin> {
    edit::get_checkin(ctx.conn, &crate::repo::resolve_prefix(ctx.conn, "checkin", id)?)
}

pub fn check_delete(ctx: &Ctx, id: &str, mode: DeleteMode) -> Result<DeleteOutcome> {
    let c = check_get(ctx, id)?;
    let im = Impact::new("checkin", &c.id, "チェックイン");
    let go = DeleteOutcome::gate(mode, &im)?;
    if go {
        edit::delete_checkin(ctx.conn, &c.id)?;
    }
    Ok(DeleteOutcome::new(mode, im, go))
}

pub fn hassle_get(ctx: &Ctx, id: &str) -> Result<Hassle> {
    record::resolve_hassle(ctx.conn, id)
}

/// ハッスルの編集。`intensity` が `None` なら強度は変えない。
pub fn hassle_edit(ctx: &Ctx, id: &str, input: &HassleInput) -> Result<Hassle> {
    check_1_5("強度 (-i)", input.intensity)?;
    if input.text.trim().is_empty() {
        return Err(Error::InvalidArgument("ハッスルの内容が空です".into()));
    }
    let mut h = hassle_get(ctx, id)?;
    h.text = input.text.trim().to_string();
    if let Some(i) = input.intensity {
        h.intensity = i;
    }
    edit::update_hassle(ctx.conn, &h)?;
    Ok(h)
}

/// ハッスルの削除。元にした ABC 記録は残し、ハッスルとの紐付けだけ外す。
pub fn hassle_delete(ctx: &Ctx, id: &str, mode: DeleteMode) -> Result<DeleteOutcome> {
    let h = hassle_get(ctx, id)?;
    let im = Impact::new("hassle", &h.id, &h.text).line(
        "このハッスルから作った ABC 記録（記録は残り、ハッスルとの紐付けだけ外れる）",
        edit::count_abc_of_hassle(ctx.conn, &h.id)?,
    );
    let go = DeleteOutcome::gate(mode, &im)?;
    if go {
        let tx = ctx.conn.unchecked_transaction()?;
        edit::delete_hassle(ctx.conn, &h.id)?;
        tx.commit()?;
    }
    Ok(DeleteOutcome::new(mode, im, go))
}

/// ABC 記録の編集内容。記録した内容を丸ごと置き換える（`None` は消す）。時刻と AI の問いは変えない。
#[derive(Debug, Clone, Deserialize)]
pub struct AbcEditInput {
    pub activating_event: String,
    #[serde(default)]
    pub belief: Option<String>,
    #[serde(default)]
    pub consequence_emotion: Option<String>,
    #[serde(default)]
    pub consequence_intensity: Option<i64>,
    #[serde(default)]
    pub dispute: Option<String>,
}

pub fn abc_get(ctx: &Ctx, id: &str) -> Result<AbcRecord> {
    edit::get_abc(
        ctx.conn,
        &crate::repo::resolve_prefix(ctx.conn, "abc_record", id)?,
    )
}

pub fn abc_edit(ctx: &Ctx, id: &str, input: &AbcEditInput) -> Result<AbcRecord> {
    check_1_5("感情の強度 (--c-intensity)", input.consequence_intensity)?;
    if input.activating_event.trim().is_empty() {
        return Err(Error::InvalidArgument("A（出来事）が空です".into()));
    }
    let blank = |s: &Option<String>| s.as_ref().map(|x| x.trim().to_string()).filter(|x| !x.is_empty());
    let mut a = abc_get(ctx, id)?;
    a.activating_event = input.activating_event.trim().to_string();
    a.belief = blank(&input.belief);
    a.consequence_emotion = blank(&input.consequence_emotion);
    a.consequence_intensity = input.consequence_intensity;
    a.dispute = blank(&input.dispute);
    edit::update_abc(ctx.conn, &a)?;
    Ok(a)
}

pub fn abc_delete(ctx: &Ctx, id: &str, mode: DeleteMode) -> Result<DeleteOutcome> {
    let a = abc_get(ctx, id)?;
    let im = Impact::new("abc_record", &a.id, &a.activating_event);
    let go = DeleteOutcome::gate(mode, &im)?;
    if go {
        edit::delete_abc(ctx.conn, &a.id)?;
    }
    Ok(DeleteOutcome::new(mode, im, go))
}

#[cfg(test)]
mod edit_tests {
    use super::*;
    use crate::service::testutil::Env;

    #[test]
    fn チェックインの編集と削除() {
        let env = Env::new();
        let ctx = env.ctx();
        let c = check(
            &ctx,
            &CheckInput {
                energy: Some(2),
                mood: Some(3),
                note: Some("眠い".into()),
                ..Default::default()
            },
        )
        .unwrap();
        let e = check_edit(
            &ctx,
            &c.id,
            &CheckInput {
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
        let listed = list_checkins(&ctx, &RangeArgs::default()).unwrap().items;
        assert_eq!(listed[0].energy, Some(4));
        assert!(check_edit(&ctx, &c.id, &CheckInput::default()).is_err());
        assert!(check_edit(
            &ctx,
            &c.id,
            &CheckInput {
                stress: Some(9),
                ..Default::default()
            }
        )
        .is_err());
        assert!(check_delete(&ctx, &c.id, DeleteMode::DRY_RUN).is_ok());
        assert_eq!(list_checkins(&ctx, &RangeArgs::default()).unwrap().items.len(), 1);
        assert!(check_delete(&ctx, &c.id, DeleteMode::default()).unwrap().deleted);
        assert!(list_checkins(&ctx, &RangeArgs::default())
            .unwrap()
            .items
            .is_empty());
        assert!(check_delete(&ctx, &c.id, DeleteMode::default()).is_err());
    }

    #[test]
    fn ハッスルの削除は_abc_の紐付けだけ外す() {
        let env = Env::new();
        let ctx = env.ctx();
        let h = hassle(
            &ctx,
            &HassleInput {
                text: "長い会議".into(),
                intensity: Some(4),
            },
        )
        .unwrap();
        let e = hassle_edit(
            &ctx,
            &h.id,
            &HassleInput {
                text: " 長すぎる会議 ".into(),
                intensity: None,
            },
        )
        .unwrap();
        assert_eq!((e.text.as_str(), e.intensity), ("長すぎる会議", 4));
        assert!(hassle_edit(
            &ctx,
            &h.id,
            &HassleInput {
                text: " ".into(),
                intensity: None
            }
        )
        .is_err());
        let a = abc_add(
            &ctx,
            &AbcInput {
                from_hassle: Some(h.id.clone()),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(hassle_delete(&ctx, &h.id, DeleteMode::default()).is_err());
        let d = hassle_delete(&ctx, &h.id, DeleteMode::FORCE).unwrap();
        assert!(d.deleted);
        assert_eq!(d.impact.total(), 1);
        let a2 = abc_get(&ctx, &a.id).unwrap();
        assert!(a2.hassle_id.is_none());
        assert_eq!(a2.activating_event, "長すぎる会議");
        assert!(list_hassles(&ctx, &RangeArgs::default())
            .unwrap()
            .items
            .is_empty());
    }

    #[test]
    fn abc_の編集と削除() {
        let env = Env::new();
        let ctx = env.ctx();
        let a = abc_add(
            &ctx,
            &AbcInput {
                activating_event: Some("A".into()),
                belief: Some("B".into()),
                consequence_intensity: Some(2),
                ..Default::default()
            },
        )
        .unwrap();
        let e = abc_edit(
            &ctx,
            &a.id,
            &AbcEditInput {
                activating_event: "A2".into(),
                belief: None,
                consequence_emotion: Some("不安".into()),
                consequence_intensity: Some(4),
                dispute: Some("本当に？".into()),
            },
        )
        .unwrap();
        assert_eq!(
            (e.belief.clone(), e.consequence_intensity, e.dispute.as_deref()),
            (None, Some(4), Some("本当に？"))
        );
        assert!(abc_edit(
            &ctx,
            &a.id,
            &AbcEditInput {
                activating_event: " ".into(),
                belief: None,
                consequence_emotion: None,
                consequence_intensity: None,
                dispute: None
            }
        )
        .is_err());
        assert!(abc_delete(&ctx, &a.id, DeleteMode::default()).unwrap().deleted);
        assert!(list_abc(&ctx, &RangeArgs::default()).unwrap().items.is_empty());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::testutil::Env;

    #[test]
    fn check_validates_range() {
        let env = Env::new();
        let ctx = env.ctx();
        assert!(check(
            &ctx,
            &CheckInput {
                energy: Some(6),
                ..Default::default()
            }
        )
        .is_err());
        assert!(check(
            &ctx,
            &CheckInput {
                mood: Some(0),
                ..Default::default()
            }
        )
        .is_err());
        assert!(check(&ctx, &CheckInput::default()).is_err());
        let c = check(
            &ctx,
            &CheckInput {
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
    fn hassle_default_and_abc_from_hassle() {
        let env = Env::new();
        let ctx = env.ctx();
        let h = hassle(
            &ctx,
            &HassleInput {
                text: "会議が長い".into(),
                intensity: None,
            },
        )
        .unwrap();
        assert_eq!(h.intensity, 3);
        assert!(hassle(
            &ctx,
            &HassleInput {
                text: "x".into(),
                intensity: Some(9)
            }
        )
        .is_err());
        let a = abc_add(
            &ctx,
            &AbcInput {
                from_hassle: Some(h.id.clone()),
                belief: Some("無駄だ".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(a.activating_event, "会議が長い");
        assert_eq!(a.hassle_id.as_deref(), Some(h.id.as_str()));
        assert!(abc_add(&ctx, &AbcInput::default()).is_err());
    }

    #[test]
    fn 範囲の一覧() {
        let env = Env::new();
        let ctx = env.ctx();
        check(
            &ctx,
            &CheckInput {
                energy: Some(3),
                ..Default::default()
            },
        )
        .unwrap();
        hassle(
            &ctx,
            &HassleInput {
                text: "x".into(),
                intensity: None,
            },
        )
        .unwrap();
        abc_add(
            &ctx,
            &AbcInput {
                activating_event: Some("a".into()),
                ..Default::default()
            },
        )
        .unwrap();
        let a = RangeArgs::default();
        assert_eq!(list_checkins(&ctx, &a).unwrap().items.len(), 1);
        assert_eq!(list_hassles(&ctx, &a).unwrap().items.len(), 1);
        assert_eq!(list_abc(&ctx, &a).unwrap().items.len(), 1);
    }
}
