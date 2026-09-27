//! The history chart overlay (§5.10): one control's statistics over a chosen
//! timeframe, optional extra series and the previous period, a cursor, and a
//! summary per series.

use chrono::{DateTime, Local};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use super::common;
use crate::tui::app::{App, ChartData, ChartState, Span};
use crate::tui::chart::{self, Bucket};
use crate::tui::model::{Cid, Kind};
use crate::tui::text::{fit, fmt_kwh, width};
use crate::tui::theme::{Grad, Theme};
use crate::tui::widgets::notchbox::{Hint, Notch, NotchBox};
use crate::tui::widgets::{cell, clear, put};

// Dot bit for (column 0/1, row-from-top 0..4)
const DOT: [[u32; 4]; 2] = [[0x01, 0x02, 0x04, 0x40], [0x08, 0x10, 0x20, 0x80]];

fn local(t: i64) -> DateTime<Local> {
    DateTime::from_timestamp(t, 0)
        .map(|d| d.with_timezone(&Local))
        .unwrap_or_else(Local::now)
}

/// Axis tick label for a timeframe.
fn tick(span: Span, t: i64) -> String {
    let d = local(t);
    match span {
        Span::H6 | Span::H24 => d.format("%H:%M").to_string(),
        Span::D7 => d.format("%a %d.").to_string(),
        Span::D30 => d.format("%a %d.%m.").to_string(),
        Span::Y1 => d.format("%b %y").to_string(),
    }
}

/// Tick times inside [from, to], aligned to local hours, days, Mondays or months.
fn ticks(span: Span, from: i64, to: i64) -> Vec<i64> {
    use chrono::{Datelike, TimeZone};
    let off = local(from).offset().local_minus_utc() as i64;
    let aligned = |step: i64, shift: i64| {
        let mut t = ((from + off - shift).div_euclid(step) + 1) * step + shift - off;
        let mut out = Vec::new();
        while t <= to {
            out.push(t);
            t += step;
        }
        out
    };
    match span {
        Span::H6 => aligned(3600, 0),
        Span::H24 => aligned(3 * 3600, 0),
        Span::D7 => aligned(86_400, 0),
        // 1970-01-05 was a Monday
        Span::D30 => aligned(7 * 86_400, 4 * 86_400),
        Span::Y1 => {
            let a = local(from);
            let (mut y, mut m) = (a.year(), a.month());
            let mut out = Vec::new();
            for _ in 0..14 {
                m += 1;
                if m > 12 {
                    m = 1;
                    y += 1;
                }
                let Some(t) = Local.with_ymd_and_hms(y, m, 1, 0, 0, 0).earliest() else {
                    continue;
                };
                if t.timestamp() > to {
                    break;
                }
                out.push(t.timestamp());
            }
            out
        }
    }
}

/// A point in time in the summary (time of day for short spans).
fn when(span: Span, t: i64) -> String {
    let d = local(t);
    match span {
        Span::H6 | Span::H24 => d.format("%H:%M").to_string(),
        _ => d.format("%d.%m. %H:%M").to_string(),
    }
}

fn num(v: f64) -> String {
    if v.abs() >= 100.0 {
        format!("{:.0}", v)
    } else if v.abs() >= 10.0 {
        format!("{:.1}", v)
    } else {
        format!("{:.2}", v)
    }
}

/// The unit of a control's values: `°`, `%`, ` kW` (from its format).
fn unit(app: &App, cid: Cid) -> String {
    let c = &app.house.ctrls[cid];
    if c.kind == Kind::Climate {
        return "°".into();
    }
    let Some(f) = &c.format else {
        return String::new();
    };
    let s = crate::tui::text::lox_format(f, 1.0);
    let t = s.trim_start_matches(|c: char| c.is_ascii_digit() || ".,-+".contains(c));
    if t.chars().count() > 6 {
        String::new()
    } else {
        t.to_string()
    }
}

/// Whether the series is power in kW (its energy is worth showing).
fn is_power(app: &App, cid: Cid) -> bool {
    let c = &app.house.ctrls[cid];
    c.stat_power
        && (matches!(c.kind, Kind::Meter | Kind::Efm | Kind::Charger)
            || c.format
                .as_deref()
                .is_some_and(|f| f.contains("kW") && !f.contains("kWh")))
}

/// A series' name, with its room when it differs from the chart's control.
fn series_name(app: &App, main: Cid, cid: Cid) -> String {
    let h = &app.house;
    let name = h.display_name(cid);
    match h.room_name(cid) {
        Some(r) if h.room_name(main) != Some(r) => format!("{} · {}", name, r),
        _ => name,
    }
}

/// Colors of the added series (the first series uses its gradient).
fn series_style(th: &Theme, i: usize) -> Style {
    match i % 4 {
        0 => th.s_info(),
        1 => th.s_warn(),
        2 => th.s_ok(),
        _ => th.s_crit(),
    }
}

struct Layer<'a> {
    cols: &'a [Option<Bucket>],
    /// `None`: the gradient by height
    style: Option<Style>,
}

/// Draw layers (later ones on top) as braille lines scaled to [lo, hi].
fn plot(buf: &mut Buffer, area: Rect, layers: &[Layer], lo: f64, hi: f64, grad: Grad, th: &Theme) {
    let (w, h) = (area.width as usize, area.height as usize);
    if w == 0 || h == 0 {
        return;
    }
    let dots = h * 4;
    let span = (hi - lo).max(1e-9);
    let y = |v: f64| (((v - lo) / span).clamp(0.0, 1.0) * (dots - 1) as f64).round() as usize;
    let mut bits = vec![0u32; w * h];
    let mut owner: Vec<Option<usize>> = vec![None; w * h];
    for (li, l) in layers.iter().enumerate() {
        let mut prev: Option<usize> = None;
        for (i, b) in l.cols.iter().enumerate().take(w * 2) {
            let Some(b) = b else {
                prev = None;
                continue;
            };
            let (mut a, mut z) = (y(b.lo), y(b.hi));
            if let Some(p) = prev {
                a = a.min(p);
                z = z.max(p);
            }
            prev = Some(y(b.avg));
            let (cx, col) = (i / 2, i % 2);
            for d in a..=z {
                let row = h - 1 - d / 4;
                let dy = 3 - d % 4;
                bits[row * w + cx] |= DOT[col][dy];
                owner[row * w + cx] = Some(li);
            }
        }
    }
    for row in 0..h {
        for cx in 0..w {
            let b = bits[row * w + cx];
            if b == 0 {
                continue;
            }
            let style = match owner[row * w + cx].and_then(|l| layers[l].style) {
                Some(s) => s,
                None => th.grad_style(grad, (h - row) as f64 / h as f64),
            };
            let sym = char::from_u32(0x2800 + b).unwrap_or(' ').to_string();
            cell(
                buf,
                area,
                area.x + cx as u16,
                area.y + row as u16,
                &sym,
                style,
            );
        }
    }
}

pub fn render(app: &App, body: Rect, buf: &mut Buffer, c: &ChartState) {
    let th = &app.th;
    let h = &app.house;
    let main = c.cids[0];
    let r = body;
    let mut nb = NotchBox::new()
        .title(Notch::new(h.display_name(main)))
        .hints(vec![
            Hint::new("1-5", "range"),
            Hint::new("←→", "period"),
            Hint::new("hl", "cursor"),
            Hint::new("c", "compare"),
            Hint::new("+-", "series"),
            Hint::new("y", "copy cmd"),
            Hint::new("Esc", "close"),
        ]);
    if let Some(room) = h.room_name(main) {
        nb = nb.title(Notch::new(room.to_string()));
    }
    for (i, s) in Span::ALL.iter().enumerate() {
        let key = char::from_digit(i as u32 + 1, 10).unwrap_or('1');
        nb = nb.meta(Notch::hot(s.label(), key).active(*s == c.span));
    }
    clear(buf, r, th.s_base());
    let inner = nb.focus(true).render(r, buf, th);
    if inner.width < 20 || inner.height < 8 {
        return;
    }
    let x = inner.x + 1;
    let w = inner.width.saturating_sub(2);

    // data per series (and the previous period of each when comparing)
    let data = |cid: Cid, prev: bool| app.charts.get(&c.key(cid, prev));
    let now = app.now as i64;
    let (from, to) = crate::tui::exec::chart_window(now, &c.key(main, false));
    let (from, to) = data(main, false).map_or((from, to), |d| (d.from, d.to));
    let dur = to - from;

    // header: the window and a legend
    let period = match c.span {
        Span::H6 | Span::H24 => format!(
            "{} → {}",
            local(from).format("%a %d.%m. %H:%M"),
            local(to).format("%a %d.%m. %H:%M")
        ),
        _ => format!(
            "{} → {}",
            local(from).format("%a %d.%m.%Y"),
            local(to).format("%a %d.%m.%Y")
        ),
    };
    let mut hx = put(
        buf,
        inner,
        x,
        inner.y,
        &period,
        th.s_text().add_modifier(Modifier::BOLD),
    );
    if c.back > 0 {
        hx = put(
            buf,
            inner,
            hx + 2,
            inner.y,
            &format!("{} back · . now", c.back),
            th.s_faint(),
        );
    }
    let _ = hx;

    // layout: plot, x axis, blank, one summary row per series, cursor row
    let nsum = c.cids.len() as u16;
    let reserved = 1 + 1 + 1 + nsum + 1;
    let ph = inner.height.saturating_sub(2 + reserved);
    let yl = 8u16; // y-axis labels
    let plot_r = Rect::new(x + yl, inner.y + 2, w.saturating_sub(yl), ph);
    app.ui.borrow_mut().chart_w = plot_r.width as usize;
    let n = plot_r.width as usize * 2;

    // bucket every series (and previous periods, shifted onto this window)
    let mut cols: Vec<(Cid, bool, Vec<Option<Bucket>>)> = Vec::new();
    for &cid in &c.cids {
        for prev in [false, true] {
            if prev && !c.compare {
                continue;
            }
            if let Some(d) = data(cid, prev) {
                let shift = if prev { dur } else { 0 };
                cols.push((
                    cid,
                    prev,
                    chart::buckets(
                        &d.series.points,
                        from - shift,
                        to - shift,
                        n,
                        !chart::is_binary(&d.series.points),
                    ),
                ));
            }
        }
    }
    let loaded = data(main, false).is_some_and(|d| !d.series.points.is_empty());
    if !loaded {
        let msg = if data(main, false).is_some()
            || app
                .polls
                .failing
                .contains(&crate::tui::app::PollKind::Chart(c.key(main, false)))
        {
            "no statistics for this period"
        } else {
            "loading statistics…"
        };
        common::empty(app, buf, plot_r, &[msg]);
    }
    let vals = cols
        .iter()
        .flat_map(|(_, _, v)| v.iter().flatten().flat_map(|b| [b.lo, b.hi]));
    let (lo, hi) = vals.fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), v| {
        (a.min(v), b.max(v))
    });
    if lo.is_finite() && ph >= 2 {
        let (lo, hi) = chart::axis(lo, hi);
        // y labels: top, middle, bottom
        for (row, v) in [
            (0, hi),
            (
                ph / 2,
                lo + (hi - lo) * (1.0 - (ph / 2) as f64 / (ph - 1).max(1) as f64),
            ),
            (ph - 1, lo),
        ] {
            put(
                buf,
                inner,
                x,
                plot_r.y + row,
                &crate::tui::text::rfit(&num(v), yl as usize - 2),
                th.s_faint(),
            );
            cell(buf, inner, x + yl - 1, plot_r.y + row, "┤", th.s_faint());
        }
        // previous periods first (faint), then added series, then the main one
        let mut layers: Vec<Layer> = Vec::new();
        for (_, prev, v) in &cols {
            if *prev {
                layers.push(Layer {
                    cols: v,
                    style: Some(th.s_faint()),
                });
            }
        }
        for (i, (cid, prev, v)) in cols.iter().enumerate().rev() {
            let _ = i;
            if *prev || *cid == main {
                continue;
            }
            let k = c.cids.iter().position(|x| x == cid).unwrap_or(1);
            layers.push(Layer {
                cols: v,
                style: Some(series_style(th, k - 1)),
            });
        }
        if let Some((_, _, v)) = cols.iter().find(|(cid, prev, _)| *cid == main && !prev) {
            layers.push(Layer {
                cols: v,
                style: None,
            });
        }
        plot(
            buf,
            plot_r,
            &layers,
            lo,
            hi,
            common::spark_grad(app, main),
            th,
        );
    }
    // x axis: ticks on whole hours / days / Mondays / months
    let ay = plot_r.y + ph;
    let pw = plot_r.width as usize;
    let mut last_end = plot_r.x;
    for t in ticks(c.span, from, to) {
        let cx = ((t - from) as f64 / dur.max(1) as f64 * pw as f64) as usize;
        if cx >= pw {
            continue;
        }
        cell(buf, inner, plot_r.x + cx as u16, ay, "┬", th.s_faint());
        let label = tick(c.span, t);
        let lw = width(&label) as u16;
        let lx = (plot_r.x + cx as u16)
            .saturating_sub(lw / 2)
            .clamp(plot_r.x, plot_r.right().saturating_sub(lw));
        if lx >= last_end {
            put(buf, inner, lx, ay + 1, &label, th.s_dim());
            last_end = lx + lw + 1;
        }
    }
    // cursor column
    let cur = c.cursor.map(|cx| cx.min(pw.saturating_sub(1)));
    if let Some(cx) = cur {
        let sel = th.s_selected();
        for row in 0..ph {
            let p = (plot_r.x + cx as u16, plot_r.y + row);
            if let Some(cl) = buf.cell_mut(p) {
                cl.set_style(sel);
            }
        }
        cell(buf, inner, plot_r.x + cx as u16, ay, "▲", th.s_accent());
    }

    // summaries
    let mut sy = ay + 3;
    for (i, &cid) in c.cids.iter().enumerate() {
        let style = if i == 0 {
            th.grad_style(common::spark_grad(app, cid), 0.8)
        } else {
            series_style(th, i - 1)
        };
        let un = unit(app, cid);
        let u = |v: f64| format!("{}{}", num(v), un);
        let mut sx = put(buf, inner, x, sy, "━━ ", style);
        let name_w = 26.min(w as usize / 4);
        sx = put(
            buf,
            inner,
            sx,
            sy,
            &fit(&series_name(app, main, cid), name_w),
            th.s_text(),
        );
        let text = match data(cid, false) {
            None => "loading…".to_string(),
            Some(d) => summary_text(app, c.span, cid, d, data(cid, true), &u),
        };
        put(
            buf,
            inner,
            sx + 2,
            sy,
            &fit(&text, (x + w).saturating_sub(sx + 2) as usize),
            th.s_dim(),
        );
        sy += 1;
    }
    // cursor readout, or what the faint line is
    if let Some(cx) = cur {
        let t = from + (dur as f64 * (cx as f64 + 0.5) / pw.max(1) as f64) as i64;
        let mut parts = vec![local(t).format("%a %d.%m. %H:%M").to_string()];
        for &cid in &c.cids {
            let un = unit(app, cid);
            let at = |prev: bool| {
                cols.iter()
                    .find(|(x, p, _)| *x == cid && *p == prev)
                    .and_then(|(_, _, v)| {
                        let a = v.get(cx * 2).copied().flatten();
                        let b = v.get(cx * 2 + 1).copied().flatten();
                        match (a, b) {
                            (Some(a), Some(b)) => Some((a.avg + b.avg) / 2.0),
                            (Some(a), None) | (None, Some(a)) => Some(a.avg),
                            _ => None,
                        }
                    })
            };
            let mut s = match at(false) {
                Some(v) => format!("{}{}", num(v), un),
                None => "—".into(),
            };
            if c.compare
                && let Some(p) = at(true)
            {
                s.push_str(&format!(" (prev {}{})", num(p), un));
            }
            if c.cids.len() > 1 {
                s = format!("{} {}", series_name(app, main, cid), s);
            }
            parts.push(s);
        }
        put(
            buf,
            inner,
            x,
            sy,
            &fit(&format!("▲ {}", parts.join("  ·  ")), w as usize),
            th.s_accent(),
        );
    } else if c.compare {
        put(
            buf,
            inner,
            x,
            sy,
            &format!("┄┄ previous {} (faint)", c.span.label()),
            th.s_faint(),
        );
    }
}

/// `min 21.3° 03:12 · max 23.6° 14:02 · ⌀ 22.4° · last 23.6° · Δ⌀ +0.3° · 12.3 kWh`
fn summary_text(
    app: &App,
    span: Span,
    cid: Cid,
    d: &ChartData,
    prev: Option<&ChartData>,
    u: &dyn Fn(f64) -> String,
) -> String {
    let Some(s) = chart::summary(&d.series.points, d.from, d.to) else {
        return "no values in this period".into();
    };
    let mut out = format!(
        "min {} {} · max {} {} · ⌀ {} · last {}",
        u(s.min.1),
        when(span, s.min.0),
        u(s.max.1),
        when(span, s.max.0),
        u(s.avg),
        u(s.last.1)
    );
    if let Some(p) = prev
        && let Some(ps) = chart::summary(&p.series.points, p.from, p.to)
    {
        let delta = s.avg - ps.avg;
        out.push_str(&format!(
            " · Δ⌀ {}{}",
            if delta >= 0.0 { "+" } else { "" },
            u(delta)
        ));
    }
    if is_power(app, cid) {
        // power in kW: the energy of the period
        let kwh = chart::energy_kwh(&d.series.points, d.from, d.to, 3600);
        out.push_str(&format!(" · {}", fmt_kwh(kwh)));
    }
    out
}
