//! アダプタ共通の補助関数。

use anyhow::Context;
use hitoiki_core::model::{AgentEvent, AgentEventKind};
use hitoiki_core::Millis;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// 完結した 1 行（改行で終わっているもの）。
pub struct RawLine {
    /// ファイル先頭からの、この行の開始オフセット。
    pub offset: u64,
    /// 改行（と直前の `\r`）を除いた内容。
    pub bytes: Vec<u8>,
}

/// `from_offset` 以降の完結行を返す。第 2 要素は次回のオフセット
/// （最後の完結行の直後。未完成の最終行は含めず、オフセットも進めない）。
pub fn read_complete_lines(file: &Path, from_offset: u64) -> anyhow::Result<(Vec<RawLine>, u64)> {
    let mut f = File::open(file).with_context(|| "ログファイルを開けません")?;
    let len = f.metadata()?.len();
    if from_offset >= len {
        return Ok((Vec::new(), from_offset));
    }
    f.seek(SeekFrom::Start(from_offset))?;
    let mut buf = Vec::with_capacity((len - from_offset) as usize);
    f.take(len - from_offset).read_to_end(&mut buf)?;

    let mut lines = Vec::new();
    let mut start = 0usize;
    while let Some(pos) = buf[start..].iter().position(|&b| b == b'\n') {
        let end = start + pos;
        let mut line = &buf[start..end];
        if line.last() == Some(&b'\r') {
            line = &line[..line.len() - 1];
        }
        lines.push(RawLine {
            offset: from_offset + start as u64,
            bytes: line.to_vec(),
        });
        start = end + 1;
    }
    Ok((lines, from_offset + start as u64))
}

/// ファイルの最初の完結行（なければ None）。
pub fn read_first_line(file: &Path) -> Option<Vec<u8>> {
    let mut f = File::open(file).ok()?;
    let mut buf = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        let n = f.read(&mut chunk).ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(p) = buf.iter().position(|&b| b == b'\n') {
            buf.truncate(p);
            if buf.last() == Some(&b'\r') {
                buf.pop();
            }
            return Some(buf);
        }
        if buf.len() > 4 * 1024 * 1024 {
            return None;
        }
    }
}

/// RFC 3339 の時刻文字列を UTC ミリ秒に。
pub fn parse_ts(s: &str) -> Option<Millis> {
    chrono::DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|d| d.timestamp_millis())
}

/// プロンプト抜粋: 改行を空白にし、`max_chars`（char 単位）で切る。0 なら保存しない。
pub fn excerpt(text: &str, max_chars: usize) -> Option<String> {
    if max_chars == 0 {
        return None;
    }
    let mut out = String::new();
    let mut prev_space = true;
    let mut count = 0;
    for c in text.chars() {
        let c = if c.is_whitespace() { ' ' } else { c };
        if c == ' ' && prev_space {
            continue;
        }
        prev_space = c == ' ';
        if count >= max_chars {
            break;
        }
        out.push(c);
        count += 1;
    }
    let out = out.trim_end().to_string();
    (!out.is_empty()).then_some(out)
}

/// 連続する `response_complete`（間に prompt が無い）を最後の 1 件にまとめる。
/// 1 回の応答が複数行に分かれて記録される場合の重複を避ける。
pub fn push_response_complete(events: &mut Vec<AgentEvent>, ev: AgentEvent) {
    if let Some(last) = events.last_mut() {
        if last.kind == AgentEventKind::ResponseComplete && last.session_id == ev.session_id {
            if ev.at >= last.at {
                *last = ev;
            }
            return;
        }
    }
    events.push(ev);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn excerpt_cuts_by_chars_and_flattens_newlines() {
        assert_eq!(excerpt("あいう\nえお  かき", 5).as_deref(), Some("あいう え"));
        assert_eq!(excerpt("abc", 0), None);
        assert_eq!(excerpt("  \n ", 10), None);
    }

    #[test]
    fn incomplete_last_line_is_not_consumed() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.jsonl");
        let mut f = File::create(&p).unwrap();
        f.write_all(b"{\"a\":1}\r\n{\"b\":2}\n{\"c\"").unwrap();
        let (lines, off) = read_complete_lines(&p, 0).unwrap();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].bytes, b"{\"a\":1}");
        assert_eq!(lines[1].offset, 9);
        assert_eq!(off, 17);
        let (lines, off2) = read_complete_lines(&p, off).unwrap();
        assert!(lines.is_empty());
        assert_eq!(off2, off);
    }
}
