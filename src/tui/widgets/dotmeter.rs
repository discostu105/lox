//! DotMeter — btop's memory-style dotted meter `⣿⣿⣿⣿⣀⣀⣀` for capacities (§6.3).

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::cell;
use crate::tui::theme::{Grad, Theme};

/// Draw a dot meter. The filled color is the gradient at the fill level (a battery at
/// 8 % is red all the way), empty dots are faint.
#[allow(clippy::too_many_arguments)]
pub fn dotmeter(
    buf: &mut Buffer,
    area: Rect,
    x: u16,
    y: u16,
    width: u16,
    frac: Option<f64>,
    grad: Grad,
    th: &Theme,
) {
    let Some(f) = frac.filter(|f| f.is_finite()) else {
        for i in 0..width {
            cell(buf, area, x + i, y, "⣀", th.s_faint());
        }
        return;
    };
    let f = f.clamp(0.0, 1.0);
    let halves = (f * width as f64 * 2.0).round() as u32;
    let s = th.grad_style(grad, f);
    for i in 0..width {
        let filled = halves.saturating_sub(i as u32 * 2).min(2);
        let (sym, st) = match filled {
            2 => ("⣿", s),
            1 => ("⡇", s),
            _ => ("⣀", th.s_faint()),
        };
        cell(buf, area, x + i, y, sym, st);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::theme::{Depth, ThemeName};

    fn line(frac: Option<f64>, w: u16) -> String {
        let th = Theme::new(ThemeName::Night, Depth::TrueColor);
        let area = Rect::new(0, 0, w, 1);
        let mut buf = Buffer::empty(area);
        dotmeter(&mut buf, area, 0, 0, w, frac, Grad::Batt, &th);
        (0..w).map(|x| buf[(x, 0)].symbol().to_string()).collect()
    }

    #[test]
    fn edge_values() {
        assert_eq!(line(Some(0.0), 5), "⣀⣀⣀⣀⣀");
        assert_eq!(line(Some(1.0), 5), "⣿⣿⣿⣿⣿");
        assert_eq!(line(Some(0.5), 5), "⣿⣿⡇⣀⣀");
        assert_eq!(line(None, 3), "⣀⣀⣀");
    }
}
