//! The six widgets every screen is built from (§6.3).

pub mod braille;
pub mod dotmeter;
pub mod dotspark;
pub mod flow;
pub mod meter;
pub mod notchbox;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;

use super::text;

/// Write `s` at (x, y), clipped to `area`. Returns the x after the text.
pub fn put(buf: &mut Buffer, area: Rect, x: u16, y: u16, s: &str, style: Style) -> u16 {
    if y < area.y || y >= area.bottom() || x >= area.right() {
        return x;
    }
    let max = (area.right() - x) as usize;
    let t = text::trunc(s, max);
    let (nx, _) = buf.set_stringn(x, y, &t, max, style);
    nx
}

/// Like [`put`] but without the ellipsis: hard clip at the area edge.
pub fn put_clip(buf: &mut Buffer, area: Rect, x: u16, y: u16, s: &str, style: Style) -> u16 {
    if y < area.y || y >= area.bottom() || x >= area.right() {
        return x;
    }
    let max = (area.right() - x) as usize;
    let (nx, _) = buf.set_stringn(x, y, s, max, style);
    nx
}

/// Set one cell's symbol and style (if inside `area`).
pub fn cell(buf: &mut Buffer, area: Rect, x: u16, y: u16, sym: &str, style: Style) {
    if x >= area.x && x < area.right() && y >= area.y && y < area.bottom() {
        buf[(x, y)].set_symbol(sym).set_style(style);
    }
}

/// Fill a rect with a style (background), keeping symbols.
pub fn fill(buf: &mut Buffer, area: Rect, style: Style) {
    buf.set_style(area, style);
}

/// Blank a rect: spaces with the given style.
pub fn clear(buf: &mut Buffer, area: Rect, style: Style) {
    for y in area.y..area.bottom() {
        for x in area.x..area.right() {
            buf[(x, y)].reset();
            buf[(x, y)].set_symbol(" ").set_style(style);
        }
    }
}
