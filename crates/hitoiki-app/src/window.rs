//! ウインドウ操作（メイン窓の表示、画面指定、チェックイン用ポップアップ）。

use crate::events;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

/// 画面名として受け付ける値（`#/<screen>` に対応）。
pub const SCREENS: &[&str] = &[
    "today",
    "log",
    "review",
    "tasks",
    "plan",
    "reflect",
    "report",
    "records",
    "okr",
    "settings",
    "onboarding",
];

/// メイン窓を表示して前面に出す。
pub fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

/// 振り返りの種別（`reflect/<kind>`）。
const REFLECT_KINDS: &[&str] = &["day_start", "day_end", "weekly"];

/// 画面指定を UI のハッシュ（`today`、`reflect/day_end` など）に正規化する。
///
/// - `reflection/<kind>`（プッシュの `open_screen`）と `reflect/<kind>` は `reflect/<kind>` にそろえる。
///   `reflect` は旧名で、UI が記録画面（`records/new/<kind>`、種類なしは `records?kind=reflection`）に写す。
/// - `checkin` / `hassle`（プッシュの `open_screen`）は記録画面の作成フォーム（`records/new/<種類>`）。
/// - 未知の画面名は `today`。
pub fn normalize_screen(screen: &str) -> String {
    let (base, rest) = match screen.split_once('/') {
        Some((b, r)) => (b, Some(r)),
        None => (screen, None),
    };
    match base {
        "reflection" | "reflect" => match rest {
            Some(k) if REFLECT_KINDS.contains(&k) => format!("reflect/{k}"),
            _ => "reflect".to_string(),
        },
        "checkin" | "hassle" => format!("records/new/{base}"),
        // 今日のデザインモード（始業の通知から）。
        "today" if rest == Some("design") => "today/design".to_string(),
        b if SCREENS.contains(&b) => b.to_string(),
        _ => "today".to_string(),
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
        assert_eq!(normalize_screen("reflection/day_end"), "reflect/day_end");
        assert_eq!(normalize_screen("reflect"), "reflect");
        assert_eq!(normalize_screen("reflection/xx"), "reflect");
        assert_eq!(normalize_screen("today/design"), "today/design");
        assert_eq!(normalize_screen("today/xx"), "today");
        assert_eq!(normalize_screen("checkin"), "records/new/checkin");
        assert_eq!(normalize_screen("hassle"), "records/new/hassle");
        assert_eq!(normalize_screen("records"), "records");
        assert_eq!(normalize_screen("log"), "log");
        assert_eq!(normalize_screen("review"), "review");
        assert_eq!(normalize_screen("onboarding"), "onboarding");
        assert_eq!(normalize_screen("nope"), "today");
    }
}
