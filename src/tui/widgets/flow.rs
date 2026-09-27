//! FlowLine — `━━━▶━━━━━▶` with moving arrowheads (§6.3).
//!
//! Speed is proportional to the value, direction flips with its sign. With motion
//! off (or zero flow) the arrowheads stand still.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;

use super::cell;
use crate::tui::theme::Theme;

const SPACING: usize = 6;

/// Horizontal flow line of `width` cells. `phase` advances with time; `value` sets
/// direction (positive = left → right) and speed; `None` draws a dashed idle line.
#[allow(clippy::too_many_arguments)]
pub fn hflow(
    buf: &mut Buffer,
    area: Rect,
    x: u16,
    y: u16,
    width: u16,
    value: Option<f64>,
    phase: f64,
    style: Style,
    th: &Theme,
) {
    let v = value.unwrap_or(0.0);
    if value.is_none() || v.abs() < 0.01 {
        for i in 0..width {
            cell(buf, area, x + i, y, "┄", th.s_faint());
        }
        return;
    }
    let forward = v > 0.0;
    let off = (phase * speed(v)).floor() as usize % SPACING;
    for i in 0..width as usize {
        let pos = if forward {
            (i + SPACING - off) % SPACING
        } else {
            (i + off) % SPACING
        };
        let sym = if pos == SPACING - 1 {
            if forward { "▶" } else { "◀" }
        } else {
            "━"
        };
        cell(buf, area, x + i as u16, y, sym, style);
    }
}

/// Vertical flow line (positive = downward).
#[allow(clippy::too_many_arguments)]
pub fn vflow(
    buf: &mut Buffer,
    area: Rect,
    x: u16,
    y: u16,
    height: u16,
    value: Option<f64>,
    phase: f64,
    style: Style,
    th: &Theme,
) {
    let v = value.unwrap_or(0.0);
    if value.is_none() || v.abs() < 0.01 {
        for i in 0..height {
            cell(buf, area, x, y + i, "┆", th.s_faint());
        }
        return;
    }
    let down = v > 0.0;
    let sp = 4usize;
    let off = (phase * speed(v)).floor() as usize % sp;
    for i in 0..height as usize {
        let pos = if down {
            (i + sp - off) % sp
        } else {
            (i + off) % sp
        };
        let sym = if pos == sp - 1 {
            if down { "▼" } else { "▲" }
        } else {
            "┃"
        };
        cell(buf, area, x, y + i as u16, sym, style);
    }
}

/// Cells per second: faster for more power (log-ish), capped.
fn speed(v: f64) -> f64 {
    (2.0 + v.abs().ln_1p() * 3.0).min(12.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::theme::{Depth, ThemeName};

    fn line(v: Option<f64>, phase: f64) -> String {
        let th = Theme::new(ThemeName::Night, Depth::TrueColor);
        let area = Rect::new(0, 0, 12, 1);
        let mut buf = Buffer::empty(area);
        hflow(&mut buf, area, 0, 0, 12, v, phase, Style::default(), &th);
        (0..12).map(|x| buf[(x, 0)].symbol().to_string()).collect()
    }

    #[test]
    fn direction_and_idle() {
        assert_eq!(line(None, 0.0), "┄".repeat(12));
        assert_eq!(line(Some(0.0), 0.0), "┄".repeat(12));
        assert_eq!(line(Some(2.0), 0.0), "━━━━━▶━━━━━▶");
        assert_eq!(line(Some(-2.0), 0.0), "━━━━━◀━━━━━◀");
    }

    #[test]
    fn arrows_move_with_phase() {
        assert_ne!(line(Some(2.0), 0.0), line(Some(2.0), 0.5));
    }
}
