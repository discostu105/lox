//! ⁵ System — overview, devices, bus & LAN, log, config history, update (§5.6).

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use super::common;
use super::inspector::resample;
use crate::tui::app::{App, Conn, Hit, SysView};
use crate::tui::data::{LogLevel, is_error_counter};
use crate::tui::keymap::{Cmd, Ctx};
use crate::tui::text::{fit, rfit};
use crate::tui::theme::Grad;
use crate::tui::update::{list_id, log_rows, sys_len};
use crate::tui::widgets::braille::{GraphOpts, graph};
use crate::tui::widgets::dotmeter::dotmeter;
use crate::tui::widgets::dotspark::dotspark;
use crate::tui::widgets::meter::meter;
use crate::tui::widgets::notchbox::{Hint, Notch, NotchBox};
use crate::tui::widgets::{cell, fill, put};

fn sel(app: &App, v: SysView) -> usize {
    app.system.sel.get(&v).copied().unwrap_or(0)
}

/// The pane frame with the sub-view tab strip in its title.
fn frame(
    app: &App,
    area: Rect,
    buf: &mut Buffer,
    meta: Option<Notch>,
    hints: Vec<Hint>,
    pos: Option<String>,
) -> Rect {
    let th = &app.th;
    let mut nb = NotchBox::new()
        .focus(app.overlays.is_empty() && app.system.pane == 0)
        .title(Notch::new("⁵system"));
    for v in SysView::ALL {
        nb = nb.title(Notch::new(v.title()).active(v == app.system.view));
    }
    if let Some(m) = meta {
        nb = nb.meta(m);
    }
    if let Some(p) = pos {
        nb = nb.position(p);
    }
    if app.overlays.is_empty() {
        let mut h = vec![Hint::new("[]", "view")];
        h.extend(hints);
        nb = nb.hints(h);
    }
    let inner = nb.render(area, buf, th);
    // sub-tab hit areas: recompute the notch positions (title notches start at x+2)
    let mut x = area.x + 2 + crate::tui::text::width("⁵system") as u16 + 2;
    for (i, v) in SysView::ALL.iter().enumerate() {
        let w = crate::tui::text::width(v.title()) as u16 + 2;
        common::hit(app, Rect::new(x, area.y, w, 1), Hit::SubTab(i));
        x += w;
    }
    common::hit(app, inner, Hit::Pane(list_id::SYS_LIST));
    inner
}

fn stale(app: &App, t: f64, every: f64) -> Option<Notch> {
    common::age(app, t, every).map(|a| Notch::new(a).styled(app.th.s_warn()))
}

pub fn render(app: &App, area: Rect, buf: &mut Buffer) {
    match app.system.view {
        SysView::Overview => overview(app, area, buf),
        SysView::Devices => devices(app, area, buf),
        SysView::BusLan => buslan(app, area, buf),
        SysView::Log => log(app, area, buf),
        SysView::Config => config(app, area, buf),
        SysView::Update => update(app, area, buf),
    }
}

// ── Overview ────────────────────────────────────────────────────────────────

fn overview(app: &App, area: Rect, buf: &mut Buffer) {
    let th = &app.th;
    let meta = match &app.diag {
        Some((t, _)) => stale(app, *t, 2.0).or(Some(Notch::new("poll 2 s"))),
        None => Some(Notch::new("loading…")),
    };
    let inner = frame(app, area, buf, meta, Vec::new(), None);
    let graph_h = (inner.height.saturating_sub(10)).clamp(4, 22);
    let inset_w = 44u16.min(inner.width / 2);
    let g = Rect::new(inner.x, inner.y, inner.width - inset_w - 1, graph_h);
    // CPU graph: btop's cpu box
    let cpu: Vec<f64> = app.diag_hist.iter().map(|x| x.0).collect();
    if cpu.is_empty() {
        common::empty(app, buf, g, &["collecting CPU samples…"]);
    } else {
        graph(
            buf,
            Rect::new(g.x + 4, g.y, g.width.saturating_sub(4), g.height),
            &cpu,
            100.0,
            Grad::Load,
            &GraphOpts {
                floor: true,
                ..Default::default()
            },
            th,
        );
        put(buf, g, g.x, g.y, "100", th.s_faint());
        put(buf, g, g.x, g.y + g.height / 2, " 50", th.s_faint());
        put(buf, g, g.x, g.bottom() - 1, "  0", th.s_faint());
        let secs = cpu.len() as f64 * 2.0;
        put(
            buf,
            g,
            g.x + 5,
            g.bottom() - 1,
            &format!("cpu · last {}", crate::tui::text::fmt_age(secs as u64)),
            th.s_faint(),
        );
    }
    // inset metrics box
    let ir = Rect::new(
        g.right() + 1,
        inner.y,
        inset_w,
        graph_h.max(9).min(inner.height),
    );
    let ii = NotchBox::new()
        .title(Notch::new("miniserver"))
        .render(ir, buf, th);
    let d = app
        .diag
        .as_ref()
        .map(|(_, d)| d.clone())
        .unwrap_or_default();
    let heap: Vec<f64> = app.diag_hist.iter().map(|x| x.1).collect();
    let rows: [(&str, Option<f64>, String, Option<&[f64]>, Grad); 2] = [
        (
            "cpu",
            d.cpu,
            d.cpu
                .map(|v| format!("{:.0} %", v))
                .unwrap_or_else(|| "—".into()),
            Some(&cpu),
            Grad::Load,
        ),
        (
            "heap",
            d.heap_pct(),
            match (d.heap_used_kb, d.heap_total_kb) {
                (Some(u), Some(t)) if t >= 100.0 * 1024.0 => {
                    format!("{:.0}/{:.0} MB", u / 1024.0, t / 1024.0)
                }
                (Some(u), Some(t)) => format!("{:.1}/{:.1} MB", u / 1024.0, t / 1024.0),
                _ => "—".into(),
            },
            Some(&heap),
            Grad::Load,
        ),
    ];
    let mut y = ii.y;
    let x = ii.x + 1;
    for (label, pct, val, spark, gr) in rows {
        put(buf, ii, x, y, label, th.s_dim());
        meter(buf, ii, x + 6, y, 10, pct.map(|p| p / 100.0), gr, th);
        put(buf, ii, x + 17, y, &rfit(&val, 13), th.s_text());
        if let Some(s) = spark
            && s.len() >= 2
        {
            dotspark(
                buf,
                ii,
                x + 31,
                y,
                ii.width.saturating_sub(33),
                &resample(s, 40),
                Some((0.0, 100.0)),
                gr,
                th,
            );
        }
        y += 1;
    }
    let dash = |v: Option<f64>| {
        v.map(|x| {
            if x >= 1000.0 {
                format!("{:.1} k", x / 1000.0)
            } else {
                format!("{:.0}", x)
            }
        })
        .unwrap_or_else(|| "—".into())
    };
    for (label, v) in [
        ("tasks", d.tasks),
        ("ctx/s", d.ctx_switches),
        ("ints/s", d.ints),
        ("comints", d.comints),
    ] {
        if y >= ii.bottom() {
            break;
        }
        // Gen 2 firmware doesn't report some counters: leave them out once polled
        if v.is_none() && app.diag.is_some() {
            continue;
        }
        put(buf, ii, x, y, label, th.s_dim());
        put(
            buf,
            ii,
            x + 8,
            y,
            &dash(v),
            if v.is_some() {
                th.s_text()
            } else {
                th.s_faint()
            },
        );
        y += 1;
    }
    if y < ii.bottom() {
        put(buf, ii, x, y, "SD", th.s_dim());
        let (s, st) = match &d.sd {
            Some(_) if d.sd_error() => ("✗ errors", th.s_crit()),
            Some(_) => ("✓ ok", th.s_ok()),
            None => ("—", th.s_faint()),
        };
        put(buf, ii, x + 8, y, s, st);
    }
    // info + devices summary
    let by = inner.y + graph_h.max(ir.height);
    if by + 3 > inner.bottom() {
        return;
    }
    let br = Rect::new(inner.x, by, inner.width, inner.bottom() - by);
    let half = br.width / 2;
    let ni = NotchBox::new().title(Notch::new("network")).render(
        Rect::new(br.x, br.y, half, br.height),
        buf,
        th,
    );
    if let Some(i) = &app.info {
        let lines = [
            format!(
                "{} · fw {}",
                if i.ms_type.is_empty() {
                    "Miniserver"
                } else {
                    &i.ms_type
                },
                i.firmware
            ),
            format!("IP {} / {}  GW {}", i.ip, i.mask, i.gateway),
            format!("DNS {}   MAC {}", i.dns.join(" · "), i.mac),
            format!(
                "DHCP {}   NTP {}",
                i.dhcp.map_or("—", |b| if b { "on" } else { "off" }),
                i.ntp.map_or("—", |b| if b { "✓" } else { "off" })
            ),
        ];
        for (k, l) in lines.iter().enumerate() {
            put(
                buf,
                ni,
                ni.x + 1,
                ni.y + k as u16,
                &fit(l, ni.width.saturating_sub(2) as usize),
                if k == 0 { th.s_text() } else { th.s_dim() },
            );
        }
    } else {
        common::empty(app, buf, ni, &["loading…"]);
    }
    let di = NotchBox::new().title(Notch::new("devices")).render(
        Rect::new(br.x + half, br.y, br.width - half, br.height),
        buf,
        th,
    );
    match &app.devices {
        Some((_, ds)) => {
            let bad: Vec<_> = ds.iter().filter(|d| d.problem() > 0).collect();
            let head = format!(
                "{} devices · {} online",
                ds.len(),
                ds.iter().filter(|d| d.online).count()
            );
            put(buf, di, di.x + 1, di.y, &head, th.s_text());
            if bad.is_empty() {
                put(buf, di, di.x + 1, di.y + 1, "✓ no problems", th.s_ok());
            }
            for (k, d) in bad
                .iter()
                .enumerate()
                .take(di.height.saturating_sub(1) as usize)
            {
                let (g, st, why) = match d.problem() {
                    3 => ("○", th.s_crit(), "offline".to_string()),
                    2 => (
                        "⚠",
                        th.s_warn(),
                        format!("battery {} %", d.battery.unwrap_or(0)),
                    ),
                    _ => ("◔", th.s_warn(), "weak signal".to_string()),
                };
                put(buf, di, di.x + 1, di.y + 1 + k as u16, g, st);
                put(
                    buf,
                    di,
                    di.x + 3,
                    di.y + 1 + k as u16,
                    &fit(
                        &format!("{} · {}", d.name, why),
                        di.width.saturating_sub(4) as usize,
                    ),
                    th.s_text(),
                );
            }
        }
        None => common::empty(app, buf, di, &["loading…"]),
    }
}

// ── Devices ─────────────────────────────────────────────────────────────────

fn devices(app: &App, area: Rect, buf: &mut Buffer) {
    let th = &app.th;
    let len = sys_len(app, SysView::Devices);
    let s = sel(app, SysView::Devices);
    let meta = app.devices.as_ref().and_then(|(t, _)| stale(app, *t, 10.0));
    let inner = frame(
        app,
        area,
        buf,
        meta,
        Vec::new(),
        Some(format!("{}/{}", (s + 1).min(len), len)),
    );
    let Some((_, ds)) = &app.devices else {
        common::empty(app, buf, inner, &["loading devices…"]);
        return;
    };
    if ds.is_empty() {
        common::empty(app, buf, inner, &["no Tree / Air devices"]);
        return;
    }
    let nw = ((inner.width as usize).saturating_sub(64)).clamp(14, 32);
    let head = format!(
        "  {:<nw$} {:<14} {:<16} {:<9} {:<6} {:<10} LAST SEEN",
        "DEVICE",
        "TYPE",
        "PLACE",
        "BATTERY",
        "SIGNAL",
        "FIRMWARE",
        nw = nw
    );
    put(
        buf,
        inner,
        inner.x + 1,
        inner.y,
        &fit(&head, inner.width as usize - 2),
        th.s_dim().add_modifier(Modifier::BOLD),
    );
    let body = Rect::new(inner.x, inner.y + 1, inner.width, inner.height - 1);
    let hgt = body.height as usize;
    common::set_page(app, hgt);
    let off = common::offset(app, list_id::SYS_LIST, s, ds.len(), hgt);
    for (i, d) in ds.iter().enumerate().skip(off).take(hgt) {
        let y = body.y + (i - off) as u16;
        let row = Rect::new(body.x, y, body.width, 1);
        common::hit(app, row, Hit::Row(list_id::SYS_LIST, i));
        if i == s {
            fill(buf, row, th.s_selected());
            cell(buf, body, body.x, y, "▌", th.s_accent());
        }
        let mut x = body.x + 1;
        let (g, st) = match d.problem() {
            3 => ("○", th.s_crit()),
            2 | 1 => ("●", th.s_warn()),
            _ => ("●", th.s_ok()),
        };
        put(buf, body, x, y, g, st);
        x += 2;
        put(buf, body, x, y, &fit(&d.name, nw), th.s_text());
        x += nw as u16 + 1;
        put(buf, body, x, y, &fit(&d.kind, 14), th.s_dim());
        x += 15;
        put(
            buf,
            body,
            x,
            y,
            &fit(d.place.as_deref().unwrap_or("—"), 16),
            th.s_dim(),
        );
        x += 17;
        match d.battery {
            Some(b) => {
                dotmeter(buf, body, x, y, 4, Some(b as f64 / 100.0), Grad::Batt, th);
                put(
                    buf,
                    body,
                    x + 5,
                    y,
                    &format!("{:>3}", b),
                    if b < 20 { th.s_warn() } else { th.s_dim() },
                );
            }
            None => {
                put(buf, body, x, y, "—", th.s_faint());
            }
        }
        x += 10;
        let bars = ["▁", "▂", "▄", "▆", "█"];
        match d.signal {
            Some(q) => {
                for k in 0..4u8 {
                    let on = k < q;
                    cell(
                        buf,
                        body,
                        x + k as u16,
                        y,
                        bars[k as usize + 1],
                        if on {
                            if q <= 1 { th.s_warn() } else { th.s_ok() }
                        } else {
                            th.s_faint()
                        },
                    );
                }
            }
            None => {
                put(buf, body, x, y, "—", th.s_faint());
            }
        }
        x += 7;
        put(
            buf,
            body,
            x,
            y,
            &fit(d.firmware.as_deref().unwrap_or("—"), 10),
            th.s_dim(),
        );
        x += 11;
        let last = if d.online {
            "online".to_string()
        } else {
            d.last_seen.clone().unwrap_or_else(|| "offline".into())
        };
        put(
            buf,
            body,
            x,
            y,
            &fit(&last, body.right().saturating_sub(x + 1) as usize),
            if d.online { th.s_faint() } else { th.s_crit() },
        );
    }
}

// ── Bus & LAN ───────────────────────────────────────────────────────────────

fn buslan(app: &App, area: Rect, buf: &mut Buffer) {
    let th = &app.th;
    let len = sys_len(app, SysView::BusLan);
    let s = sel(app, SysView::BusLan);
    let meta = match &app.buslan {
        Some((t, _)) => stale(app, *t, 5.0).or(Some(Notch::new("poll 5 s"))),
        None => None,
    };
    let inner = frame(
        app,
        area,
        buf,
        meta,
        Vec::new(),
        Some(format!("{}/{}", (s + 1).min(len), len)),
    );
    let Some((_, b)) = &app.buslan else {
        common::empty(app, buf, inner, &["loading counters…"]);
        return;
    };
    let head = format!("  {:<24} {:>14} {:>10}", "COUNTER", "TOTAL", "Δ 5 s");
    put(
        buf,
        inner,
        inner.x + 1,
        inner.y,
        &head,
        th.s_dim().add_modifier(Modifier::BOLD),
    );
    let body = Rect::new(inner.x, inner.y + 1, inner.width, inner.height - 1);
    let hgt = body.height as usize;
    common::set_page(app, hgt);
    let off = common::offset(app, list_id::SYS_LIST, s, b.counters.len(), hgt);
    for (i, (name, v)) in b.counters.iter().enumerate().skip(off).take(hgt) {
        let y = body.y + (i - off) as u16;
        let row = Rect::new(body.x, y, body.width, 1);
        common::hit(app, row, Hit::Row(list_id::SYS_LIST, i));
        if i == s {
            fill(buf, row, th.s_selected());
            cell(buf, body, body.x, y, "▌", th.s_accent());
        }
        let prev = app
            .buslan_prev
            .as_ref()
            .and_then(|p| p.counters.get(i))
            .and_then(|c| c.1);
        let delta = match (v, prev) {
            (Some(a), Some(b)) => Some(a.saturating_sub(b)),
            _ => None,
        };
        let err = is_error_counter(name);
        // non-zero error deltas flash red once, then stay amber
        let st: Style = match app.buslan_flash.get(name) {
            Some(t) if app.now - t < 6.0 => th.s_crit().add_modifier(Modifier::BOLD),
            Some(_) => th.s_warn(),
            None if err && v.is_some_and(|x| x > 0) => th.s_warn(),
            None => th.s_text(),
        };
        put(
            buf,
            body,
            body.x + 3,
            y,
            &fit(name, 24),
            if err { th.s_dim() } else { th.s_text() },
        );
        put(
            buf,
            body,
            body.x + 28,
            y,
            &rfit(&v.map(|x| x.to_string()).unwrap_or_else(|| "—".into()), 14),
            st,
        );
        let dt = delta
            .map(|d| {
                if d > 0 {
                    format!("+{}", d)
                } else {
                    "·".into()
                }
            })
            .unwrap_or_default();
        put(
            buf,
            body,
            body.x + 43,
            y,
            &rfit(&dt, 10),
            if delta.unwrap_or(0) > 0 && err {
                th.s_crit()
            } else {
                th.s_dim()
            },
        );
    }
}

// ── Log ─────────────────────────────────────────────────────────────────────

fn log(app: &App, area: Rect, buf: &mut Buffer) {
    let th = &app.th;
    let rows = log_rows(app);
    let s = sel(app, SysView::Log).min(rows.len().saturating_sub(1));
    let meta = if !app.system.log_search.is_empty() {
        Some(Notch::new(format!("/{} · {} hits", app.system.log_search, rows.len())).active(true))
    } else {
        app.log
            .as_ref()
            .and_then(|(t, _)| stale(app, *t, 30.0))
            .or(Some(Notch::new("def.log")))
    };
    let hints = common::ctx_hints(Ctx::Log, &[Cmd::Filter, Cmd::NextMatch]);
    let inner = frame(
        app,
        area,
        buf,
        meta,
        hints,
        Some(format!(
            "{}/{}",
            if rows.is_empty() { 0 } else { s + 1 },
            rows.len()
        )),
    );
    let Some((_, lines)) = &app.log else {
        common::empty(app, buf, inner, &["loading log…"]);
        return;
    };
    if rows.is_empty() {
        common::empty(
            app,
            buf,
            inner,
            &[if lines.is_empty() {
                "log is empty"
            } else {
                "no match"
            }],
        );
        return;
    }
    let hgt = inner.height as usize;
    common::set_page(app, hgt);
    let off = common::offset(app, list_id::SYS_LIST, s, rows.len(), hgt);
    for (k, &i) in rows.iter().enumerate().skip(off).take(hgt) {
        let l = &lines[i];
        let y = inner.y + (k - off) as u16;
        let row = Rect::new(inner.x, y, inner.width, 1);
        common::hit(app, row, Hit::Row(list_id::SYS_LIST, k));
        if k == s {
            fill(buf, row, th.s_selected());
            cell(buf, inner, inner.x, y, "▌", th.s_accent());
        }
        let st = match l.level {
            LogLevel::Error => th.s_crit(),
            LogLevel::Warning => th.s_warn(),
            LogLevel::Important => th.s_info(),
            LogLevel::Info => th.s_text(),
        };
        let t = l.time.get(5..19).unwrap_or(&l.time);
        put(buf, inner, inner.x + 1, y, &fit(t, 14), th.s_faint());
        common::put_name(
            buf,
            inner,
            inner.x + 16,
            y,
            &l.text,
            inner.width.saturating_sub(17) as usize,
            st,
            "",
            th,
        );
    }
}

// ── Config (gitops history) ─────────────────────────────────────────────────

fn config(app: &App, area: Rect, buf: &mut Buffer) {
    let th = &app.th;
    let s = sel(app, SysView::Config);
    let len = sys_len(app, SysView::Config);
    let mut hints = vec![Hint::new("Tab", "diff")];
    if !app.opts.read_only {
        hints.extend(common::ctx_hints(Ctx::Config, &[Cmd::Pull]));
    }
    let inner = frame(
        app,
        area,
        buf,
        Some(Notch::new("lox config pull history")),
        hints,
        Some(format!("{}/{}", (s + 1).min(len), len)),
    );
    let commits = match &app.commits {
        None => {
            common::empty(app, buf, inner, &["reading git history…"]);
            return;
        }
        Some(Err(e)) => {
            common::empty(
                app,
                buf,
                inner,
                &["no config history", e, "set it up with: lox config init"],
            );
            return;
        }
        Some(Ok(c)) if c.is_empty() => {
            common::empty(
                app,
                buf,
                inner,
                &["no commits yet", "P runs lox config pull"],
            );
            return;
        }
        Some(Ok(c)) => c,
    };
    let lw = (inner.width / 3).clamp(30, 50);
    let list = Rect::new(inner.x, inner.y, lw, inner.height);
    let hgt = list.height as usize;
    if app.system.pane == 0 {
        common::set_page(app, hgt);
    }
    let off = common::offset(app, list_id::SYS_LIST, s, commits.len(), hgt);
    for (i, c) in commits.iter().enumerate().skip(off).take(hgt) {
        let y = list.y + (i - off) as u16;
        let row = Rect::new(list.x, y, list.width, 1);
        common::hit(app, row, Hit::Row(list_id::SYS_LIST, i));
        if i == s {
            fill(
                buf,
                row,
                if app.system.pane == 0 {
                    th.s_selected()
                } else {
                    Style::default()
                },
            );
            cell(
                buf,
                list,
                list.x,
                y,
                if app.system.pane == 0 { "▌" } else { "▸" },
                th.s_accent(),
            );
        }
        let date = c.date.get(..16).unwrap_or(&c.date);
        put(buf, list, list.x + 1, y, date, th.s_faint());
        put(
            buf,
            list,
            list.x + 18,
            y,
            &fit(&c.subject, lw.saturating_sub(19) as usize),
            th.s_text(),
        );
    }
    for y in inner.y..inner.bottom() {
        cell(buf, inner, list.right(), y, "│", th.s_border(false));
    }
    let dr = Rect::new(
        list.right() + 1,
        inner.y,
        inner.right() - list.right() - 1,
        inner.height,
    );
    common::hit(app, dr, Hit::Pane(list_id::SYS_DIFF));
    let Some(c) = commits.get(s) else { return };
    let Some(diff) = app.diffs.get(&c.hash) else {
        common::empty(app, buf, dr, &["loading diff…"]);
        return;
    };
    if diff.is_empty() {
        common::empty(
            app,
            buf,
            dr,
            &["no semantic changes", "(layout or metadata only)"],
        );
        return;
    }
    let hgt = dr.height as usize;
    if app.system.pane == 1 {
        common::set_page(app, hgt);
    }
    let off = app.system.diff_scroll.min(diff.len().saturating_sub(hgt));
    for (k, l) in diff.iter().enumerate().skip(off).take(hgt) {
        let y = dr.y + (k - off) as u16;
        let st = match l.chars().next() {
            Some('+') => th.s_ok(),
            Some('-') => th.s_crit(),
            Some('~') => th.s_warn(),
            _ => th.s_dim(),
        };
        let w = dr.width.saturating_sub(2) as usize;
        // word-level emphasis: the new value after the last arrow
        match l.rfind(" → ") {
            Some(p) if l.starts_with('~') => {
                let x = put(
                    buf,
                    dr,
                    dr.x + 1,
                    y,
                    &crate::tui::text::trunc(&l[..p + " → ".len()], w),
                    st,
                );
                let rest = w.saturating_sub((x - dr.x - 1) as usize);
                put(
                    buf,
                    dr,
                    x,
                    y,
                    &fit(&l[p + " → ".len()..], rest),
                    th.s_text().add_modifier(Modifier::BOLD),
                );
            }
            _ => {
                put(buf, dr, dr.x + 1, y, &fit(l, w), st);
            }
        }
    }
}

// ── Update ──────────────────────────────────────────────────────────────────

fn update(app: &App, area: Rect, buf: &mut Buffer) {
    let th = &app.th;
    let hints = if app.opts.read_only {
        Vec::new()
    } else {
        common::ctx_hints(Ctx::Update, &[Cmd::Install, Cmd::Reboot])
    };
    let inner = frame(app, area, buf, None, hints, None);
    let x = inner.x + 2;
    let mut y = inner.y + 1;
    let (fw, typ) = app
        .info
        .as_ref()
        .map_or(("—".to_string(), String::new()), |i| {
            (i.firmware.clone(), i.ms_type.clone())
        });
    put(buf, inner, x, y, "firmware", th.s_dim());
    put(
        buf,
        inner,
        x + 12,
        y,
        &fw,
        th.s_text().add_modifier(Modifier::BOLD),
    );
    y += 1;
    if !typ.is_empty() {
        put(buf, inner, x, y, "model", th.s_dim());
        put(buf, inner, x + 12, y, &typ, th.s_text());
        y += 1;
    }
    y += 1;
    let (s, st) = match &app.conn {
        Conn::OutOfService => (
            "⏻ out of service — the Miniserver is restarting or updating",
            th.s_warn(),
        ),
        Conn::Reconnecting { .. } => ("◐ reconnecting…", th.s_warn()),
        Conn::Offline(_) => ("○ offline", th.s_crit()),
        _ => ("● running", th.s_ok()),
    };
    put(buf, inner, x, y, s, st);
    y += 2;
    let lines = [
        "U  install the latest release (lox update install)",
        "R  reboot the Miniserver (lox reboot)",
        "",
        "Both ask you to type the context name. The TUI stays open and",
        "reconnects on its own when the Miniserver is back.",
        "Release notes: https://www.loxone.com/enen/support/downloads/",
    ];
    for l in lines {
        if y >= inner.bottom() {
            break;
        }
        put(
            buf,
            inner,
            x,
            y,
            &fit(l, inner.width.saturating_sub(4) as usize),
            if app.opts.read_only {
                th.s_faint()
            } else {
                th.s_dim()
            },
        );
        y += 1;
    }
    if app.opts.read_only && y + 1 < inner.bottom() {
        put(
            buf,
            inner,
            x,
            y + 1,
            "read-only: install and reboot are disabled",
            th.s_warn(),
        );
    }
}
