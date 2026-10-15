//! ウインドウ操作（メイン窓の表示、画面指定、調子用ポップアップ）。

use crate::events;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

/// 画面名として受け付ける値（`#/<screen>` に対応）。
pub const SCREENS: &[&str] = &[
    "today", "actual", "tasks", "goals", "records", "report", "settings", "welcome",
];

/// メイン窓を表示して前面に出す。
pub fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

/// 記録の作成フォーム（`records/new/<kind>`）の種類。
const RECORD_FORMS: &[&str] = &[
    "day_start",
    "day_end",
    "weekly",
    "condition",
    "concern",
    "reframe",
];

/// 画面指定を UI のハッシュ（`today`、`records/new/day_end`、`actual?status=review` など）に正規化する。
///
/// - 画面名（`SCREENS`）はそのまま。クエリ（`?...`）は残す。
/// - `today/design`（今日のデザインモード。始業の問いかけから）と、`records/new/<kind>`（記録の作成フォーム）は
///   サブパスを残す。
/// - 未知の画面名は `today`。
pub fn normalize_screen(screen: &str) -> String {
    let (path, query) = match screen.split_once('?') {
        Some((p, q)) => (p, Some(q)),
        None => (screen, None),
    };
    let (base, rest) = match path.split_once('/') {
        Some((b, r)) => (b, Some(r)),
        None => (path, None),
    };
    let page = match (base, rest) {
        ("today", Some("design")) => "today/design".to_string(),
        ("records", Some(r)) => match r.strip_prefix("new/") {
            Some(k) if RECORD_FORMS.contains(&k) => format!("records/new/{k}"),
            _ => "records".to_string(),
        },
        (b, _) if SCREENS.contains(&b) => b.to_string(),
        _ => return "today".to_string(),
    };
    match query {
        Some(q) if !q.is_empty() => format!("{page}?{q}"),
        _ => page,
    }
}

/// メイン窓を表示し、指定の画面へ切り替える。
pub fn open_screen(app: &AppHandle, screen: &str) {
    let screen = normalize_screen(screen);
    show_main(app);
    events::emit_open_screen(app, &screen);
}

/// 通知トーストが出せないときのフォールバック: 画面右下に小さなポップアップ窓を出す。
/// 中身は UI 側がウインドウラベル `popup` を見て描画する。
pub fn open_popup(app: &AppHandle) -> tauri::Result<()> {
    if let Some(w) = app.get_webview_window("popup") {
        w.show()?;
        w.set_focus()?;
        return Ok(());
    }
    let (w, h) = (360.0, 320.0);
    let mut b = WebviewWindowBuilder::new(app, "popup", WebviewUrl::App("index.html".into()))
        .title("Hitoiki")
        .inner_size(w, h)
        .resizable(false)
        .maximizable(false)
        .minimizable(false)
        .always_on_top(true)
        .skip_taskbar(true);
    if let Ok(Some(m)) = app.primary_monitor() {
        let scale = m.scale_factor();
        let size = m.size().to_logical::<f64>(scale);
        let pos = m.position().to_logical::<f64>(scale);
        b = b.position(pos.x + size.width - w - 24.0, pos.y + size.height - h - 72.0);
    }
    b.build()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::normalize_screen;

    #[test]
    fn 画面名の正規化() {
        assert_eq!(normalize_screen("today"), "today");
        assert_eq!(normalize_screen("records/new/day_end"), "records/new/day_end");
        assert_eq!(normalize_screen("records/new/condition"), "records/new/condition");
        assert_eq!(normalize_screen("records/new/concern"), "records/new/concern");
        assert_eq!(normalize_screen("records/new/xx"), "records");
        assert_eq!(normalize_screen("today/design"), "today/design");
        assert_eq!(normalize_screen("today/xx"), "today");
        assert_eq!(normalize_screen("records"), "records");
        assert_eq!(normalize_screen("actual"), "actual");
        assert_eq!(normalize_screen("actual?status=review"), "actual?status=review");
        assert_eq!(normalize_screen("goals"), "goals");
        assert_eq!(normalize_screen("welcome"), "welcome");
        // 旧名は受け付けない。
        for old in [
            "log",
            "review",
            "okr",
            "plan",
            "reflect",
            "reflection/day_end",
            "checkin",
            "hassle",
            "onboarding",
        ] {
            assert_eq!(normalize_screen(old), "today", "{old}");
        }
        assert_eq!(normalize_screen("nope"), "today");
    }
}
