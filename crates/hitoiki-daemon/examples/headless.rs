//! `run_headless` 相当を約 20 秒動かして、収集と再計算の件数だけを表示する動作確認用サンプル。
//! `HITOIKI_HOME` は一時ディレクトリに差し替える（実環境の DB は使わない）。内容は表示しない。

use hitoiki_core::config::Config;
use hitoiki_core::timefmt::Range;
use hitoiki_core::{db, paths, rebuild, Clock, SystemClock};
use hitoiki_daemon::{init_logging, run_headless_with, RuntimeOptions};
use hitoiki_ipc::{Message, PipeName};
use std::time::Duration;

fn main() -> anyhow::Result<()> {
    let home = std::env::temp_dir().join(format!("hitoiki-headless-example-{}", std::process::id()));
    std::fs::create_dir_all(&home)?;
    std::env::set_var("HITOIKI_HOME", &home);
    let _guard = init_logging()?;

    let pipe = PipeName::custom(format!("headless-example-{}", std::process::id()));
    let mut opts = RuntimeOptions::new(Config::default());
    opts.pipe_name = Some(pipe.clone());
    opts.rebuild_interval = Duration::from_secs(5);
    opts.watch_agents = false;

    // 20 秒後に IPC で停止する。
    let p2 = pipe.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(20));
        let _ = hitoiki_ipc::send(&p2, &Message::Shutdown, Duration::from_secs(2));
    });
    run_headless_with(opts, false)?;

    let mut conn = db::open(&paths::db_file())?;
    let count = |c: &rusqlite::Connection, t: &str| -> i64 {
        c.query_row(&format!("SELECT COUNT(*) FROM {t}"), [], |r| r.get(0))
            .unwrap_or(-1)
    };
    let open: i64 = conn.query_row("SELECT COUNT(*) FROM window_span WHERE end_at IS NULL", [], |r| {
        r.get(0)
    })?;
    println!(
        "window_span: {} 件（未終了 {open} 件）",
        count(&conn, "window_span")
    );
    println!("idle_span: {} 件", count(&conn, "idle_span"));
    println!(
        "activity_segment（常駐中の再計算）: {} 件",
        count(&conn, "activity_segment")
    );
    let now = SystemClock.now_ms();
    let st = rebuild::rebuild(
        &mut conn,
        &Config::default(),
        Range::new(now - 3_600_000, now),
        now,
    )?;
    println!("rebuild 結果: 区間 {} 件", st.segments);
    drop(conn);
    let _ = std::fs::remove_dir_all(&home);
    Ok(())
}
