//! Optional terminal queries and presentation modes complement the upstream VT screen.
use super::*;

/// Query answers remain guest-owned bytes; the parser has no process or clipboard authority.
#[derive(Clone)]
pub(super) struct Listener(pub Rc<RefCell<Replies>>);
impl vt100::Callbacks for Listener {
    fn unhandled_csi(
        &mut self,
        screen: &mut vt100::Screen,
        i1: Option<u8>,
        i2: Option<u8>,
        params: &[&[u16]],
        c: char,
    ) {
        let value = params.first().and_then(|p| p.first()).copied().unwrap_or(0);
        let mut replies = self.0.borrow_mut();
        match (i1, i2, c, value) {
            (None, None, 'n', 5) => replies.bytes.extend_from_slice(b"\x1b[0n"),
            (None, None, 'n', 6) => {
                let (row, col) = screen.cursor_position();
                replies.bytes.extend_from_slice(
                    format!("\x1b[{};{}R", row + 1, col.min(screen.size().1 - 1) + 1).as_bytes(),
                );
            }
            (None, None, 'c', _) => replies.bytes.extend_from_slice(b"\x1b[?6c"),
            (Some(b'>'), None, 'c', _) => replies.bytes.extend_from_slice(b"\x1b[>0;1;0c"),
            (Some(b'?'), None, 'h' | 'l', _) if params.iter().any(|p| *p == [1004]) => {
                replies.focus = c == 'h'
            }
            (Some(b' '), None, 'q', _) => {
                replies.cursor = match value {
                    1 | 2 => 1,
                    3 | 4 => 3,
                    _ => 5,
                }
            }
            (None, None, 't', 14 | 16 | 18) => {
                let (w, h) = replies.cell_size;
                let (rows, cols) = replies.grid_size;
                let response = match value {
                    14 => format!(
                        "\x1b[4;{};{}t",
                        u32::from(h) * u32::from(rows),
                        u32::from(w) * u32::from(cols)
                    ),
                    16 => format!("\x1b[6;{h};{w}t"),
                    _ => format!("\x1b[8;{rows};{cols}t"),
                };
                replies.bytes.extend_from_slice(response.as_bytes());
            }
            _ => {}
        }
    }
    fn unhandled_osc(&mut self, _: &mut vt100::Screen, params: &[&[u8]]) {
        let Some(code) = params
            .first()
            .and_then(|v| std::str::from_utf8(v).ok()?.parse::<usize>().ok())
        else {
            return;
        };
        let mut replies = self.0.borrow_mut();
        let entries: Vec<(usize, &[u8])> = match code {
            4 => params[1..]
                .chunks_exact(2)
                .filter_map(|pair| {
                    Some((std::str::from_utf8(pair[0]).ok()?.parse().ok()?, pair[1]))
                })
                .collect(),
            10..=12 => params
                .iter()
                .skip(1)
                .enumerate()
                .map(|(i, value)| (256 + code - 10 + i, *value))
                .collect(),
            104 => {
                replies.colors.retain(|index, _| *index >= 256);
                return;
            }
            110..=112 => {
                replies.colors.remove(&(256 + code - 110));
                return;
            }
            _ => return,
        };
        for (index, value) in entries.into_iter().filter(|(index, _)| *index < 269) {
            if value == b"?" {
                let rgb = replies
                    .colors
                    .get(&index)
                    .copied()
                    .or_else(|| replies.palette.get(index).copied())
                    .unwrap_or(0);
                let prefix = if code == 4 {
                    format!("4;{index}")
                } else {
                    (index - 256 + 10).to_string()
                };
                let (r, g, b) = ((rgb >> 16) & 255, (rgb >> 8) & 255, rgb & 255);
                replies.bytes.extend_from_slice(
                    format!(
                        "\x1b]{prefix};rgb:{:04x}/{:04x}/{:04x}\x07",
                        r * 257,
                        g * 257,
                        b * 257
                    )
                    .as_bytes(),
                );
            } else if let Some(rgb) = parse_color(value) {
                replies.colors.insert(index, rgb);
            }
        }
    }
}

/// Xterm colors accept one to four hex digits per component; scale each to eight bits.
fn parse_color(bytes: &[u8]) -> Option<u32> {
    let text = std::str::from_utf8(bytes).ok()?;
    if let Some(hex) = text.strip_prefix('#').filter(|hex| hex.len() == 6) {
        return u32::from_str_radix(hex, 16).ok();
    }
    let parts: Vec<_> = text.strip_prefix("rgb:")?.split('/').collect();
    if parts.len() != 3 {
        return None;
    }
    let mut rgb = 0;
    for part in parts {
        if part.is_empty() || part.len() > 4 {
            return None;
        }
        let component = u32::from_str_radix(part, 16).ok()?;
        rgb = (rgb << 8) | (component * 255 / ((1u32 << (part.len() * 4)) - 1));
    }
    Some(rgb)
}
