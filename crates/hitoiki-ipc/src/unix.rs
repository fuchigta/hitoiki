//! Unix ドメインソケット実装（簡易）。

use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::time::Instant;

use crate::{IpcError, Result};

pub(crate) type Conn = UnixStream;

pub(crate) fn current_user_id() -> Result<String> {
    // SAFETY: getuid は常に成功する。
    Ok(unsafe { libc::getuid() }.to_string())
}

fn sock_path(suffix: &str) -> PathBuf {
    let dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    dir.join(format!("hitoiki-{suffix}.sock"))
}

pub(crate) fn full_path(suffix: &str) -> String {
    sock_path(suffix).display().to_string()
}

pub(crate) fn connect(suffix: &str, _deadline: Instant) -> Result<UnixStream> {
    UnixStream::connect(sock_path(suffix)).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused => IpcError::NotRunning,
        _ => IpcError::from(e),
    })
}

pub(crate) fn finish(_s: &mut UnixStream) {}

pub(crate) struct Listener {
    inner: UnixListener,
    path: PathBuf,
}

impl Listener {
    pub(crate) fn bind(suffix: &str) -> Result<Self> {
        let path = sock_path(suffix);
        if UnixStream::connect(&path).is_ok() {
            return Err(IpcError::AlreadyRunning);
        }
        let _ = std::fs::remove_file(&path);
        let inner = UnixListener::bind(&path)?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        Ok(Listener { inner, path })
    }

    pub(crate) fn accept(&mut self) -> Result<UnixStream> {
        Ok(self.inner.accept()?.0)
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}
