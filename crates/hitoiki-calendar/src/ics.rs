//! ICS (RFC 5545) の行展開・プロパティ解析。必要な範囲だけの軽量パーサ。

/// 1 つのプロパティ行（`NAME;PARAM=V:value`）。
#[derive(Debug, Clone)]
pub struct Prop {
    /// 大文字化した名前。
    pub name: String,
    /// (大文字化したパラメータ名, 値)。値の引用符は除去済み。
    pub params: Vec<(String, String)>,
    pub value: String,
}

impl Prop {
    pub fn param(&self, key: &str) -> Option<&str> {
        self.params
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }
}

/// 折り返し行を展開する（行頭が空白/タブの行は直前の行の続き）。
pub fn unfold(text: &str) -> Vec<String> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut lines: Vec<String> = Vec::new();
    for raw in text.split('\n') {
        let raw = raw.strip_suffix('\r').unwrap_or(raw);
        if let Some(rest) = raw.strip_prefix([' ', '\t']) {
            if let Some(last) = lines.last_mut() {
                last.push_str(rest);
                continue;
            }
        }
        if !raw.is_empty() {
            lines.push(raw.to_string());
        }
    }
    lines
}

/// 1 行をプロパティに分解する。名前を取れない行は None。
pub fn parse_line(line: &str) -> Option<Prop> {
    // 名前: 最初の ':' か ';' まで
    let name_end = line.find([':', ';'])?;
    let name = line[..name_end].trim().to_ascii_uppercase();
    if name.is_empty() {
        return None;
    }
    let mut params = Vec::new();
    let mut rest = &line[name_end..];
    // パラメータ部（引用符内の ; : は区切りとして扱わない）
    while let Some(r) = rest.strip_prefix(';') {
        let bytes = r.as_bytes();
        let mut in_q = false;
        let mut end = None;
        for (i, &b) in bytes.iter().enumerate() {
            match b {
                b'"' => in_q = !in_q,
                b';' | b':' if !in_q => {
                    end = Some(i);
                    break;
                }
                _ => {}
            }
        }
        let end = end?;
        let p = &r[..end];
        if let Some((k, v)) = p.split_once('=') {
            params.push((
                k.trim().to_ascii_uppercase(),
                v.trim().trim_matches('"').to_string(),
            ));
        }
        rest = &r[end..];
    }
    let value = rest.strip_prefix(':')?.to_string();
    Some(Prop { name, params, value })
}

/// TEXT 値のエスケープ解除。
pub fn unescape_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c == '\\' {
            match it.next() {
                Some('n') | Some('N') => out.push('\n'),
                Some(o) => out.push(o),
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// VEVENT ごとのプロパティ列を取り出す。入れ子コンポーネント（VALARM 等）の中身は無視する。
pub fn vevents(text: &str) -> Vec<Vec<Prop>> {
    let mut events = Vec::new();
    let mut cur: Option<Vec<Prop>> = None;
    let mut depth = 0usize; // VEVENT 内の入れ子の深さ
    for line in unfold(text) {
        let Some(p) = parse_line(&line) else { continue };
        match p.name.as_str() {
            "BEGIN" => {
                if cur.is_some() {
                    depth += 1;
                } else if p.value.eq_ignore_ascii_case("VEVENT") {
                    cur = Some(Vec::new());
                    depth = 0;
                }
            }
            "END" => {
                if cur.is_some() {
                    if depth > 0 {
                        depth -= 1;
                    } else if p.value.eq_ignore_ascii_case("VEVENT") {
                        events.extend(cur.take());
                    }
                }
            }
            _ => {
                if depth == 0 {
                    if let Some(c) = cur.as_mut() {
                        c.push(p);
                    }
                }
            }
        }
    }
    events
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unfold_lines() {
        let l = unfold("A:1\r\nSUMMARY:abc\r\n def\r\n\tghi\r\nB:2\r\n");
        assert_eq!(l, vec!["A:1", "SUMMARY:abcdefghi", "B:2"]);
    }

    #[test]
    fn params_with_quotes() {
        let p = parse_line("DTSTART;TZID=\"a:b;c\";VALUE=DATE-TIME:20250101T000000").unwrap();
        assert_eq!(p.param("TZID"), Some("a:b;c"));
        assert_eq!(p.param("VALUE"), Some("DATE-TIME"));
        assert_eq!(p.value, "20250101T000000");
    }

    #[test]
    fn unescape() {
        assert_eq!(unescape_text(r"a\, b\nc\\d\;"), "a, b\nc\\d;");
    }
}
