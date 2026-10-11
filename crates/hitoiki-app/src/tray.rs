//! タスクトレイ（docs/dev/architecture.md「GUI」）。メニューは状態が変わるたびに作り直す。

use crate::state::{AppState, RuntimeMode};
use crate::{events, window};
use hitoiki_core::model::{TaskKind, TaskStatus};
use hitoiki_core::service::record::{self, CheckInput};
use hitoiki_core::service::task::{self, TaskFilter};
use hitoiki_core::service::{status, update, Ctx};
use hitoiki_core::{Clock, SystemClock};
use tauri::menu::{Menu, MenuBuilder, MenuItemBuilder, SubmenuBuilder};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};

const TRAY_ID: &str = "main";
/// タスク切り替えサブメニューに出す件数。
const SWITCH_LIMIT: usize = 8;

/// 表示用の「1時間23分」。
pub fn fmt_duration(ms: i64) -> String {
    let min = (ms.max(0) + 30_000) / 60_000;
    if min >= 60 {
        format!("{}時間{}分", min / 60, min % 60)
    } else {
        format!("{min}分")
    }
}

struct Snapshot {
    doing: Option<(String, i64)>,
    today_ms: i64,
    tasks: Vec<(String, String, bool)>,
}

fn snapshot(ctx: &Ctx) -> hitoiki_core::Result<Snapshot> {
    let st = status::status(ctx)?;
    let doing = st.doing.first().map(|d| {
        let since = d.since.unwrap_or(st.now);
        (d.task.task.title.clone(), (st.now - since).max(0))
    });
    let list = task::list(
        ctx,
        &TaskFilter {
            statuses: vec![TaskStatus::Doing, TaskStatus::Todo],
            kind: None::<TaskKind>,
        },
    )?;
    let mut tasks: Vec<_> = list
        .tasks
        .into_iter()
        .map(|t| {
            let is_doing = t.task.status == TaskStatus::Doing;
            (t.task.id, t.task.title, is_doing)
        })
        .collect();
    // doing を先頭に（安定ソート）。
    tasks.sort_by_key(|(_, _, d)| !*d);
    tasks.truncate(SWITCH_LIMIT);
    Ok(Snapshot {
        doing,
        today_ms: st.today.total_ms,
        tasks,
    })
}

fn build_menu(app: &AppHandle) -> tauri::Result<(Menu<tauri::Wry>, String)> {
    let state = app.state::<AppState>();
    let snap = state.with_ctx(snapshot).ok();
    let snoozed = state.is_snoozed(SystemClock.now_ms());

    let (doing_text, today_text) = match &snap {
        Some(s) => (
            match &s.doing {
                Some((title, _)) => format!("いま: {title}"),
                None => "いま: （なし）".to_string(),
            },
            format!("今日: {}", fmt_duration(s.today_ms)),
        ),
        None => ("いま: （取得できません）".into(), "今日: -".into()),
    };

    let mut switch = SubmenuBuilder::new(app, "タスクを切り替え");
    match snap.as_ref().map(|s| &s.tasks) {
        Some(tasks) if !tasks.is_empty() => {
            for (id, title, is_doing) in tasks {
                let mark = if *is_doing { "● " } else { "" };
                let item =
                    MenuItemBuilder::with_id(format!("task:{id}"), format!("{mark}{title}")).build(app)?;
                switch = switch.item(&item);
            }
        }
        _ => {
            let none = MenuItemBuilder::new("（タスクがありません）")
                .enabled(false)
                .build(app)?;
            switch = switch.item(&none);
        }
    }
    let switch = switch.build()?;

    let mut checkin = SubmenuBuilder::new(app, "チェックイン（エネルギー）");
    for n in 1..=5 {
        let item = MenuItemBuilder::with_id(format!("checkin:{n}"), format!("{n}")).build(app)?;
        checkin = checkin.item(&item);
    }
    let checkin = checkin.build()?;

    let doing_item = MenuItemBuilder::with_id("info-doing", &doing_text)
        .enabled(false)
        .build(app)?;
    let today_item = MenuItemBuilder::with_id("info-today", &today_text)
        .enabled(false)
        .build(app)?;
    let quick_label = match crate::quick::registered() {
        Some(s) => format!("タスクを素早く入力…（{s}）"),
        None => "タスクを素早く入力…".to_string(),
    };
    let quick = MenuItemBuilder::with_id("quick", quick_label).build(app)?;
    let hassle = MenuItemBuilder::with_id("hassle", "ハッスルを記録…").build(app)?;
    let open = MenuItemBuilder::with_id("open", "UI を開く").build(app)?;
    let snooze = MenuItemBuilder::with_id(
        "snooze",
        if snoozed {
            "通知の一時停止を解除"
        } else {
            "通知を 1 時間止める"
        },
    )
    .build(app)?;
    let quit = MenuItemBuilder::with_id("quit", "終了").build(app)?;
    // 新しい版が見つかっている間だけ出す（保存済みの確認結果から。ネットワークには出ない）。
    let update_item = match state.with_ctx(|c| {
        if c.cfg.general.update_check == Some(true) {
            update::available(c.conn, env!("CARGO_PKG_VERSION"))
        } else {
            Ok(None)
        }
    }) {
        Ok(Some(v)) => Some(
            MenuItemBuilder::with_id("update", format!("新しいバージョン v{v} をダウンロード…"))
                .build(app)?,
        ),
        _ => None,
    };

    // 収集を自前で行っていないときは、その理由をメニューに出す。
    let mode = state.runtime_mode();
    let note = match &mode {
        RuntimeMode::External { message } | RuntimeMode::Failed { message } => Some(message.clone()),
        _ => None,
    };
    // 記録の状態は DB のハートビートで判定する（daemon 側で収集している場合も同じ）。
    let recording = state
        .with_ctx(|c| hitoiki_core::recording::is_recording(c.conn, c.now()))
        .unwrap_or(false);
    let rec_text = recording_text(&mode, recording);
    let rec_item = MenuItemBuilder::with_id("info-recording", &rec_text)
        .enabled(false)
        .build(app)?;
    let mut menu = MenuBuilder::new(app)
        .item(&rec_item)
        .item(&doing_item)
        .item(&today_item);
    if let Some(m) = &note {
        let item = MenuItemBuilder::with_id("info-runtime", format!("※ {m}"))
            .enabled(false)
            .build(app)?;
        menu = menu.item(&item);
    }
    if let Some(item) = &update_item {
        menu = menu.item(item);
    }
    let menu = menu
        .separator()
        .item(&switch)
        .item(&checkin)
        .item(&quick)
        .item(&hassle)
        .item(&open)
        .separator()
        .item(&snooze)
        .separator()
        .item(&quit)
        .build()?;
    let mut tooltip = format!("Hitoiki\n{doing_text}\n{today_text}");
    if let Some(m) = &note {
        tooltip.push_str(&format!("\n{m}"));
    }
    Ok((menu, tooltip))
}

/// トレイの先頭に出す記録の状態。
fn recording_text(mode: &RuntimeMode, recording: bool) -> String {
    match (mode, recording) {
        (RuntimeMode::Starting, _) => "記録: 開始しています".into(),
        (RuntimeMode::External { .. }, true) => "記録中（daemon 側で収集）".into(),
        (RuntimeMode::External { .. }, false) => {
            "記録停止中（daemon 側の収集が止まっています。ログを確認してください）".into()
        }
        (RuntimeMode::Failed { .. }, _) => "記録停止中（常駐処理を起動できませんでした）".into(),
        (RuntimeMode::Own, true) => "記録中".into(),
        (RuntimeMode::Own, false) => "記録停止中（収集が止まっています。ログを確認してください）".into(),
    }
}

/// トレイを作る。
pub fn init(app: &AppHandle) -> tauri::Result<()> {
    let (menu, tooltip) = build_menu(app)?;
    let icon = app
        .default_window_icon()
        .cloned()
        .ok_or_else(|| tauri::Error::AssetNotFound("icon".into()))?;
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        .tooltip(tooltip)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| on_menu_event(app, event.id().as_ref()))
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                window::open_screen(tray.app_handle(), "today");
            }
        })
        .build(app)?;
    Ok(())
}

/// メニューと tooltip を最新の状態に作り直す（どのスレッドからでも呼べる）。
pub fn refresh(app: &AppHandle) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        let Some(tray) = handle.tray_by_id(TRAY_ID) else {
            return;
        };
        match build_menu(&handle) {
            Ok((menu, tooltip)) => {
                let _ = tray.set_menu(Some(menu));
                let _ = tray.set_tooltip(Some(tooltip));
            }
            Err(e) => tracing::warn!("トレイメニューを更新できませんでした: {e}"),
        }
    });
}

/// 作業時間表示のため、定期的にメニューを更新する。
pub fn start_refresh_loop(app: &AppHandle) {
    let handle = app.clone();
    std::thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_secs(60));
        refresh(&handle);
    });
}

fn on_menu_event(app: &AppHandle, id: &str) {
    let state = app.state::<AppState>();
    if let Some(task_id) = id.strip_prefix("task:") {
        match state.with_ctx(|c| task::start(c, task_id)) {
            Ok(_) => {
                events::emit_doing_changed(app);
                refresh(app);
                state.request_rebuild();
            }
            Err(e) => tracing::warn!("タスクを開始できませんでした: {}", e.message),
        }
    } else if let Some(n) = id.strip_prefix("checkin:") {
        let energy = n.parse::<i64>().ok();
        match state.with_ctx(|c| {
            record::check(
                c,
                &CheckInput {
                    energy,
                    ..Default::default()
                },
            )
        }) {
            Ok(_) => events::emit_segments_updated(app),
            Err(e) => tracing::warn!("チェックインできませんでした: {}", e.message),
        }
    } else {
        match id {
            "quick" => crate::quick::open(app),
            "hassle" => window::open_screen(app, "records"),
            "open" => window::open_screen(app, "today"),
            "snooze" => {
                let now = SystemClock.now_ms();
                let until = if state.is_snoozed(now) { 0 } else { now + 3_600_000 };
                state.set_snooze_until(until);
                refresh(app);
            }
            "update" => crate::notify::open_release_page(),
            "quit" => crate::runtime::quit(app),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{fmt_duration, recording_text};
    use crate::state::RuntimeMode;

    #[test]
    fn 記録の状態の表示() {
        assert_eq!(recording_text(&RuntimeMode::Own, true), "記録中");
        assert!(recording_text(&RuntimeMode::Own, false).starts_with("記録停止中"));
        let ext = RuntimeMode::External { message: "x".into() };
        assert!(recording_text(&ext, true).contains("daemon"));
        assert!(recording_text(&ext, false).starts_with("記録停止中"));
    }

    #[test]
    fn 時間の表示() {
        assert_eq!(fmt_duration(0), "0分");
        assert_eq!(fmt_duration(59 * 60_000), "59分");
        assert_eq!(fmt_duration(83 * 60_000), "1時間23分");
    }
}
