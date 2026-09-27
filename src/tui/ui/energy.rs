//! ⁴ Energy — flow diagram, today's mirrored graph, meters table (§5.5).

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use super::common;
use super::inspector::resample;
use crate::tui::app::{App, ERange, Hit, PollKind};
use crate::tui::data;
use crate::tui::keymap::{Cmd, Ctx};
use crate::tui::model::{Cid, EnergyNode, Kind, Role};
use crate::tui::text::{fit, fmt_kw, fmt_kwh, rfit};
use crate::tui::theme::Grad;
use crate::tui::update::list_id;
use crate::tui::widgets::braille::{GraphOpts, graph};
use crate::tui::widgets::dotmeter::dotmeter;
use crate::tui::widgets::dotspark::dotspark;
use crate::tui::widgets::flow::{hflow, vflow};
use crate::tui::widgets::meter::meter;
use crate::tui::widgets::notchbox::{Hint, Notch, NotchBox};
use crate::tui::widgets::{cell, put};

/// Current power flows (kW). Grid: + import / − export. Battery: + charging.
#[derive(Debug, Default, Clone)]
pub struct Flows {
    pub pv: Option<f64>,
    pub grid: Option<f64>,
    pub batt: Option<f64>,
    pub soc: Option<f64>,
    pub home: Option<f64>,
    /// (name, kW) of chargers / wallboxes
    pub chargers: Vec<(String, f64)>,
    /// State UUIDs for sparklines
    pub pv_uuid: Option<String>,
    pub grid_uuid: Option<String>,
    pub home_uuid: Option<String>,
}

/// Power (kW) of a node: the EFM node state if the Miniserver has sent it, else
/// the node's meter. (Real EFMs only send `actualN` on change, so an idle grid
/// or battery may never report there.)
fn node_power<'a>(app: &'a App, n: &'a EnergyNode) -> Option<(f64, &'a str)> {
    let h = &app.house;
    let meter = n.ctrl.and_then(|c| h.ctrls[c].state("actual"));
    [n.power_state.as_deref(), meter]
        .into_iter()
        .flatten()
        .find_map(|u| app.store.num(u).map(|v| (v, u)))
}

fn sum(app: &App, role: Role) -> (Option<f64>, Option<String>) {
    let h = &app.house;
    let mut total = None;
    let mut first = None;
    for n in h.energy.by_role(role) {
        if let Some((v, u)) = node_power(app, n) {
            *total.get_or_insert(0.0) += v;
            first.get_or_insert_with(|| u.to_string());
        } else if let Some(u) = n.power_uuid(h) {
            first.get_or_insert_with(|| u.to_string());
        }
    }
    // last resort: the EFM's aggregate outputs
    if total.is_none()
        && let Some(efm) = h.energy.efm
    {
        let st = match role {
            Role::Grid => "Gpwr",
            Role::Production => "Ppwr",
            Role::Storage => "Spwr",
            Role::Load => "",
        };
        if let Some(u) = h.ctrls[efm].state(st)
            && let Some(v) = app.store.num(u)
        {
            total = Some(v);
            first.get_or_insert_with(|| u.to_string());
        }
    }
    (total, first)
}

pub fn flows(app: &App) -> Flows {
    let h = &app.house;
    let (pv, pv_uuid) = sum(app, Role::Production);
    let (grid, grid_uuid) = sum(app, Role::Grid);
    let (batt, _) = sum(app, Role::Storage);
    let soc = h
        .energy
        .by_role(Role::Storage)
        .find_map(|n| n.ctrl.and_then(|c| app.store.st(h, c, "storage")))
        .or_else(|| h.energy.efm.and_then(|c| app.store.st(h, c, "Ssoc")));
    // Home: the EFM's Load node if it carries power, else production + grid − storage
    let (load, home_uuid) = sum(app, Role::Load);
    let home = load.or_else(|| {
        if pv.is_none() && grid.is_none() {
            None
        } else {
            Some(pv.unwrap_or(0.0) + grid.unwrap_or(0.0) - batt.unwrap_or(0.0))
        }
    });
    let chargers = h
        .top_level()
        .filter(|c| h.ctrls[*c].kind == Kind::Charger)
        .filter_map(|c| {
            let p = app
                .store
                .st(h, c, "power")
                .or_else(|| app.store.st(h, c, "actualPower"))?;
            Some((h.ctrls[c].name.clone(), p))
        })
        .collect();
    Flows {
        pv,
        grid,
        batt,
        soc,
        home,
        chargers,
        pv_uuid,
        grid_uuid,
        home_uuid,
    }
}

pub fn render(app: &App, area: Rect, buf: &mut Buffer) {
    let th = &app.th;
    let h = &app.house;
    if h.energy.nodes.is_empty() && h.energy.meters.is_empty() {
        let inner = NotchBox::new()
            .title(Notch::new("⁴energy"))
            .render(area, buf, th);
        common::empty(
            app,
            buf,
            inner,
            &[
                "no energy meters in this installation",
                "add Meter or Energy Flow Monitor blocks in Loxone Config",
            ],
        );
        return;
    }
    let has_flow = !h.energy.nodes.is_empty();
    // 13 rows fit two rows of node boxes with a flow line between them
    let flow_h = if !has_flow {
        0
    } else if area.height >= 34 {
        13
    } else {
        12u16.min(area.height / 2)
    };
    if has_flow {
        flow_pane(app, Rect::new(area.x, area.y, area.width, flow_h), buf);
    }
    let rest = Rect::new(area.x, area.y + flow_h, area.width, area.height - flow_h);
    let lw = if area.width >= 120 {
        rest.width * 5 / 11
    } else {
        rest.width / 2
    };
    today(app, Rect::new(rest.x, rest.y, lw, rest.height), buf);
    meters(
        app,
        Rect::new(rest.x + lw, rest.y, rest.width - lw, rest.height),
        buf,
    );
}

fn node_box(
    app: &App,
    buf: &mut Buffer,
    area: Rect,
    x: u16,
    y: u16,
    glyph: &str,
    title: &str,
    value: &str,
    sub: Option<(&str, Style)>,
    meter_frac: Option<(f64, Grad)>,
) {
    let th = &app.th;
    let w = 16u16;
    let r = Rect::new(x, y, w, 5).intersection(area);
    if r.width < 6 || r.height < 3 {
        return;
    }
    let nb = NotchBox::new();
    let inner = nb.render(r, buf, th);
    put(
        buf,
        inner,
        inner.x + 1,
        inner.y,
        &format!("{} {}", glyph, fit(title, 11)),
        th.s_text().add_modifier(Modifier::BOLD),
    );
    put(buf, inner, inner.x + 1, inner.y + 1, value, th.s_text());
    if let Some((s, st)) = sub {
        put(buf, inner, inner.x + 1, inner.y + 2, s, st);
    } else if let Some((f, g)) = meter_frac {
        dotmeter(
            buf,
            inner,
            inner.x + 1,
            inner.y + 2,
            inner.width.saturating_sub(2),
            Some(f),
            g,
            th,
        );
    }
}

fn flow_pane(app: &App, area: Rect, buf: &mut Buffer) {
    let th = &app.th;
    let f = flows(app);
    let mut nb = NotchBox::new()
        .title(Notch::new("⁴energy"))
        .title(Notch::new("now").active(true));
    if let Some(pv) = f.pv
        && pv > 0.05
    {
        // self-use: share of production not exported
        let export = f.grid.map(|g| (-g).max(0.0)).unwrap_or(0.0);
        let self_use = (((pv - export) / pv) * 100.0).clamp(0.0, 100.0);
        nb = nb.meta(Notch::new(format!("self-use {:.0}%", self_use)));
    }
    let inner = nb.render(area, buf, th);
    common::hit(app, inner, Hit::Pane(list_id::HOME_ENERGY));
    if inner.height < 7 {
        return;
    }
    let phase = if app.opts.motion {
        app.now - app.started
    } else {
        0.0
    };
    let gap = ((inner.width.saturating_sub(3 * 16 + 4)) / 2).clamp(6, 30);
    // centered when the pane is wider than the diagram
    let x0 = inner.x + 2 + inner.width.saturating_sub(3 * 16 + 2 * gap + 4) / 2;
    let xh = x0 + 16 + gap;
    let xg = xh + 16 + gap;
    let y0 = inner.y;
    let kw = |v: Option<f64>| v.map(fmt_kw).unwrap_or_else(|| "—".into());
    // PV → Home
    node_box(
        app,
        buf,
        inner,
        x0,
        y0,
        "☼",
        "PV",
        &kw(f.pv),
        None,
        f.pv.map(|p| ((p / 10.0).clamp(0.0, 1.0), Grad::Pv)),
    );
    hflow(
        buf,
        inner,
        x0 + 16,
        y0 + 2,
        gap,
        f.pv.filter(|p| *p > 0.01),
        phase,
        th.s_color(th.pv),
        th,
    );
    node_box(
        app,
        buf,
        inner,
        xh,
        y0,
        "⌂",
        "Home",
        &kw(f.home),
        None,
        f.home.map(|p| ((p / 12.0).clamp(0.0, 1.0), Grad::Use)),
    );
    // Grid ↔ Home: flows toward home when importing
    hflow(
        buf,
        inner,
        xh + 16,
        y0 + 2,
        gap,
        f.grid.map(|g| -g),
        phase,
        th.s_color(th.grid),
        th,
    );
    let gsub = match f.grid {
        Some(g) if g < -0.01 => Some(("exporting", th.s_ok())),
        Some(g) if g > 0.01 => Some(("importing", th.s_warn())),
        Some(_) => Some(("idle", th.s_dim())),
        None => None,
    };
    node_box(
        app,
        buf,
        inner,
        xg,
        y0,
        "≋",
        "Grid",
        &kw(f.grid),
        gsub,
        None,
    );
    // Battery below PV
    let y1 = y0 + 5 + 1;
    if h_has(app, Role::Storage) && y1 + 3 <= inner.bottom() {
        vflow(
            buf,
            inner,
            x0 + 7,
            y0 + 5,
            1,
            f.batt,
            phase,
            th.s_color(th.batt),
            th,
        );
        let bval = match (f.soc, f.batt) {
            (Some(s), Some(b)) => format!(
                "{:.0} % {}",
                s,
                if b > 0.01 {
                    "▲"
                } else if b < -0.01 {
                    "▼"
                } else {
                    ""
                }
            ),
            (Some(s), None) => format!("{:.0} %", s),
            (None, b) => kw(b),
        };
        node_box(
            app,
            buf,
            inner,
            x0,
            y1,
            "▮",
            "Battery",
            &bval,
            None,
            f.soc.map(|s| ((s / 100.0).clamp(0.0, 1.0), Grad::Batt)),
        );
        if let Some(b) = f.batt {
            put(buf, inner, x0 + 9, y0 + 5, &fmt_kw(b.abs()), th.s_dim());
        }
    }
    // Chargers below Home
    if let Some((name, p)) = f.chargers.first()
        && y1 + 3 <= inner.bottom()
    {
        vflow(
            buf,
            inner,
            xh + 7,
            y0 + 5,
            1,
            Some(*p),
            phase,
            th.s_color(th.load),
            th,
        );
        let sub = if *p > 0.05 {
            ("charging", th.s_on())
        } else {
            ("idle", th.s_dim())
        };
        node_box(
            app,
            buf,
            inner,
            xh,
            y1,
            "⏚",
            name,
            &fmt_kw(*p),
            Some(sub),
            None,
        );
    }
    // Right side: sparklines of the last hours (session history)
    let sx = xg + 18;
    if sx + 20 < inner.right() {
        let sw = inner.right() - sx - 2;
        let rows: [(&str, Option<&String>, Grad); 3] = [
            ("PV", f.pv_uuid.as_ref(), Grad::Pv),
            ("Home", f.home_uuid.as_ref(), Grad::Use),
            ("Grid", f.grid_uuid.as_ref(), Grad::Grid),
        ];
        let mut y = y0 + 6;
        for (label, u, g) in rows {
            let Some(u) = u else { continue };
            let data = app.store.series(u);
            if data.len() < 2 || y >= inner.bottom() {
                continue;
            }
            put(buf, inner, sx, y, &fit(label, 5), th.s_dim());
            dotspark(
                buf,
                inner,
                sx + 5,
                y,
                sw.saturating_sub(5),
                &data,
                None,
                g,
                th,
            );
            y += 1;
        }
    }
}

fn h_has(app: &App, role: Role) -> bool {
    app.house.energy.by_role(role).next().is_some()
}

/// Today's quarter-hours for a role, summed over its meters, from session history.
fn session_qh(app: &App, role: Role) -> Option<Vec<f64>> {
    let h = &app.house;
    let now = app.now as i64;
    let mut out: Option<Vec<f64>> = None;
    for n in h.energy.by_role(role) {
        let Some(u) = n.power_uuid(h) else { continue };
        let Some(hist) = app.store.hist.get(u) else {
            continue;
        };
        let pts: Vec<(i64, f64)> = hist.iter().map(|(t, v)| (*t as i64, *v)).collect();
        let q = data::quarter_hours(&pts, app.tz, now);
        let acc = out.get_or_insert_with(|| vec![f64::NAN; 96]);
        for (a, b) in acc.iter_mut().zip(q) {
            if b.is_finite() {
                *a = if a.is_finite() { *a + b } else { b };
            }
        }
    }
    out
}

/// Statistics when the Miniserver has them, else what this session has seen.
fn day_series(app: &App) -> Option<(Vec<f64>, Vec<f64>, bool)> {
    if let Some((pv, usage)) = &app.energy_day
        && (pv.iter().any(|v| v.is_finite()) || usage.iter().any(|v| v.is_finite()))
    {
        return Some((pv.clone(), usage.clone(), false));
    }
    let pv = session_qh(app, Role::Production);
    let usage = session_qh(app, Role::Load).or_else(|| {
        let grid = session_qh(app, Role::Grid)?;
        let st = session_qh(app, Role::Storage);
        let pv = pv.clone();
        Some(
            (0..96)
                .map(|i| {
                    let p = pv
                        .as_ref()
                        .map_or(0.0, |p| if p[i].is_finite() { p[i] } else { 0.0 });
                    let s = st
                        .as_ref()
                        .map_or(0.0, |s| if s[i].is_finite() { s[i] } else { 0.0 });
                    if grid[i].is_finite() {
                        p + grid[i] - s
                    } else {
                        f64::NAN
                    }
                })
                .collect(),
        )
    });
    if pv.is_none() && usage.is_none() {
        return None;
    }
    Some((
        pv.unwrap_or_else(|| vec![f64::NAN; 96]),
        usage.unwrap_or_else(|| vec![f64::NAN; 96]),
        true,
    ))
}

fn today(app: &App, area: Rect, buf: &mut Buffer) {
    let th = &app.th;
    let day = day_series(app);
    let now_q = (app.local_secs() as f64 / 900.0) as usize;
    let kwh = |d: &[f64]| {
        d.iter()
            .take(now_q + 1)
            .filter(|v| v.is_finite())
            .sum::<f64>()
            / 4.0
    };
    let mut nb = NotchBox::new().title(Notch::new("today"));
    if let Some((pv, usage, session)) = &day {
        nb = nb.meta(Notch::new(format!(
            "☼ {} · ⌂ {}",
            fmt_kwh(kwh(pv)),
            fmt_kwh(kwh(usage))
        )));
        if *session {
            nb = nb.title(Notch::new("this session").styled(th.s_faint()));
        }
    }
    nb = nb.bottom_right(Notch::new("☼ pv ▲  ⌂ use ▼ kW"));
    let inner = nb.render(area, buf, th);
    let Some((pv, usage, _)) = &day else {
        let msg: &[&str] = if app
            .polls
            .inflight
            .keys()
            .any(|k| matches!(k, PollKind::EnergyDay))
        {
            &["loading statistics…"]
        } else {
            &[
                "no statistics for today",
                "the graph fills from live values while lox tui runs",
            ]
        };
        common::empty(app, buf, inner, msg);
        return;
    };
    if inner.height < 4 || inner.width < 12 {
        return;
    }
    let gx = inner.x + 4;
    let gw = inner.width.saturating_sub(5);
    let n = gw as usize * 2;
    // scale the 96 quarter-hours of the day onto the graph width
    let spread = |d: &[f64]| -> Vec<f64> {
        (0..n)
            .map(|i| {
                let k = i * 96 / n;
                d.get(k).copied().unwrap_or(f64::NAN)
            })
            .collect()
    };
    let now_i = (now_q * n / 96).min(n.saturating_sub(1));
    let pv_s = spread(pv);
    let use_s: Vec<f64> = spread(usage)
        .into_iter()
        .enumerate()
        .map(|(i, v)| if i > now_i { f64::NAN } else { v })
        .collect();
    let max = pv_s
        .iter()
        .chain(use_s.iter())
        .copied()
        .filter(|v| v.is_finite())
        .fold(1.0, f64::max);
    let gh = inner.height.saturating_sub(1);
    let up_h = gh / 2;
    let down_h = gh - up_h;
    let up = Rect::new(gx, inner.y, gw, up_h);
    let down = Rect::new(gx, inner.y + up_h, gw, down_h);
    graph(
        buf,
        up,
        &pv_s,
        max,
        Grad::Pv,
        &GraphOpts {
            floor: true,
            faint_from: Some(now_i + 1),
            marker: Some(now_i),
            ..Default::default()
        },
        th,
    );
    graph(
        buf,
        down,
        &use_s,
        max,
        Grad::Use,
        &GraphOpts {
            down: true,
            marker: Some(now_i),
            ..Default::default()
        },
        th,
    );
    put(
        buf,
        inner,
        inner.x,
        inner.y,
        &rfit(&format!("{:.0}", max), 3),
        th.s_faint(),
    );
    put(
        buf,
        inner,
        inner.x,
        inner.y + up_h,
        &rfit("0", 3),
        th.s_faint(),
    );
    put(
        buf,
        inner,
        inner.x,
        inner.y + gh - 1,
        &rfit(&format!("{:.0}", max), 3),
        th.s_faint(),
    );
    // hour axis
    let ay = inner.bottom() - 1;
    for hh in (0..=24).step_by(if gw > 60 { 3 } else { 6 }) {
        let x = gx + (hh as u16 * gw / 24).min(gw.saturating_sub(2));
        put(buf, inner, x, ay, &format!("{:02}", hh), th.s_faint());
    }
}

fn meters(app: &App, area: Rect, buf: &mut Buffer) {
    let th = &app.th;
    let h = &app.house;
    let ms = &h.energy.meters;
    let focus = app.overlays.is_empty();
    let mut nb = NotchBox::new().focus(focus).title(Notch::new("meters"));
    for r in ERange::ALL {
        nb = nb.title(Notch::new(r.title()).active(r == app.energy.range));
    }
    nb = nb.position(format!(
        "{}/{}",
        (app.energy.sel_meter + 1).min(ms.len()),
        ms.len()
    ));
    if focus {
        let mut hints = vec![Hint::new("[]", "range")];
        if let Some(c) = ms.get(app.energy.sel_meter) {
            hints.extend(
                common::item_hints(app, *c)
                    .into_iter()
                    .filter(|h| h.key == "⏎" || h.key == "w"),
            );
        }
        hints.extend(common::ctx_hints(Ctx::Item, &[Cmd::Pin]));
        nb = nb.hints(hints);
    }
    let inner = nb.render(area, buf, th);
    common::hit(app, inner, Hit::Pane(list_id::METERS));
    if ms.is_empty() {
        common::empty(app, buf, inner, &["no meters"]);
        return;
    }
    let hgt = inner.height as usize;
    if focus {
        common::set_page(app, hgt);
    }
    let off = common::offset(app, list_id::METERS, app.energy.sel_meter, ms.len(), hgt);
    let max = ms
        .iter()
        .filter_map(|c| app.store.st(h, *c, "actual"))
        .map(f64::abs)
        .fold(1.0, f64::max);
    let total_state = match app.energy.range {
        ERange::Now | ERange::Today => "totalDay",
        ERange::Week => "totalWeek",
        ERange::Month => "totalMonth",
    };
    let nw = ((inner.width as usize).saturating_sub(40)).clamp(10, 24);
    for (i, &c) in ms.iter().enumerate().skip(off).take(hgt) {
        let y = inner.y + (i - off) as u16;
        let row = Rect::new(inner.x, y, inner.width, 1);
        common::hit(app, row, Hit::Row(list_id::METERS, i));
        let sel = i == app.energy.sel_meter;
        if sel {
            crate::tui::widgets::fill(buf, row, th.s_selected());
            cell(buf, inner, inner.x, y, "▌", th.s_accent());
        }
        meter_row(app, buf, inner, y, c, nw, max, total_state);
    }
}

#[allow(clippy::too_many_arguments)]
fn meter_row(
    app: &App,
    buf: &mut Buffer,
    inner: Rect,
    y: u16,
    c: Cid,
    nw: usize,
    max: f64,
    total_state: &str,
) {
    let th = &app.th;
    let h = &app.house;
    let mut x = inner.x + 1;
    let a = app.store.st(h, c, "actual");
    let pin = if app.is_pinned(c) { "★" } else { " " };
    put(buf, inner, x, y, pin, th.s_accent());
    x += 1;
    put(buf, inner, x, y, &fit(&h.ctrls[c].name, nw), th.s_text());
    x += nw as u16 + 1;
    put(
        buf,
        inner,
        x,
        y,
        &rfit(&a.map(fmt_kw).unwrap_or_else(|| "—".into()), 9),
        th.s_text(),
    );
    x += 10;
    let rest = inner.right().saturating_sub(x);
    if rest > 30 {
        cell(buf, inner, x, y, "▕", th.s_faint());
        let g = if h.ctrls[c].meter_type() == "storage" {
            Grad::Batt
        } else {
            Grad::Use
        };
        meter(buf, inner, x + 1, y, 8, a.map(|v| v.abs() / max), g, th);
        cell(buf, inner, x + 9, y, "▏", th.s_faint());
        x += 11;
    }
    let total = app.store.st(h, c, total_state);
    put(
        buf,
        inner,
        x,
        y,
        &rfit(&total.map(fmt_kwh).unwrap_or_else(|| "—".into()), 10),
        th.s_dim(),
    );
    x += 11;
    if let Some(u) = h.ctrls[c].state("actual") {
        let data = app.store.series(u);
        let w = inner.right().saturating_sub(x + 1);
        if data.len() >= 2 && w >= 4 {
            dotspark(
                buf,
                inner,
                x,
                y,
                w.min(16),
                &resample(&data, 32),
                None,
                Grad::Use,
                th,
            );
        }
    }
}
