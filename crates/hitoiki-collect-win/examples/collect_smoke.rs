//! 3 つのコレクタを起動し、15 秒間イベントを標準出力に出す動作確認用サンプル。

#[cfg(windows)]
fn main() -> anyhow::Result<()> {
    use crossbeam_channel::{select, unbounded};
    use hitoiki_core::config::Collect;
    use std::time::{Duration, Instant};

    let (ftx, frx) = unbounded();
    let (itx, irx) = unbounded();
    let (stx, srx) = unbounded();
    // 動作確認しやすいよう無操作の閾値は短くする。
    let cfg = Collect {
        idle_threshold_sec: 10,
        ..Collect::default()
    };
    let handles = hitoiki_collect_win::spawn_all(&cfg, ftx, itx, stx)?;

    let end = Instant::now() + Duration::from_secs(15);
    while Instant::now() < end {
        select! {
            recv(frx) -> e => println!("[foreground] {:?}", e?),
            recv(irx) -> e => println!("[idle]       {:?}", e?),
            recv(srx) -> e => println!("[session]    {:?}", e?),
            default(Duration::from_millis(200)) => {}
        }
    }
    handles.stop_and_join()?;
    println!("done");
    Ok(())
}

#[cfg(not(windows))]
fn main() {
    eprintln!("Windows 専用です");
}
