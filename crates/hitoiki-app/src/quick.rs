//! クイック入力: グローバルショートカットの登録と、入力用の小さなウインドウ（docs/dev/architecture.md「GUI」）。
//!
//! ショートカットは `tauri-plugin-global-shortcut`（Windows では `RegisterHotKey`）で登録する。OS から受け取るのは
//! 登録した組み合わせが押されたという通知だけで、キー入力の記録や低レベルフックは使わない。
//! ロジック（作成・選択して開始、直前の doing）は core の `service::quick` にあり、ここは薄いラッパー。

use crate::error::{AppError, AppResult};
use crate::state::AppState;
use crate::{events, tray};
use hitoiki_core::config::Config;
use hitoiki_core::service::quick::{self, SwitchInput, SwitchResult};
use hitoiki_core::service::task::TaskView;
use hitoiki_core::shortcut;
use serde_json::{json, Value};
use std::sync::Mutex;
use tauri::{AppHandle, Manager, PhysicalPosition, State, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

/// クイック入力ウインドウのラベル（UI がこの名前を見て描画を切り替える）。
pub const LABEL: &str = "quick";
/// ウインドウの大きさ（論理ピクセル）。
const WIDTH: f64 = 520.0;
const HEIGHT: f64 = 380.0;
/// 画面上端から、画面の高さに対してこの割合の位置に置く（中央上寄せ）。
const TOP_RATIO: f64 = 0.16;

/// いま OS に登録している正規形のショートカット。
static REGISTERED: Mutex<Option<String>> = Mutex::new(None);
/// 直近の登録失敗の理由（設定画面で表示する）。
static LAST_ERROR: Mutex<Option<String>> = Mutex::new(None);

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// 登録中のショートカット（トレイの表示用）。
pub fn registered() -> Option<String> {
    lock(&REGISTERED).clone()
}

/// 画面の中央上寄せの位置（物理ピクセル）。`mon` は (x, y, 幅, 高さ)。
pub fn top_center(mon: (i32, i32, u32, u32), scale: f64, width_logical: f64) -> (i32, i32) {
    let (mx, my, mw, mh) = mon;
    let w = (width_logical * scale).round() as i32;
    (
        mx + (mw as i32 - w) / 2,
        my + (mh as f64 * TOP_RATIO).round() as i32,
    )
}

/// ウインドウを（無ければ非表示で）作る。起動時に作っておくと、ショートカットで即座に出せる。
fn ensure_window(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    if let Some(w) = app.get_webview_window(LABEL) {
        return Ok(w);
    }
    WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("index.html".into()))
        .title("Hitoiki クイック入力")
        .inner_size(WIDTH, HEIGHT)
        .resizable(false)
        .maximizable(false)
        .minimizable(false)
        .decorations(false)
        .shadow(true)
        .always_on_top(true)
        .skip_taskbar(true)
        .visible(false)
        .build()
}

/// 起動時に呼ぶ: ウインドウを用意し、設定のショートカットを登録する。失敗してもアプリは続ける。
pub fn init(app: &AppHandle) {
    if let Err(e) = ensure_window(app) {
        tracing::warn!("クイック入力のウインドウを作れませんでした: {e}");
    }
    let setting = Config::load()
        .map(|c| c.general.quick_entry_shortcut)
        .unwrap_or_default();
    match apply(app, &setting) {
        Ok(Some(s)) => tracing::info!("クイック入力のショートカットを登録しました: {s}"),
        Ok(None) => {}
        Err(e) => {
            tracing::warn!("クイック入力のショートカットを登録できませんでした: {e}");
            *lock(&LAST_ERROR) = Some(e);
        }
    }
}

/// 設定値のショートカットを OS に登録し直す。空文字は登録解除。成功すれば登録した正規形を返す。
/// 失敗したときは、それまでの登録を維持する。
pub fn apply(app: &AppHandle, setting: &str) -> Result<Option<String>, String> {
    let wanted = shortcut::parse(setting)
        .map_err(|e| e.to_string())?
        .map(|s| s.canonical());
    let old = registered();
    if wanted == old {
        *lock(&LAST_ERROR) = None;
        return Ok(wanted);
    }
    let gs = app.global_shortcut();
    if let Some(o) = &old {
        let _ = gs.unregister(o.as_str());
        *lock(&REGISTERED) = None;
    }
    if let Some(w) = &wanted {
        if let Err(e) = register(app, w) {
            // 元のショートカットは使い続ける。
            if let Some(o) = &old {
                if register(app, o).is_ok() {
                    *lock(&REGISTERED) = Some(o.clone());
                }
            }
            return Err(format!(
                "ショートカット「{w}」を登録できませんでした（他のアプリが使っている可能性があります）: {e}"
            ));
        }
    }
    *lock(&REGISTERED) = wanted.clone();
    *lock(&LAST_ERROR) = None;
    Ok(wanted)
}

fn register(app: &AppHandle, canonical: &str) -> Result<(), tauri_plugin_global_shortcut::Error> {
    app.global_shortcut()
        .on_shortcut(canonical, |app, _shortcut, event| {
            // 押したときだけ反応する（離したときの通知は使わない）。
            if event.state() == ShortcutState::Pressed {
                open(app);
            }
        })
}

/// クイック入力ウインドウを開いてフォーカスを取る（どのスレッドからでも呼べる）。
pub fn open(app: &AppHandle) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        let w = match ensure_window(&handle) {
            Ok(w) => w,
            Err(e) => {
                tracing::warn!("クイック入力のウインドウを開けませんでした: {e}");
                return;
            }
        };
        let monitor = handle
            .cursor_position()
            .ok()
            .and_then(|p| handle.monitor_from_point(p.x, p.y).ok().flatten())
            .or_else(|| handle.primary_monitor().ok().flatten());
        if let Some(m) = monitor {
            let (x, y) = top_center(
                (m.position().x, m.position().y, m.size().width, m.size().height),
                m.scale_factor(),
                WIDTH,
            );
            let _ = w.set_position(PhysicalPosition::new(x, y));
        }
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
        events::emit_quick_open(&handle);
    });
}

/// ウインドウを隠す。
pub fn close(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(LABEL) {
        let _ = w.hide();
    }
}

/// doing が変わった後の後処理（UI とトレイへ通知し、再計算を要求する）。
fn after_switch(app: &AppHandle, state: &AppState) {
    events::emit_doing_changed(app);
    tray::refresh(app);
    state.request_rebuild();
}

// ===== Tauri コマンド =====

/// 直前まで doing だったタスク（「戻る」候補）。
#[tauri::command(rename_all = "snake_case")]
pub fn quick_previous(state: State<AppState>) -> AppResult<Option<TaskView>> {
    state.with_ctx(quick::previous_doing)
}

/// 作成または選択して開始する。
#[tauri::command(rename_all = "snake_case")]
pub fn quick_switch(app: AppHandle, state: State<AppState>, input: SwitchInput) -> AppResult<SwitchResult> {
    let r = state.with_ctx(|c| quick::switch(c, &input))?;
    after_switch(&app, &state);
    Ok(r)
}

/// 直前の doing に戻る。
#[tauri::command(rename_all = "snake_case")]
pub fn quick_back(app: AppHandle, state: State<AppState>) -> AppResult<SwitchResult> {
    let r = state.with_ctx(quick::back)?;
    after_switch(&app, &state);
    Ok(r)
}

/// クイック入力ウインドウを閉じる。
#[tauri::command(rename_all = "snake_case")]
pub fn quick_close(app: AppHandle) {
    close(&app);
}

/// 設定画面用: 設定値・登録中の正規形・直近の登録失敗の理由。
#[tauri::command(rename_all = "snake_case")]
pub fn quick_shortcut_state() -> Value {
    let setting = Config::load()
        .map(|c| c.general.quick_entry_shortcut)
        .unwrap_or_default();
    json!({ "shortcut": setting, "registered": registered(), "error": *lock(&LAST_ERROR) })
}

/// ショートカットを変更する。構文を検査し、再起動なしで登録し直してから設定ファイルに保存する。
/// 登録に失敗（他のアプリと衝突など）したときはエラーを返し、設定も登録も変えない。
/// 登録は main スレッドに依頼して待つので、このコマンドは async にしている。
#[tauri::command(rename_all = "snake_case")]
pub async fn quick_shortcut_set(app: AppHandle, shortcut: String) -> AppResult<Value> {
    let parsed = shortcut::parse(&shortcut)?;
    let canonical = parsed.map(|s| s.canonical()).unwrap_or_default();
    let previous = registered().unwrap_or_default();
    apply(&app, &canonical).map_err(|m| AppError {
        code: "shortcut",
        message: m,
    })?;
    let path = hitoiki_core::paths::config_file();
    if let Err(e) =
        hitoiki_core::config::set_value(&path, "general.quick_entry_shortcut", &format!("\"{canonical}\""))
    {
        let _ = apply(&app, &previous);
        return Err(e.into());
    }
    tray::refresh(&app);
    Ok(json!({ "shortcut": canonical, "registered": registered() }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 画面の中央上寄せ() {
        // 1920x1080、倍率 1: 幅 520 → 左 700、上は高さの 16%。
        assert_eq!(top_center((0, 0, 1920, 1080), 1.0, 520.0), (700, 173));
        // 倍率 1.5（物理 2880x1620）。副モニタの原点オフセットも足す。
        assert_eq!(
            top_center((-2880, 100, 2880, 1620), 1.5, 520.0),
            (-2880 + 1050, 100 + 259)
        );
    }
}
