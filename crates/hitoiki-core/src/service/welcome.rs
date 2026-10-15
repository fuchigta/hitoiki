//! 初回起動時のはじめにの完了状態（`setting` テーブルに保存する）。

use super::Ctx;
use crate::{Millis, Result};
use rusqlite::{params, OptionalExtension};
use serde::Serialize;

/// 完了時刻（Unix ミリ秒）を保存する `setting` のキー。
pub const COMPLETED_KEY: &str = "welcome.completed_at";

#[derive(Debug, Clone, Serialize)]
pub struct WelcomeStatus {
    pub completed: bool,
    pub completed_at: Option<Millis>,
}

/// はじめにの完了状態を返す。
pub fn status(ctx: &Ctx) -> Result<WelcomeStatus> {
    let v: Option<String> = ctx
        .conn
        .query_row(
            "SELECT value_json FROM setting WHERE key = ?1",
            [COMPLETED_KEY],
            |r| r.get(0),
        )
        .optional()?;
    let completed_at = v.and_then(|s| s.trim().parse::<i64>().ok());
    Ok(WelcomeStatus {
        completed: completed_at.is_some(),
        completed_at,
    })
}

/// 完了済みにする（すでに完了していれば時刻は更新しない）。
pub fn complete(ctx: &Ctx) -> Result<WelcomeStatus> {
    ctx.conn.execute(
        "INSERT INTO setting(key, value_json) VALUES (?1, ?2)
         ON CONFLICT(key) DO NOTHING",
        params![COMPLETED_KEY, ctx.now().to_string()],
    )?;
    status(ctx)
}

/// 未完了に戻す（「もう一度見る」用）。
pub fn reset(ctx: &Ctx) -> Result<WelcomeStatus> {
    ctx.conn
        .execute("DELETE FROM setting WHERE key = ?1", [COMPLETED_KEY])?;
    status(ctx)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::testutil::{now, Env};

    #[test]
    fn 初期状態は未完了() {
        let env = Env::new();
        let s = status(&env.ctx()).unwrap();
        assert!(!s.completed);
        assert_eq!(s.completed_at, None);
    }

    #[test]
    fn 完了と取り消し() {
        let env = Env::new();
        let s = complete(&env.ctx()).unwrap();
        assert!(s.completed);
        assert_eq!(s.completed_at, Some(now()));
        // 再度完了にしても時刻は変わらない。
        env.clock.set(now() + 1000);
        assert_eq!(complete(&env.ctx()).unwrap().completed_at, Some(now()));
        let s = reset(&env.ctx()).unwrap();
        assert!(!s.completed);
        assert!(!status(&env.ctx()).unwrap().completed);
    }
}
