use crate::state::{AppState, NudgeEvent};
use crate::{events, runtime, window};
use hitoiki_core::nudge::{NudgeAction, NudgeInput, NudgePayload, NudgeTrigger};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

/// アプリ固有の AUMID。
pub const AUMID: &str = "Hitoiki.App";

/// 通知のアクティベーション（ボタン押下／本体クリック）。
///
/// トーストの引数 `action=<id>&value=<value>&nudge=<nudge_id>&open=<screen>` から作る。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Activation {
    /// 押されたボタンの `NudgeAction.id`（本体クリックは None）。
    pub action: Option<String>,
    pub value: Option<i64>,
    /// 入力欄のテキスト。
    pub input: Option<String>,
    pub nudge_id: Option<String>,
    /// ペイロードの `open_screen`。
    pub open_screen: Option<String>,
}

/// 入力欄だけで答える通知（ボタン無し）用の、テキスト送信ボタンの `NudgeAction.id`。
pub const TEXT_ACTION: &str = "text";

/// 設定画面の動作確認用のテスト通知（調子を尋ねる問いかけ相当。nudge_log には紐付かない）。
pub fn test_payload() -> NudgePayload {
    NudgePayload {
        trigger: NudgeTrigger::LongWork,
        title: "ひといき".into(),
        body: "いまのエネルギーは？".into(),
        actions: (1..=5)
            .map(|n| NudgeAction {
                id: "energy".into(),
                label: n.to_string(),
                value: Some(n),
            })
            .collect(),
        input: Some(NudgeInput {
            id: "concern".into(),
            placeholder: "一言メモ（任意）".into(),
        }),
        open_screen: Some("records".into()),
        idle_range: None,
        task_id: None,
        block_id: None,
    }
}

/// 更新のトーストのボタン `NudgeAction.id`。押すとリリースページをブラウザで開く。
pub const UPDATE_ACTION: &str = "update";
/// 更新のトーストの本体クリックを表す `open_screen` の値（画面名ではない）。
pub const UPDATE_OPEN: &str = "update";

/// 「新しいバージョンがあります」のトースト用ペイロード。`trigger` は型を満たすための値で、
/// 問いかけ（nudge_log）とは無関係。
pub fn update_payload(version: &str) -> NudgePayload {
    NudgePayload {
        trigger: NudgeTrigger::LongWork,
        title: "ひといき".into(),
        body: format!("新しいバージョン v{version} があります"),
        actions: vec![NudgeAction {
            id: UPDATE_ACTION.into(),
            label: "ダウンロードページを開く".into(),
            value: None,
        }],
        input: None,
        open_screen: Some(UPDATE_OPEN.into()),
        idle_range: None,
        task_id: None,
        block_id: None,
    }
}

/// 最新リリースのページを既定のブラウザで開く（固定 URL。自動ダウンロードはしない）。
pub fn open_release_page() {
    let url = hitoiki_core::service::update::RELEASE_PAGE_URL;
    let r = if cfg!(windows) {
        std::process::Command::new("explorer.exe").arg(url).spawn()
    } else {
        std::process::Command::new("xdg-open").arg(url).spawn()
    };
    if let Err(e) = r {
        tracing::warn!("リリースページを開けませんでした: {e}");
    }
}

/// 起動時に一度呼ぶ。AUMID を登録する。失敗してもフォールバックがあるので致命的ではない。
pub fn init() {
    if let Err(e) = platform::register() {
        tracing::warn!("AUMID を登録できませんでした: {e}");
    }
}

/// トースト通知を出す。ボタン押下は `runtime::handle_activation` に渡る。
pub fn show_nudge(app: &AppHandle, nudge_id: Option<&str>, payload: &NudgePayload) -> Result<(), String> {
    let handle = app.clone();
    let persistent = hitoiki_core::config::Config::load()
        .map(|c| c.nudge.toast_persistent)
        .unwrap_or(true);
    let scenario = toast_scenario(nudge_id, payload, persistent);
    platform::show(nudge_id, payload, scenario, move |act| {
        runtime::handle_activation(&handle, act)
    })
}

/// 問いかけを届ける: UI へ `nudge` イベントを送り、トーストを試し、失敗したらポップアップ窓に切り替える。
/// 通知の一時停止中は何もしない（`false` を返す）。
pub fn deliver(app: &AppHandle, nudge_id: &str, payload: &NudgePayload) -> bool {
    let state = app.state::<AppState>();
    if state.is_snoozed(hitoiki_core::Clock::now_ms(&hitoiki_core::SystemClock)) {
        tracing::info!(nudge_id, "通知の一時停止中のため表示しません");
        return false;
    }
    let ev = NudgeEvent {
        nudge_id: Some(nudge_id.to_string()),
        payload: payload.clone(),
    };
    state.set_last_nudge(Some(ev.clone()));
    events::emit_nudge(app, &ev);
    if let Err(e) = show_nudge(app, Some(nudge_id), payload) {
        tracing::warn!("トーストを出せないためポップアップに切り替えます: {e}");
        if let Err(e) = window::open_popup(app) {
            tracing::warn!("ポップアップも開けませんでした: {e}");
        }
    }
    true
}

/// トーストの `scenario` 属性。回答を求める通知（nudge_log に紐づき、ボタンか入力欄があるもの）は、
/// `persistent` のとき `reminder`（操作するまで画面に残る）にする。更新のお知らせやテスト通知
/// （nudge_log に紐づかない）、ボタンの無い通知は既定のまま（None）。
/// なお `reminder` はボタンが 1 つ以上必要で、入力欄だけの通知にはここの [`toast_actions`] が送信ボタンを足す。
pub fn toast_scenario(nudge_id: Option<&str>, p: &NudgePayload, persistent: bool) -> Option<&'static str> {
    (persistent && nudge_id.is_some() && !toast_actions(p).is_empty()).then_some("reminder")
}

/// トーストに出すボタンの一覧。入力欄だけの通知には送信ボタンを足す。
pub fn toast_actions(p: &NudgePayload) -> Vec<NudgeAction> {
    let mut v: Vec<NudgeAction> = p.actions.iter().take(5).cloned().collect();
    if v.is_empty() && p.input.is_some() {
        v.push(NudgeAction {
            id: TEXT_ACTION.into(),
            label: "ひとこと残す".into(),
            value: None,
        });
    }
    v
}

// ===== Windows 実装 =====

#[cfg(windows)]
mod platform {
    use super::{toast_actions, Activation, AUMID};
    use hitoiki_core::nudge::{NudgeAction, NudgePayload};
    use std::sync::{Arc, Mutex};
    use windows::core::{Interface, HSTRING, PCWSTR};
    use windows::Data::Xml::Dom::XmlDocument;
    use windows::Foundation::{IPropertyValue, TypedEventHandler};
    use windows::Win32::System::Registry::{RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ};
    use windows::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID;
    use windows::UI::Notifications::{
        ToastActivatedEventArgs, ToastDismissedEventArgs, ToastFailedEventArgs, ToastNotification,
        ToastNotificationManager,
    };

    const ICON_PNG: &[u8] = include_bytes!("../icons/icon.png");

    /// 表示中トーストの参照を保持する（破棄されるとイベントが届かなくなるため）。
    static LIVE: Mutex<Vec<ToastNotification>> = Mutex::new(Vec::new());

    fn reg_set(subkey: &str, name: &str, value: &str) -> Result<(), String> {
        let sub = HSTRING::from(subkey);
        let nm = HSTRING::from(name);
        let wide: Vec<u16> = value.encode_utf16().chain(std::iter::once(0)).collect();
        // SAFETY: wide は NUL 終端で、呼び出しの間生存する。サイズはバイト数。
        let r = unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                PCWSTR(sub.as_ptr()),
                PCWSTR(nm.as_ptr()),
                REG_SZ.0,
                Some(wide.as_ptr().cast()),
                (wide.len() * 2) as u32,
            )
        };
        if r.0 == 0 {
            Ok(())
        } else {
            Err(format!("レジストリ書き込み失敗 (code {})", r.0))
        }
    }

    pub fn register() -> Result<(), String> {
        let icon = hitoiki_core::paths::data_dir().join("app-icon.png");
        if let Some(dir) = icon.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        std::fs::write(&icon, ICON_PNG).map_err(|e| e.to_string())?;
        let key = format!("Software\\Classes\\AppUserModelId\\{AUMID}");
        reg_set(&key, "DisplayName", "Hitoiki")?;
        reg_set(&key, "IconUri", &icon.display().to_string())?;
        reg_set(&key, "IconBackgroundColor", "FF4F8A8B")?;
        // 前回起動時のトーストはボタンを押しても受け手がいないので、アクションセンターから消す。
        if let Ok(h) = ToastNotificationManager::History() {
            let _ = h.ClearWithId(&HSTRING::from(AUMID));
        }
        // SAFETY: AUMID は有効な文字列。
        unsafe { SetCurrentProcessExplicitAppUserModelID(&HSTRING::from(AUMID)) }
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    fn esc(s: &str) -> String {
        s.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
            .replace('\'', "&apos;")
    }

    /// 引数文字列用の簡易パーセントエンコード。
    fn enc(s: &str) -> String {
        let mut o = String::new();
        for b in s.bytes() {
            if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b':') {
                o.push(b as char);
            } else {
                o.push_str(&format!("%{b:02X}"));
            }
        }
        o
    }

    fn dec(s: &str) -> String {
        let b = s.as_bytes();
        let mut out = Vec::new();
        let mut i = 0;
        while i < b.len() {
            if b[i] == b'%' && i + 2 < b.len() {
                let hex = std::str::from_utf8(&b[i + 1..i + 3]).ok();
                if let Some(v) = hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                    out.push(v);
                    i += 3;
                    continue;
                }
            }
            out.push(b[i]);
            i += 1;
        }
        String::from_utf8_lossy(&out).into_owned()
    }

    fn args(action: Option<&NudgeAction>, nudge: Option<&str>, open: Option<&str>) -> String {
        let mut v = Vec::new();
        if let Some(a) = action {
            v.push(format!("action={}", enc(&a.id)));
            if let Some(n) = a.value {
                v.push(format!("value={n}"));
            }
        }
        if let Some(p) = nudge {
            v.push(format!("nudge={}", enc(p)));
        }
        if let Some(o) = open {
            v.push(format!("open={}", enc(o)));
        }
        v.join("&")
    }

    fn parse_args(s: &str) -> Activation {
        let mut a = Activation::default();
        for kv in s.split('&') {
            match kv.split_once('=') {
                Some(("action", v)) => a.action = Some(dec(v)),
                Some(("value", v)) => a.value = dec(v).parse().ok(),
                Some(("nudge", v)) => a.nudge_id = Some(dec(v)),
                Some(("open", v)) => a.open_screen = Some(dec(v)),
                _ => {}
            }
        }
        a
    }

    pub(super) fn build_xml(nudge_id: Option<&str>, p: &NudgePayload, scenario: Option<&str>) -> String {
        let open = p.open_screen.as_deref();
        let scenario = scenario.map_or_else(String::new, |s| format!(" scenario=\"{}\"", esc(s)));
        let mut x = format!(
            "<toast{scenario} launch=\"{}\" activationType=\"foreground\"><visual><binding template=\"ToastGeneric\">             <text>{}</text><text>{}</text></binding></visual><actions>",
            esc(&args(None, nudge_id, open)),
            esc(&p.title),
            esc(&p.body)
        );
        if let Some(i) = &p.input {
            x.push_str(&format!(
                "<input id=\"note\" type=\"text\" placeHolderContent=\"{}\"/>",
                esc(&i.placeholder)
            ));
        }
        // トーストのボタンは最大 5 個。
        for a in toast_actions(p) {
            x.push_str(&format!(
                "<action content=\"{}\" arguments=\"{}\" activationType=\"foreground\"/>",
                esc(&a.label),
                esc(&args(Some(&a), nudge_id, open))
            ));
        }
        x.push_str("</actions></toast>");
        x
    }

    pub fn show(
        nudge_id: Option<&str>,
        payload: &NudgePayload,
        scenario: Option<&str>,
        on_activated: impl Fn(Activation) + Send + Sync + 'static,
    ) -> Result<(), String> {
        let e = |e: windows::core::Error| e.to_string();
        let xml = build_xml(nudge_id, payload, scenario);
        let doc = XmlDocument::new().map_err(e)?;
        doc.LoadXml(&HSTRING::from(xml)).map_err(e)?;
        let toast = ToastNotification::CreateToastNotification(&doc).map_err(e)?;

        let cb = Arc::new(on_activated);
        toast
            .Activated(&TypedEventHandler::new(
                move |_s: windows::core::Ref<ToastNotification>,
                      a: windows::core::Ref<windows::core::IInspectable>| {
                    let Some(insp) = a.as_ref() else {
                        return Ok(());
                    };
                    let Ok(args) = insp.cast::<ToastActivatedEventArgs>() else {
                        return Ok(());
                    };
                    let raw = args.Arguments().map(|s| s.to_string()).unwrap_or_default();
                    let mut act = parse_args(&raw);
                    let input = args
                        .UserInput()
                        .ok()
                        .and_then(|vs| vs.Lookup(&HSTRING::from("note")).ok())
                        .and_then(|v| v.cast::<IPropertyValue>().ok())
                        .and_then(|pv| pv.GetString().ok())
                        .map(|s| s.to_string());
                    act.input = input;
                    cb(act);
                    Ok(())
                },
            ))
            .map_err(e)?;
        toast
            .Failed(&TypedEventHandler::new(
                |_s: windows::core::Ref<ToastNotification>, a: windows::core::Ref<ToastFailedEventArgs>| {
                    if let Some(a) = a.as_ref() {
                        tracing::warn!("トースト失敗: {:?}", a.ErrorCode());
                    }
                    Ok(())
                },
            ))
            .map_err(e)?;
        toast
            .Dismissed(&TypedEventHandler::new(
                |_s: windows::core::Ref<ToastNotification>,
                 a: windows::core::Ref<ToastDismissedEventArgs>| {
                    if let Some(a) = a.as_ref() {
                        tracing::debug!("トースト消去: {:?}", a.Reason());
                    }
                    Ok(())
                },
            ))
            .map_err(e)?;

        let notifier =
            ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(AUMID)).map_err(e)?;
        // 診断: 通知が OS 側で無効化されていないか（集中モード等とは別の設定）。
        tracing::debug!("通知設定: {:?}", notifier.Setting().ok());
        notifier.Show(&toast).map_err(e)?;
        if let Ok(h) = ToastNotificationManager::History() {
            let n = h
                .GetHistoryWithId(&HSTRING::from(AUMID))
                .ok()
                .map(|v| v.Size().unwrap_or(0));
            tracing::debug!("アクションセンター上の通知数: {n:?}");
        }
        if let Ok(mut live) = LIVE.lock() {
            live.push(toast);
            if live.len() > 20 {
                live.remove(0);
            }
        }
        Ok(())
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::notify::test_payload;
        use hitoiki_core::nudge::NudgeInput;

        #[test]
        fn 引数の往復() {
            let act = NudgeAction {
                id: "energy".into(),
                label: "3".into(),
                value: Some(3),
            };
            let a = args(Some(&act), Some("01 AB&x"), Some("records/new/day_end"));
            let p = parse_args(&a);
            assert_eq!(p.action.as_deref(), Some("energy"));
            assert_eq!(p.value, Some(3));
            assert_eq!(p.nudge_id.as_deref(), Some("01 AB&x"));
            assert_eq!(p.open_screen.as_deref(), Some("records/new/day_end"));
        }

        #[test]
        fn xmlにボタンと入力欄が入る() {
            let mut p = test_payload();
            p.title = "a<b".into();
            let x = build_xml(Some("P1"), &p, None);
            assert_eq!(x.matches("<action ").count(), 5);
            assert!(!x.contains("scenario"));
            assert!(x.contains("<input id=\"note\""));
            assert!(x.contains("a&lt;b"));
            assert!(x.contains("action=energy&amp;value=3&amp;nudge=P1"));
        }

        #[test]
        fn 入力欄だけの通知には送信ボタンが付く() {
            let mut p = test_payload();
            p.actions.clear();
            p.input = Some(NudgeInput {
                id: "concern".into(),
                placeholder: "x".into(),
            });
            let x = build_xml(Some("P1"), &p, Some("reminder"));
            assert_eq!(x.matches("<action ").count(), 1);
            assert!(x.starts_with("<toast scenario=\"reminder\" "));
            assert!(x.contains("action=text"));
            p.input = None;
            assert_eq!(build_xml(None, &p, None).matches("<action ").count(), 0);
        }
    }
}

// ===== Windows 以外（トースト無し。常にフォールバックへ） =====

#[cfg(not(windows))]
mod platform {
    use super::Activation;
    use hitoiki_core::nudge::NudgePayload;

    pub fn register() -> Result<(), String> {
        Ok(())
    }

    pub fn show(
        _nudge_id: Option<&str>,
        _payload: &NudgePayload,
        _scenario: Option<&str>,
        _on_activated: impl Fn(Activation) + Send + Sync + 'static,
    ) -> Result<(), String> {
        Err("このプラットフォームではトースト通知に未対応です".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 回答を求める通知だけ残り続ける() {
        let ask = test_payload();
        assert_eq!(toast_scenario(Some("P1"), &ask, true), Some("reminder"));
        // 設定で切れる。
        assert_eq!(toast_scenario(Some("P1"), &ask, false), None);
        // 更新のお知らせ・テスト通知（nudge_log に紐づかない）は対象外。
        assert_eq!(toast_scenario(None, &update_payload("1.0.0"), true), None);
        assert_eq!(toast_scenario(None, &ask, true), None);
        // 入力欄だけの通知は送信ボタンが付くので対象。ボタンも入力欄も無ければ対象外。
        let mut only_input = ask.clone();
        only_input.actions.clear();
        assert_eq!(toast_scenario(Some("P1"), &only_input, true), Some("reminder"));
        only_input.input = None;
        assert_eq!(toast_scenario(Some("P1"), &only_input, true), None);
    }
}
