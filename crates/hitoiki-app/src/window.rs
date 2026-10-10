//! ウインドウ操作（メイン窓の表示、画面指定、チェックイン用ポップアップ）。

use crate::events;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

/// 画面名として受け付ける値（`#/<screen>` に対応）。
pub const SCREENS: &[&str] = &[
    "today", "review", "tasks", "plan", "reflect", "report", "records", "okr", "settings",
];

/// メイン窓を表示して前面に出す。
pub fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

/// メイン窓を表示し、指定の画面へ切り替える。未知の画面名は `today`。
pub fn open_screen(app: &AppHandle, screen: &str) {
    let screen = if SCREENS.contains(&screen) {
        screen
    } else {
        "today"
    };
    show_main(app);
    events::emit_open_screen(app, screen);
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
