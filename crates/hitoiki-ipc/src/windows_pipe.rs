//! Windows 名前付きパイプ実装。

use std::fs::{File, OpenOptions};
use std::os::windows::io::FromRawHandle;
use std::time::{Duration, Instant};

use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{
    CloseHandle, LocalFree, ERROR_ACCESS_DENIED, ERROR_PIPE_BUSY, ERROR_PIPE_CONNECTED, HANDLE, HLOCAL,
    INVALID_HANDLE_VALUE,
};
use windows::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows::Win32::Security::{
    GetTokenInformation, TokenUser, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER,
};
use windows::Win32::Storage::FileSystem::{
    FlushFileBuffers, FILE_FLAGS_AND_ATTRIBUTES, FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_ACCESS_DUPLEX,
};
use windows::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS,
    PIPE_TYPE_BYTE, PIPE_WAIT,
};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

use crate::{IpcError, Result};

/// サーバ側接続（所有ハンドルを `File` で包む。Drop で CloseHandle）。
pub(crate) type Conn = File;

pub(crate) fn full_path(suffix: &str) -> String {
    format!(r"\\.\pipe\hitoiki-{suffix}")
}

fn os_err(what: &str, e: windows::core::Error) -> IpcError {
    IpcError::Io(format!("{what}: {e}"))
}

/// 現在のユーザーの SID 文字列。
fn current_sid() -> Result<String> {
    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token)
            .map_err(|e| os_err("OpenProcessToken", e))?;
        let r = (|| {
            let mut len = 0u32;
            let _ = GetTokenInformation(token, TokenUser, None, 0, &mut len);
            if len == 0 {
                return Err(IpcError::Io("GetTokenInformation: 長さ不明".into()));
            }
            // 8 バイト境界を保証するため u64 で確保する。
            let mut buf = vec![0u64; (len as usize).div_ceil(8)];
            GetTokenInformation(token, TokenUser, Some(buf.as_mut_ptr().cast()), len, &mut len)
                .map_err(|e| os_err("GetTokenInformation", e))?;
            let user = &*(buf.as_ptr() as *const TOKEN_USER);
            let mut s = PWSTR::null();
            ConvertSidToStringSidW(user.User.Sid, &mut s).map_err(|e| os_err("ConvertSidToStringSidW", e))?;
            let out = s.to_string().map_err(|e| IpcError::Io(e.to_string()));
            let _ = LocalFree(Some(HLOCAL(s.0.cast())));
            out
        })();
        let _ = CloseHandle(token);
        r
    }
}

pub(crate) fn current_user_id() -> Result<String> {
    current_sid()
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// 作成者ユーザーのみアクセス可のセキュリティ記述子。
struct OwnerOnlySd(PSECURITY_DESCRIPTOR);

impl OwnerOnlySd {
    fn new() -> Result<Self> {
        let sddl = wide(&format!("D:P(A;;GA;;;{})", current_sid()?));
        let mut sd = PSECURITY_DESCRIPTOR::default();
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                PCWSTR(sddl.as_ptr()),
                SDDL_REVISION_1,
                &mut sd,
                None,
            )
            .map_err(|e| os_err("ConvertStringSecurityDescriptorToSecurityDescriptorW", e))?;
        }
        Ok(OwnerOnlySd(sd))
    }
}

impl Drop for OwnerOnlySd {
    fn drop(&mut self) {
        unsafe {
            let _ = LocalFree(Some(HLOCAL(self.0 .0)));
        }
    }
}

pub(crate) struct Listener {
    path: Vec<u16>,
    sd: OwnerOnlySd,
    /// 次の接続を待つ待機中インスタンス。
    pending: Option<File>,
}

// SAFETY: セキュリティ記述子は作成後に読み取りのみで、スレッド間で移動して問題ない。
unsafe impl Send for Listener {}

impl Listener {
    pub(crate) fn bind(suffix: &str) -> Result<Self> {
        let mut l = Listener {
            path: wide(&full_path(suffix)),
            sd: OwnerOnlySd::new()?,
            pending: None,
        };
        l.pending = Some(l.create_instance(true)?);
        Ok(l)
    }

    fn create_instance(&self, first: bool) -> Result<File> {
        let sa = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: self.sd.0 .0,
            bInheritHandle: false.into(),
        };
        let mut open_mode: FILE_FLAGS_AND_ATTRIBUTES = PIPE_ACCESS_DUPLEX;
        if first {
            open_mode |= FILE_FLAG_FIRST_PIPE_INSTANCE;
        }
        let h = unsafe {
            CreateNamedPipeW(
                PCWSTR(self.path.as_ptr()),
                open_mode,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                255,
                64 * 1024,
                64 * 1024,
                0,
                Some(&sa),
            )
        };
        if h == INVALID_HANDLE_VALUE {
            let e = windows::core::Error::from_thread();
            if first && e.code() == ERROR_ACCESS_DENIED.to_hresult() {
                return Err(IpcError::AlreadyRunning);
            }
            return Err(os_err("CreateNamedPipeW", e));
        }
        // SAFETY: 有効な所有ハンドルを File に移す。
        Ok(unsafe { File::from_raw_handle(h.0) })
    }

    /// 接続を 1 つ待って返す。次のインスタンスは直ちに用意する。
    pub(crate) fn accept(&mut self) -> Result<File> {
        use std::os::windows::io::AsRawHandle;
        let inst = match self.pending.take() {
            Some(p) => p,
            None => self.create_instance(false)?,
        };
        let h = HANDLE(inst.as_raw_handle());
        if let Err(e) = unsafe { ConnectNamedPipe(h, None) } {
            if e.code() != ERROR_PIPE_CONNECTED.to_hresult() {
                self.pending = Some(inst);
                return Err(os_err("ConnectNamedPipe", e));
            }
        }
        self.pending = self.create_instance(false).ok();
        Ok(inst)
    }
}

/// 応答の書き込み完了を保証して切断する。
pub(crate) fn finish(f: &mut File) {
    use std::os::windows::io::AsRawHandle;
    let h = HANDLE(f.as_raw_handle());
    unsafe {
        let _ = FlushFileBuffers(h);
        let _ = DisconnectNamedPipe(h);
    }
}

/// クライアント接続。サーバ不在なら `NotRunning`、全インスタンス使用中なら期限まで再試行。
pub(crate) fn connect(suffix: &str, deadline: Instant) -> Result<File> {
    let path = full_path(suffix);
    loop {
        match OpenOptions::new().read(true).write(true).open(&path) {
            Ok(f) => return Ok(f),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(IpcError::NotRunning),
            Err(e) if e.raw_os_error() == Some(ERROR_PIPE_BUSY.0 as i32) => {
                if Instant::now() >= deadline {
                    return Err(IpcError::Timeout);
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(e) => return Err(e.into()),
        }
    }
}
