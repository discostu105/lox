//! Rendering (§5, §6). Pure functions of `&App` into a ratatui `Buffer`; the
//! only thing written back is the hit-test / scroll cache in `app.ui`.

pub mod common;
pub mod energy;
pub mod events;
pub mod home;
pub mod inspector;
pub mod overlays;
pub mod rooms;
pub mod sites;
pub mod system;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Modifier;

use super::app::{App, Conn, Hit, Screen, ToastKind};
use super::update::{MIN_H, MIN_W};
use super::widgets::{clear, put};

pub fn render(app: &App, area: Rect, buf: &mut Buffer) {
    {
        let mut ui = app.ui.borrow_mut();
        ui.hits.clear();
    }
    let th = &app.th;
    clear(buf, area, th.s_base());
    if area.width < MIN_W || area.height < MIN_H {
        too_small(app, area, buf);
        return;
    }
    let header = Rect::new(area.x, area.y, area.width, 1);
    let body = Rect::new(area.x, area.y + 1, area.width, area.height - 1);
    render_header(app, header, buf);
    match app.screen {
        Screen::Home => home::render(app, body, buf),
        Screen::Rooms => rooms::render(app, body, buf),
        Screen::Events => events::render(app, body, buf),
        Screen::Energy => energy::render(app, body, buf),
        Screen::System => system::render(app, body, buf),
        Screen::Sites => sites::render(app, body, buf),
    }
    render_toasts(app, body, buf);
    overlays::render(app, body, buf);
}

fn too_small(app: &App, area: Rect, buf: &mut Buffer) {
    let th = &app.th;
    let lines = [
        "Terminal too small".to_string(),
        format!(
            "need {}×{}, have {}×{}",
            MIN_W, MIN_H, area.width, area.height
        ),
        "q quit".to_string(),
    ];
    let y0 = area.y + area.height.saturating_sub(3) / 2;
    for (i, l) in lines.iter().enumerate() {
        let w = super::text::width(l) as u16;
        let x = area.x + area.width.saturating_sub(w) / 2;
        let st = if i == 0 {
            th.s_warn().add_modifier(Modifier::BOLD)
        } else {
            th.s_dim()
        };
        put(buf, area, x, y0 + i as u16, l, st);
    }
}

const SUP: [&str; 6] = ["¹", "²", "³", "⁴", "⁵", "⁶"];

fn render_header(app: &App, area: Rect, buf: &mut Buffer) {
    let th = &app.th;
    let mut x = area.x + 1;
    x = put(
        buf,
        area,
        x,
        area.y,
        "lox",
        th.s_accent().add_modifier(Modifier::BOLD),
    ) + 2;
    let narrow = area.width < 100;
    for (i, s) in Screen::ALL.iter().enumerate() {
        let active = *s == app.screen;
        let st = if active {
            th.s_accent()
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
        } else {
            th.s_dim()
        };
        let label = if narrow && !active {
            s.title()[..3].to_string()
        } else {
            s.title().to_string()
        };
        let x0 = x;
        x = put(
            buf,
            area,
            x,
            area.y,
            SUP[i],
            if active { th.s_accent() } else { th.s_faint() },
        );
        x = put(buf, area, x, area.y, &label, st) + 1;
        app.ui
            .borrow_mut()
            .hits
            .push((Rect::new(x0, area.y, x - x0, 1), Hit::Tab(*s)));
    }
    // right side, built from the end
    let mut parts: Vec<(String, ratatui::style::Style)> = Vec::new();
    parts.push((app.ctx_name.clone(), th.s_text()));
    let (conn, cst) = match &app.conn {
        Conn::Live => ("● live".to_string(), th.s_ok()),
        Conn::Connecting => ("◌ connecting".to_string(), th.s_dim()),
        Conn::Reconnecting { at, .. } => {
            let secs = (at - app.now).max(0.0).ceil();
            (format!("◐ reconnecting {}s", secs), th.s_warn())
        }
        Conn::Offline(_) => ("○ offline — cached values".to_string(), th.s_crit()),
        Conn::OutOfService => ("⏻ out of service".to_string(), th.s_warn()),
    };
    parts.push((conn, cst));
    if let Some(mode) = op_mode(app) {
        parts.push((mode, th.s_dim()));
    }
    if let Some(t) = outside_temp(app) {
        parts.push((t, th.s_text()));
    }
    let mut badges: Vec<(String, ratatui::style::Style)> = Vec::new();
    if app.opts.demo {
        badges.push((" DEMO ".into(), th.s_badge(th.info)));
    }
    if app.opts.read_only {
        badges.push((" READ-ONLY ".into(), th.s_badge(th.warn)));
    }
    if app.paused {
        badges.push((" PAUSED ".into(), th.s_badge(th.warn)));
    }
    if app.unread_errors > 0 {
        badges.push((format!(" ! {} ", app.unread_errors), th.s_badge(th.crit)));
    }
    let help = "? help";
    let mut right_w: usize = parts
        .iter()
        .map(|(s, _)| super::text::width(s) + 2)
        .sum::<usize>()
        + badges
            .iter()
            .map(|(s, _)| super::text::width(s) + 1)
            .sum::<usize>()
        + help.len()
        + 1;
    // drop optional parts when narrow
    while right_w + (x - area.x) as usize + 2 > area.width as usize && parts.len() > 2 {
        let (s, _) = parts.pop().unwrap();
        right_w -= super::text::width(&s) + 2;
    }
    let rx = area.right().saturating_sub(right_w as u16);
    // rule with the clock centered in it
    let rule_start = x;
    let rule_end = rx.saturating_sub(1);
    if rule_end > rule_start + 2 {
        for cx in rule_start..rule_end {
            super::widgets::cell(buf, area, cx, area.y, "─", th.s_faint());
        }
        let clock = format!(" {} ", app.clock());
        let cw = clock.len() as u16;
        if rule_end - rule_start > cw + 4 {
            let cx = rule_start + (rule_end - rule_start - cw) / 2;
            put(buf, area, cx, area.y, &clock, th.s_text());
        }
    }
    let mut x = rx;
    for (s, st) in badges {
        x = put(buf, area, x, area.y, &s, st) + 1;
    }
    for (s, st) in parts {
        x = put(buf, area, x, area.y, &s, st) + 2;
    }
    let hx = put(
        buf,
        area,
        x,
        area.y,
        "?",
        th.s_accent().add_modifier(Modifier::BOLD),
    );
    put(buf, area, hx, area.y, " help", th.s_dim());
}

/// Current operating mode name (global state).
pub fn op_mode(app: &App) -> Option<String> {
    let u = app.house.globals.get("operatingMode")?;
    let v = app.store.num(u)? as i64;
    Some(
        app.house
            .op_modes
            .get(&v)
            .cloned()
            .unwrap_or_else(|| format!("mode {}", v))
            .to_lowercase(),
    )
}

/// Outside temperature: a temperature sensor named outside/outdoor/außen.
pub fn outside_temp(app: &App) -> Option<String> {
    let h = &app.house;
    let c = h.top_level().find(|c| {
        let n = h.ctrls[*c].name.to_lowercase();
        h.ctrls[*c].is_temperature()
            && (n.contains("outside")
                || n.contains("outdoor")
                || n.contains("außen")
                || n.contains("aussen"))
    })?;
    let v = app.store.st(h, c, "value")?;
    Some(format!("{:.1}°", v))
}

fn render_toasts(app: &App, body: Rect, buf: &mut Buffer) {
    let th = &app.th;
    for (i, t) in app.toasts.iter().rev().enumerate() {
        let y = body.bottom().saturating_sub(2 + i as u16);
        if y <= body.y {
            break;
        }
        let st = match t.kind {
            ToastKind::Ok => th.s_ok(),
            ToastKind::Info => th.s_text(),
            ToastKind::Err => th.s_crit(),
        };
        let glyph = match t.kind {
            ToastKind::Ok => "✓",
            ToastKind::Info => "›",
            ToastKind::Err => "✗",
        };
        let maxw = (body.width as usize).saturating_sub(8).min(90);
        let s = format!(" {} {} ", glyph, super::text::trunc(&t.text, maxw));
        let w = super::text::width(&s) as u16;
        let x = body.right().saturating_sub(w + 2);
        let bg = if th.is_mono() {
            ratatui::style::Style::default().add_modifier(Modifier::REVERSED)
        } else {
            st.bg(th.surface)
        };
        put(buf, body, x, y, &s, bg);
    }
}
