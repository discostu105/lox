//! Meter — gradient bar with an eighth-block tip (§6.3).
//!
//! Each filled cell takes the gradient color of *its position*, so a bar at 30 % is
//! all green and one at 95 % runs green → yellow → red (btop's trick).

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;

use super::cell;
use crate::tui::theme::{Grad, Theme};

const TIPS: [&str; 8] = [" ", "▏", "▎", "▍", "▌", "▋", "▊", "▉"];

/// Draw a meter of `width` cells at (x, y). `frac` is 0..=1; `None` = unavailable.
pub fn meter(
    buf: &mut Buffer,
    area: Rect,
    x: u16,
    y: u16,
    width: u16,
    frac: Option<f64>,
    grad: Grad,
    th: &Theme,
) {
    let track = if th.is_mono() {
        th.s_faint()
    } else {
        Style::default().bg(th.track)
    };
    let Some(f) = frac.filter(|f| f.is_finite()) else {
        for i in 0..width {
            cell(
                buf,
                area,
                x + i,
                y,
                if th.is_mono() { "·" } else { " " },
                track,
            );
        }
        return;
    };
    let f = f.clamp(0.0, 1.0);
    let eighths = (f * width as f64 * 8.0).round() as u32;
    for i in 0..width {
        let t = if width <= 1 {
            f
        } else {
            i as f64 / (width - 1) as f64
        };
        let full = eighths / 8;
        let s = th.grad_style(grad, t);
        if (i as u32) < full {
            cell(buf, area, x + i, y, "█", s);
        } else if i as u32 == full && !eighths.is_multiple_of(8) {
            let st = if th.is_mono() { s } else { s.bg(th.track) };
            cell(buf, area, x + i, y, TIPS[(eighths % 8) as usize], st);
        } else {
            cell(
                buf,
                area,
                x + i,
                y,
                if th.is_mono() { "·" } else { " " },
                track,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::theme::{Depth, ThemeName};

    fn line(frac: Option<f64>, w: u16) -> String {
        let th = Theme::new(ThemeName::Mono, Depth::Mono);
        let area = Rect::new(0, 0, w, 1);
        let mut buf = Buffer::empty(area);
        meter(&mut buf, area, 0, 0, w, frac, Grad::Load, &th);
        (0..w).map(|x| buf[(x, 0)].symbol().to_string()).collect()
    }

    #[test]
    fn edge_values() {
        assert_eq!(line(Some(0.0), 8), "········");
        assert_eq!(line(Some(1.0), 8), "████████");
        assert_eq!(line(Some(0.5), 8), "████····");
        assert_eq!(line(Some(0.53), 8), "████▎···");
        assert_eq!(line(Some(7.0), 4), "████");
        assert_eq!(line(None, 4), "····");
        assert_eq!(line(Some(f64::NAN), 4), "····");
    }

    #[test]
    fn gradient_by_position() {
        let th = Theme::new(ThemeName::Night, Depth::TrueColor);
        let area = Rect::new(0, 0, 10, 1);
        let mut buf = Buffer::empty(area);
        meter(&mut buf, area, 0, 0, 10, Some(1.0), Grad::Load, &th);
        assert_eq!(buf[(0, 0)].fg, th.grad(Grad::Load, 0.0));
        assert_eq!(buf[(9, 0)].fg, th.grad(Grad::Load, 1.0));
    }
}
