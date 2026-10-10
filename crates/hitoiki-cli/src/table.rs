//! 外部クレートを使わない簡易テーブル整形。全角文字は幅 2 として扱う。

/// 文字の表示幅（東アジアの全角・絵文字を 2、制御文字を 0 とする簡易判定）。
fn char_width(c: char) -> usize {
    let u = c as u32;
    if u < 0x20 || (0x7f..0xa0).contains(&u) {
        return 0;
    }
    let wide = matches!(u,
        0x1100..=0x115F
        | 0x2E80..=0xA4CF
        | 0xAC00..=0xD7A3
        | 0xF900..=0xFAFF
        | 0xFE30..=0xFE6F
        | 0xFF00..=0xFF60
        | 0xFFE0..=0xFFE6
        | 0x1F300..=0x1F64F
        | 0x1F900..=0x1F9FF
        | 0x20000..=0x3FFFD);
    if wide {
        2
    } else {
        1
    }
}

pub fn width(s: &str) -> usize {
    s.chars().map(char_width).sum()
}

/// 表示幅が `max` を超える場合は末尾を `…` で切り詰める。改行は空白にする。
pub fn truncate(s: &str, max: usize) -> String {
    let s: String = s.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    if width(&s) <= max {
        return s;
    }
    let mut out = String::new();
    let mut w = 0;
    for c in s.chars() {
        let cw = char_width(c);
        if w + cw + 1 > max {
            break;
        }
        out.push(c);
        w += cw;
    }
    out.push('…');
    out
}

pub fn pad(s: &str, w: usize) -> String {
    let cur = width(s);
    format!("{s}{}", " ".repeat(w.saturating_sub(cur)))
}

/// ヘッダ付きの表を作る。最終行の末尾空白は付けない。
pub fn table(headers: &[&str], rows: &[Vec<String>]) -> String {
    let cols = headers.len();
    let mut widths: Vec<usize> = headers.iter().map(|h| width(h)).collect();
    let rows: Vec<Vec<String>> = rows
        .iter()
        .map(|r| {
            r.iter()
                .map(|c| {
                    c.chars()
                        .map(|ch| if ch.is_control() { ' ' } else { ch })
                        .collect()
                })
                .collect()
        })
        .collect();
    for r in &rows {
        for (i, c) in r.iter().enumerate().take(cols) {
            widths[i] = widths[i].max(width(c));
        }
    }
    let line = |cells: Vec<&str>| -> String {
        let mut parts = Vec::new();
        for (i, c) in cells.iter().enumerate().take(cols) {
            parts.push(if i + 1 == cols {
                (*c).to_string()
            } else {
                pad(c, widths[i])
            });
        }
        parts.join("  ").trim_end().to_string()
    };
    let mut out = vec![line(headers.to_vec())];
    out.push(
        widths
            .iter()
            .map(|w| "-".repeat(*w))
            .collect::<Vec<_>>()
            .join("  "),
    );
    for r in &rows {
        out.push(line(r.iter().map(String::as_str).collect()));
    }
    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn width_counts_fullwidth() {
        assert_eq!(width("abc"), 3);
        assert_eq!(width("作業"), 4);
        assert_eq!(truncate("あいうえお", 6), "あい…");
        assert_eq!(truncate("abc", 6), "abc");
    }

    #[test]
    fn table_aligns_by_width() {
        let t = table(
            &["名前", "x"],
            &[vec!["あ".into(), "1".into()], vec!["abcd".into(), "2".into()]],
        );
        let lines: Vec<&str> = t.lines().collect();
        assert_eq!(width(lines[2].split("  ").next().unwrap().trim_end()), 2);
        assert!(lines[3].starts_with("abcd"));
        // 2 列目の開始位置が揃う
        let p = |l: &str| width(&l[..l.rfind(|c: char| c.is_ascii_digit()).unwrap()]);
        assert_eq!(p(lines[2]), p(lines[3]));
    }
}
