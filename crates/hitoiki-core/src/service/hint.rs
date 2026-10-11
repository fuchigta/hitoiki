//! 画面に 1 回だけ出す案内（閉じたら二度と出さない）の記録。`setting` に `hint.<名前>.dismissed` で持つ。

use super::Ctx;
use crate::{Error, Result};
use rusqlite::{params, OptionalExtension};

/// 案内の名前。未知の名前は受け付けない。
pub const HINTS: &[&str] = &["calendar"];

fn key(name: &str) -> Result<String> {
    if !HINTS.contains(&name) {
        return Err(Error::InvalidArgument(format!("不明な案内です: {name}")));
    }
    Ok(format!("hint.{name}.dismissed"))
}

/// 案内が閉じられていれば true。
pub fn is_dismissed(ctx: &Ctx, name: &str) -> Result<bool> {
    let v: Option<String> = ctx
        .conn
        .query_row(
            "SELECT value_json FROM setting WHERE key = ?1",
            [key(name)?],
            |r| r.get(0),
        )
        .optional()?;
    Ok(v.is_some())
}

/// 案内を閉じたことを記録する。
pub fn dismiss(ctx: &Ctx, name: &str) -> Result<()> {
    ctx.conn.execute(
        "INSERT INTO setting(key, value_json) VALUES (?1, 'true')
         ON CONFLICT(key) DO UPDATE SET value_json = excluded.value_json",
        params![key(name)?],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::testutil::Env;

    #[test]
    fn dismiss_once() {
        let env = Env::new();
        let ctx = env.ctx();
        assert!(!is_dismissed(&ctx, "calendar").unwrap());
        dismiss(&ctx, "calendar").unwrap();
        dismiss(&ctx, "calendar").unwrap();
        assert!(is_dismissed(&ctx, "calendar").unwrap());
        assert!(dismiss(&ctx, "x").is_err());
    }
}
