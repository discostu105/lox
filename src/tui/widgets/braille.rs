//! BrailleGraph — multi-row area graph, 2×4 dots per cell, colored by row height (§6.3).

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::cell;
use crate::tui::theme::{Grad, Theme};

// Dot bit for (column 0/1, row-from-top 0..4)
const DOT: [[u32; 4]; 2] = [[0x01, 0x02, 0x04, 0x40], [0x08, 0x10, 0x20, 0x80]];

#[derive(Debug, Clone, Default)]
pub struct GraphOpts {
    /// Grow downward from the top edge (lower half of a mirrored graph)
    pub down: bool,
    /// Draw faint baseline dots where a sample is zero/absent
    pub floor: bool,
    /// Samples at index ≥ this are drawn faint (forecast after "now")
    pub faint_from: Option<usize>,
    /// Column (in samples) to mark with a `┊` "now" marker
    pub marker: Option<usize>,
}

/// Draw `data` (right-aligned, 2 samples per cell) into `area`, scaled to `max`.
// dot indices take part in the arithmetic, not just in indexing DOT
#[allow(clippy::needless_range_loop)]
pub fn graph(
    buf: &mut Buffer,
    area: Rect,
    data: &[f64],
    max: f64,
    grad: Grad,
    opts: &GraphOpts,
    th: &Theme,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let rows = area.height as usize;
    let dots_h = rows * 4;
    let n = area.width as usize * 2;
    let start = data.len().saturating_sub(n);
    let vis = &data[start..];
    let pad = n - vis.len();
    let max = if max > 0.0 && max.is_finite() {
        max
    } else {
        1.0
    };
    // dot heights per sample column
    let heights: Vec<Option<usize>> = (0..n)
        .map(|i| {
            i.checked_sub(pad).and_then(|k| vis.get(k)).map(|v| {
                if !v.is_finite() || *v <= 0.0 {
                    0
                } else {
                    ((v / max).clamp(0.0, 1.0) * dots_h as f64).round().max(1.0) as usize
                }
            })
        })
        .collect();
    for cx in 0..area.width as usize {
        for ry in 0..rows {
            let mut bits = 0u32;
            for col in 0..2 {
                let si = cx * 2 + col;
                let Some(h) = heights[si] else { continue };
                for dy in 0..4 {
                    // dot index measured from the baseline
                    let from_base = if opts.down {
                        ry * 4 + dy
                    } else {
                        (rows - 1 - ry) * 4 + (3 - dy)
                    };
                    let on = from_base < h || (opts.floor && h == 0 && from_base == 0);
                    if on {
                        bits |= DOT[col][dy];
                    }
                }
            }
            let x = area.x + cx as u16;
            let y = area.y + ry as u16;
            let sample_i = cx * 2;
            let is_marker = opts
                .marker
                .is_some_and(|m| m + pad == sample_i || m + pad == sample_i + 1);
            if bits == 0 {
                if is_marker {
                    cell(buf, area, x, y, "┊", th.s_dim());
                }
                continue;
            }
            // row height → gradient position
            let t = if opts.down {
                (ry as f64 + 0.5) / rows as f64
            } else {
                (rows - ry) as f64 / rows as f64
            };
            let faint = opts.faint_from.is_some_and(|f| sample_i >= f + pad)
                || heights[sample_i].unwrap_or(0) == 0
                    && heights.get(sample_i + 1).copied().flatten().unwrap_or(0) == 0;
            let style = if faint {
                th.s_faint()
            } else {
                th.grad_style(grad, t)
            };
            let sym = char::from_u32(0x2800 + bits).unwrap_or(' ').to_string();
            cell(buf, area, x, y, &sym, style);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::theme::{Depth, ThemeName};

    fn lines(data: &[f64], w: u16, h: u16, opts: &GraphOpts) -> Vec<String> {
        let th = Theme::new(ThemeName::Night, Depth::TrueColor);
        let area = Rect::new(0, 0, w, h);
        let mut buf = Buffer::empty(area);
        graph(&mut buf, area, data, 4.0, Grad::Load, opts, &th);
        (0..h)
            .map(|y| (0..w).map(|x| buf[(x, y)].symbol().to_string()).collect())
            .collect()
    }

    #[test]
    fn full_and_empty() {
        let l = lines(&[4.0, 4.0], 1, 1, &GraphOpts::default());
        assert_eq!(l, ["⣿"]);
        let l = lines(&[], 3, 2, &GraphOpts::default());
        assert_eq!(l, ["   ", "   "]);
    }

    #[test]
    fn two_rows_ramp() {
        // 8 dot rows, max 4 → heights 2, 4, 6, 8 dots
        let l = lines(&[1.0, 2.0, 3.0, 4.0], 2, 2, &GraphOpts::default());
        assert_eq!(l, [" ⣼", "⣼⣿"]);
    }

    #[test]
    fn mirrored_down_and_floor() {
        let l = lines(
            &[4.0, 0.0],
            1,
            1,
            &GraphOpts {
                down: true,
                floor: true,
                ..Default::default()
            },
        );
        assert_eq!(l, ["⡏"]);
    }
}
