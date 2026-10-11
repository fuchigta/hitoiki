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
    }
}

/// idle_return のボタン ID の接頭辞。
pub const IDLE_ACTION_PREFIX: &str = "idle_";
