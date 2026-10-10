//! プッシュ通知の表示（9.3）。
//!
//! 検証の結果、`tauri-plugin-notification` のデスクトップ版（notify-rust 経由）は Windows で
//! アクションボタン・入力欄・アクティベーションのコールバックに対応していないため、
//! `windows` クレートの WinRT トースト API を直接使う（`winrt` モジュール）。
//!
//! - AUMID は `HKCU\Software\Classes\AppUserModelId\<AUMID>` に DisplayName/IconUri を登録する
//!   （管理者権限・パッケージ化・ショートカット不要）。
//! - アプリ実行中のボタン押下・入力値は `ToastNotification.Activated` で受ける。
//! - トーストを出せない場合は、右下の小さなポップアップ窓（`window::open_popup`）に切り替える。

use crate::{events, runtime, state::AppState, window};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Manager};

/// アプリ固有の AUMID。
pub const AUMID: &str = "Hitoiki.App";

/// プッシュのペイロード（暫定型。`hitoiki_core::push` の型が確定したら差し替える）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PushPayload {
    pub title: String,
    pub body: String,
    pub actions: Vec<PushAction>,
    /// テキスト入力欄（一言メモ）。
    pub input: Option<PushInput>,
    /// 応答の紐付け用（push_log の ID）。
    pub push_id: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PushAction {
    /// 例: `energy:3`、`open:records`。
    pub id: String,
    pub label: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PushInput {
    pub placeholder: String,
}

impl PushPayload {
    /// `serde_json::Value` から作る。
    pub fn from_value(v: &Value) -> Result<Self, String> {
        serde_json::from_value(v.clone()).map_err(|e| format!("プッシュのペイロードが不正です: {e}"))
    }

    /// チェックイン（エネルギー 1〜5 ＋ 一言メモ）の既定ペイロード。
    pub fn checkin() -> Self {
        Self {
            title: "ひといき".into(),
            body: "いまのエネルギーは？".into(),
            actions: (1..=5)
                .map(|n| PushAction {
                    id: format!("energy:{n}"),
                    label: n.to_string(),
                })
                .collect(),
            input: Some(PushInput {
                placeholder: "一言メモ（任意）".into(),
            }),
            push_id: None,
        }
    }
}

/// 通知のアクティベーション（ボタン押下／本体クリック）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Activation {
    pub action_id: Option<String>,
    pub input: Option<String>,
    pub push_id: Option<String>,
}

/// 起動時に一度呼ぶ。AUMID を登録する。失敗してもフォールバックがあるので致命的ではない。
pub fn init() {
    if let Err(e) = platform::register() {
        tracing::warn!("AUMID を登録できませんでした: {e}");
    }
}

/// トースト通知を出す。ボタン押下は `runtime::handle_activation` に渡る。
pub fn show_push(app: &AppHandle, payload: &PushPayload) -> Result<(), String> {
    let handle = app.clone();
    platform::show(payload, move |act| runtime::handle_activation(&handle, act))
}

/// プッシュを届ける: UI へ `push` イベントを送り、トーストを試し、失敗したらポップアップ窓に切り替える。
/// 通知の一時停止中は何もしない（`false` を返す）。
pub fn deliver(app: &AppHandle, payload: &Value) -> bool {
    let state = app.state::<AppState>();
    if state.is_snoozed(hitoiki_core::Clock::now_ms(&hitoiki_core::SystemClock)) {
        return false;
    }
    let p = match PushPayload::from_value(payload) {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!("{e}");
            return false;
        }
    };
    state.set_last_push(Some(payload.clone()));
    events::emit_push(app, payload);
    if let Err(e) = show_push(app, &p) {
        tracing::warn!("トーストを出せないためポップアップに切り替えます: {e}");
        if let Err(e) = window::open_popup(app) {
            tracing::warn!("ポップアップも開けませんでした: {e}");
        }
    }
    true
}

// ===== Windows 実装 =====

#[cfg(windows)]
mod platform {
    use super::{Activation, PushPayload, AUMID};
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

    fn args(action: Option<&str>, push: Option<&str>) -> String {
        let mut v = Vec::new();
        if let Some(a) = action {
            v.push(format!("action={}", enc(a)));
        }
        if let Some(p) = push {
            v.push(format!("push={}", enc(p)));
        }
        v.join("&")
    }

    fn parse_args(s: &str) -> (Option<String>, Option<String>) {
        let (mut action, mut push) = (None, None);
        for kv in s.split('&') {
            match kv.split_once('=') {
                Some(("action", v)) => action = Some(dec(v)),
                Some(("push", v)) => push = Some(dec(v)),
                _ => {}
            }
        }
        (action, push)
    }

    pub(super) fn build_xml(p: &PushPayload) -> String {
        let push = p.push_id.as_deref();
        let mut x = format!(
            "<toast launch=\"{}\" activationType=\"foreground\"><visual><binding template=\"ToastGeneric\">\
             <text>{}</text><text>{}</text></binding></visual><actions>",
            esc(&args(None, push)),
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
        for a in p.actions.iter().take(5) {
            x.push_str(&format!(
                "<action content=\"{}\" arguments=\"{}\" activationType=\"foreground\"/>",
                esc(&a.label),
                esc(&args(Some(&a.id), push))
            ));
        }
        x.push_str("</actions></toast>");
        x
    }

    pub fn show(
        payload: &PushPayload,
        on_activated: impl Fn(Activation) + Send + Sync + 'static,
    ) -> Result<(), String> {
        let e = |e: windows::core::Error| e.to_string();
        let xml = build_xml(payload);
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
                    let (action_id, push_id) = parse_args(&raw);
                    let input = args
                        .UserInput()
                        .ok()
                        .and_then(|vs| vs.Lookup(&HSTRING::from("note")).ok())
                        .and_then(|v| v.cast::<IPropertyValue>().ok())
                        .and_then(|pv| pv.GetString().ok())
                        .map(|s| s.to_string());
                    cb(Activation {
                        action_id,
                        input,
                        push_id,
                    });
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
        use crate::notify::{PushAction, PushInput};

        #[test]
        fn 引数の往復() {
            let a = args(Some("energy:3"), Some("01 AB&x"));
            let (act, push) = parse_args(&a);
            assert_eq!(act.as_deref(), Some("energy:3"));
            assert_eq!(push.as_deref(), Some("01 AB&x"));
        }

        #[test]
        fn xmlにボタンと入力欄が入る() {
            let mut p = PushPayload::checkin();
            p.title = "a<b".into();
            let x = build_xml(&p);
            assert_eq!(x.matches("<action ").count(), 5);
            assert!(x.contains("<input id=\"note\""));
            assert!(x.contains("a&lt;b"));
            let _ = (PushAction::default(), PushInput::default());
        }
    }
}

// ===== Windows 以外（トースト無し。常にフォールバックへ） =====

#[cfg(not(windows))]
mod platform {
    use super::{Activation, PushPayload};

    pub fn register() -> Result<(), String> {
        Ok(())
    }

    pub fn show(
        _payload: &PushPayload,
        _on_activated: impl Fn(Activation) + Send + Sync + 'static,
    ) -> Result<(), String> {
        Err("このプラットフォームではトースト通知に未対応です".into())
    }
}
