//! グローバルショートカットの文字列（`general.quick_entry_shortcut`）の解釈と正規化。
//!
//! OS への登録は GUI（hitoiki-app）が行う。ここでは設定値の検証と、登録に渡す正規形への変換だけを持つ。
//! 受け付ける形は `Ctrl+Alt+Space` のように修飾キー（1 つ以上）と主キー（1 つ）を `+` でつないだもの
//! （大文字小文字は区別しない）。空文字は「無効」を表す。

use crate::{Error, Result};

/// 解釈したショートカット。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shortcut {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    /// Windows キー。
    pub win: bool,
    /// 主キーの正規形（`A`、`5`、`F9`、`Space` など）。
    pub key: String,
}

impl Shortcut {
    /// 登録・表示に使う正規形（例: `Ctrl+Alt+Space`）。修飾キーの並びは Ctrl, Alt, Shift, Super 固定。
    pub fn canonical(&self) -> String {
        let mut parts: Vec<&str> = Vec::new();
        if self.ctrl {
            parts.push("Ctrl");
        }
        if self.alt {
            parts.push("Alt");
        }
        if self.shift {
            parts.push("Shift");
        }
        if self.win {
            parts.push("Super");
        }
        parts.push(&self.key);
        parts.join("+")
    }
}

fn bad(s: &str, why: &str) -> Error {
    Error::InvalidArgument(format!("ショートカット「{s}」を解釈できません: {why}"))
}

/// 主キーの正規形を返す（受け付けないものは `None`）。
fn normalize_key(token: &str) -> Option<String> {
    let up = token.to_uppercase();
    if up.len() == 1 {
        let c = up.chars().next()?;
        if c.is_ascii_alphanumeric() {
            return Some(up);
        }
    }
    if let Some(n) = up.strip_prefix('F').and_then(|n| n.parse::<u32>().ok()) {
        if (1..=24).contains(&n) && up == format!("F{n}") {
            return Some(up);
        }
    }
    let named = match up.as_str() {
        "SPACE" => "Space",
        "ENTER" | "RETURN" => "Enter",
        "TAB" => "Tab",
        "ESC" | "ESCAPE" => "Escape",
        "BACKSPACE" => "Backspace",
        "DELETE" | "DEL" => "Delete",
        "INSERT" => "Insert",
        "HOME" => "Home",
        "END" => "End",
        "PAGEUP" => "PageUp",
        "PAGEDOWN" => "PageDown",
        "UP" | "ARROWUP" => "Up",
        "DOWN" | "ARROWDOWN" => "Down",
        "LEFT" | "ARROWLEFT" => "Left",
        "RIGHT" | "ARROWRIGHT" => "Right",
        _ => return None,
    };
    Some(named.to_string())
}

/// ショートカット文字列を解釈する。空文字（無効）は `Ok(None)`。
///
/// 修飾キーを 1 つ以上含み、主キーがちょうど 1 つであること（修飾キー無しの単独キーは、
/// 通常の入力を奪うので受け付けない）。
pub fn parse(s: &str) -> Result<Option<Shortcut>> {
    let s = s.trim();
    if s.is_empty() {
        return Ok(None);
    }
    let mut sc = Shortcut {
        ctrl: false,
        alt: false,
        shift: false,
        win: false,
        key: String::new(),
    };
    let mut key: Option<String> = None;
    for raw in s.split('+') {
        let token = raw.trim();
        if token.is_empty() {
            return Err(bad(s, "空の要素があります（`+` 自体をキーにはできません）"));
        }
        if key.is_some() {
            return Err(bad(s, "主キーは修飾キーの後に 1 つだけ指定します"));
        }
        match token.to_uppercase().as_str() {
            "CTRL" | "CONTROL" | "CMDORCTRL" | "COMMANDORCONTROL" | "COMMANDORCTRL" | "CMDORCONTROL" => {
                sc.ctrl = true
            }
            "ALT" | "OPTION" => sc.alt = true,
            "SHIFT" => sc.shift = true,
            "SUPER" | "WIN" | "WINDOWS" | "META" | "CMD" | "COMMAND" => sc.win = true,
            _ => {
                key = Some(
                    normalize_key(token).ok_or_else(|| bad(s, &format!("キー「{token}」は使えません")))?,
                );
            }
        }
    }
    sc.key = key.ok_or_else(|| bad(s, "主キーがありません"))?;
    if !(sc.ctrl || sc.alt || sc.shift || sc.win) {
        return Err(bad(
            s,
            "修飾キー（Ctrl / Alt / Shift / Super）を 1 つ以上含めてください",
        ));
    }
    Ok(Some(sc))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn canon(s: &str) -> String {
        parse(s).unwrap().unwrap().canonical()
    }

    #[test]
    fn 正規形に直す() {
        assert_eq!(canon("ctrl+alt+space"), "Ctrl+Alt+Space");
        assert_eq!(canon("Alt + Ctrl + k"), "Ctrl+Alt+K");
        assert_eq!(canon("Win+Shift+f9"), "Shift+Super+F9");
        assert_eq!(canon("CmdOrCtrl+Enter"), "Ctrl+Enter");
        assert_eq!(canon("Ctrl+5"), "Ctrl+5");
        assert_eq!(canon("Ctrl+ArrowUp"), "Ctrl+Up");
    }

    #[test]
    fn 空は無効() {
        assert_eq!(parse("").unwrap(), None);
        assert_eq!(parse("  ").unwrap(), None);
    }

    #[test]
    fn 解釈できない値はエラー() {
        for s in [
            "Space",
            "F9",
            "Ctrl+",
            "Ctrl+Alt",
            "Ctrl+A+B",
            "Ctrl+Space+Alt",
            "Ctrl++",
            "Ctrl+Nope",
            "Ctrl+F25",
            "Ctrl+F0",
            "Ctrl+AB",
            "abc",
        ] {
            let e = parse(s).unwrap_err();
            assert_eq!(e.exit_code(), 2, "{s}");
        }
    }
}
