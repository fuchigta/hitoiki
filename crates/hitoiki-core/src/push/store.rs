//! push_log の読み書きと、通知への応答の反映（checkin / hassle / スヌーズ）。

use super::payload::{
    BLOCK_SKIP, IDLE_ACTION_PREFIX, IDLE_TASK_ACTION, KR_SKIP_WEEK, TASK_LATER, TASK_SKIP_WEEK, TASK_START,
};
use super::{PushDecision, PushPayload, PushTrigger};
use crate::id::new_id;
use crate::model::PushLog;
use crate::service::correction;
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

/// 緊急・重要のしるしが付いた未完了のタスクを、通知の判定用に読む。`PushContext::tasks` の材料。
pub fn load_task_nudges(conn: &Connection) -> Result<Vec<super::TaskNudge>> {
    let last = crate::repo::segment::last_worked_by_open_task(conn)?;
    Ok(crate::repo::raw::open_tasks(conn)?
        .into_iter()
        .filter(|t| t.urgent == Some(true) || t.important == Some(true))
        .map(|t| super::TaskNudge {
            last_worked_at: last.get(&t.id).copied(),
            urgent: t.urgent == Some(true),
            important: t.important == Some(true),
            doing: t.status == crate::model::TaskStatus::Doing,
            id: t.id,
            title: t.title,
            created_at: t.created_at,
        })
        .collect())
}

/// 今週まだ時間が入っていない成果の指標を、通知の判定用に読む。`PushContext::kr_idle` の材料。
/// 対象の選び方は [`crate::service::goal::idle_key_results_this_week`]。
pub fn load_kr_idle(conn: &Connection, tz: Tz, now: Millis) -> Result<Vec<super::KrIdleNudge>> {
    Ok(crate::service::goal::idle_key_results_this_week(conn, tz, now)?
        .into_iter()
        .map(|k| super::KrIdleNudge {
            id: k.id,
            title: k.title,
        })
        .collect())
}

/// 今日の time_block と doing ログが指すタスクを、状態を問わず読む（`PushContext::task_refs` の材料）。
/// 存在しない ID は無視する。
pub fn load_task_refs(
    conn: &Connection,
    blocks: &[crate::model::TimeBlock],
    doing: &[super::DoingSpan],
) -> Result<Vec<super::TaskRef>> {
    let mut ids: Vec<&str> = blocks
        .iter()
        .filter_map(|b| b.task_id.as_deref())
        .chain(doing.iter().map(|d| d.task_id.as_str()))
        .collect();
    ids.sort_unstable();
    ids.dedup();
    let mut out = Vec::new();
    for id in ids {
        match crate::repo::task::get(conn, id) {
            Ok(t) => out.push(super::TaskRef {
                id: t.id,
                title: t.title,
                status: t.status,
            }),
            Err(Error::NotFound(_)) => {}
            Err(e) => return Err(e),
        }
    }
    Ok(out)
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
/// `idle_task`（idle_return の「{タスク名}をしていた」。無操作の期間をそのタスクの PC 外の作業として確定する）、
/// `block_skip`（block_start の「今回はやめる」。記録だけ）、
/// `task_start`（urgent_task / important_task の対象タスクを始める。開始そのものは `ResponseOutcome::start_task`
/// を受けた呼び出し側が行う）、`task_later` / `task_skip_week`（記録だけ。後者は今週その通知の対象から外す）、
/// `kr_skip_week`（kr_idle の「今週は見送る」。記録だけ）、
/// `open`（アプリを開いた）、`dismiss`（見送り。応答とは数えず `responded_at` を付けない。
/// `response_json` には残るので未回答の一覧からは外れるが、応答率の算出では無応答のまま）。
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
    /// `task_start` の応答で始めるタスクの ID。実際の開始は `Ctx` を持つ呼び出し側が
    /// `service::pushlog::start_pushed_task` で行う。
    pub start_task: Option<String>,
}

fn check_scale(v: Option<i64>) -> Result<i64> {
    match v {
        Some(v) if (1..=5).contains(&v) => Ok(v),
        _ => Err(Error::InvalidArgument("値は 1〜5 で指定してください".into())),
    }
}

/// 通知のボタン以外で与える、チェックインの値（CLI の `push answer` など）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Scales {
    pub energy: Option<i64>,
    pub mood: Option<i64>,
    pub stress: Option<i64>,
}

/// 応答を反映する。
///
/// - 記録（checkin / hassle）の `at` は、応答時刻ではなく **通知の時刻**（`push_log.at`）にする。
///   通知の時点の状態への答えなので、あとから答えても時系列がずれない。
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
    apply_response_with(conn, push_id, response, Scales::default(), now, tz)
}

/// [`apply_response`] に、ボタン以外のチェックイン値（`scales`）を加えたもの。値は 1〜5。
/// ボタン（`energy` / `stress`）と同じ項目が重なったときはボタンの値を優先する。
pub fn apply_response_with(
    conn: &Connection,
    push_id: &str,
    response: PushResponse,
    scales: Scales,
    now: Millis,
    tz: Tz,
) -> Result<ResponseOutcome> {
    let log = get_log(conn, push_id)?;
    let action = response.action.as_deref();
    let mut out = ResponseOutcome::default();
    let opt_scale = |v: Option<i64>| v.map(|v| check_scale(Some(v))).transpose();

    let mut energy = opt_scale(scales.energy)?;
    let mood = opt_scale(scales.mood)?;
    let mut stress = opt_scale(scales.stress)?;
    match action {
        Some("energy") => energy = Some(check_scale(response.value)?),
        Some("stress") => stress = Some(check_scale(response.value)?),
        Some("snooze_15") => out.snoozed_until = Some(now + 15 * 60_000),
        Some("snooze_60") => out.snoozed_until = Some(now + 60 * 60_000),
        Some("snooze_today") => {
            out.snoozed_until = Some(day_range(tz, local_date_of(tz, now)).end);
        }
        Some(TASK_START) => {
            let task_id = serde_json::from_str::<PushPayload>(&log.payload_json)
                .ok()
                .and_then(|p| p.task_id)
                .ok_or_else(|| Error::InvalidArgument("この通知には対象のタスクがありません".into()))?;
            out.start_task = Some(task_id);
        }
        Some(TASK_LATER) | Some(TASK_SKIP_WEEK) | Some(BLOCK_SKIP) | Some(KR_SKIP_WEEK) => {}
        Some(IDLE_TASK_ACTION) => {
            let p = serde_json::from_str::<PushPayload>(&log.payload_json).ok();
            let range = p
                .as_ref()
                .and_then(|p| p.idle_range)
                .ok_or_else(|| Error::InvalidArgument("この通知には無操作の期間がありません".into()))?;
            let task_id = p
                .and_then(|p| p.task_id)
                .ok_or_else(|| Error::InvalidArgument("この通知には対象のタスクがありません".into()))?;
            out.idle_labeled = correction::assign_off_pc_range(conn, range.0, range.1, &task_id, now)?;
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

    if energy.is_some() || mood.is_some() || stress.is_some() {
        let id = new_id();
        conn.execute(
            "INSERT INTO checkin (id, at, energy, mood, stress, push_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, log.at, energy, mood, stress, push_id],
        )?;
        out.checkin_id = Some(id);
    }
    if let Some(text) = response.text.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
        let id = new_id();
        let intensity = match response.value {
            Some(v) if action == Some("stress") && (1..=5).contains(&v) => v,
            _ => stress.unwrap_or(3),
        };
        conn.execute(
            "INSERT INTO hassle (id, at, text, intensity, push_id) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![id, log.at, text, intensity, push_id],
        )?;
        out.hassle_id = Some(id);
    }

    let responded = (action != Some("dismiss")).then_some(now);
    let mut json = serde_json::to_value(&response).map_err(anyhow::Error::from)?;
    for (k, v) in [
        ("energy", scales.energy),
        ("mood", scales.mood),
        ("stress", scales.stress),
    ] {
        if let (Some(v), Some(o)) = (v, json.as_object_mut()) {
            o.insert(k.into(), v.into());
        }
    }
    let json = json.to_string();
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
