//! UI へ送る Tauri イベント（docs/dev/architecture.md「GUI」）。発火は `emit_*` 経由に統一する。

use crate::state::PushEvent;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};

pub const SEGMENTS_UPDATED: &str = "segments-updated";
pub const PUSH: &str = "push";
pub const DOING_CHANGED: &str = "doing-changed";
pub const IMPORT_COMPLETED: &str = "import-completed";
/// `hitoiki ui <screen>` などで画面を切り替える。ペイロードは `{ "screen": "today" }`。
pub const OPEN_SCREEN: &str = "open-screen";

fn emit(app: &AppHandle, name: &str, payload: Value) {
    if let Err(e) = app.emit(name, payload) {
        tracing::warn!("イベント {name} を送れませんでした: {e}");
    }
}

/// 区間（割り当て結果）が更新された。
pub fn emit_segments_updated(app: &AppHandle) {
    emit(app, SEGMENTS_UPDATED, json!({}));
}

/// プッシュ通知が発生した。ペイロードは `{ push_id, payload }`。
pub fn emit_push(app: &AppHandle, ev: &PushEvent) {
    emit(app, PUSH, serde_json::to_value(ev).unwrap_or(Value::Null));
}

/// doing タスクが変わった。
pub fn emit_doing_changed(app: &AppHandle) {
    emit(app, DOING_CHANGED, json!({}));
}

/// 外部ソース（カレンダー・エージェントログ等）の取り込みが完了した。
pub fn emit_import_completed(app: &AppHandle, source: &str) {
    emit(app, IMPORT_COMPLETED, json!({ "source": source }));
}

/// クイック入力ウインドウを開いた（UI が入力欄と候補を初期化する）。
pub const QUICK_OPEN: &str = "quick-open";

pub fn emit_quick_open(app: &AppHandle) {
    emit(app, QUICK_OPEN, json!({}));
}

pub fn emit_open_screen(app: &AppHandle, screen: &str) {
    emit(app, OPEN_SCREEN, json!({ "screen": screen }));
}
