//! hitoiki-ipc: CLI から常駐アプリへの即時通知。
//!
//! Windows では名前付きパイプ（作成者ユーザーのみアクセス可）、Unix では
//! Unix ドメインソケットを使う。ネットワークポートは開かない。
//! メッセージは改行区切り JSON で、1 リクエストにつき 1 応答。

use std::io::{BufRead, BufReader, Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows_pipe;

#[cfg(unix)]
use unix as os;
#[cfg(windows)]
use windows_pipe as os;

/// 1 行の最大長（バイト）。
const MAX_LINE: u64 = 1024 * 1024;

/// IPC のエラー。
#[derive(Debug, thiserror::Error)]
pub enum IpcError {
    /// サーバが起動していない（呼び出し側で無視してよい）。
    #[error("常駐アプリが起動していません")]
    NotRunning,
    /// 既にサーバが起動している。
    #[error("既に起動しています")]
    AlreadyRunning,
    /// タイムアウト。
    #[error("タイムアウトしました")]
    Timeout,
    /// プロトコル違反（JSON 不正など）。
    #[error("プロトコルエラー: {0}")]
    Protocol(String),
    /// その他の I/O・OS エラー。
    #[error("IPC エラー: {0}")]
    Io(String),
}

pub type Result<T> = std::result::Result<T, IpcError>;

impl From<std::io::Error> for IpcError {
    fn from(e: std::io::Error) -> Self {
        IpcError::Io(e.to_string())
    }
}

/// CLI から常駐アプリへ送るメッセージ。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Message {
    TaskChanged { task_id: Option<String> },
    DataChanged { what: String },
    OpenUi { screen: Option<String> },
    Ping,
    Shutdown,
}

/// 応答。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reply {
    pub ok: bool,
    pub message: Option<String>,
}

impl Reply {
    pub fn ok() -> Self {
        Reply {
            ok: true,
            message: None,
        }
    }
    pub fn error(message: impl Into<String>) -> Self {
        Reply {
            ok: false,
            message: Some(message.into()),
        }
    }
}

/// パイプ（ソケット）の名前。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PipeName {
    suffix: String,
}

impl PipeName {
    /// 現在のユーザーの既定名（Windows: ユーザー SID、Unix: uid）。
    ///
    /// 環境変数 `HITOIKI_HOME` が設定されているときは、そのパス（正規化したもの）のハッシュを
    /// 名前に含め、既定の実アプリのパイプとは別名にする（テストが実アプリへ通知を送らないように）。
    /// CLI・デーモン・アプリは同じこの関数を使うので、同じ `HITOIKI_HOME` 同士では通信できる。
    pub fn for_current_user() -> Result<Self> {
        let home = std::env::var_os("HITOIKI_HOME").filter(|h| !h.is_empty());
        Self::for_user_and_home(os::current_user_id()?, home.as_deref())
    }

    fn for_user_and_home(user: String, home: Option<&std::ffi::OsStr>) -> Result<Self> {
        let suffix = match home {
            Some(h) => format!("{user}-h{:016x}", fnv1a64(normalize_home(h).as_bytes())),
            None => user,
        };
        Ok(PipeName { suffix })
    }

    /// テスト用などに任意のサフィックスを指定する。
    pub fn custom(suffix: impl Into<String>) -> Self {
        PipeName {
            suffix: suffix.into(),
        }
    }

    pub fn suffix(&self) -> &str {
        &self.suffix
    }

    /// OS 上の実際のパス（Windows: `\\.\pipe\hitoiki-<suffix>`）。
    pub fn path(&self) -> String {
        os::full_path(&self.suffix)
    }
}

/// `HITOIKI_HOME` を比較用に正規化する（実在すれば canonicalize、Windows では大文字小文字と区切りを揃える）。
fn normalize_home(h: &std::ffi::OsStr) -> String {
    let path = std::path::Path::new(h);
    let p = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let mut s = p.to_string_lossy().replace('\\', "/");
    if let Some(rest) = s.strip_prefix("//?/") {
        s = rest.to_string();
    }
    while s.len() > 1 && s.ends_with('/') {
        s.pop();
    }
    if cfg!(windows) {
        s = s.to_lowercase();
    }
    s
}

/// FNV-1a 64bit。プロセス間・バージョン間で値が変わらないよう自前で持つ。
fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

/// メッセージを送り、応答を受け取る。サーバ不在なら `IpcError::NotRunning`。
pub fn send(name: &PipeName, msg: &Message, timeout: Duration) -> Result<Reply> {
    let deadline = Instant::now() + timeout;
    let mut line = serde_json::to_string(msg).map_err(|e| IpcError::Protocol(e.to_string()))?;
    line.push('\n');
    let suffix = name.suffix.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let r = (|| {
            let mut s = os::connect(&suffix, deadline)?;
            s.write_all(line.as_bytes())?;
            s.flush()?;
            let mut buf = String::new();
            BufReader::new(s.take(MAX_LINE)).read_line(&mut buf)?;
            if buf.trim().is_empty() {
                return Err(IpcError::Protocol("応答がありません".into()));
            }
            serde_json::from_str::<Reply>(buf.trim()).map_err(|e| IpcError::Protocol(e.to_string()))
        })();
        let _ = tx.send(r);
    });
    match rx.recv_timeout(timeout) {
        Ok(r) => r,
        Err(_) => Err(IpcError::Timeout),
    }
}

/// 失敗しても無視する通知（既定名・300ms）。送れて応答が ok だったかを返す。
pub fn notify_best_effort(msg: &Message) -> bool {
    match PipeName::for_current_user() {
        Ok(name) => send(&name, msg, Duration::from_millis(300))
            .map(|r| r.ok)
            .unwrap_or(false),
        Err(_) => false,
    }
}

type Handler = dyn Fn(Message) -> Reply + Send + Sync + 'static;

/// 稼働中のサーバ。`stop` または Drop で停止する。
pub struct ServerHandle {
    suffix: String,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl ServerHandle {
    /// 停止して受付スレッドの終了を待つ。
    pub fn stop(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            // accept のブロックを解くためダミー接続する。
            let _ = os::connect(&self.suffix, Instant::now() + Duration::from_secs(1));
            let _ = t.join();
        }
    }
}

impl Drop for ServerHandle {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// サーバを起動する。既にサーバがいれば `IpcError::AlreadyRunning`。
pub fn serve(
    name: &PipeName,
    handler: impl Fn(Message) -> Reply + Send + Sync + 'static,
) -> Result<ServerHandle> {
    let handler: Arc<Handler> = Arc::new(handler);
    let stop = Arc::new(AtomicBool::new(false));
    let listener = os::Listener::bind(&name.suffix)?;
    let stop2 = stop.clone();
    let thread = std::thread::Builder::new()
        .name("hitoiki-ipc".into())
        .spawn(move || accept_loop(listener, stop2, handler))
        .map_err(|e| IpcError::Io(e.to_string()))?;
    Ok(ServerHandle {
        suffix: name.suffix.clone(),
        stop,
        thread: Some(thread),
    })
}

fn accept_loop(mut listener: os::Listener, stop: Arc<AtomicBool>, handler: Arc<Handler>) {
    loop {
        let conn = listener.accept();
        if stop.load(Ordering::SeqCst) {
            break;
        }
        match conn {
            Ok(stream) => {
                let h = handler.clone();
                std::thread::spawn(move || {
                    let _ = handle_conn(stream, &*h);
                });
            }
            Err(_) => std::thread::sleep(Duration::from_millis(50)),
        }
    }
}

fn handle_conn(mut s: os::Conn, handler: &Handler) -> std::io::Result<()> {
    let mut line = String::new();
    BufReader::new((&mut s).take(MAX_LINE)).read_line(&mut line)?;
    let reply = match serde_json::from_str::<Message>(line.trim()) {
        Ok(m) => handler(m),
        Err(e) => Reply::error(format!("不正なメッセージ: {e}")),
    };
    let mut out = serde_json::to_string(&reply).unwrap_or_else(|_| "{\"ok\":false}".into());
    out.push('\n');
    s.write_all(out.as_bytes())?;
    s.flush()?;
    os::finish(&mut s);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    fn unique(tag: &str) -> PipeName {
        PipeName::custom(format!("test-{tag}-{}", std::process::id()))
    }

    #[test]
    fn roundtrip() {
        let name = unique("rt");
        let seen = Arc::new(Mutex::new(Vec::new()));
        let s2 = seen.clone();
        let srv = serve(&name, move |m| {
            s2.lock().unwrap().push(m.clone());
            Reply {
                ok: true,
                message: Some(format!("{m:?}")),
            }
        })
        .unwrap();
        let msg = Message::TaskChanged {
            task_id: Some("t1".into()),
        };
        let r = send(&name, &msg, Duration::from_secs(2)).unwrap();
        assert!(r.ok);
        let r = send(&name, &Message::Ping, Duration::from_secs(2)).unwrap();
        assert!(r.ok);
        assert_eq!(seen.lock().unwrap().len(), 2);
        assert_eq!(seen.lock().unwrap()[0], msg);
        srv.stop();
    }

    #[test]
    fn not_running() {
        let name = unique("nr");
        let e = send(&name, &Message::Ping, Duration::from_secs(1)).unwrap_err();
        assert!(matches!(e, IpcError::NotRunning), "{e:?}");
    }

    #[test]
    fn already_running() {
        let name = unique("ar");
        let srv = serve(&name, |_| Reply::ok()).unwrap();
        let e = serve(&name, |_| Reply::ok()).err().unwrap();
        assert!(matches!(e, IpcError::AlreadyRunning), "{e:?}");
        srv.stop();
    }

    #[test]
    fn stop_then_not_running_and_restart() {
        let name = unique("st");
        let srv = serve(&name, |_| Reply::ok()).unwrap();
        assert!(send(&name, &Message::Ping, Duration::from_secs(2)).unwrap().ok);
        srv.stop();
        let e = send(&name, &Message::Ping, Duration::from_secs(1)).unwrap_err();
        assert!(matches!(e, IpcError::NotRunning), "{e:?}");
        let srv = serve(&name, |_| Reply::ok()).unwrap();
        srv.stop();
    }

    #[test]
    fn bad_json_gets_error_reply() {
        let name = unique("bj");
        let srv = serve(&name, |_| Reply::ok()).unwrap();
        let mut s = os::connect(name.suffix(), Instant::now() + Duration::from_secs(2)).unwrap();
        s.write_all(b"{\"type\":\"nope\"}\n").unwrap();
        let mut buf = String::new();
        BufReader::new(s).read_line(&mut buf).unwrap();
        let r: Reply = serde_json::from_str(buf.trim()).unwrap();
        assert!(!r.ok);
        srv.stop();
    }

    #[test]
    fn message_json_shape() {
        let j = serde_json::to_string(&Message::OpenUi { screen: None }).unwrap();
        assert_eq!(j, r#"{"type":"open_ui","screen":null}"#);
        assert_eq!(
            serde_json::from_str::<Message>(r#"{"type":"ping"}"#).unwrap(),
            Message::Ping
        );
    }

    #[test]
    fn home_指定で別名になりパスは含まれない() {
        let base = PipeName::for_user_and_home("U1".into(), None).unwrap();
        assert_eq!(base.suffix(), "U1");
        let dir = std::env::temp_dir().join("hitoiki-ipc-name-test");
        std::fs::create_dir_all(&dir).unwrap();
        let a = PipeName::for_user_and_home("U1".into(), Some(dir.as_os_str())).unwrap();
        assert_ne!(a, base);
        assert!(a.suffix().starts_with("U1-h"));
        assert!(!a.suffix().to_lowercase().contains("hitoiki-ipc-name-test"));
        // 同じ場所（末尾の区切り付き）は同じ名前。
        let with_slash = format!("{}/", dir.display());
        let b = PipeName::for_user_and_home("U1".into(), Some(std::ffi::OsStr::new(&with_slash))).unwrap();
        assert_eq!(a, b);
        // 別の場所は別名。
        let other = std::env::temp_dir().join("hitoiki-ipc-name-test2");
        let c = PipeName::for_user_and_home("U1".into(), Some(other.as_os_str())).unwrap();
        assert_ne!(a, c);
    }

    #[test]
    fn fnv_は固定値() {
        assert_eq!(fnv1a64(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a64(b"a"), 0xaf63_dc4c_8601_ec8c);
    }

    #[test]
    fn current_user_name() {
        let n = PipeName::for_current_user().unwrap();
        assert!(!n.suffix().is_empty());
        assert!(n.path().contains("hitoiki-"));
    }
}
