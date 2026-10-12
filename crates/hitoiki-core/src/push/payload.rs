//! 通知の中身。「1 アクションで答えられる」ことを基本にする（docs/dev/architecture.md「プッシュ通知」）。
//! 文言は押し付けない寄り添うトーンで、評価や良し悪しの断定をしない（docs/dev/architecture.md「設計原則」）。

use super::PushTrigger;
use crate::Millis;
use serde::{Deserialize, Serialize};

/// 通知のボタン。押すと [`super::PushResponse`] の `action` / `value` になる。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PushAction {
    /// "energy" | "stress" | "snooze_15" | "snooze_60" | "snooze_today" | "open" など。
    pub id: String,
    pub label: String,
    /// 1〜5 の段階値など。
    pub value: Option<i64>,
}

/// 任意のテキスト入力欄（一言メモ）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PushInput {
    /// 入力の保存先を示す ID（"hassle"）。
    pub id: String,
    pub placeholder: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PushPayload {
    pub trigger: PushTrigger,
    pub title: String,
    pub body: String,
    pub actions: Vec<PushAction>,
    pub input: Option<PushInput>,
    /// 通知クリックで開くアプリの画面（例 "reflection/day_start"）。
    pub open_screen: Option<String>,
    /// idle_return: 「さっきの無操作は？」の対象の期間 (開始, 終了)。ボタンの応答でこの期間に分類をつける。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idle_range: Option<(Millis, Millis)>,
    /// urgent_task / important_task: 対象のタスク。`task_start` の応答でこのタスクを始める。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
}

fn scale(id: &str) -> Vec<PushAction> {
    (1..=5)
        .map(|v| PushAction {
            id: id.into(),
            label: v.to_string(),
            value: Some(v),
        })
        .collect()
}

fn open_action(label: &str) -> PushAction {
    PushAction {
        id: "open".into(),
        label: label.into(),
        value: None,
    }
}

fn hassle_input() -> Option<PushInput> {
    Some(PushInput {
        id: "hassle".into(),
        placeholder: "ひとこと（気になったことなど）".into(),
    })
}

pub(crate) fn day_start() -> PushPayload {
    PushPayload {
        trigger: PushTrigger::DayStart,
        title: "おはようございます".into(),
        body: "今日のはじまりに、いまの元気度を教えてください（1 = ゆったり 〜 5 = 元気いっぱい）。".into(),
        actions: scale("energy"),
        input: None,
        // 始業の通知は「今日のデザイン」（今日の画面）へ。始業の振り返りは今日の画面から開ける。
        open_screen: Some("today/design".into()),
        idle_range: None,
        task_id: None,
    }
}

pub(crate) fn day_end(needs_review: u32) -> PushPayload {
    let body = if needs_review > 0 {
        format!(
            "おつかれさまでした。確認待ちの区間が {needs_review} 件あります。よければ振り返りで見ていきませんか。"
        )
    } else {
        "おつかれさまでした。よければ今日を一緒に振り返りませんか。".into()
    };
    PushPayload {
        trigger: PushTrigger::DayEnd,
        title: "今日の終わりに".into(),
        body,
        actions: vec![open_action("振り返りを開く")],
        input: None,
        open_screen: Some("reflection/day_end".into()),
        idle_range: None,
        task_id: None,
    }
}

pub(crate) fn weekly() -> PushPayload {
    PushPayload {
        trigger: PushTrigger::Weekly,
        title: "今週をふりかえる時間です".into(),
        body: "一週間の時間の使い方を、ゆっくり眺めてみませんか。".into(),
        actions: vec![open_action("週次の振り返りを開く")],
        input: None,
        open_screen: Some("reflection/weekly".into()),
        idle_range: None,
        task_id: None,
    }
}

pub(crate) fn long_focus(minutes: u64, label: Option<&str>) -> PushPayload {
    let what = label.unwrap_or("同じ作業");
    PushPayload {
        trigger: PushTrigger::LongFocus,
        title: "一息つきませんか".into(),
        body: format!(
            "{what}に {minutes} 分ほど続けて取り組んでいます。いまの元気度はどのくらいですか（1 〜 5）。"
        ),
        actions: scale("energy"),
        input: None,
        open_screen: Some("checkin".into()),
        idle_range: None,
        task_id: None,
    }
}

pub(crate) fn switch_spike(window_min: u64, count: u32) -> PushPayload {
    PushPayload {
        trigger: PushTrigger::SwitchSpike,
        title: "切り替えが続いています".into(),
        body: format!(
            "直近 {window_min} 分で画面の切り替えが {count} 回ありました。いまのストレス感を教えてください（1 = 穏やか 〜 5 = 強め）。"
        ),
        actions: scale("stress"),
        input: hassle_input(),
        open_screen: Some("hassle".into()),
        idle_range: None,
        task_id: None,
    }
}

pub(crate) fn after_interrupt() -> PushPayload {
    PushPayload {
        trigger: PushTrigger::AfterInterrupt,
        title: "割り込みの対応、おつかれさまでした".into(),
        body: "よければ、何があったかをひとこと残しませんか。気持ちの強さも選べます（1 〜 5）。".into(),
        actions: scale("stress"),
        input: hassle_input(),
        open_screen: Some("hassle".into()),
        idle_range: None,
        task_id: None,
    }
}

pub(crate) fn meeting_overrun() -> PushPayload {
    PushPayload {
        trigger: PushTrigger::MeetingOverrun,
        title: "会議が予定の時間を過ぎています".into(),
        body: "気になったことがあれば、あとで思い出せるようにひとこと残せます。".into(),
        actions: vec![],
        input: hassle_input(),
        open_screen: Some("hassle".into()),
        idle_range: None,
        task_id: None,
    }
}

pub(crate) fn late_work() -> PushPayload {
    PushPayload {
        trigger: PushTrigger::LateWork,
        title: "遅い時間ですね".into(),
        body: "いまの元気度を教えてください（1 〜 5）。無理のない範囲で。".into(),
        actions: scale("energy"),
        input: None,
        open_screen: Some("checkin".into()),
        idle_range: None,
        task_id: None,
    }
}

/// idle_return: 無操作から戻ったときに、その時間が何だったかを 1 タップで答えてもらう。
/// ボタンの `id` は `idle_<分類>`（`service::review::IdlePreset` の ID）。
pub(crate) fn idle_return(start: Millis, end: Millis) -> PushPayload {
    let minutes = ((end - start) / 60_000).max(1);
    PushPayload {
        trigger: PushTrigger::IdleReturn,
        title: "おかえりなさい".into(),
        body: format!("さっきの {minutes} 分は、どう過ごしていましたか？ 答えなくても大丈夫です。"),
        actions: crate::service::review::IdlePreset::ALL
            .iter()
            .map(|p| PushAction {
                id: format!("{IDLE_ACTION_PREFIX}{}", p.as_str()),
                label: p.label().into(),
                value: None,
            })
            .collect(),
        input: None,
        open_screen: Some("review".into()),
        idle_range: Some((start, end)),
        task_id: None,
    }
}

/// idle_return のボタン ID の接頭辞。
pub const IDLE_ACTION_PREFIX: &str = "idle_";

/// タスク通知のボタン ID。
pub const TASK_START: &str = "task_start";
pub const TASK_LATER: &str = "task_later";
pub const TASK_SKIP_WEEK: &str = "task_skip_week";

fn task_button(id: &str, label: &str) -> PushAction {
    PushAction {
        id: id.into(),
        label: label.into(),
        value: None,
    }
}

/// urgent_task: 緊急のしるしが付いたタスクを 1 件示す。`others` は同じしるしの付いたほかのタスクの数。
pub(crate) fn urgent_task(task_id: &str, title: &str, others: usize) -> PushPayload {
    let more = if others > 0 {
        format!("（ほかに {others} 件）")
    } else {
        String::new()
    };
    PushPayload {
        trigger: PushTrigger::UrgentTask,
        title: "緊急のタスクがあります".into(),
        body: format!("「{title}」に緊急のしるしが付いています。いま取りかかりますか？{more}"),
        actions: vec![
            task_button(TASK_START, "始める"),
            task_button(TASK_LATER, "あとで"),
        ],
        input: None,
        open_screen: Some("tasks".into()),
        idle_range: None,
        task_id: Some(task_id.into()),
    }
}

/// important_task: 重要だが緊急でないタスクに、少し時間を取ることを提案する。`days` は最後に手をつけてからの日数
/// （一度も時間が入っていなければ None）。
pub(crate) fn important_task(task_id: &str, title: &str, days: Option<u64>) -> PushPayload {
    let since = match days {
        Some(n) => format!("ここ {n} 日ほど時間が入っていません"),
        None => "まだ時間が入っていません".to_string(),
    };
    PushPayload {
        trigger: PushTrigger::ImportantTask,
        title: "重要なタスクに少し時間を取りませんか".into(),
        body: format!("「{title}」に、{since}。今日どこかで少し時間を取りますか？"),
        actions: vec![
            task_button(TASK_START, "始める"),
            // 押すと今日のデザイン画面（open_screen）を開く。
            open_action("今日の予定に入れる"),
            task_button(TASK_SKIP_WEEK, "今週は見送る"),
        ],
        input: None,
        open_screen: Some("today/design".into()),
        idle_range: None,
        task_id: Some(task_id.into()),
    }
}
