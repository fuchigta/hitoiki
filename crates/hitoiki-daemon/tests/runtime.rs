use hitoiki_agents::ImportStats;
use hitoiki_core::config::Config;
use hitoiki_core::push::PushPayload;
use hitoiki_core::timefmt::Range;
use hitoiki_core::{db, Clock, SystemClock};
use hitoiki_daemon::{Runtime, RuntimeOptions, RuntimeSink};
use hitoiki_ipc::{IpcError, Message, PipeName};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Default)]
struct TestSink {
    segments: Mutex<Vec<Range>>,
    doing: Mutex<Vec<Option<String>>>,
    ui: Mutex<Vec<Option<String>>>,
    shutdown: Mutex<u32>,
}

impl RuntimeSink for TestSink {
    fn segments_updated(&self, range: Range) {
        self.segments.lock().unwrap().push(range);
    }
    fn push(&self, _: &str, _: &PushPayload) {}
    fn doing_changed(&self, id: Option<&str>) {
        self.doing.lock().unwrap().push(id.map(String::from));
    }
    fn import_completed(&self, _: &ImportStats) {}
    fn open_ui(&self, screen: Option<&str>) {
        self.ui.lock().unwrap().push(screen.map(String::from));
    }
    fn shutdown_requested(&self) {
        *self.shutdown.lock().unwrap() += 1;
    }
}

fn wait_until(what: &str, f: impl Fn() -> bool) {
    let end = Instant::now() + Duration::from_secs(10);
    while Instant::now() < end {
        if f() {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("タイムアウト: {what}");
}

fn opts(dir: &std::path::Path, tag: &str) -> (RuntimeOptions, PipeName) {
    let pipe = PipeName::custom(format!("daemon-test-{tag}-{}", std::process::id()));
    let mut o = RuntimeOptions::new(Config::default());
    o.collect = false;
    o.watch_agents = false;
    o.pipe_name = Some(pipe.clone());
    o.db_path = Some(dir.join("hitoiki.db"));
    o.rebuild_debounce = Duration::from_millis(100);
    o.rebuild_interval = Duration::from_secs(3600);
    o.push_interval = Duration::from_secs(3600);
    o.calendar_interval = Some(Duration::from_secs(3600));
    (o, pipe)
}

fn ping(pipe: &PipeName) -> Result<bool, IpcError> {
    hitoiki_ipc::send(pipe, &Message::Ping, Duration::from_secs(2)).map(|r| r.ok)
}

#[test]
fn ping_data_changed_rebuild_and_stop() {
    let dir = tempfile::tempdir().unwrap();
    let (o, pipe) = opts(dir.path(), "main");
    let db_path = o.db_path.clone().unwrap();
    let sink = Arc::new(TestSink::default());
    let handle = Runtime::start(o, sink.clone()).unwrap();

    assert!(ping(&pipe).unwrap());
    // 起動直後の再計算
    wait_until("初回の再計算", || !sink.segments.lock().unwrap().is_empty());

    // 生データを足して DataChanged を送ると再計算が走り、区間ができる。
    let now = SystemClock.now_ms();
    {
        let conn = db::open(&db_path).unwrap();
        conn.execute(
            "INSERT INTO window_span (id, start_at, end_at, hwnd, exe_name, title, excluded)
             VALUES ('W1', ?1, ?2, 'H', 'a.exe', 't', 0)",
            [now - 20 * 60_000, now - 10 * 60_000],
        )
        .unwrap();
    }
    let before = sink.segments.lock().unwrap().len();
    let r = hitoiki_ipc::send(
        &pipe,
        &Message::DataChanged { what: "test".into() },
        Duration::from_secs(2),
    )
    .unwrap();
    assert!(r.ok);
    wait_until("DataChanged による再計算", || {
        sink.segments.lock().unwrap().len() > before
    });
    {
        let conn = db::open(&db_path).unwrap();
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM activity_segment", [], |r| r.get(0))
            .unwrap();
        assert!(n >= 1, "区間が作られていない");
    }

    // 他のメッセージ
    let before = sink.segments.lock().unwrap().len();
    hitoiki_ipc::send(
        &pipe,
        &Message::TaskChanged {
            task_id: Some("T1".into()),
        },
        Duration::from_secs(2),
    )
    .unwrap();
    wait_until("TaskChanged", || {
        sink.doing.lock().unwrap().as_slice() == [Some("T1".to_string())]
            && sink.segments.lock().unwrap().len() > before
    });
    hitoiki_ipc::send(
        &pipe,
        &Message::OpenUi {
            screen: Some("today".into()),
        },
        Duration::from_secs(2),
    )
    .unwrap();
    assert_eq!(sink.ui.lock().unwrap().as_slice(), [Some("today".to_string())]);
    hitoiki_ipc::send(&pipe, &Message::Shutdown, Duration::from_secs(2)).unwrap();
    assert_eq!(*sink.shutdown.lock().unwrap(), 1);

    handle.stop();
    assert!(matches!(ping(&pipe), Err(IpcError::NotRunning)));
}

#[test]
fn second_start_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let (o, pipe) = opts(dir.path(), "dup");
    let handle = Runtime::start(o.clone(), Arc::new(TestSink::default())).unwrap();
    let err = Runtime::start(o, Arc::new(TestSink::default()))
        .err()
        .expect("二重起動は失敗する");
    assert!(
        err.to_string().contains("既に常駐プロセスが動いています"),
        "{err}"
    );
    // 失敗した側が先行プロセスの IPC を壊していない。
    assert!(ping(&pipe).unwrap());
    handle.stop();
}

#[test]
fn collect_enabled_starts_and_stops() {
    // 収集器つきでも起動・停止できる（Windows 以外では収集器は何もしない）。
    let dir = tempfile::tempdir().unwrap();
    let (mut o, _pipe) = opts(dir.path(), "collect");
    o.collect = true;
    let handle = Runtime::start(o, Arc::new(TestSink::default())).unwrap();
    std::thread::sleep(Duration::from_millis(500));
    handle.stop();
}
