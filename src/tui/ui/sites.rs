//! ⁶ Sites — every configured context at a glance (§5.7).

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Modifier;

use super::common;
use crate::tui::app::{App, Conn, Hit};
use crate::tui::text::{fit, rfit};
use crate::tui::theme::Grad;
use crate::tui::update::list_id;
use crate::tui::widgets::meter::meter;
use crate::tui::widgets::notchbox::{Hint, Notch, NotchBox};
use crate::tui::widgets::{cell, fill, put};

pub fn render(app: &App, area: Rect, buf: &mut Buffer) {
    let th = &app.th;
    let n = app.contexts.len();
    let mut nb = NotchBox::new()
        .focus(app.overlays.is_empty())
        .title(Notch::new("⁶sites"))
        .meta(Notch::new(format!("{} contexts", n)))
        .position(format!("{}/{}", (app.sites_ui.sel + 1).min(n), n));
    if app.overlays.is_empty() {
        nb = nb.hints(vec![Hint::new("⏎", "switch"), Hint::new("C", "contexts")]);
    }
    let inner = nb.render(area, buf, th);
    common::hit(app, inner, Hit::Pane(list_id::SITES));
    if n <= 1 {
        common::empty(
            app,
            buf,
            inner,
            &[
                "only one Miniserver configured",
                "add more with: lox ctx add <name> --host … --user …",
            ],
        );
        return;
    }
    let head = format!(
        "  {:<14} {:<26} {:<16} {:<11} {:>5} {:<12} NOTE",
        "CONTEXT", "HOST", "STATUS", "FW", "CPU", "HEAP"
    );
    put(
        buf,
        inner,
        inner.x + 1,
        inner.y,
        &fit(&head, inner.width as usize - 2),
        th.s_dim().add_modifier(Modifier::BOLD),
    );
    for (i, name) in app
        .contexts
        .iter()
        .enumerate()
        .take(inner.height.saturating_sub(1) as usize)
    {
        let y = inner.y + 1 + i as u16;
        let row = Rect::new(inner.x, y, inner.width, 1);
        common::hit(app, row, Hit::Row(list_id::SITES, i));
        if i == app.sites_ui.sel {
            fill(buf, row, th.s_selected());
            cell(buf, inner, inner.x, y, "▌", th.s_accent());
        }
        let active = *name == app.ctx_name;
        let mut x = inner.x + 3;
        put(
            buf,
            inner,
            x,
            y,
            &fit(name, 12),
            if active {
                th.s_accent().add_modifier(Modifier::BOLD)
            } else {
                th.s_text()
            },
        );
        if active {
            put(buf, inner, x + 13, y, "◆", th.s_accent());
        }
        x += 15;
        let site = app.sites.iter().find(|s| s.name == *name);
        let host = site.map(|s| s.host.as_str()).unwrap_or("");
        put(buf, inner, x, y, &fit(host, 26), th.s_dim());
        x += 27;
        let (status, st) = if active {
            match &app.conn {
                Conn::Live => ("● live".to_string(), th.s_ok()),
                Conn::Offline(_) => ("○ offline".to_string(), th.s_crit()),
                _ => ("◐ connecting".to_string(), th.s_warn()),
            }
        } else {
            match site {
                Some(s) if s.online => (format!("● up {}ms", s.latency_ms.unwrap_or(0)), th.s_ok()),
                Some(_) => ("○ unreachable".to_string(), th.s_crit()),
                None => ("… checking".to_string(), th.s_faint()),
            }
        };
        put(buf, inner, x, y, &fit(&status, 16), st);
        x += 17;
        let fw = site.and_then(|s| s.firmware.clone()).or_else(|| {
            if active {
                app.info.as_ref().map(|i| i.firmware.clone())
            } else {
                None
            }
        });
        put(
            buf,
            inner,
            x,
            y,
            &fit(fw.as_deref().unwrap_or("—"), 11),
            th.s_dim(),
        );
        x += 12;
        let (cpu, heap) = if active {
            let d = app.diag.as_ref().map(|(_, d)| d);
            (d.and_then(|d| d.cpu), d.and_then(|d| d.heap_pct()))
        } else {
            (site.and_then(|s| s.cpu), site.and_then(|s| s.heap_pct))
        };
        put(
            buf,
            inner,
            x,
            y,
            &rfit(
                &cpu.map(|c| format!("{:.0} %", c))
                    .unwrap_or_else(|| "—".into()),
                5,
            ),
            th.s_text(),
        );
        x += 6;
        meter(buf, inner, x, y, 6, heap.map(|h| h / 100.0), Grad::Load, th);
        put(
            buf,
            inner,
            x + 7,
            y,
            &heap.map(|h| format!("{:.0}%", h)).unwrap_or_default(),
            th.s_dim(),
        );
        x += 13;
        let note = site.and_then(|s| s.error.clone()).unwrap_or_default();
        put(
            buf,
            inner,
            x,
            y,
            &fit(&note, inner.right().saturating_sub(x + 1) as usize),
            th.s_warn(),
        );
    }
}
