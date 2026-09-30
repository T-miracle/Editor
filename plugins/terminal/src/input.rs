//! Xterm keyboard encoding; printable text is delivered separately by the platform IME.

/// Encode navigation/control keys, preserving application cursor mode and modifiers.
pub fn key(key: &str, ctrl: bool, alt: bool, shift: bool, app_cursor: bool) -> Option<Vec<u8>> {
    let modifier = 1 + u8::from(shift) + 2 * u8::from(alt) + 4 * u8::from(ctrl);
    let cursor = match key {
        "up" => Some("A"),
        "down" => Some("B"),
        "right" => Some("C"),
        "left" => Some("D"),
        "home" => Some("H"),
        "end" => Some("F"),
        _ => None,
    };
    if let Some(final_byte) = cursor {
        return Some(
            if modifier != 1 {
                format!("\x1b[1;{modifier}{final_byte}")
            } else if app_cursor {
                format!("\x1bO{final_byte}")
            } else {
                format!("\x1b[{final_byte}")
            }
            .into_bytes(),
        );
    }
    let tilde = match key {
        "insert" => Some(2),
        "delete" => Some(3),
        "pageup" => Some(5),
        "pagedown" => Some(6),
        "f5" => Some(15),
        "f6" => Some(17),
        "f7" => Some(18),
        "f8" => Some(19),
        "f9" => Some(20),
        "f10" => Some(21),
        "f11" => Some(23),
        "f12" => Some(24),
        _ => None,
    };
    if let Some(code) = tilde {
        return Some(
            if modifier == 1 {
                format!("\x1b[{code}~")
            } else {
                format!("\x1b[{code};{modifier}~")
            }
            .into_bytes(),
        );
    }
    if let Some(code) = match key {
        "f1" => Some('P'),
        "f2" => Some('Q'),
        "f3" => Some('R'),
        "f4" => Some('S'),
        _ => None,
    } {
        return Some(
            if modifier == 1 {
                format!("\x1bO{code}")
            } else {
                format!("\x1b[1;{modifier}{code}")
            }
            .into_bytes(),
        );
    }
    let mut bytes = match key {
        "enter" => vec![b'\r'],
        "backspace" => vec![if ctrl { 8 } else { 127 }],
        "escape" => vec![27],
        "tab" if shift => b"\x1b[Z".to_vec(),
        "tab" => vec![9],
        "space" if ctrl => vec![0],
        _ if ctrl && key.len() == 1 => {
            let byte = key.as_bytes()[0].to_ascii_uppercase();
            if (b'@'..=b'_').contains(&byte) {
                vec![byte & 0x1f]
            } else {
                return None;
            }
        }
        _ if alt && key.chars().count() == 1 => key.as_bytes().to_vec(),
        _ => return None,
    };
    if alt {
        bytes.insert(0, 27);
    }
    Some(bytes)
}

/// Bracketed paste prevents multiline pastes from being interpreted as individual key presses.
pub fn paste(text: &str, bracketed: bool) -> Vec<u8> {
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    if bracketed {
        format!("\x1b[200~{}\x1b[201~", text.replace('\x1b', "")).into_bytes()
    } else {
        text.replace('\n', "\r").into_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    /// Encode raw control bytes and application arrows; the UI intercepts clipboard keys first.
    #[test]
    fn control_and_navigation() {
        assert_eq!(key("c", true, false, false, false), Some(vec![3]));
        assert_eq!(
            key("up", false, false, false, true),
            Some(b"\x1bOA".to_vec())
        );
        assert_eq!(
            key("left", true, true, false, false),
            Some(b"\x1b[1;7D".to_vec())
        );
        assert_eq!(key("a", false, false, false, false), None);
    }
    /// Embedded escape sequences cannot terminate bracketed paste prematurely.
    #[test]
    fn paste_is_delimited() {
        assert_eq!(
            paste("a\r\nb\x1b[201~", true),
            b"\x1b[200~a\nb[201~\x1b[201~"
        );
    }
}
