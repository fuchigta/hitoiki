//! Hitoiki の常駐 GUI（Tauri v2）。タスクトレイ常駐、UI ウインドウ、通知を担う。
//!
//! ロジックは持たず、`hitoiki-core` の service を呼ぶ（docs/dev/architecture.md「GUI」）。本番ではローカルサーバもポートも持たない。

pub mod commands;
pub mod dto;
pub mod error;
pub mod events;
pub mod notify;
pub mod runtime;
pub mod state;
pub mod tray;
pub mod window;

use state::AppState;
use tauri::{AppHandle, Manager, RunEvent, WindowEvent};
use tauri_plugin_autostart::ManagerExt;

/// ログ書き込みスレッドのガード（終了時に flush するため、`run` のスコープではなく静的に持つ）。
static LOG_GUARD: std::sync::Mutex<Option<hitoiki_daemon::LogGuard>> = std::sync::Mutex::new(None);

/// 引数から `--name value` の値を取る。
pub fn arg_value(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1).cloned())
}

/// 設定 `general.autostart` を OS の自動起動に反映する。
pub fn apply_autostart(app: &AppHandle) {
    let want = match hitoiki_core::config::Config::load() {
        Ok(c) => c.general.autostart,
        Err(_) => return,
    };
    let al = app.autolaunch();
    let r = if want { al.enable() } else { al.disable() };
    if let Err(e) = r {
        tracing::warn!("自動起動の設定に失敗しました: {e}");
    }
}

pub fn run() {
    // ヘッドレス daemon と同じログ（%LOCALAPPDATA%\hitoiki\logs の日次ファイル）。作れなければ stderr のみ。
    match hitoiki_daemon::init_logging() {
        Ok(g) => {
            // `app.run` はプロセスごと終了するので、終了処理（RunEvent::Exit）で明示的に flush する。
            if let Ok(mut slot) = LOG_GUARD.lock() {
                *slot = Some(g);
            }
        }
        Err(e) => {
            tracing_subscriber::fmt()
                .with_env_filter(
                    tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
                )
                .init();
            tracing::warn!("ログファイルを作れないため stderr のみに出します: {e:#}");
        }
    }
    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        args = ?std::env::args().skip(1).collect::<Vec<_>>(),
        "hitoiki-app を起動します"
    );

    let state = match AppState::open() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("DB を開けませんでした: {e}");
            std::process::exit(1);
        }
    };

    let result = tauri::Builder::default()
        // 多重起動防止: 2 つ目は既存のインスタンスに引数（--screen）を渡して終了する。
        .plugin(tauri_plugin_single_instance::init(
            |app, args, _cwd| match arg_value(&args, "--screen") {
                Some(s) => window::open_screen(app, &s),
                None => window::show_main(app),
            },
        ))
        // 自動起動エントリ名は製品名（インストーラーの都合で変わりうる）に依存させず "Hitoiki" に固定する。
        // インストーラーのフック（windows/hitoiki-path.ps1）もこの名前の Run 値を削除する。
        .plugin(
            tauri_plugin_autostart::Builder::new()
                .app_name("Hitoiki")
                .arg("--background")
                .build(),
        )
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            commands::status,
            commands::task_list,
            commands::task_show,
            commands::task_add,
            commands::task_edit,
            commands::task_start,
            commands::task_done,
            commands::task_drop,
            commands::rule_add,
            commands::rule_list,
            commands::log,
            commands::review_list,
            commands::review_set,
            commands::block_add,
            commands::block_list,
            commands::block_delete,
            commands::block_update,
            commands::block_template_list,
            commands::block_template_add,
            commands::block_template_edit,
            commands::block_template_delete,
            commands::block_template_suggest,
            commands::block_template_preview,
            commands::block_template_apply,
            commands::block_template_undo,
            commands::block_template_save_from,
            commands::hint_dismissed,
            commands::hint_dismiss,
            commands::calendar_list,
            commands::checkin_add,
            commands::checkin_list,
            commands::hassle_add,
            commands::hassle_list,
            commands::abc_add,
            commands::abc_list,
            commands::context,
            commands::reflect_generate,
            commands::reflect_answer,
            commands::reflect_list,
            commands::reflect_show,
            commands::abc_dispute,
            commands::runtime_state,
            commands::okr_list,
            commands::okr_add_objective,
            commands::okr_add_kr,
            commands::skill_list,
            commands::skill_add,
            commands::skill_import_preset,
            commands::report,
            commands::config_all,
            commands::config_get,
            commands::config_set,
            commands::config_path,
            commands::open_screen,
            commands::snooze,
            commands::snooze_state,
            commands::pending_push,
            commands::take_initial_screen,
            commands::push_respond,
            commands::notify_test,
            commands::onboarding_status,
            commands::onboarding_complete,
            commands::onboarding_reset,
        ])
        .on_window_event(|window, event| {
            // メイン窓を閉じてもトレイに残る。
            if window.label() == "main" {
                if let WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .setup(|app| {
            let handle = app.handle().clone();
            notify::init();
            tray::init(&handle)?;
            tray::start_refresh_loop(&handle);
            runtime::start_background(&handle);

            // 開発・テスト時(debug / HITOIKI_HOME 指定)は OS の自動起動設定に触らない。
            if !cfg!(debug_assertions) && std::env::var_os("HITOIKI_HOME").is_none() {
                apply_autostart(&handle);
            }

            let args: Vec<String> = std::env::args().collect();
            if !args.iter().any(|a| a == "--background") {
                window::show_main(&handle);
            }
            // UI の読み込み前にイベントを送っても届かないので、状態に積んで UI が取りに来る。
            // 画面指定が無く、オンボーディング未完了なら案内画面から始める。
            let mut initial = arg_value(&args, "--screen").map(|s| window::normalize_screen(&s));
            if initial.is_none() {
                let state = handle.state::<AppState>();
                let done = state
                    .with_ctx(hitoiki_core::service::onboarding::status)
                    .map(|s| s.completed)
                    .unwrap_or(true);
                if !done {
                    initial = Some("onboarding".to_string());
                    window::show_main(&handle);
                }
            }
            handle.state::<AppState>().set_initial_screen(initial);
            // 通知の動作確認用: HITOIKI_APP_NOTIFY_TEST=1 で起動 2 秒後にテスト通知を出す。
            if std::env::var_os("HITOIKI_APP_NOTIFY_TEST").is_some() {
                let h = handle.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_secs(2));
                    let r = notify::show_push(&h, None, &notify::test_payload());
                    tracing::info!("テスト通知の結果: {r:?}");
                });
            }
            Ok(())
        })
        .build(tauri::generate_context!());

    let app = match result {
        Ok(app) => app,
        Err(e) => {
            eprintln!("アプリを起動できませんでした: {e}");
            std::process::exit(1);
        }
    };
    app.run(|app, event| match event {
        // 全ウインドウを閉じても（暗黙の終了要求）トレイに常駐し続ける。明示的な終了（コード付き）は通す。
        RunEvent::ExitRequested { api, code, .. } if code.is_none() => api.prevent_exit(),
        // 終了時は必ず常駐処理を止める（Recorder が進行中の区間を閉じる）。
        RunEvent::Exit => {
            tracing::info!("hitoiki-app を終了します");
            app.state::<AppState>().stop_runtime();
            tracing::info!("hitoiki-app を終了しました");
            // ログ書き込みスレッドの残りを書き出す。
            if let Ok(mut slot) = LOG_GUARD.lock() {
                drop(slot.take());
            }
        }
        _ => {}
    });
}

#[cfg(test)]
mod tests {
    use super::arg_value;

    #[test]
    fn 引数の値を取る() {
        let a: Vec<String> = ["app", "--screen", "review"].map(String::from).to_vec();
        assert_eq!(arg_value(&a, "--screen").as_deref(), Some("review"));
        assert_eq!(arg_value(&a, "--x"), None);
    }
}
