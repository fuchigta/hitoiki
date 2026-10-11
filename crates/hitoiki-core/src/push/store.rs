//! push_log の読み書きと、通知への応答の反映（checkin / hassle / スヌーズ）。

use super::payload::IDLE_ACTION_PREFIX;
use super::{PushDecision, PushPayload, PushTrigger};
use crate::id::new_id;
use crate::model::PushLog;
use crate::service::review::{self, IdlePreset};
use crate::timefmt::{day_range, local_date_of};
use crate::{Error, Millis, Result};
use chrono_tz::Tz;
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};

fn row_to_log(r: &Row<'_>) -> rusqlite::Result<PushLog> {
    Ok(PushLog {
        id: r.get(0)?,
        at: r.get(1)?,
        trigger: r.get(2)?,
        payload_json: r.get(3)?,
        delivered: r.get::<_, i64>(4)? != 0,
        responded_at: r.get(5)?,
        response_json: r.get(6)?,
        snoozed_until: r.get(7)?,
        suppressed_reason: r.get(8)?,
    })
}

const COLS: &str =
    "id, at, trigger, payload_json, delivered, responded_at, response_json, snoozed_until, suppressed_reason";

pub fn insert_log(conn: &Connection, log: &PushLog) -> Result<()> {
    conn.execute(
        "INSERT INTO push_log (id, at, trigger, payload_json, delivered, responded_at, response_json, snoozed_until, suppressed_reason)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            log.id,
            log.at,
            log.trigger,
            log.payload_json,
            log.delivered as i64,
            log.responded_at,
            log.response_json,
            log.snoozed_until,
            log.suppressed_reason
        ],
    )?;
    Ok(())
}

/// 判定結果を push_log に残す。Deliver は配信済み、Suppress は `suppressed_reason` 付きで記録し、
/// 追加した行の ID を返す。Defer は記録しない（None）。
pub fn record_decision(conn: &Connection, decision: &PushDecision, now: Millis) -> Result<Option<String>> {
    let log = match decision {
        PushDecision::Deliver(p) => PushLog {
            id: new_id(),
            at: now,
            trigger: p.trigger.as_str().into(),
            payload_json: serde_json::to_string(p).map_err(anyhow::Error::from)?,
            delivered: true,
            responded_at: None,
            response_json: None,
            snoozed_until: None,
            suppressed_reason: None,
        },
        PushDecision::Suppress { trigger, reason } => PushLog {
            id: new_id(),
            at: now,
            trigger: trigger.as_str().into(),
            payload_json: "{}".into(),
            delivered: false,
            responded_at: None,
            response_json: None,
            snoozed_until: None,
            suppressed_reason: Some(reason.clone()),
        },
        PushDecision::Defer { .. } => return Ok(None),
    };
    insert_log(conn, &log)?;
    Ok(Some(log.id))
}

/// `since` 以降の push_log を古い順に返す。`PushContext::push_log` の材料。
/// 応答率の算出用に、`since` より前の配信済み直近 `extra_delivered` 件も含める。
pub fn load_recent(conn: &Connection, since: Millis, extra_delivered: usize) -> Result<Vec<PushLog>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLS} FROM push_log
         WHERE at >= ?1
            OR id IN (SELECT id FROM push_log WHERE at < ?1 AND delivered = 1 ORDER BY at DESC LIMIT ?2)
         ORDER BY at"
    ))?;
    let rows = stmt
        .query_map(params![since, extra_delivered as i64], row_to_log)?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

pub fn get_log(conn: &Connection, id: &str) -> Result<PushLog> {
    conn.query_row(
        &format!("SELECT {COLS} FROM push_log WHERE id = ?1"),
        [id],
        row_to_log,
    )
    .optional()?
    .ok_or_else(|| Error::NotFound(format!("push_log の ID {id}")))
}

/// 通知への応答。ボタン（`action` + `value`）とテキスト入力（`text`）のどちらか、または両方。
///
/// `action` の値: `energy` / `stress`（value は 1〜5）、`snooze_15` / `snooze_60` / `snooze_today`、
/// `idle_break` / `idle_meeting` / `idle_thinking` / `idle_away`（idle_return の分類。value は無し）、
/// `open`（アプリを開いた）、`dismiss`（閉じただけ。応答とは数えない）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PushResponse {
    pub action: Option<String>,
    pub value: Option<i64>,
    pub text: Option<String>,
}

/// `apply_response` が作成・設定したもの。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResponseOutcome {
    pub checkin_id: Option<String>,
    pub hassle_id: Option<String>,
    pub snoozed_until: Option<Millis>,
    /// idle_return への応答で分類をつけた無操作区間の数。
    pub idle_labeled: usize,
}

fn check_scale(v: Option<i64>) -> Result<i64> {
    match v {
        Some(v) if (1..=5).contains(&v) => Ok(v),
        _ => Err(Error::InvalidArgument("値は 1〜5 で指定してください".into())),
    }
}

/// 応答を反映する。
///
/// - push_log: `response_json` に応答を保存。`responded_at = now`（`dismiss` のみ設定しない）。
/// - `energy` / `stress`: checkin を作成（`push_id` 付き）。
/// - `text`（空白のみは無視）: hassle を作成（`push_id` 付き）。強度は `stress` の値、なければ 3。
///   `stress` と `text` が同時なら、checkin と hassle の両方を作る。
/// - `snooze_15` / `snooze_60`: now + 15 分 / 60 分。`snooze_today`: ローカルの翌日 0 時（`tz` で決まる）。
///   `snoozed_until` を当該 push_log に設定する（以後の `evaluate` が全トリガーを抑制）。
pub fn apply_response(
    conn: &Connection,
    push_id: &str,
    response: PushResponse,
    now: Millis,
    tz: Tz,
) -> Result<ResponseOutcome> {
    let log = get_log(conn, push_id)?;
    let action = response.action.as_deref();
    let mut out = ResponseOutcome::default();

    let (mut energy, mut stress) = (None, None);
    match action {
        Some("energy") => energy = Some(check_scale(response.value)?),
        Some("stress") => stress = Some(check_scale(response.value)?),
        Some("snooze_15") => out.snoozed_until = Some(now + 15 * 60_000),
        Some("snooze_60") => out.snoozed_until = Some(now + 60 * 60_000),
        Some("snooze_today") => {
            out.snoozed_until = Some(day_range(tz, local_date_of(tz, now)).end);
        }
        Some("open") | Some("dismiss") | None => {}
        Some(other) if other.starts_with(IDLE_ACTION_PREFIX) => {
            let preset: IdlePreset = other[IDLE_ACTION_PREFIX.len()..].parse()?;
            let range = serde_json::from_str::<PushPayload>(&log.payload_json)
                .ok()
                .and_then(|p| p.idle_range)
                .ok_or_else(|| Error::InvalidArgument("この通知には無操作の期間がありません".into()))?;
            out.idle_labeled = review::set_idle_range(conn, range.0, range.1, preset, now)?;
        }
        Some(other) => return Err(Error::InvalidArgument(format!("不明な応答アクション: {other}"))),
    }

    if energy.is_some() || stress.is_some() {
        let id = new_id();
        conn.execute(
            "INSERT INTO checkin (id, at, energy, stress, push_id) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![id, now, energy, stress, push_id],
        )?;
        out.checkin_id = Some(id);
    }
    if let Some(text) = response.text.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
        let id = new_id();
        let intensity = match response.value {
            Some(v) if action == Some("stress") && (1..=5).contains(&v) => v,
            _ => 3,
        };
        conn.execute(
            "INSERT INTO hassle (id, at, text, intensity, push_id) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![id, now, text, intensity, push_id],
        )?;
        out.hassle_id = Some(id);
    }

    let responded = (action != Some("dismiss")).then_some(now);
    let json = serde_json::to_string(&response).map_err(anyhow::Error::from)?;
    conn.execute(
        "UPDATE push_log SET responded_at = ?2, response_json = ?3, snoozed_until = COALESCE(?4, snoozed_until) WHERE id = ?1",
        params![log.id, responded, json, out.snoozed_until],
    )?;
    Ok(out)
}

/// 指定トリガーの今日の配信件数など、簡易な確認用。
pub fn count_delivered(conn: &Connection, trigger: PushTrigger, since: Millis) -> Result<i64> {
    Ok(conn.query_row(
        "SELECT COUNT(*) FROM push_log WHERE trigger = ?1 AND delivered = 1 AND at >= ?2",
        params![trigger.as_str(), since],
        |r| r.get(0),
    )?)
}
