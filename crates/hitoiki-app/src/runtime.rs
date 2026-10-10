//! 常駐処理（収集・取り込み・再計算・プッシュ）の差し込み口。
//!
//! 別クレートのランタイム（`hitoiki-daemon` 相当）が完成したら、`start_background` から起動する。
//! ランタイムは次を行うこと:
//! - 区間を更新したら [`crate::events::emit_segments_updated`]
//! - 取り込み完了で [`crate::events::emit_import_completed`]
//! - プッシュは [`crate::notify::deliver`]（トースト → 失敗時ポップアップのフォールバック）
//! - 通知の一時停止は [`crate::state::AppState::is_snoozed`] を参照する

use crate::notify::Activation;
use crate::state::AppState;
use crate::{events, window};
use hitoiki_core::service::record::{self, CheckInput};
use tauri::{AppHandle, Manager};

/// バックグラウンド処理を起動する。今は何もしない（後でランタイムを統合する）。
pub fn start_background(_app: &AppHandle) {
    tracing::debug!("バックグラウンドランタイムは未統合です");
}

/// 通知のアクション（トーストのボタン／ポップアップのボタン）を受けたときの処理。
///
/// `action_id` の規約: `energy:N`（チェックイン、N は 1〜5）、`open:<screen>`（画面を開く）。
/// アクション無し（通知本体のクリック）は記録画面を開く。
pub fn handle_activation(app: &AppHandle, act: Activation) {
    tracing::info!("通知のアクティベーション: {act:?}");
    let action = act.action_id.as_deref().unwrap_or("");
    if let Some(n) = action.strip_prefix("energy:") {
        let Ok(energy) = n.parse::<i64>() else {
            return;
        };
        let note = act.input.clone().filter(|s| !s.trim().is_empty());
        let state = app.state::<AppState>();
        match state.with_ctx(|ctx| {
            record::check(
                ctx,
                &CheckInput {
                    energy: Some(energy),
                    note,
                    ..Default::default()
                },
            )
        }) {
            Ok(_) => {
                // ポップアップから応答したときは窓を閉じる。
                if let Some(w) = app.get_webview_window("popup") {
                    let _ = w.close();
                }
                events::emit_segments_updated(app);
            }
            Err(e) => tracing::warn!("チェックインを記録できませんでした: {}", e.message),
        }
    } else if let Some(screen) = action.strip_prefix("open:") {
        window::open_screen(app, screen);
    } else {
        window::open_screen(app, "records");
    }
}
