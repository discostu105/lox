//! The inspector: type-specific visual, history graph, raw states, CLI (§5.3).

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Modifier;

use super::common;
use crate::tui::app::{App, Hit};
use crate::tui::model::{Cid, Kind};
use crate::tui::store::{Val, fmt_val};
use crate::tui::text::{clean, fit, trunc};
use crate::tui::theme::{Grad, Theme};
use crate::tui::update::list_id;
use crate::tui::vm::{self, Verb};
use crate::tui::widgets::braille::{GraphOpts, graph};
use crate::tui::widgets::meter::meter;
use crate::tui::widgets::notchbox::{Notch, NotchBox};
use crate::tui::widgets::{cell, put};

/// Nearest-neighbour stretch of a short series to `n` points, so sparse
/// history (hourly statistics) spans the graph instead of hugging its right edge.
pub fn stretch(data: &[f64], n: usize) -> Vec<f64> {
    if data.len() >= n || data.len() < 2 {
        return data.to_vec();
    }
    (0..n).map(|i| data[i * data.len() / n]).collect()
}

/// Average `data` down to `n` points (keeps the shape of long series).
pub fn resample(data: &[f64], n: usize) -> Vec<f64> {
    if data.len() <= n || n == 0 {
        return data.to_vec();
    }
    let step = data.len() as f64 / n as f64;
    (0..n)
        .map(|i| {
            let a = (i as f64 * step) as usize;
            let b = (((i + 1) as f64 * step) as usize)
                .max(a + 1)
                .min(data.len());
            data[a..b].iter().sum::<f64>() / (b - a) as f64
        })
        .collect()
}

/// A braille graph scaled to the data's own range (so small variations show).
pub fn range_graph(
    buf: &mut Buffer,
    area: Rect,
    data: &[f64],
    grad: Grad,
    th: &Theme,
) -> Option<(f64, f64)> {
    let data: Vec<f64> = data.iter().copied().filter(|v| v.is_finite()).collect();
    if data.is_empty() || area.width == 0 {
        return None;
    }
    let d = resample(&data, area.width as usize * 2);
    let lo = d.iter().copied().fold(f64::INFINITY, f64::min);
    let hi = d.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let span = (hi - lo).max(1e-6);
    // leave a sliver at the bottom so the minimum is still visible
    let base = lo - span * 0.1;
    let shifted: Vec<f64> = d.iter().map(|v| v - base).collect();
    graph(
        buf,
        area,
        &shifted,
        hi - base,
        grad,
        &GraphOpts {
            floor: true,
            ..Default::default()
        },
        th,
    );
    Some((lo, hi))
}

/// `sel`: the selected state row when the inspector has focus (⏎ shows its full value).
pub fn render(app: &App, area: Rect, buf: &mut Buffer, cid: Cid, focus: bool, sel: Option<usize>) {
    let th = &app.th;
    let h = &app.house;
    let c = &h.ctrls[cid];
    let mut nb = NotchBox::new()
        .focus(focus)
        .title(Notch::new(h.display_name(cid)))
        .meta(Notch::new(c.typ.clone()));
    if focus {
        nb = nb.hints(common::item_hints(app, cid));
    }
    let inner = nb.render(area, buf, th);
    common::hit(app, inner, Hit::Pane(list_id::INSPECTOR));
    if inner.height < 3 {
        return;
    }
    let x = inner.x + 1;
    let w = inner.width.saturating_sub(2);
    let mut y = inner.y;
    let bottom = inner.bottom();
    // context line
    let ctx: Vec<&str> = [h.room_name(cid), h.cat_name(cid)]
        .into_iter()
        .flatten()
        .collect();
    put(
        buf,
        inner,
        x,
        y,
        &fit(&ctx.join(" · "), w as usize),
        th.s_dim(),
    );
    y += 2;

    // type-specific visual
    let v = vm::view(&app.store, h, cid);
    // label column: as wide as the longest sub-control name, up to a third
    let lw = c
        .subs
        .iter()
        .map(|s| crate::tui::text::width(&h.ctrls[*s].name) + 1)
        .max()
        .unwrap_or(0)
        .clamp(10, (w as usize / 3).max(10)) as u16;
    let big = |buf: &mut Buffer, y: u16, label: &str, frac: Option<f64>, grad: Grad, val: &str| {
        put(buf, inner, x, y, &fit(label, lw as usize - 1), th.s_dim());
        let mw = w.saturating_sub(lw + 12);
        if frac.is_some() && mw > 3 {
            cell(buf, inner, x + lw, y, "▕", th.s_faint());
            meter(buf, inner, x + lw + 1, y, mw, frac, grad, th);
            cell(buf, inner, x + lw + 1 + mw, y, "▏", th.s_faint());
        }
        put(
            buf,
            inner,
            x + w.saturating_sub(10),
            y,
            &crate::tui::text::rfit(val, 10),
            common::tone(th, v.tone),
        );
    };
    match c.kind {
        Kind::Blind | Kind::CentralBlind | Kind::Gate => {
            let pos = app.store.st(h, cid, "position");
            big(buf, y, "position", pos, Grad::Info, &v.value);
            y += 1;
            if let Some(sh) = app.store.st(h, cid, "shadePosition") {
                big(
                    buf,
                    y,
                    "slats",
                    Some(sh),
                    Grad::Info,
                    &format!("{:.0} %", sh * 100.0),
                );
                y += 1;
            }
            if !v.extra.is_empty() {
                put(
                    buf,
                    inner,
                    x,
                    y,
                    &fit(&v.extra, w as usize),
                    common::tone(th, v.extra_tone),
                );
                y += 1;
            }
        }
        Kind::LightCtl | Kind::CentralLight => {
            let act = vm::active_moods(&app.store, h, cid).unwrap_or_default();
            let moods = vm::mood_list(&app.store, h, cid);
            put(
                buf,
                inner,
                x,
                y,
                "MOODS",
                th.s_dim().add_modifier(Modifier::BOLD),
            );
            y += 1;
            let mut cx = x;
            for m in &moods {
                let on = act.contains(&(m.id as u32));
                let s = format!("{} {}", if on { "●" } else { "○" }, clean(&m.name));
                let sw = crate::tui::text::width(&s) as u16 + 2;
                if cx + sw > x + w {
                    y += 1;
                    cx = x;
                }
                if y >= bottom {
                    break;
                }
                put(
                    buf,
                    inner,
                    cx,
                    y,
                    &s,
                    if on { th.s_on() } else { th.s_dim() },
                );
                cx += sw;
            }
            y += 2;
            for &s in &c.subs {
                if y >= bottom {
                    break;
                }
                let sv = vm::view(&app.store, h, s);
                big(buf, y, &h.ctrls[s].name, sv.frac, Grad::Lamp, &sv.value);
                y += 1;
            }
        }
        Kind::Climate => {
            put(
                buf,
                inner,
                x,
                y,
                &v.value,
                th.s_text().add_modifier(Modifier::BOLD),
            );
            y += 1;
            if !v.extra.is_empty() {
                put(
                    buf,
                    inner,
                    x,
                    y,
                    &fit(&v.extra, w as usize),
                    common::tone(th, v.extra_tone),
                );
                y += 1;
            }
        }
        _ => {
            big(buf, y, "value", v.frac, v.grad, &v.value);
            y += 1;
            if !v.extra.is_empty() {
                put(
                    buf,
                    inner,
                    x,
                    y,
                    &fit(&v.extra, w as usize),
                    common::tone(th, v.extra_tone),
                );
                y += 1;
            }
        }
    }
    y += 1;

    // history: Miniserver statistics if available, else what this session has seen
    let (label, data) = match app.history.get(&cid) {
        Some(s) => (
            "24 h",
            stretch(
                &s.since(app.now as i64 - 86_400).values(),
                w.saturating_sub(8) as usize * 2,
            ),
        ),
        None => (
            "session",
            common::main_state(app, cid)
                .map(|u| app.store.series(&u))
                .unwrap_or_default(),
        ),
    };
    let gh = 4u16.min(bottom.saturating_sub(y + 6));
    if gh >= 2 && data.len() >= 2 {
        let g = Rect::new(x + 8, y, w.saturating_sub(8), gh);
        put(buf, inner, x, y, label, th.s_dim());
        if let Some((lo, hi)) = range_graph(buf, g, &data, common::spark_grad(app, cid), th) {
            put(buf, inner, x, y + 1, &trunc(&num(hi), 7), th.s_faint());
            put(buf, inner, x, y + gh - 1, &trunc(&num(lo), 7), th.s_faint());
        }
        if c.has_stats && gh >= 4 {
            let cx = put(buf, inner, x, y + 2, "c", th.s_accent());
            put(buf, inner, cx + 1, y + 2, "chart", th.s_faint());
        }
        y += gh + 1;
    } else if c.has_stats && !app.history.contains_key(&cid) {
        put(buf, inner, x, y, "statistics loading…", th.s_faint());
        y += 2;
    }

    // raw states
    if y + 2 < bottom {
        put(
            buf,
            inner,
            x,
            y,
            "STATES",
            th.s_dim().add_modifier(Modifier::BOLD),
        );
        y += 1;
        let cli_rows = 2;
        if sel.is_some() {
            let hint = "⏎ full value";
            put(
                buf,
                inner,
                (x + w).saturating_sub(crate::tui::text::width(hint) as u16),
                y - 1,
                hint,
                th.s_faint(),
            );
        }
        let states: Vec<(&String, &String)> = c.states.iter().collect();
        let vis = bottom.saturating_sub(y + cli_rows) as usize;
        let sel = sel.map(|s| s.min(states.len().saturating_sub(1)));
        let scroll = sel.map_or(0, |s| s.saturating_sub(vis.saturating_sub(1)));
        let kw = states
            .iter()
            .map(|(k, _)| k.len())
            .max()
            .unwrap_or(8)
            .min(18) as u16;
        for (i, (k, u)) in states.iter().enumerate().skip(scroll) {
            if y + cli_rows >= bottom {
                break;
            }
            if sel == Some(i) {
                crate::tui::widgets::fill(
                    buf,
                    Rect::new(inner.x, y, inner.width, 1),
                    th.s_selected(),
                );
                cell(buf, inner, inner.x, y, "▌", th.s_accent());
            }
            // text first: text states may also carry a (meaningless) number
            let val = match (app.store.text(u), app.store.num(u)) {
                (Some(t), _) => format!("\"{}\"", clean(t)),
                (None, Some(n)) => fmt_val(&Some(Val::Num(n))),
                _ => "—".into(),
            };
            put(buf, inner, x, y, &fit(k, kw as usize), th.s_dim());
            put(
                buf,
                inner,
                x + kw + 1,
                y,
                &fit(&val, w.saturating_sub(kw + 1) as usize),
                th.s_text(),
            );
            y += 1;
        }
    }
    // the equivalent CLI command
    let opt = app.pending.get(&cid).and_then(|p| p.optimistic);
    let cli = vm::verb(
        &app.store,
        h,
        cid,
        Verb::Primary,
        opt,
        app.last_dir.get(&cid).copied(),
    )
    .map(|p| p.action.to_cli(&c.name, h.room_name(cid)))
    .unwrap_or_else(|| format!("lox get {}", crate::actions::shell_quote(&c.name)));
    let cy = bottom - 1;
    put(
        buf,
        inner,
        x,
        cy,
        "y",
        th.s_accent().add_modifier(Modifier::BOLD),
    );
    put(
        buf,
        inner,
        x + 2,
        cy,
        &fit(&cli, w.saturating_sub(2) as usize),
        th.s_dim(),
    );
}

fn num(v: f64) -> String {
    if v.abs() >= 100.0 {
        format!("{:.0}", v)
    } else {
        format!("{:.1}", v)
    }
}
