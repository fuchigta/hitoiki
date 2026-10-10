//! 補助コレクタからの詳細コンテキストの取り込み（6.4）。`context_hint` に保存する。

use super::Ctx;
use crate::id::new_id;
use crate::model::ContextHint;
use crate::{Error, Millis, Result};
use chrono::DateTime;
use rusqlite::params;
use serde::Serialize;
use serde_json::Value;

/// 1 件あたりの `value` の最大文字数。
const MAX_VALUE_CHARS: usize = 4096;

#[derive(Debug, Clone, Serialize)]
pub struct IngestResult {
    pub saved: usize,
}

/// JSON（単体オブジェクトまたは配列）を検証して保存する。1 件でも不正なら何も保存しない。
pub fn ingest_json(ctx: &Ctx, text: &str) -> Result<IngestResult> {
    let v: Value = serde_json::from_str(text)
        .map_err(|e| Error::InvalidArgument(format!("JSON を解釈できません: {e}")))?;
    let items: Vec<&Value> = match &v {
        Value::Array(a) => a.iter().collect(),
        Value::Object(_) => vec![&v],
        _ => {
            return Err(Error::InvalidArgument(
                "JSON はオブジェクトまたは配列にしてください".into(),
            ))
        }
    };
    let now = ctx.now();
    let hints = items
        .iter()
        .enumerate()
        .map(|(i, it)| parse_hint(it, now).map_err(|e| prefix(i, items.len(), e)))
        .collect::<Result<Vec<_>>>()?;

    let tx = ctx.conn.unchecked_transaction()?;
    for h in &hints {
        tx.execute(
            "INSERT INTO context_hint(id, at, source, hwnd, kind, value) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![h.id, h.at, h.source, h.hwnd, h.kind, h.value],
        )?;
    }
    tx.commit()?;
    Ok(IngestResult { saved: hints.len() })
}

fn prefix(i: usize, n: usize, e: Error) -> Error {
    if n > 1 {
        Error::InvalidArgument(format!("{} 件目: {e}", i + 1))
    } else {
        e
    }
}

fn str_field(o: &serde_json::Map<String, Value>, key: &str, required: bool) -> Result<Option<String>> {
    match o.get(key) {
        None | Some(Value::Null) => {
            if required {
                Err(Error::InvalidArgument(format!("`{key}` は必須です")))
            } else {
                Ok(None)
            }
        }
        Some(Value::String(s)) => {
            let s = s.trim();
            if s.is_empty() {
                if required {
                    Err(Error::InvalidArgument(format!("`{key}` が空です")))
                } else {
                    Ok(None)
                }
            } else {
                Ok(Some(s.to_string()))
            }
        }
        Some(_) => Err(Error::InvalidArgument(format!("`{key}` は文字列にしてください"))),
    }
}

fn parse_hint(v: &Value, now: Millis) -> Result<ContextHint> {
    let o = v
        .as_object()
        .ok_or_else(|| Error::InvalidArgument("各要素はオブジェクトにしてください".into()))?;
    let at = match o.get("at") {
        None | Some(Value::Null) => now,
        Some(Value::Number(n)) => n
            .as_i64()
            .ok_or_else(|| Error::InvalidArgument("`at` はミリ秒の整数にしてください".into()))?,
        Some(Value::String(s)) => DateTime::parse_from_rfc3339(s)
            .map_err(|_| Error::InvalidArgument(format!("`at` を RFC 3339 として解釈できません: {s}")))?
            .timestamp_millis(),
        Some(_) => {
            return Err(Error::InvalidArgument(
                "`at` は RFC 3339 またはミリ秒にしてください".into(),
            ))
        }
    };
    let source = str_field(o, "source", true)?.unwrap_or_default();
    let kind = str_field(o, "kind", true)?.unwrap_or_default();
    let value = str_field(o, "value", true)?.unwrap_or_default();
    if value.chars().count() > MAX_VALUE_CHARS {
        return Err(Error::InvalidArgument(format!(
            "`value` が長すぎます（最大 {MAX_VALUE_CHARS} 文字）"
        )));
    }
    Ok(ContextHint {
        id: new_id(),
        at,
        source,
        hwnd: str_field(o, "hwnd", false)?,
        kind,
        value,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::testutil::Env;

    fn count(env: &Env) -> i64 {
        env.conn
            .query_row("SELECT COUNT(*) FROM context_hint", [], |r| r.get(0))
            .unwrap()
    }

    #[test]
    fn single_object_defaults_at_to_now() {
        let env = Env::new();
        let r = ingest_json(
            &env.ctx(),
            r#"{"source":"browser","kind":"url","value":"https://example.com","hwnd":"0x10"}"#,
        )
        .unwrap();
        assert_eq!(r.saved, 1);
        let (at, hwnd): (i64, Option<String>) = env
            .conn
            .query_row("SELECT at, hwnd FROM context_hint", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!(at, env.ctx().now());
        assert_eq!(hwnd.as_deref(), Some("0x10"));
    }

    #[test]
    fn array_with_rfc3339_and_millis() {
        let env = Env::new();
        let r = ingest_json(
            &env.ctx(),
            r#"[{"at":"2026-01-01T00:00:00Z","source":"vscode","kind":"workspace","value":"/w"},
                {"at":1000,"source":"vscode","kind":"file","value":"a.rs"}]"#,
        )
        .unwrap();
        assert_eq!(r.saved, 2);
        let ats: Vec<i64> = env
            .conn
            .prepare("SELECT at FROM context_hint ORDER BY at")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(|x| x.unwrap())
            .collect();
        assert_eq!(ats, vec![1000, 1_767_225_600_000]);
    }

    #[test]
    fn invalid_saves_nothing() {
        let env = Env::new();
        let ctx = env.ctx();
        assert!(ingest_json(&ctx, "not json").is_err());
        assert!(ingest_json(&ctx, "42").is_err());
        assert!(ingest_json(&ctx, r#"{"source":"b","kind":"url"}"#).is_err());
        assert!(ingest_json(&ctx, r#"{"source":"b","kind":"url","value":""}"#).is_err());
        assert!(ingest_json(&ctx, r#"{"at":"x","source":"b","kind":"url","value":"v"}"#).is_err());
        let e = ingest_json(
            &ctx,
            r#"[{"source":"b","kind":"url","value":"v"},{"source":"b","kind":"url"}]"#,
        )
        .unwrap_err();
        assert_eq!(e.exit_code(), 2);
        assert_eq!(count(&env), 0);
    }
}
