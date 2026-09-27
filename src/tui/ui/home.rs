//! ¹ Home — room cards, attention, energy, pinned, quick, live (§5.2).

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Modifier;

use super::common;
use super::energy;
use crate::tui::app::{App, Hit, HomePane};
use crate::tui::keymap::{Cmd, Ctx};
use crate::tui::lists;
use crate::tui::model::Kind;
use crate::tui::text::{fit, fmt_kw, rfit, width};
use crate::tui::theme::Grad;
use crate::tui::update::list_id;
use crate::tui::vm;
use crate::tui::widgets::dotspark::dotspark;
use crate::tui::widgets::meter::meter;
use crate::tui::widgets::notchbox::{Hint, Notch, NotchBox};
use crate::tui::widgets::{cell, fill, put};

const CARD_W: u16 = 24;
const CARD_H: u16 = 6;

pub fn render(app: &App, area: Rect, buf: &mut Buffer) {
    let rw = if area.width >= 150 {
        46
    } else if area.width >= 110 {
        42
    } else {
        36
    };
    let left = Rect::new(area.x, area.y, area.width - rw, area.height);
    let right = Rect::new(left.right(), area.y, rw, area.height);
    let live_h = (area.height / 4).clamp(5, 10);
    cards(
        app,
        Rect::new(left.x, left.y, left.width, left.height - live_h),
        buf,
    );
    live(
        app,
        Rect::new(left.x, left.bottom() - live_h, left.width, live_h),
        buf,
    );

    // right column: attention · energy · pinned · quick
    let atts = lists::attention(app);
    let att_h = (atts.len().max(1) as u16 + 2).clamp(3, 8);
    let has_energy = !app.house.energy.nodes.is_empty();
    let en_h = if has_energy { 6 } else { 0 };
    let quick_h = if app.scenes.is_empty() {
        0
    } else {
        3 + (app.scenes.len() as u16 / 3).min(2)
    };
    let pin_h = right.height.saturating_sub(att_h + en_h + quick_h);
    let mut y = right.y;
    attention(app, Rect::new(right.x, y, rw, att_h), buf, &atts);
    y += att_h;
    if has_energy {
        energy_panel(app, Rect::new(right.x, y, rw, en_h), buf);
        y += en_h;
    }
    if pin_h >= 3 {
        pinned(app, Rect::new(right.x, y, rw, pin_h), buf);
        y += pin_h;
    }
    if quick_h > 0 && y + quick_h <= right.bottom() {
        quick(app, Rect::new(right.x, y, rw, quick_h), buf);
    }
}

fn focused(app: &App, p: HomePane) -> bool {
    app.home.pane == p && app.overlays.is_empty()
}

fn cards(app: &App, area: Rect, buf: &mut Buffer) {
    let th = &app.th;
    let focus = focused(app, HomePane::Rooms);
    let order = &app.home.order;
    let mut nb = NotchBox::new().focus(focus).title(Notch::new("¹rooms"));
    nb = nb.position(format!(
        "{}/{}",
        (app.home.sel_room + 1).min(order.len()),
        order.len()
    ));
    if focus {
        let mut hints = vec![Hint::new("⏎", "open")];
        if !app.opts.read_only {
            hints.push(Hint::new("<>", "lights"));
            hints.push(Hint::new("m", "mood"));
            hints.push(Hint::new("a", "all"));
        }
        nb = nb.hints(hints);
    }
    let inner = nb.render(area, buf, th);
    common::hit(app, inner, Hit::Pane(list_id::HOME_CARDS));
    if order.is_empty() {
        common::empty(
            app,
            buf,
            inner,
            &["no rooms", "this structure has no rooms with controls"],
        );
        return;
    }
    let cols = ((inner.width.saturating_sub(1)) / (CARD_W + 1)).max(1) as usize;
    let cw = (inner.width.saturating_sub(1)) / cols as u16 - 1;
    let rows_vis = (inner.height / CARD_H).max(1) as usize;
    app.ui.borrow_mut().home_cols = cols;
    let sel_row = app.home.sel_room / cols;
    let first_row = common::offset(
        app,
        list_id::HOME_CARDS,
        sel_row,
        order.len().div_ceil(cols),
        rows_vis,
    );
    for (i, &r) in order
        .iter()
        .enumerate()
        .skip(first_row * cols)
        .take(rows_vis * cols)
    {
        let k = i - first_row * cols;
        let x = inner.x + 1 + (k % cols) as u16 * (cw + 1);
        let y = inner.y + (k / cols) as u16 * CARD_H;
        let rect = Rect::new(x, y, cw, CARD_H).intersection(inner);
        if rect.height < 3 {
            break;
        }
        common::hit(app, rect, Hit::Row(list_id::HOME_CARDS, i));
        card(app, buf, rect, r, focus && i == app.home.sel_room);
    }
    let shown = (first_row * cols + rows_vis * cols).min(order.len());
    if shown < order.len() {
        let s = format!("… {} more", order.len() - shown);
        put(
            buf,
            inner,
            inner.right().saturating_sub(width(&s) as u16 + 1),
            inner.bottom() - 1,
            &s,
            th.s_faint(),
        );
    }
}

fn card(app: &App, buf: &mut Buffer, rect: Rect, r: usize, selected: bool) {
    let th = &app.th;
    let h = &app.house;
    let s = lists::room_sum(app, r);
    let mut nb = NotchBox::new()
        .focus(selected)
        .title(Notch::new(h.rooms[r].name.clone()));
    if s.attention {
        nb = nb.meta(Notch::new("⚠").styled(th.s_warn()));
    }
    let inner = nb.render(rect, buf, th);
    let x = inner.x + 1;
    let w = inner.width.saturating_sub(2);
    let mut y = inner.y;
    // temperature (→ target) and trend
    match s.temp {
        Some(t) => {
            let mut cx = put(
                buf,
                inner,
                x,
                y,
                &format!("{:.1}°", t),
                th.grad_style(Grad::Temp, vm::temp_frac(t))
                    .add_modifier(Modifier::BOLD),
            );
            if let Some(tg) = s.target {
                cx = put(buf, inner, cx, y, &format!(" → {:.1}°", tg), th.s_dim());
                let arrow = if t < tg - 0.3 {
                    ("▲", th.s_warn())
                } else if t > tg + 0.3 {
                    ("▼", th.s_info())
                } else {
                    ("·", th.s_faint())
                };
                let _ = cx;
                put(buf, inner, x + w.saturating_sub(1), y, arrow.0, arrow.1);
            }
        }
        None => {
            put(buf, inner, x, y, "—", th.s_faint());
        }
    }
    y += 1;
    // lights
    if s.lights > 0 {
        if s.lights_on > 0 {
            let lvl = s
                .light_level
                .map(|l| format!("  {:.0} %", l))
                .unwrap_or_default();
            let n = if s.lights_on == 1 {
                "light".to_string()
            } else {
                "lights".to_string()
            };
            put(
                buf,
                inner,
                x,
                y,
                &fit(&format!("● {} {}{}", s.lights_on, n, lvl), w as usize),
                th.s_on(),
            );
        } else {
            put(buf, inner, x, y, "○ lights off", th.s_dim());
        }
        y += 1;
    }
    // blinds / windows / gate
    if s.windows_open > 0 {
        put(
            buf,
            inner,
            x,
            y,
            &fit(
                &format!(
                    "◫ {} window{} open",
                    s.windows_open,
                    if s.windows_open == 1 { "" } else { "s" }
                ),
                w as usize,
            ),
            th.s_warn(),
        );
        y += 1;
    } else if s.blinds > 0 && y < inner.bottom() {
        let lab = if s.blinds_moving {
            "▾ moving "
        } else {
            "▾ blinds "
        };
        put(
            buf,
            inner,
            x,
            y,
            lab,
            if s.blinds_moving {
                th.s_accent()
            } else {
                th.s_dim()
            },
        );
        let mw = w.saturating_sub(10);
        if mw > 2 {
            meter(buf, inner, x + 9, y, mw, s.blind_pos, Grad::Info, th);
        }
        y += 1;
    } else if s.gate_open && y < inner.bottom() {
        put(buf, inner, x, y, "⌂ gate open", th.s_warn());
        y += 1;
    }
    // temperature sparkline
    if y < inner.bottom()
        && let Some((_, u)) = &h.rooms[r].temp
    {
        let data = app.store.series(u);
        if data.len() >= 2 {
            dotspark(
                buf,
                inner,
                x,
                inner.bottom() - 1,
                w,
                &data,
                None,
                Grad::Temp,
                th,
            );
        }
    }
}

fn attention(app: &App, area: Rect, buf: &mut Buffer, atts: &[lists::Att]) {
    let th = &app.th;
    let focus = focused(app, HomePane::Attention);
    let mut nb = NotchBox::new().focus(focus).title(Notch::new("attention"));
    if !atts.is_empty() {
        nb = nb.meta(Notch::new(atts.len().to_string()));
    }
    if focus
        && atts
            .get(app.home.sel_attention)
            .is_some_and(|a| a.cid.is_some())
    {
        nb = nb.hints(vec![Hint::new("⏎", "show"), Hint::new("w", "wiring")]);
    }
    let inner = nb.render(area, buf, th);
    common::hit(app, inner, Hit::Pane(list_id::HOME_ATTENTION));
    if atts.is_empty() {
        put(
            buf,
            inner,
            inner.x + 1,
            inner.y,
            "✓ All good",
            th.s_ok().add_modifier(Modifier::BOLD),
        );
        return;
    }
    let hgt = inner.height as usize;
    let off = common::offset(
        app,
        list_id::HOME_ATTENTION,
        app.home.sel_attention,
        atts.len(),
        hgt,
    );
    for (i, a) in atts.iter().enumerate().skip(off).take(hgt) {
        let y = inner.y + (i - off) as u16;
        let row = Rect::new(inner.x, y, inner.width, 1);
        common::hit(app, row, Hit::Row(list_id::HOME_ATTENTION, i));
        if focus && i == app.home.sel_attention {
            fill(buf, row, th.s_selected());
            cell(buf, inner, inner.x, y, "▌", th.s_accent());
        }
        let (g, st) = match a.level {
            3 => ("⚠", th.s_crit()),
            2 => ("⚠", th.s_warn()),
            _ => ("●", th.s_info()),
        };
        put(buf, inner, inner.x + 1, y, g, st);
        put(
            buf,
            inner,
            inner.x + 3,
            y,
            &fit(&a.text, inner.width.saturating_sub(4) as usize),
            th.s_text(),
        );
    }
}

fn energy_panel(app: &App, area: Rect, buf: &mut Buffer) {
    let th = &app.th;
    let focus = focused(app, HomePane::Energy);
    let mut nb = NotchBox::new().focus(focus).title(Notch::new("energy"));
    if focus {
        nb = nb.hints(vec![Hint::new("⏎", "details")]);
    }
    let inner = nb.render(area, buf, th);
    common::hit(app, inner, Hit::Pane(list_id::HOME_ENERGY));
    let f = energy::flows(app);
    let x = inner.x + 1;
    let mw = inner.width.saturating_sub(24).min(12);
    let rows: [(&str, Option<f64>, Grad, f64, Option<&String>); 3] = [
        ("PV", f.pv, Grad::Pv, 10.0, f.pv_uuid.as_ref()),
        ("Home", f.home, Grad::Use, 12.0, f.home_uuid.as_ref()),
        ("Grid", f.grid, Grad::Grid, 12.0, f.grid_uuid.as_ref()),
    ];
    let mut y = inner.y;
    for (label, v, g, scale, u) in rows {
        if y >= inner.bottom() {
            break;
        }
        put(buf, inner, x, y, &fit(label, 5), th.s_dim());
        cell(buf, inner, x + 5, y, "▕", th.s_faint());
        meter(
            buf,
            inner,
            x + 6,
            y,
            mw,
            v.map(|v| (v.abs() / scale).clamp(0.0, 1.0)),
            g,
            th,
        );
        cell(buf, inner, x + 6 + mw, y, "▏", th.s_faint());
        let mut txt = v.map(fmt_kw).unwrap_or_else(|| "—".into());
        if label == "Grid" && v.is_some_and(|g| g < -0.01) {
            txt.push_str(" ⇢");
        }
        let vx = put(buf, inner, x + 8 + mw, y, &rfit(&txt, 9), th.s_text()) + 1;
        if let Some(u) = u {
            let data = app.store.series(u);
            let sw = inner.right().saturating_sub(vx + 1);
            if data.len() >= 2 && sw >= 3 {
                dotspark(buf, inner, vx, y, sw, &data, None, g, th);
            }
        }
        y += 1;
    }
    if y < inner.bottom()
        && let Some(soc) = f.soc
    {
        put(buf, inner, x, y, "Batt ", th.s_dim());
        cell(buf, inner, x + 5, y, "▕", th.s_faint());
        meter(buf, inner, x + 6, y, mw, Some(soc / 100.0), Grad::Batt, th);
        cell(buf, inner, x + 6 + mw, y, "▏", th.s_faint());
        let arrow = match f.batt {
            Some(b) if b > 0.01 => format!(" +{}", fmt_kw(b)),
            Some(b) if b < -0.01 => format!(" {}", fmt_kw(b)),
            _ => String::new(),
        };
        put(
            buf,
            inner,
            x + 8 + mw,
            y,
            &format!("{:>3.0} %{}", soc, arrow),
            th.s_text(),
        );
    }
}

fn pinned(app: &App, area: Rect, buf: &mut Buffer) {
    let th = &app.th;
    let focus = focused(app, HomePane::Pinned);
    let pins = app.pinned();
    let mut nb = NotchBox::new().focus(focus).title(Notch::new("pinned"));
    if !pins.is_empty() {
        nb = nb.position(format!(
            "{}/{}",
            (app.home.sel_pin + 1).min(pins.len()),
            pins.len()
        ));
    }
    if focus && let Some(c) = pins.get(app.home.sel_pin) {
        let mut hints = common::item_hints(app, *c);
        hints.truncate(3);
        hints.extend(common::ctx_hints(Ctx::Item, &[Cmd::Pin]));
        nb = nb.hints(hints);
    }
    let inner = nb.render(area, buf, th);
    common::hit(app, inner, Hit::Pane(list_id::HOME_PINNED));
    if pins.is_empty() {
        common::empty(
            app,
            buf,
            inner,
            &["nothing pinned", "press * on any control"],
        );
        return;
    }
    let hgt = inner.height as usize;
    let off = common::offset(app, list_id::HOME_PINNED, app.home.sel_pin, pins.len(), hgt);
    let nw = (inner.width as usize).saturating_sub(4) / 2;
    for (i, &c) in pins.iter().enumerate().skip(off).take(hgt) {
        let y = inner.y + (i - off) as u16;
        let row = Rect::new(inner.x, y, inner.width, 1);
        common::hit(app, row, Hit::Row(list_id::HOME_PINNED, i));
        let sel = focus && i == app.home.sel_pin;
        if sel {
            fill(buf, row, th.s_selected());
            cell(buf, inner, inner.x, y, "▌", th.s_accent());
        }
        let v = vm::view(&app.store, &app.house, c);
        let gst = match v.on {
            Some(true) => th.s_on(),
            Some(false) => th.s_faint(),
            None => common::tone(th, v.tone),
        };
        put(
            buf,
            inner,
            inner.x + 1,
            y,
            common::glyph(app, app.house.ctrls[c].kind),
            gst,
        );
        put(
            buf,
            inner,
            inner.x + 3,
            y,
            &fit(&app.house.display_name(c), nw),
            th.s_text(),
        );
        let vx = inner.x + 4 + nw as u16;
        let vw = inner.right().saturating_sub(vx + 1);
        let val = fit(&v.value, 12.min(vw as usize));
        let ex = put(buf, inner, vx, y, &val, common::tone(th, v.tone)) + 1;
        let rest = inner.right().saturating_sub(ex + 1);
        if let Some((m, st)) = common::pending_mark(app, c) {
            cell(buf, inner, inner.right() - 2, y, m, st);
        } else if rest >= 4 {
            if v.frac.is_some() && !app.house.ctrls[c].is_temperature() {
                meter(buf, inner, ex, y, rest.min(10), v.frac, v.grad, th);
            } else if let Some(u) = common::main_state(app, c) {
                let data = app.store.series(&u);
                if data.len() >= 2 {
                    dotspark(
                        buf,
                        inner,
                        ex,
                        y,
                        rest.min(12),
                        &data,
                        None,
                        common::spark_grad(app, c),
                        th,
                    );
                }
            }
        }
    }
}

fn quick(app: &App, area: Rect, buf: &mut Buffer) {
    let th = &app.th;
    let focus = focused(app, HomePane::Quick);
    let mut nb = NotchBox::new().focus(focus).title(Notch::new("quick"));
    if focus && !app.opts.read_only {
        nb = nb.hints(vec![Hint::new("␣", "run")]);
    }
    let inner = nb.render(area, buf, th);
    common::hit(app, inner, Hit::Pane(list_id::HOME_QUICK));
    let mut x = inner.x + 1;
    let mut y = inner.y;
    for (i, s) in app.scenes.iter().enumerate() {
        let label = format!("▶ {}", crate::tui::text::clean(s));
        let w = width(&label) as u16 + 2;
        if x + w > inner.right() {
            x = inner.x + 1;
            y += 1;
        }
        if y >= inner.bottom() {
            break;
        }
        let sel = focus && i == app.home.sel_quick;
        let st = if sel {
            th.s_selected().add_modifier(Modifier::BOLD)
        } else {
            th.s_text()
        };
        common::hit(app, Rect::new(x, y, w, 1), Hit::Row(list_id::HOME_QUICK, i));
        put(buf, inner, x, y, &label, st);
        x += w + 1;
    }
}

fn live(app: &App, area: Rect, buf: &mut Buffer) {
    let th = &app.th;
    let focus = focused(app, HomePane::Live);
    let rate = app.store.rate_series(10, 16);
    let mut nb = NotchBox::new()
        .focus(focus)
        .title(Notch::new("live"))
        .meta(Notch::new(format!("{}/min", app.store.per_minute())));
    if focus {
        nb = nb.hints(vec![Hint::new("3", "events")]);
    }
    let inner = nb.render(area, buf, th);
    common::hit(app, inner, Hit::Pane(list_id::HOME_LIVE));
    let meta_w = format!("{}/min", app.store.per_minute()).chars().count() as u16 + 4;
    if rate.iter().any(|v| *v > 0.0) && inner.width > 40 {
        dotspark(
            buf,
            area,
            area.right().saturating_sub(meta_w + 10),
            area.y,
            8,
            &rate,
            None,
            Grad::Acc,
            th,
        );
    }
    // Home shows house *activity*: meters and sensors tick constantly and would
    // drown out lights, doors and blinds (the Events screen has everything).
    let h = &app.house;
    let rows: Vec<usize> = lists::event_rows(app)
        .into_iter()
        .filter(|&i| {
            let e = &app.store.events[i];
            e.cid.is_some_and(|c| {
                !matches!(
                    h.ctrls[h.top(c)].kind,
                    Kind::Meter | Kind::Efm | Kind::Analog | Kind::Text
                )
            })
        })
        .collect();
    if rows.is_empty() {
        common::empty(
            app,
            buf,
            inner,
            &[
                "waiting for activity…",
                "lights, blinds, doors and presence appear here",
            ],
        );
        return;
    }
    let hgt = inner.height as usize;
    let rw = 12usize;
    let nw = ((inner.width as usize).saturating_sub(10 + rw + 30)).clamp(10, 24);
    for (k, &i) in rows.iter().rev().take(hgt).enumerate() {
        let e = &app.store.events[i];
        let y = inner.y + k as u16;
        // fade with age
        let age = (app.now - e.t).max(0.0);
        let fade = (age / 120.0).clamp(0.0, 1.0);
        let tst = if age < 2.0 {
            th.s_accent()
        } else {
            th.grad_style(Grad::Fade, fade)
        };
        let mut x = inner.x + 1;
        x = put(buf, inner, x, y, &app.hhmmss(e.t), tst) + 2;
        let room = e.cid.and_then(|c| h.room_name(c)).unwrap_or("—");
        put(buf, inner, x, y, &fit(room, rw), th.s_dim());
        x += rw as u16 + 1;
        let name = e
            .cid
            .map(|c| h.display_name(c))
            .unwrap_or_else(|| e.state.clone());
        put(buf, inner, x, y, &fit(&name, nw), th.s_text());
        x += nw as u16 + 1;
        let ch = super::events::change_text(e);
        let cw = inner.right().saturating_sub(x + 1) as usize;
        put(
            buf,
            inner,
            x,
            y,
            &fit(&ch, cw),
            if age < 2.0 { th.s_text() } else { th.s_dim() },
        );
    }
}
