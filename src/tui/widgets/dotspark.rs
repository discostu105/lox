//! DotSpark — one-row braille sparkline, 2 samples per cell, 4 levels (§6.3).

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::cell;
use crate::tui::theme::{Grad, Theme};

// Bottom-up fill masks for the left (dots 7,3,2,1) and right (dots 8,6,5,4) columns.
const LEFT: [u32; 5] = [0, 0x40, 0x44, 0x46, 0x47];
const RIGHT: [u32; 5] = [0, 0x80, 0xA0, 0xB0, 0xB8];

/// Level 0..=4 for a value in [lo, hi]; present values get at least one dot.
fn level(v: f64, lo: f64, hi: f64) -> usize {
    if !v.is_finite() {
        return 0;
    }
    if hi - lo < 1e-9 {
        return 2;
    }
    1 + (((v - lo) / (hi - lo)).clamp(0.0, 1.0) * 3.0).round() as usize
}

/// Draw the last `width * 2` samples of `data`, right-aligned. `range` fixes the
/// scale (and the gradient mapping); `None` autoscales to the visible data.
#[allow(clippy::too_many_arguments)]
pub fn dotspark(
    buf: &mut Buffer,
    area: Rect,
    x: u16,
    y: u16,
    width: u16,
    data: &[f64],
    range: Option<(f64, f64)>,
    grad: Grad,
    th: &Theme,
) {
    let n = (width as usize) * 2;
    let start = data.len().saturating_sub(n);
    let vis = &data[start..];
    let (lo, hi) = range.unwrap_or_else(|| {
        let lo = vis
            .iter()
            .cloned()
            .filter(|v| v.is_finite())
            .fold(f64::INFINITY, f64::min);
        let hi = vis
            .iter()
            .cloned()
            .filter(|v| v.is_finite())
            .fold(f64::NEG_INFINITY, f64::max);
        if lo.is_finite() { (lo, hi) } else { (0.0, 1.0) }
    });
    // right-align: pad on the left
    let pad = n - vis.len();
    for i in 0..width as usize {
        let a = (i * 2).checked_sub(pad).and_then(|k| vis.get(k));
        let b = (i * 2 + 1).checked_sub(pad).and_then(|k| vis.get(k));
        let la = a.map(|v| level(*v, lo, hi)).unwrap_or(0);
        let lb = b.map(|v| level(*v, lo, hi)).unwrap_or(0);
        let bits = LEFT[la] | RIGHT[lb];
        let sym = char::from_u32(0x2800 + bits).unwrap_or(' ').to_string();
        let v = b.or(a).copied().unwrap_or(lo);
        let t = if hi - lo < 1e-9 {
            0.5
        } else {
            (v - lo) / (hi - lo)
        };
        let style = if a.is_none() && b.is_none() {
            th.s_faint()
        } else {
            th.grad_style(grad, t)
        };
        cell(buf, area, x + i as u16, y, &sym, style);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::theme::{Depth, ThemeName};

    fn line(data: &[f64], w: u16, range: Option<(f64, f64)>) -> String {
        let th = Theme::new(ThemeName::Night, Depth::TrueColor);
        let area = Rect::new(0, 0, w, 1);
        let mut buf = Buffer::empty(area);
        dotspark(&mut buf, area, 0, 0, w, data, range, Grad::Temp, &th);
        (0..w).map(|x| buf[(x, 0)].symbol().to_string()).collect()
    }

    #[test]
    fn empty_and_flat_series() {
        assert_eq!(line(&[], 3, None), "⠀⠀⠀");
        // flat data sits mid-height
        assert_eq!(line(&[5.0, 5.0], 1, None), "⣤");
    }

    #[test]
    fn rising_series_and_right_alignment() {
        let s = line(&[0.0, 1.0, 2.0, 3.0], 3, Some((0.0, 3.0)));
        assert_eq!(s, "⠀⣠⣾");
        assert_eq!(line(&[0.0, 3.0], 1, Some((0.0, 3.0))), "⣸");
    }
}
