//! ⁵ System — overview, devices, bus & LAN, log, config history, update (§5.6).

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use super::common;
use crate::tui::app::{App, Conn, Hit, SysView};
use crate::tui::data::{DiagSample, LogLevel, NetSample, is_error_counter};
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

/// Step-hold `pts` (time, value) onto `n` columns spanning [from, to];
/// columns before the first sample are 0.
fn timeline(pts: &[(f64, f64)], from: f64, to: f64, n: usize) -> Vec<f64> {
    let mut out = Vec::with_capacity(n);
    let mut k = 0;
    let mut cur = None;
    for i in 0..n {
        let t = from + (i as f64 + 1.0) * (to - from) / n as f64;
        while k < pts.len() && pts[k].0 <= t {
            cur = Some(pts[k].1);
            k += 1;
        }
        out.push(cur.unwrap_or(0.0));
    }
    out
}

/// "−5 min", "−30 s"
fn ago(secs: f64) -> String {
    if secs >= 120.0 {
        format!("−{:.0} min", secs / 60.0)
    } else {
        format!("−{:.0} s", secs)
    }
}

fn overview(app: &App, area: Rect, buf: &mut Buffer) {
    let th = &app.th;
    let meta = match &app.diag {
        Some((t, _)) => stale(app, *t, 2.0).or(Some(Notch::new("poll 2 s"))),
        None => Some(Notch::new("loading…")),
    };
    let inner = frame(app, area, buf, meta, Vec::new(), None);
    // the graph takes under half the height: the panes below carry details
    let graph_h = (inner.height * 9 / 20).clamp(8, 20).min(inner.height);
    let inset_w = 46u16.min(inner.width / 2);
    let g = Rect::new(inner.x, inner.y, inner.width - inset_w - 1, graph_h);
    let hist: Vec<_> = app.diag_hist.iter().copied().collect();
    // the window grows with the history, up to 10 minutes, so it is never
    // a speck at the right edge
    let span = hist.first().map_or(0.0, |s| app.now - s.t);
    let win = ((span / 60.0).ceil() * 60.0).clamp(60.0, 600.0);
    let from = app.now - win;
    let series = |f: &dyn Fn(&DiagSample) -> Option<f64>| -> Vec<(f64, f64)> {
        hist.iter().filter_map(|s| f(s).map(|v| (s.t, v))).collect()
    };
    let cpu_pts = series(&|s| Some(s.cpu));
    if hist.is_empty() {
        common::empty(app, buf, g, &["collecting CPU samples…"]);
    } else {
        let gr = Rect::new(g.x + 4, g.y, g.width.saturating_sub(4), g.height - 1);
        graph(
            buf,
            gr,
            &timeline(&cpu_pts, from, app.now, gr.width as usize * 2),
            100.0,
            Grad::Load,
            &GraphOpts {
                floor: true,
                ..Default::default()
            },
            th,
        );
        put(buf, g, g.x, g.y, "100", th.s_faint());
        put(buf, g, g.x, g.y + (g.height - 1) / 2, " 50", th.s_faint());
        put(buf, g, g.x, g.bottom() - 2, "  0", th.s_faint());
        // time axis
        let y = g.bottom() - 1;
        put(buf, g, gr.x, y, &ago(win), th.s_faint());
        let mid = ago(win / 2.0);
        put(
            buf,
            g,
            gr.x + gr.width / 2 - mid.chars().count() as u16 / 2,
            y,
            &mid,
            th.s_faint(),
        );
        put(buf, g, gr.right().saturating_sub(3), y, "now", th.s_faint());
        let d = app.diag.as_ref().map(|(_, d)| d);
        let head = format!(
            "cpu {}",
            d.and_then(|d| d.cpu)
                .map_or("—".into(), |c| format!("{:.0} %", c))
        );
        put(
            buf,
            g,
            gr.x + 1,
            g.y,
            &head,
            th.s_text().add_modifier(Modifier::BOLD),
        );
    }
    // inset metrics box
    let ir = Rect::new(
        g.right() + 1,
        inner.y,
        inset_w,
        graph_h.max(12).min(inner.height),
    );
    let ii = NotchBox::new()
        .title(Notch::new("miniserver"))
        .render(ir, buf, th);
    let d = app
        .diag
        .as_ref()
        .map(|(_, d)| d.clone())
        .unwrap_or_default();
    let spark_w = ii.width.saturating_sub(33);
    let spark = |pts: &[(f64, f64)]| timeline(pts, from, app.now, spark_w as usize * 2);
    let plc_pts = series(&|s| s.plc);
    let heap_pts = series(&|s| s.heap);
    let rows: [(&str, Option<f64>, String, &[(f64, f64)]); 3] = [
        (
            "cpu",
            d.cpu,
            d.cpu
                .map(|v| format!("{:.0} %", v))
                .unwrap_or_else(|| "—".into()),
            &cpu_pts,
        ),
        // the share of the CPU the PLC program takes
        (
            "plc",
            d.sps,
            d.sps
                .map(|v| format!("{:.0} %", v))
                .unwrap_or_else(|| "—".into()),
            &plc_pts,
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
            &heap_pts,
        ),
    ];
    let mut y = ii.y;
    let x = ii.x + 1;
    for (label, pct, val, pts) in rows {
        if pct.is_none() && label == "plc" && app.diag.is_some() {
            continue;
        }
        put(buf, ii, x, y, label, th.s_dim());
        meter(
            buf,
            ii,
            x + 6,
            y,
            10,
            pct.map(|p| p / 100.0),
            Grad::Load,
            th,
        );
        put(buf, ii, x + 17, y, &rfit(&val, 13), th.s_text());
        if pts.len() >= 2 {
            dotspark(
                buf,
                ii,
                x + 31,
                y,
                spark_w,
                &spark(pts),
                Some((0.0, 100.0)),
                Grad::Load,
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
    let mut line = |y: &mut u16, label: &str, text: &str, st: Style| {
        if *y < ii.bottom() {
            put(buf, ii, x, *y, label, th.s_dim());
            put(
                buf,
                ii,
                x + 8,
                *y,
                &fit(text, ii.width.saturating_sub(10) as usize),
                st,
            );
            *y += 1;
        }
    };
    for (label, v) in [
        ("tasks", d.tasks),
        ("ctx/s", d.ctx_switches),
        ("ints/s", d.ints),
        ("comints", d.comints),
    ] {
        // Gen 2 firmware doesn't report some counters: leave them out once polled
        if v.is_none() && app.diag.is_some() {
            continue;
        }
        let st = if v.is_some() {
            th.s_text()
        } else {
            th.s_faint()
        };
        line(&mut y, label, &dash(v), st);
    }
    if app.diag.is_some() {
        // PLC: is the program running, and how fast does it cycle
        if let Some(run) = d.plc_running() {
            let text = match (run, d.plc_rate()) {
                (true, Some(r)) => format!("● running · {:.0} cycles/s", r),
                (true, None) => "● running".into(),
                (false, _) => format!("○ {}", d.plc.clone().unwrap_or_default().to_lowercase()),
            };
            line(
                &mut y,
                "program",
                &text,
                if run { th.s_ok() } else { th.s_crit() },
            );
        }
        // clock: a drifting Miniserver clock skews timers, logs and statistics
        if let Some(dr) = d.clock_drift {
            let (text, st) = if dr.abs() < 3.0 {
                ("✓ in sync".to_string(), th.s_ok())
            } else {
                let amount = if dr.abs() >= 120.0 {
                    format!("{:.0} min", dr.abs() / 60.0)
                } else {
                    format!("{:.0} s", dr.abs())
                };
                (
                    format!("⚠ {} {}", amount, if dr > 0.0 { "fast" } else { "slow" }),
                    if dr.abs() >= 60.0 {
                        th.s_crit()
                    } else {
                        th.s_warn()
                    },
                )
            };
            line(&mut y, "clock", &text, st);
        }
        let (sd, st) = match &d.sd {
            Some(_) if d.sd_error() => ("✗ errors".to_string(), th.s_crit()),
            Some(_) => {
                let mbs = |k: &str| d.sd_num(k).map(|v| format!("{:.1}", v / 1024.0));
                match (mbs("Read:"), mbs("Write:")) {
                    (Some(r), Some(w)) => (format!("✓ ok · r {} w {} MB/s", r, w), th.s_ok()),
                    _ => ("✓ ok".into(), th.s_ok()),
                }
            }
            None => ("—".into(), th.s_faint()),
        };
        line(&mut y, "SD", &sd, st);
        // SD wear: worn-out cards are a classic Miniserver failure
        if let Some(used) = d.sd_num("Used:") {
            let st = if used >= 90.0 {
                th.s_crit()
            } else if used >= 70.0 {
                th.s_warn()
            } else {
                th.s_text()
            };
            let mut t = format!("{:.0} % used", used);
            if let Some(n) = d.sd_num("PowerOnCycles:") {
                t.push_str(&format!(" · {:.0} power-ons", n));
            }
            line(&mut y, "SD life", &t, st);
        }
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
        net_rates(app, buf, ni, ni.y + lines.len() as u16 + 1);
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
            let mut parts = vec![(
                format!(
                    "{} devices · {} online",
                    ds.len(),
                    ds.iter().filter(|d| d.online).count()
                ),
                th.s_text(),
            )];
            for (p, what, st) in [
                (3, "offline", th.s_crit()),
                (2, "low battery", th.s_warn()),
                (1, "weak signal", th.s_warn()),
            ] {
                let n = bad.iter().filter(|d| d.problem() == p).count();
                if n > 0 {
                    parts.push((format!(" · {} {}", n, what), st));
                }
            }
            let mut hx = di.x + 1;
            for (t, st) in parts {
                put(buf, di, hx, di.y, &t, st);
                hx += t.chars().count() as u16;
            }
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

/// Live LAN / CAN packet rates and new error counts (from the bus & LAN poll).
fn net_rates(app: &App, buf: &mut Buffer, area: Rect, y: u16) {
    let th = &app.th;
    let x = area.x + 1;
    if y >= area.bottom() {
        return;
    }
    let Some(last) = app.net_hist.back() else {
        put(buf, area, x, y, "LAN   measuring…", th.s_faint());
        return;
    };
    let rate = |v: Option<f64>| {
        v.map_or("—".into(), |r| {
            if r >= 1000.0 {
                format!("{:.1} k/s", r / 1000.0)
            } else {
                format!("{:.0}/s", r)
            }
        })
    };
    type Pick = fn(&NetSample) -> Option<f64>;
    let rows: [(&str, Option<f64>, Option<f64>, Grad, Pick); 2] = [
        ("LAN", last.lan_rx, last.lan_tx, Grad::Grid, |s| s.lan_rx),
        ("CAN", last.can_rx, last.can_tx, Grad::Pv, |s| s.can_rx),
    ];
    let mut y = y;
    for (label, rx, tx, gr, pick) in rows {
        if y >= area.bottom() || (rx.is_none() && tx.is_none()) {
            continue;
        }
        put(buf, area, x, y, label, th.s_dim());
        let t = format!("↓ {:>8}  ↑ {:>8}", rate(rx), rate(tx));
        put(buf, area, x + 6, y, &t, th.s_text());
        let sx = x + 6 + t.chars().count() as u16 + 2;
        let pts: Vec<f64> = app.net_hist.iter().filter_map(pick).collect();
        if pts.len() >= 2 && sx + 8 < area.right() {
            let w = (area.right() - sx - 1).min(40);
            dotspark(
                buf,
                area,
                sx,
                y,
                w,
                &super::inspector::stretch(&pts, w as usize * 2),
                None,
                gr,
                th,
            );
        }
        y += 1;
    }
    if y < area.bottom() {
        let lan: u64 = app.net_hist.iter().map(|s| s.lan_err).sum();
        let can: u64 = app.net_hist.iter().map(|s| s.can_err).sum();
        let dropped: u64 = app.net_hist.iter().map(|s| s.lan_drop).sum();
        let (mut t, st) = if lan + can == 0 {
            ("✓ no new bus or LAN errors".to_string(), th.s_ok())
        } else {
            (
                format!("⚠ new errors: LAN +{} · CAN +{}  (bus & lan tab)", lan, can),
                th.s_warn(),
            )
        };
        if dropped > 0 {
            t.push_str(&format!(" · {} rx dropped (no buffer)", dropped));
        }
        put(
            buf,
            area,
            x,
            y,
            &fit(&t, area.width.saturating_sub(2) as usize),
            st,
        );
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

/// What `P` does and how the last pull went (bottom of the Config list).
fn pull_status(app: &App, newest: Option<&crate::tui::app::Commit>) -> (String, Style) {
    let th = &app.th;
    match &app.pull {
        None => (
            "P pulls the newest backup from the Miniserver".into(),
            th.s_dim(),
        ),
        Some((t, None)) => (
            format!(
                "{} pulling… downloading the newest backup (FTP) · {:.0} s",
                spinner(app),
                app.now - t
            ),
            th.s_info(),
        ),
        Some((t, Some(Ok(true)))) => (
            format!("✓ {} new config committed", clock(app, *t)),
            th.s_ok(),
        ),
        Some((t, Some(Ok(false)))) => (
            format!(
                "✓ {} up to date · last save {}",
                clock(app, *t),
                newest
                    .and_then(|c| c.saved.clone())
                    .unwrap_or_else(|| "the last pull".into())
            ),
            th.s_ok(),
        ),
        Some((_, Some(Err(e)))) => (format!("✗ pull failed: {}", e), th.s_crit()),
    }
}

fn spinner(app: &App) -> &'static str {
    const F: [&str; 8] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧"];
    F[((app.now * 8.0) as usize) % F.len()]
}

/// Wall-clock HH:MM of an app time.
fn clock(app: &App, t: f64) -> String {
    let unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
        - (app.now - t);
    chrono::DateTime::from_timestamp(unix as i64, 0)
        .map(|d| d.with_timezone(&chrono::Local).format("%H:%M").to_string())
        .unwrap_or_default()
}

fn config(app: &App, area: Rect, buf: &mut Buffer) {
    let th = &app.th;
    let s = sel(app, SysView::Config);
    let len = sys_len(app, SysView::Config);
    let mut hints = vec![Hint::new("Tab", "diff")];
    // pull only reads from the Miniserver: offered read-only too
    hints.push(Hint::new("P", "pull from Miniserver"));
    let inner = frame(
        app,
        area,
        buf,
        Some(Notch::new("config history")),
        hints,
        Some(format!("{}/{}", (s + 1).min(len), len)),
    );
    let explain = [
        "Loxone Config writes a backup to the Miniserver's SD card on every save.",
        "`lox config pull` (P) downloads the newest one and commits it to git when it changed.",
    ];
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
                &[
                    "no config history",
                    e,
                    "set it up with: lox config init <dir>",
                    explain[0],
                ],
            );
            return;
        }
        Some(Ok(c)) if c.is_empty() => {
            let (st, _) = pull_status(app, None);
            common::empty(
                app,
                buf,
                inner,
                &["no commits yet", &st, explain[0], explain[1]],
            );
            return;
        }
        Some(Ok(c)) => c,
    };
    let lw = (inner.width * 2 / 5).clamp(36, 64);
    // list above, pull status + explanation below
    let foot = if inner.height >= 12 { 3 } else { 1 };
    let list = Rect::new(inner.x, inner.y, lw, inner.height.saturating_sub(foot));
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
        // when it was saved in Loxone Config, not when it was pulled
        let date = c
            .saved
            .clone()
            .unwrap_or_else(|| c.date.get(..16).unwrap_or(&c.date).to_string());
        let mut x = put(buf, list, list.x + 1, y, &date, th.s_faint());
        if let Some(v) = &c.version {
            x = put(buf, list, x + 1, y, &rfit(v, 4), th.s_dim());
        }
        // the semantic diff's totals once computed; the pull-time summary until then
        let diff = app.diffs.get(&c.hash);
        let changes: Vec<&String> = diff
            .into_iter()
            .flatten()
            .filter(|l| !l.starts_with("= ") && !l.starts_with("# "))
            .collect();
        // a single change reads better than its count
        let single = (changes.len() == 1)
            .then(|| changes[0].split_whitespace().collect::<Vec<_>>().join(" "));
        let totals = single.as_deref().or_else(|| {
            diff.and_then(|d| d.first())
                .and_then(|l| l.strip_prefix("= "))
        });
        let (text, st) = if let Some(t) = totals {
            let st = match t.chars().next() {
                Some('+') if single.is_some() => th.s_ok(),
                Some('-') if single.is_some() => th.s_crit(),
                _ => th.s_text(),
            };
            (t, st)
        } else if app.diffs.get(&c.hash).is_some_and(|d| d.is_empty()) {
            ("no logic changes", th.s_dim())
        } else if c.summary.is_empty() {
            (c.subject.as_str(), th.s_text())
        } else {
            let st = match c.summary.chars().next() {
                Some('+') => th.s_ok(),
                Some('-') => th.s_crit(),
                Some('n') => th.s_dim(),
                _ => th.s_text(),
            };
            (c.summary.as_str(), st)
        };
        put(
            buf,
            list,
            x + 2,
            y,
            &fit(text, list.right().saturating_sub(x + 3) as usize),
            st,
        );
    }
    // footer under the list: pull status, then what pulling means
    let (pst, pstyle) = pull_status(app, commits.first());
    let fy = list.bottom();
    let fw = lw.saturating_sub(2) as usize;
    if foot == 3 {
        for x in inner.x..inner.x + lw {
            cell(buf, inner, x, fy, "─", th.s_border(false));
        }
        put(buf, inner, inner.x + 1, fy + 1, &fit(&pst, fw), pstyle);
        put(
            buf,
            inner,
            inner.x + 1,
            fy + 2,
            &fit("backups are written when you save in Loxone Config", fw),
            th.s_faint(),
        );
    } else {
        put(buf, inner, inner.x + 1, fy, &fit(&pst, fw), pstyle);
    }
    for y in inner.y..inner.bottom() {
        cell(buf, inner, inner.x + lw, y, "│", th.s_border(false));
    }
    let dr = Rect::new(
        inner.x + lw + 1,
        inner.y,
        inner.right() - inner.x - lw - 1,
        inner.height,
    );
    common::hit(app, dr, Hit::Pane(list_id::SYS_DIFF));
    let Some(c) = commits.get(s) else { return };
    // header: what this commit is
    let mut head = Vec::new();
    if let Some(d) = &c.saved {
        head.push(format!("saved {}", d));
    }
    if let Some(v) = &c.version {
        head.push(v.clone());
    }
    head.push(format!("pulled {}", c.date.get(..16).unwrap_or(&c.date)));
    head.push(c.hash.get(..7).unwrap_or(&c.hash).to_string());
    put(
        buf,
        dr,
        dr.x + 1,
        dr.y,
        &fit(&head.join(" · "), dr.width.saturating_sub(2) as usize),
        th.s_faint(),
    );
    let body = Rect::new(dr.x, dr.y + 2, dr.width, dr.height.saturating_sub(2));
    let Some(diff) = app.diffs.get(&c.hash) else {
        if let Some(e) = app.diff_errs.get(&c.hash) {
            common::empty(
                app,
                buf,
                body,
                &["couldn't compare this config", e, "retrying every 30 s"],
            );
        } else {
            common::empty(
                app,
                buf,
                body,
                &[&format!(
                    "{} comparing with the previous backup…",
                    spinner(app)
                )],
            );
        }
        return;
    };
    if diff.is_empty() {
        common::empty(
            app,
            buf,
            body,
            &[
                "no changes to blocks, wires or parameters",
                "(layout or metadata only)",
            ],
        );
        return;
    }
    let hgt = body.height as usize;
    if app.system.pane == 1 {
        common::set_page(app, hgt);
    }
    let off = app.system.diff_scroll.min(diff.len().saturating_sub(hgt));
    let w = body.width.saturating_sub(2) as usize;
    for (k, l) in diff.iter().enumerate().skip(off).take(hgt) {
        let y = body.y + (k - off) as u16;
        if let Some(t) = l.strip_prefix("= ") {
            put(
                buf,
                body,
                body.x + 1,
                y,
                &fit(t, w),
                th.s_text().add_modifier(Modifier::BOLD),
            );
            continue;
        }
        if let Some(t) = l.strip_prefix("# ") {
            put(
                buf,
                body,
                body.x + 1,
                y,
                &fit(&format!("▸ {}", t), w),
                th.s_accent().add_modifier(Modifier::BOLD),
            );
            continue;
        }
        let recreated = l.contains("re-created");
        let st = match l.chars().next() {
            _ if recreated => th.s_dim(),
            Some('+') => th.s_ok(),
            Some('-') => th.s_crit(),
            Some('~') => th.s_warn(),
            _ => th.s_dim(),
        };
        // word-level emphasis: the new value after the last arrow
        match l.rfind(" → ") {
            Some(p) if l.starts_with('~') && !recreated => {
                let x = put(
                    buf,
                    body,
                    body.x + 3,
                    y,
                    &crate::tui::text::trunc(&l[..p + " → ".len()], w.saturating_sub(2)),
                    st,
                );
                let rest = w.saturating_sub((x - body.x - 1) as usize);
                put(
                    buf,
                    body,
                    x,
                    y,
                    &fit(&l[p + " → ".len()..], rest),
                    th.s_text().add_modifier(Modifier::BOLD),
                );
            }
            _ => {
                put(buf, body, body.x + 3, y, &fit(l, w.saturating_sub(2)), st);
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
