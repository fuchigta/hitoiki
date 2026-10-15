//! 常駐処理（収集・取り込み・再計算・問いかけ・IPC）の統合。
//!
//! 実体は `hitoiki-daemon` の [`Runtime`] で、ヘッドレス（`hitoiki daemon`）と共通。
//! IPC サーバもここ（Runtime 側）が持つので、アプリ側で別に立てない。
//! UI への通知は [`TauriSink`] が Tauri イベント・トースト・トレイ更新に変換する。

use crate::notify::{self, Activation, TEXT_ACTION};
use crate::state::{AppState, RuntimeMode, EXTERNAL_DAEMON_MESSAGE};
use crate::{events, tray, window};
use hitoiki_agents::ImportStats;
use hitoiki_core::config::Config;
use hitoiki_core::nudge::{self, NudgePayload, NudgeResponse};
use hitoiki_core::service::nudgelog;
use hitoiki_core::service::record::{self, CheckInput};
use hitoiki_core::timefmt::Range;
use hitoiki_core::{Clock, SystemClock};
use hitoiki_daemon::{AlreadyRunning, Runtime, RuntimeOptions, RuntimeSink};
use std::sync::Arc;
use tauri::{AppHandle, Manager};

/// 常駐処理からの通知を Tauri 側へ橋渡しする。全メソッドは任意のスレッドから呼ばれる。
pub struct TauriSink {
    app: AppHandle,
}

impl TauriSink {
    pub fn new(app: AppHandle) -> Self {
        Self { app }
    }
}

impl RuntimeSink for TauriSink {
    fn segments_updated(&self, range: Range) {
        tracing::info!(start = range.start, end = range.end, "区間を更新しました");
        events::emit_segments_updated(&self.app);
        tray::refresh(&self.app);
    }

    fn nudge(&self, nudge_id: &str, payload: &NudgePayload) {
        // 一時停止中は deliver が何もしない。
        notify::deliver(&self.app, nudge_id, payload);
    }

    fn doing_changed(&self, task_id: Option<&str>) {
        tracing::info!(task_id = task_id.unwrap_or("-"), "doing タスクが変わりました");
        events::emit_doing_changed(&self.app);
        tray::refresh(&self.app);
    }

    fn import_completed(&self, _stats: &ImportStats) {
        events::emit_import_completed(&self.app, "agent");
    }

    fn open_ui(&self, screen: Option<&str>) {
        window::open_screen(&self.app, screen.unwrap_or("today"));
    }

    fn update_available(&self, version: &str, _url: &str) {
        tracing::info!(version, "新しいバージョンがあります");
        tray::refresh(&self.app);
        let state = self.app.state::<AppState>();
        if state.is_snoozed(SystemClock.now_ms()) {
            // 通知の一時停止中はトーストを出さない（トレイのメニューには出る）。
            return;
        }
        if let Err(e) = notify::show_nudge(&self.app, None, &notify::update_payload(version)) {
            tracing::warn!("更新のトーストを出せませんでした: {e}");
        }
    }

    fn shutdown_requested(&self) {
        tracing::info!("IPC で終了が要求されました");
        self.app.exit(0);
    }
}

/// バックグラウンド処理（Runtime）を起動する。失敗してもアプリは UI のみで動き続ける。
///
/// - 別の常駐プロセス（`hitoiki daemon`）が動いていれば `RuntimeMode::External`。
/// - それ以外の失敗は `RuntimeMode::Failed`（メッセージを画面とトレイに出す）。
pub fn start_background(app: &AppHandle) {
    let state = app.state::<AppState>();
    let cfg = match Config::load() {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("設定を読めないため常駐処理を起動しません: {e}");
            state.set_runtime_mode(RuntimeMode::Failed {
                message: format!("設定を読めませんでした: {e}"),
            });
            return;
        }
    };
    let sink = Arc::new(TauriSink::new(app.clone()));
    match Runtime::start(RuntimeOptions::new(cfg), sink) {
        Ok(h) => {
            state.set_runtime(Some(h));
            state.set_runtime_mode(RuntimeMode::Own);
            tracing::info!("常駐処理を開始しました");
        }
        Err(e) if e.downcast_ref::<AlreadyRunning>().is_some() => {
            tracing::info!("{EXTERNAL_DAEMON_MESSAGE}");
            state.set_runtime_mode(RuntimeMode::External {
                message: EXTERNAL_DAEMON_MESSAGE.into(),
            });
        }
        Err(e) => {
            tracing::error!("常駐処理を起動できませんでした: {e:#}");
            state.set_runtime_mode(RuntimeMode::Failed {
                message: format!("常駐処理を起動できませんでした: {e:#}"),
            });
        }
    }
    tray::refresh(app);
}

/// 常駐処理を止めてからアプリを終了する（トレイの「終了」）。
/// 停止は終了処理（`RunEvent::Exit`）でも行うので、ここでは終了を要求するだけ。
pub fn quit(app: &AppHandle) {
    app.exit(0);
}

/// 通知のアクション（トーストのボタン／ポップアップのボタン）を受けたときの処理。
///
/// `nudge_id` があれば `nudge::apply_response` に渡す（nudge_log・checkin・hassle・スヌーズは core が処理）。
/// ボタンが `open`、または本体クリックなら、ペイロードの `open_screen` の画面を開く。
/// `nudge_id` が無い（テスト通知）場合は、`energy` を直接チェックインとして記録する。
pub fn handle_activation(app: &AppHandle, act: Activation) {
    tracing::info!("通知のアクティベーション: {act:?}");
    // 更新のトースト: ボタンでも本体クリックでも、リリースページを開く。
    if act.nudge_id.is_none()
        && (act.action.as_deref() == Some(notify::UPDATE_ACTION)
            || act.open_screen.as_deref() == Some(notify::UPDATE_OPEN))
    {
        notify::open_release_page();
        return;
    }
    let state = app.state::<AppState>();
    let note = act.input.clone().filter(|s| !s.trim().is_empty());
    // 入力欄だけの通知の送信ボタンは、アクション無し＋テキストとして扱う。
    let action = act.action.clone().filter(|a| a != TEXT_ACTION);
    let opens = act.action.is_none() || action.as_deref() == Some("open");

    let mut changed = false;
    if let Some(nudge_id) = act.nudge_id.as_deref() {
        let resp = NudgeResponse {
            action: action
                .clone()
                .or_else(|| act.action.is_none().then(|| "open".to_string())),
            value: act.value,
            text: note.clone(),
        };
        let applied = state.with_ctx(|c| {
            let out = nudge::apply_response(c.conn, nudge_id, resp, c.now(), c.tz()?)?;
            // タスク通知の「始める」: 開始は Ctx を持つここで行う（core の応答処理はタスク ID を返すだけ）。
            let started = out
                .start_task
                .as_deref()
                .map(|id| nudgelog::start_nudged_task(c, id).map(|_| id.to_string()));
            Ok((out, started))
        });
        match applied {
            Ok((out, started)) => {
                match started {
                    Some(Ok(task_id)) => {
                        tracing::info!(task_id, "通知からタスクを始めました");
                        events::emit_doing_changed(app);
                        tray::refresh(app);
                        state.request_rebuild();
                    }
                    Some(Err(e)) => tracing::warn!("通知のタスクを始められませんでした: {e}"),
                    None => {}
                }
                changed = out.checkin_id.is_some() || out.hassle_id.is_some() || out.idle_labeled > 0;
                if out.idle_labeled > 0 || action.as_deref().is_some_and(|a| a.starts_with("idle_")) {
                    // 無操作の分類は確定で保存済み。区間への反映を早める。
                    state.request_rebuild();
                }
                if let Some(until) = out.snoozed_until {
                    state.set_snooze_until(until);
                    tray::refresh(app);
                }
            }
            Err(e) => tracing::warn!("通知への応答を保存できませんでした: {}", e.message),
        }
    } else if action.as_deref() == Some("energy") || note.is_some() {
        let r = state.with_ctx(|c| {
            record::check(
                c,
                &CheckInput {
                    energy: if action.as_deref() == Some("energy") {
                        act.value
                    } else {
                        None
                    },
                    note: note.clone(),
                    ..Default::default()
                },
            )
        });
        match r {
            Ok(_) => changed = true,
            Err(e) => tracing::warn!("調子を記録できませんでした: {}", e.message),
        }
    }

    if changed {
        events::emit_segments_updated(app);
    }
    // ポップアップから応答したときは窓を閉じる。
    if let Some(w) = app.get_webview_window("popup") {
        let _ = w.close();
    }
    if opens {
        window::open_screen(app, act.open_screen.as_deref().unwrap_or("records"));
    }
}
