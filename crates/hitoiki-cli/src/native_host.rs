//! ブラウザ拡張のネイティブメッセージングホスト（docs/architecture.md「CLI」）。
//!
//! プロトコル: 4 バイト（リトルエンディアン）の長さ + UTF-8 JSON。標準出力にはフレーム以外を書かない。

use hitoiki_core::{Error, Result};
use std::io::{self, Read, Write};

/// ブラウザ → ホストのメッセージの最大サイズ（1MB）。
pub const MAX_MESSAGE_BYTES: usize = 1024 * 1024;

pub const HOST_NAME: &str = "com.hitoiki.native_host";

/// 1 フレームを読む。入力が先頭で閉じていれば `None`。
/// 長さが上限を超える場合は `InvalidData` エラー（本文は読まない）。
pub fn read_frame<R: Read>(r: &mut R) -> io::Result<Option<Vec<u8>>> {
    let mut len = [0u8; 4];
    let mut got = 0;
    while got < 4 {
        let n = r.read(&mut len[got..])?;
        if n == 0 {
            if got == 0 {
                return Ok(None);
            }
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "長さプレフィクスが途中で終わりました",
            ));
        }
        got += n;
    }
    let len = u32::from_le_bytes(len) as usize;
    if len > MAX_MESSAGE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("メッセージが大きすぎます（{len} バイト、上限 {MAX_MESSAGE_BYTES}）"),
        ));
    }
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf)?;
    Ok(Some(buf))
}

/// 1 フレームを書いてフラッシュする。
pub fn write_frame<W: Write>(w: &mut W, payload: &[u8]) -> io::Result<()> {
    w.write_all(&(payload.len() as u32).to_le_bytes())?;
    w.write_all(payload)?;
    w.flush()
}

/// EOF までメッセージを処理する。`handle` は本文（JSON 文字列）を受け取り、保存件数を返す。
/// 応答は `{"ok":true,"saved":n}` または `{"ok":false,"error":"..."}`。
pub fn serve<R: Read, W: Write>(
    r: &mut R,
    w: &mut W,
    mut handle: impl FnMut(&str) -> Result<usize>,
) -> io::Result<()> {
    loop {
        let frame = match read_frame(r) {
            Ok(Some(f)) => f,
            Ok(None) => return Ok(()),
            Err(e) if e.kind() == io::ErrorKind::InvalidData => {
                // 続きを同期できないので、拒否を返して終了する。
                let reply = serde_json::json!({"ok": false, "error": e.to_string()});
                write_frame(w, reply.to_string().as_bytes())?;
                return Err(e);
            }
            Err(e) => return Err(e),
        };
        let reply = match std::str::from_utf8(&frame)
            .map_err(|e| Error::InvalidArgument(format!("UTF-8 ではありません: {e}")))
            .and_then(&mut handle)
        {
            Ok(n) => serde_json::json!({"ok": true, "saved": n}),
            Err(e) => serde_json::json!({"ok": false, "error": e.to_string()}),
        };
        write_frame(w, reply.to_string().as_bytes())?;
    }
}

/// `native-host` の引数がホスト起動（ブラウザからの起動）を表すか。
pub fn is_host_invocation(action: Option<&str>) -> bool {
    match action {
        None => true,
        Some(a) => {
            a.starts_with("chrome-extension://") || a.starts_with("moz-extension://") || a.ends_with(".json")
        }
    }
}

#[cfg(windows)]
pub use win::{install, uninstall};

#[cfg(not(windows))]
pub fn install(_chrome_ids: &[String], _firefox_ids: &[String]) -> Result<InstallReport> {
    Err(Error::Other(anyhow::anyhow!(
        "native-host install は Windows のみ対応しています"
    )))
}

#[cfg(not(windows))]
pub fn uninstall() -> Result<InstallReport> {
    Err(Error::Other(anyhow::anyhow!(
        "native-host uninstall は Windows のみ対応しています"
    )))
}

/// install / uninstall の結果（書いた/消したファイルとレジストリキー）。
#[derive(Debug, Default, serde::Serialize)]
pub struct InstallReport {
    pub files: Vec<String>,
    pub registry: Vec<String>,
}

/// Chrome / Edge 用マニフェスト。
pub fn chrome_manifest(path: &str, ids: &[String]) -> serde_json::Value {
    serde_json::json!({
        "name": HOST_NAME,
        "description": "Hitoiki: ブラウザの URL を詳細コンテキストとして送るネイティブメッセージングホスト",
        "path": path,
        "type": "stdio",
        "allowed_origins": ids.iter().map(|i| format!("chrome-extension://{i}/")).collect::<Vec<_>>(),
    })
}

/// Firefox 用マニフェスト。
pub fn firefox_manifest(path: &str, ids: &[String]) -> serde_json::Value {
    serde_json::json!({
        "name": HOST_NAME,
        "description": "Hitoiki: ブラウザの URL を詳細コンテキストとして送るネイティブメッセージングホスト",
        "path": path,
        "type": "stdio",
        "allowed_extensions": ids,
    })
}

#[cfg(windows)]
mod win {
    use super::*;
    use hitoiki_core::paths;
    use std::path::Path;
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    const CHROME_KEY: &str = r"Software\Google\Chrome\NativeMessagingHosts";
    const EDGE_KEY: &str = r"Software\Microsoft\Edge\NativeMessagingHosts";
    const FIREFOX_KEY: &str = r"Software\Mozilla\NativeMessagingHosts";

    fn dir() -> std::path::PathBuf {
        paths::data_dir().join("native-host")
    }

    fn chrome_file() -> std::path::PathBuf {
        dir().join(format!("{HOST_NAME}.json"))
    }

    fn firefox_file() -> std::path::PathBuf {
        dir().join(format!("{HOST_NAME}.firefox.json"))
    }

    fn set_default(parent: &str, file: &Path) -> Result<String> {
        let key = format!(r"{parent}\{HOST_NAME}");
        let (k, _) = RegKey::predef(HKEY_CURRENT_USER)
            .create_subkey(&key)
            .map_err(Error::Io)?;
        k.set_value("", &file.display().to_string()).map_err(Error::Io)?;
        Ok(format!(r"HKCU\{key}"))
    }

    fn delete_key(parent: &str) -> Result<Option<String>> {
        let key = format!(r"{parent}\{HOST_NAME}");
        match RegKey::predef(HKEY_CURRENT_USER).delete_subkey_all(&key) {
            Ok(()) => Ok(Some(format!(r"HKCU\{key}"))),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(Error::Io(e)),
        }
    }

    fn write_json(path: &Path, v: &serde_json::Value) -> Result<()> {
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p)?;
        }
        std::fs::write(
            path,
            serde_json::to_string_pretty(v).map_err(anyhow::Error::from)?,
        )?;
        Ok(())
    }

    pub fn install(chrome_ids: &[String], firefox_ids: &[String]) -> Result<InstallReport> {
        if chrome_ids.is_empty() && firefox_ids.is_empty() {
            return Err(Error::InvalidArgument(
                "--extension-id（Chrome / Edge）または --firefox-extension-id を指定してください".into(),
            ));
        }
        let exe = std::env::current_exe()?.canonicalize()?;
        // `\\?\` 付きのパスはブラウザが扱えないことがあるので外す。
        let exe = exe.display().to_string();
        let exe = exe.strip_prefix(r"\\?\").unwrap_or(&exe).to_string();
        let mut rep = InstallReport::default();
        if !chrome_ids.is_empty() {
            let f = chrome_file();
            write_json(&f, &chrome_manifest(&exe, chrome_ids))?;
            rep.files.push(f.display().to_string());
            rep.registry.push(set_default(CHROME_KEY, &f)?);
            rep.registry.push(set_default(EDGE_KEY, &f)?);
        }
        if !firefox_ids.is_empty() {
            let f = firefox_file();
            write_json(&f, &firefox_manifest(&exe, firefox_ids))?;
            rep.files.push(f.display().to_string());
            rep.registry.push(set_default(FIREFOX_KEY, &f)?);
        }
        Ok(rep)
    }

    pub fn uninstall() -> Result<InstallReport> {
        let mut rep = InstallReport::default();
        for k in [CHROME_KEY, EDGE_KEY, FIREFOX_KEY] {
            if let Some(s) = delete_key(k)? {
                rep.registry.push(s);
            }
        }
        for f in [chrome_file(), firefox_file()] {
            match std::fs::remove_file(&f) {
                Ok(()) => rep.files.push(f.display().to_string()),
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.into()),
            }
        }
        let _ = std::fs::remove_dir(dir());
        Ok(rep)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn frame_roundtrip() {
        let mut buf = Vec::new();
        write_frame(&mut buf, "日本語".as_bytes()).unwrap();
        assert_eq!(&buf[..4], &(9u32).to_le_bytes());
        let mut c = Cursor::new(buf);
        assert_eq!(read_frame(&mut c).unwrap().unwrap(), "日本語".as_bytes());
        assert!(read_frame(&mut c).unwrap().is_none());
    }

    #[test]
    fn oversize_and_truncated() {
        let mut c = Cursor::new(((MAX_MESSAGE_BYTES + 1) as u32).to_le_bytes().to_vec());
        assert_eq!(read_frame(&mut c).unwrap_err().kind(), io::ErrorKind::InvalidData);
        let mut c = Cursor::new(vec![1, 0]);
        assert_eq!(
            read_frame(&mut c).unwrap_err().kind(),
            io::ErrorKind::UnexpectedEof
        );
        let mut c = Cursor::new(vec![5, 0, 0, 0, b'a']);
        assert!(read_frame(&mut c).is_err());
    }

    #[test]
    fn serve_replies_per_message() {
        let mut input = Vec::new();
        write_frame(&mut input, b"ok").unwrap();
        write_frame(&mut input, b"ng").unwrap();
        let mut out = Vec::new();
        serve(&mut Cursor::new(input), &mut out, |s| {
            if s == "ok" {
                Ok(2)
            } else {
                Err(Error::InvalidArgument("bad".into()))
            }
        })
        .unwrap();
        let mut c = Cursor::new(out);
        let a: serde_json::Value = serde_json::from_slice(&read_frame(&mut c).unwrap().unwrap()).unwrap();
        let b: serde_json::Value = serde_json::from_slice(&read_frame(&mut c).unwrap().unwrap()).unwrap();
        assert_eq!(a, serde_json::json!({"ok": true, "saved": 2}));
        assert_eq!(b["ok"], false);
    }

    #[test]
    fn manifests() {
        let m = chrome_manifest(r"C:\h.exe", &["abc".into(), "def".into()]);
        assert_eq!(m["allowed_origins"][1], "chrome-extension://def/");
        assert_eq!(m["type"], "stdio");
        let f = firefox_manifest("x", &["a@b".into()]);
        assert_eq!(f["allowed_extensions"][0], "a@b");
    }

    #[test]
    fn host_invocation_detection() {
        assert!(is_host_invocation(None));
        assert!(is_host_invocation(Some("chrome-extension://abc/")));
        assert!(!is_host_invocation(Some("install")));
    }
}
