//! 手入力の記録: チェックイン、デイリーハッスル、ABC（10 章、12.2）。

use super::{check_1_5, Ctx};
use crate::id::new_id;
use crate::model::{AbcRecord, Checkin, ErgTag, Hassle};
use crate::repo::record;
use crate::{Error, Result};

#[derive(Debug, Clone, Default)]
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

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone, Default)]
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
}
