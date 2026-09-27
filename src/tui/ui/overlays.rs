//! Overlays (§5.8): help, palette, menu, picker, confirm, input, contexts,
//! message log, wiring (§5.9) and the compact inspector.

use std::collections::HashSet;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use super::common;
use super::inspector;
use crate::tui::app::{
    App, Confirm, FilterTarget, InputKind, Line, Overlay, PalItem, WiringDoc, WiringState,
};
use crate::tui::keymap::{self, BINDINGS, Ctx};
use crate::tui::lists;
use crate::tui::palette;
use crate::tui::store::{Val, fmt_val};
use crate::tui::text::{clean, fit, rfit, width};
use crate::tui::update::{contexts, wiring_rows};
use crate::tui::vm;
use crate::tui::widgets::notchbox::{Hint, Notch, NotchBox};
use crate::tui::widgets::{cell, clear, fill, put};

pub fn render(app: &App, body: Rect, buf: &mut Buffer) {
    // Filters render as a bar; everything else stacks as boxes (only the top one is live)
    for (i, o) in app.overlays.iter().enumerate() {
        let top = i + 1 == app.overlays.len();
        match o {
            Overlay::Help { scroll, filter } => help(app, body, buf, *scroll, filter),
            Overlay::Palette(p) => palette_box(app, body, buf, &p.line, p.sel),
            Overlay::Menu {
                cids,
                items,
                sel,
                room,
            } => menu(app, body, buf, cids, *room, items, *sel),
            Overlay::Picker {
                title,
                choices,
                sel,
                ..
            } => picker(app, body, buf, title, choices, *sel),
            Overlay::Confirm(c) => confirm(app, body, buf, c),
            Overlay::Input { kind, line, err } => {
                input(app, body, buf, kind, line, err.as_deref(), top)
            }
            Overlay::Contexts { sel } => ctx_switcher(app, body, buf, *sel),
            Overlay::MsgLog { scroll } => msglog(app, body, buf, *scroll),
            Overlay::Wiring(w) => wiring(app, body, buf, w),
            Overlay::Inspector { cid, scroll } => {
                let w = (body.width * 2 / 3).clamp(60.min(body.width), 90.min(body.width));
                let h = body.height.saturating_sub(2).min(34);
                let r = centered(body, w, h);
                clear(buf, r, app.th.s_base());
                inspector::render(app, r, buf, *cid, top, *scroll);
            }
        }
    }
}

pub fn centered(area: Rect, w: u16, h: u16) -> Rect {
    let w = w.min(area.width);
    let h = h.min(area.height);
    Rect::new(
        area.x + (area.width - w) / 2,
        area.y + (area.height - h) / 3,
        w,
        h,
    )
}

fn boxed(app: &App, area: Rect, buf: &mut Buffer, nb: NotchBox) -> Rect {
    clear(buf, area, app.th.s_base());
    nb.focus(true).render(area, buf, &app.th)
}

/// An editable line with a block cursor.
fn put_line(
    app: &App,
    buf: &mut Buffer,
    area: Rect,
    x: u16,
    y: u16,
    line: &Line,
    mask: bool,
    w: usize,
) {
    let th = &app.th;
    let text: String = if mask {
        "•".repeat(line.buf.chars().count())
    } else {
        line.buf.clone()
    };
    let chars: Vec<char> = text.chars().collect();
    // keep the cursor visible
    let start = line.cur.saturating_sub(w.saturating_sub(1));
    let mut cx = x;
    for (i, ch) in chars.iter().enumerate().skip(start) {
        if (cx - x) as usize >= w {
            break;
        }
        let st = if i == line.cur {
            th.s_selected().add_modifier(Modifier::REVERSED)
        } else if line.fresh {
            th.s_selected()
        } else {
            th.s_text()
        };
        cx = put(buf, area, cx, y, &ch.to_string(), st);
    }
    if line.cur >= chars.len() && ((cx - x) as usize) < w {
        cell(
            buf,
            area,
            cx,
            y,
            " ",
            Style::default().add_modifier(Modifier::REVERSED),
        );
    }
}

fn sel_row(app: &App, buf: &mut Buffer, inner: Rect, y: u16) {
    fill(
        buf,
        Rect::new(inner.x, y, inner.width, 1),
        app.th.s_selected(),
    );
    cell(buf, inner, inner.x, y, "▌", app.th.s_accent());
}

// ── Help ────────────────────────────────────────────────────────────────────

pub fn help_lines(app: &App, filter: &str) -> Vec<(Option<Ctx>, String, String)> {
    let mut order = contexts(app);
    for c in [
        Ctx::Item,
        Ctx::Rooms,
        Ctx::Events,
        Ctx::System,
        Ctx::Log,
        Ctx::Config,
        Ctx::Update,
        Ctx::Global,
    ] {
        if !order.contains(&c) {
            order.push(c);
        }
    }
    let mut out = Vec::new();
    for ctx in order {
        let rows: Vec<_> = BINDINGS
            .iter()
            .filter(|b| b.ctx == ctx)
            .map(|b| {
                (
                    b.keys
                        .iter()
                        .map(|k| keymap::display(k))
                        .collect::<Vec<_>>()
                        .join(" "),
                    b.help,
                )
            })
            .filter(|(k, h)| {
                filter.is_empty() || lists::fuzzy(filter, &format!("{} {}", k, h)).is_some()
            })
            .collect();
        if rows.is_empty() {
            continue;
        }
        out.push((Some(ctx), ctx.title().to_string(), String::new()));
        let mut seen = HashSet::new();
        for (k, h) in rows {
            if seen.insert(h) {
                out.push((None, k, h.to_string()));
            }
        }
    }
    out
}

fn help(app: &App, body: Rect, buf: &mut Buffer, scroll: usize, filter: &str) {
    let th = &app.th;
    let r = centered(body, 84, body.height.saturating_sub(2));
    let mut nb = NotchBox::new()
        .title(Notch::new("help"))
        .meta(Notch::new("lox tui"));
    if !filter.is_empty() {
        nb = nb.title(Notch::new(format!("/{}", filter)).active(true));
    } else {
        nb = nb.title(Notch::hot("/ search", '/'));
    }
    nb = nb.hints(vec![Hint::new("j k", "scroll"), Hint::new("Esc", "close")]);
    let inner = boxed(app, r, buf, nb);
    let lines = help_lines(app, filter);
    let hgt = inner.height as usize;
    let scroll = scroll.min(lines.len().saturating_sub(hgt));
    for (k, (ctx, a, b)) in lines.iter().enumerate().skip(scroll).take(hgt) {
        let y = inner.y + (k - scroll) as u16;
        if ctx.is_some() {
            put(
                buf,
                inner,
                inner.x + 1,
                y,
                &a.to_uppercase(),
                th.s_accent().add_modifier(Modifier::BOLD),
            );
        } else {
            put(buf, inner, inner.x + 3, y, &fit(a, 16), th.s_accent());
            put(
                buf,
                inner,
                inner.x + 20,
                y,
                &fit(b, inner.width.saturating_sub(21) as usize),
                th.s_text(),
            );
        }
    }
    if lines.is_empty() {
        common::empty(app, buf, inner, &["no key matches"]);
    }
}

// ── Palette ─────────────────────────────────────────────────────────────────

fn pal_label(app: &App, it: &PalItem) -> (String, String) {
    let h = &app.house;
    match it {
        PalItem::Ctrl(c) => {
            let v = vm::view(&app.store, h, *c);
            let room = h.room_name(*c).unwrap_or("");
            (
                format!(
                    "{} {}",
                    common::glyph(app, h.ctrls[*c].kind),
                    h.display_name(*c)
                ),
                format!("{}  {}", room, v.value),
            )
        }
        PalItem::Room(r) => (
            format!("⌂ {}", h.rooms[*r].name),
            format!("{} controls", h.rooms[*r].ctrls.len()),
        ),
        PalItem::Scene(s) | PalItem::RunScene(s) => (format!("▶ {}", clean(s)), "scene".into()),
        PalItem::Screen(s) => (format!("{} {}", s.num(), s.title()), "screen".into()),
        PalItem::Site(s) => (
            format!("◆ {}", s),
            if *s == app.ctx_name {
                "active".into()
            } else {
                "switch".into()
            },
        ),
        PalItem::Run { label, plans } => (
            format!("⏎ {}", label),
            if plans.len() > 1 {
                format!("{} controls", plans.len())
            } else {
                String::new()
            },
        ),
        PalItem::Note(n) => (format!("ℹ {}", n), String::new()),
    }
}

fn palette_box(app: &App, body: Rect, buf: &mut Buffer, line: &Line, sel: usize) {
    let th = &app.th;
    let r = centered(body, 76, body.height.saturating_sub(4).min(22));
    let items = palette::items(app, &line.buf);
    let nb = NotchBox::new()
        .title(Notch::new(":"))
        .meta(Notch::new(if palette::is_command(&line.buf) {
            "command"
        } else {
            "search"
        }))
        .hints(vec![
            Hint::new("⏎", "run"),
            Hint::new("Tab", "complete"),
            Hint::new("↑↓", "select"),
            Hint::new("Esc", "close"),
        ])
        .position(format!("{}", items.len()));
    let inner = boxed(app, r, buf, nb);
    put(
        buf,
        inner,
        inner.x + 1,
        inner.y,
        "›",
        th.s_accent().add_modifier(Modifier::BOLD),
    );
    put_line(
        app,
        buf,
        inner,
        inner.x + 3,
        inner.y,
        line,
        false,
        inner.width.saturating_sub(4) as usize,
    );
    if line.buf.is_empty() {
        put(
            buf,
            inner,
            inner.x + 4,
            inner.y,
            "control, room, scene — or a lox command (light on …)",
            th.s_faint(),
        );
    }
    let list = Rect::new(
        inner.x,
        inner.y + 2,
        inner.width,
        inner.height.saturating_sub(2),
    );
    let hgt = list.height as usize;
    // rows with section headers
    let mut rows: Vec<(Option<&str>, Option<usize>)> = Vec::new();
    let mut last = "";
    for (i, (sec, _)) in items.iter().enumerate() {
        if *sec != last {
            rows.push((Some(sec), None));
            last = sec;
        }
        rows.push((None, Some(i)));
    }
    let sel_row_i = rows.iter().position(|r| r.1 == Some(sel)).unwrap_or(0);
    let off = sel_row_i.saturating_sub(hgt.saturating_sub(1));
    let filter = if palette::is_command(&line.buf) {
        ""
    } else {
        line.buf.as_str()
    };
    for (k, (sec, idx)) in rows.iter().enumerate().skip(off).take(hgt) {
        let y = list.y + (k - off) as u16;
        if let Some(s) = sec {
            put(
                buf,
                list,
                list.x + 1,
                y,
                &s.to_uppercase(),
                th.s_dim().add_modifier(Modifier::BOLD),
            );
            continue;
        }
        let Some(i) = idx else { continue };
        if *i == sel {
            sel_row(app, buf, list, y);
        }
        let (a, b) = pal_label(app, &items[*i].1);
        let bw = width(&b).min(30);
        let aw = (list.width as usize).saturating_sub(bw + 4);
        let st = if matches!(items[*i].1, PalItem::Note(_)) {
            th.s_warn()
        } else {
            th.s_text()
        };
        common::put_name(buf, list, list.x + 2, y, &a, aw, st, filter, th);
        put(
            buf,
            list,
            list.right().saturating_sub(bw as u16 + 1),
            y,
            &rfit(&b, bw),
            th.s_dim(),
        );
    }
    if items.is_empty() && !line.buf.is_empty() {
        common::empty(app, buf, list, &["no match"]);
    }
}

// ── Menu / picker ───────────────────────────────────────────────────────────

fn menu(
    app: &App,
    body: Rect,
    buf: &mut Buffer,
    cids: &[usize],
    room: Option<usize>,
    items: &[(String, Option<char>, crate::actions::Action)],
    sel: usize,
) {
    let th = &app.th;
    let h = &app.house;
    let title = match (room, cids) {
        (Some(r), _) => h.rooms[r].name.clone(),
        (None, [c]) => h.display_name(*c),
        (None, cs) => format!("{} controls", cs.len()),
    };
    let r = centered(body, 72, items.len() as u16 + 4);
    let nb = NotchBox::new()
        .title(Notch::new(title))
        .meta(Notch::new("actions"))
        .hints(vec![
            Hint::new("⏎", "run"),
            Hint::new("key", "direct"),
            Hint::new("Esc", "close"),
        ]);
    let inner = boxed(app, r, buf, nb);
    let cli_for = |a: &crate::actions::Action| match cids.first() {
        Some(c) if cids.len() == 1 => a.to_cli(&h.ctrls[*c].name, h.room_name(*c)),
        _ => String::new(),
    };
    // scroll so the selected row stays visible (long mood lists)
    let hgt = inner.height as usize;
    let off = sel.saturating_sub(hgt.saturating_sub(1));
    for (i, (label, key, action)) in items.iter().enumerate().skip(off).take(hgt) {
        let y = inner.y + (i - off) as u16;
        if i == sel {
            sel_row(app, buf, inner, y);
        }
        let k = key.map(|c| c.to_string()).unwrap_or_default();
        put(
            buf,
            inner,
            inner.x + 2,
            y,
            &fit(&k, 2),
            th.s_accent().add_modifier(Modifier::BOLD),
        );
        put(buf, inner, inner.x + 5, y, &fit(label, 24), th.s_text());
        let cli = cli_for(action);
        put(
            buf,
            inner,
            inner.x + 30,
            y,
            &fit(&cli, inner.width.saturating_sub(31) as usize),
            th.s_faint(),
        );
    }
}

fn picker(
    app: &App,
    body: Rect,
    buf: &mut Buffer,
    title: &str,
    choices: &[vm::Choice],
    sel: usize,
) {
    let th = &app.th;
    let w = choices
        .iter()
        .map(|c| width(&c.label))
        .max()
        .unwrap_or(10)
        .max(width(title)) as u16
        + 12;
    let r = centered(body, w.clamp(30, 60), choices.len() as u16 + 4);
    let nb = NotchBox::new()
        .title(Notch::new(title.to_string()))
        .hints(vec![Hint::new("⏎", "choose"), Hint::new("1-9", "direct")]);
    let inner = boxed(app, r, buf, nb);
    let hgt = inner.height as usize;
    let off = sel.saturating_sub(hgt.saturating_sub(1));
    for (i, c) in choices.iter().enumerate().skip(off).take(hgt) {
        let y = inner.y + (i - off) as u16;
        if i == sel {
            sel_row(app, buf, inner, y);
        }
        if i < 9 {
            put(
                buf,
                inner,
                inner.x + 2,
                y,
                &(i + 1).to_string(),
                th.s_accent(),
            );
        }
        put(
            buf,
            inner,
            inner.x + 4,
            y,
            if c.current { "●" } else { "○" },
            if c.current { th.s_on() } else { th.s_faint() },
        );
        put(
            buf,
            inner,
            inner.x + 6,
            y,
            &fit(&clean(&c.label), inner.width.saturating_sub(9) as usize),
            th.s_text(),
        );
        if c.pin {
            put(buf, inner, inner.right() - 3, y, "PIN", th.s_warn());
        }
    }
}

// ── Confirm / input ─────────────────────────────────────────────────────────

fn confirm(app: &App, body: Rect, buf: &mut Buffer, c: &Confirm) {
    let th = &app.th;
    let h = &app.house;
    let plan_rows = c.plans.len().min(8);
    let hgt = c.body.len() as u16 + plan_rows as u16 + if c.plans.len() > 8 { 1 } else { 0 } + 5;
    let r = centered(body, 70, hgt);
    let hints = if c.typed.is_some() {
        vec![Hint::new("⏎", "confirm"), Hint::new("Esc", "cancel")]
    } else {
        vec![Hint::new("y", "yes"), Hint::new("N", "no (default)")]
    };
    let nb = NotchBox::new()
        .title(Notch::new(c.title.clone()).styled(th.s_warn().add_modifier(Modifier::BOLD)))
        .hints(hints);
    let inner = boxed(app, r, buf, nb);
    let mut y = inner.y;
    for l in &c.body {
        put(
            buf,
            inner,
            inner.x + 1,
            y,
            &fit(l, inner.width.saturating_sub(2) as usize),
            th.s_text(),
        );
        y += 1;
    }
    for p in c.plans.iter().take(8) {
        let s = format!("· {} — {}", h.display_name(p.cid), p.label);
        put(
            buf,
            inner,
            inner.x + 2,
            y,
            &fit(&s, inner.width.saturating_sub(3) as usize),
            th.s_dim(),
        );
        y += 1;
    }
    if c.plans.len() > 8 {
        put(
            buf,
            inner,
            inner.x + 2,
            y,
            &format!("… and {} more", c.plans.len() - 8),
            th.s_faint(),
        );
        y += 1;
    }
    y += 1;
    match &c.typed {
        Some(word) => {
            put(
                buf,
                inner,
                inner.x + 1,
                y,
                &format!("type {} ›", word),
                th.s_dim(),
            );
            put_line(
                app,
                buf,
                inner,
                inner.x + 9 + width(word) as u16,
                y,
                &c.input,
                false,
                24,
            );
        }
        None => {
            put(
                buf,
                inner,
                inner.x + 1,
                y,
                "Continue? [y/N]",
                th.s_text().add_modifier(Modifier::BOLD),
            );
        }
    }
}

fn input(
    app: &App,
    body: Rect,
    buf: &mut Buffer,
    kind: &InputKind,
    line: &Line,
    err: Option<&str>,
    top: bool,
) {
    let th = &app.th;
    match kind {
        InputKind::Filter { target, .. } => {
            if !top || *target == FilterTarget::Help {
                if *target == FilterTarget::Help && top {
                    // Help's own search bar at the bottom of the help box
                    let y = body.bottom().saturating_sub(2);
                    let r = Rect::new(body.x + 2, y, body.width.saturating_sub(4), 1);
                    clear(buf, r, th.s_base());
                    put(
                        buf,
                        r,
                        r.x,
                        y,
                        "/",
                        th.s_accent().add_modifier(Modifier::BOLD),
                    );
                    put_line(
                        app,
                        buf,
                        r,
                        r.x + 2,
                        y,
                        line,
                        false,
                        r.width.saturating_sub(3) as usize,
                    );
                }
                return;
            }
            // a one-line bar over the bottom border of the screen
            let y = body.bottom() - 1;
            let r = Rect::new(body.x + 1, y, body.width.saturating_sub(2), 1);
            clear(buf, r, th.s_base());
            put(
                buf,
                r,
                r.x + 1,
                y,
                "/",
                th.s_accent().add_modifier(Modifier::BOLD),
            );
            put_line(
                app,
                buf,
                r,
                r.x + 3,
                y,
                line,
                false,
                r.width.saturating_sub(30) as usize,
            );
            let hint = "⏎ keep  Esc clear  ↑↓ move";
            put(
                buf,
                r,
                r.right().saturating_sub(width(hint) as u16 + 1),
                y,
                hint,
                th.s_faint(),
            );
        }
        InputKind::Value { cids, spec } => {
            let title = match cids.as_slice() {
                [c] => app.house.display_name(*c),
                cs => format!("{} controls", cs.len()),
            };
            let r = centered(body, 56, 6);
            let nb = NotchBox::new()
                .title(Notch::new(title))
                .meta(Notch::new("set"))
                .hints(vec![Hint::new("⏎", "set"), Hint::new("Esc", "cancel")]);
            let inner = boxed(app, r, buf, nb);
            let x = put(
                buf,
                inner,
                inner.x + 1,
                inner.y,
                &format!("{} ▏", spec.what),
                th.s_dim(),
            );
            put_line(app, buf, inner, x, inner.y, line, false, 16);
            let range = if spec.text {
                spec.unit.to_string()
            } else {
                format!("{} ({}–{})", spec.unit, spec.lo, spec.hi)
            };
            put(buf, inner, x + 18, inner.y, &range, th.s_dim());
            if let Some(cur) = spec.cur {
                put(
                    buf,
                    inner,
                    inner.x + 1,
                    inner.y + 1,
                    &format!("now {}", fmt_val(&Some(Val::Num(cur)))),
                    th.s_faint(),
                );
            }
            if let Some(e) = err {
                put(
                    buf,
                    inner,
                    inner.x + 1,
                    inner.y + 2,
                    &fit(&format!("✗ {}", e), inner.width.saturating_sub(2) as usize),
                    th.s_crit(),
                );
            }
        }
        InputKind::Pin { plans } => {
            let r = centered(body, 48, 6);
            let title = plans
                .first()
                .map(|p| app.house.display_name(p.cid))
                .unwrap_or_default();
            let nb = NotchBox::new()
                .title(Notch::new(title))
                .meta(Notch::new("PIN"))
                .hints(vec![Hint::new("⏎", "send"), Hint::new("Esc", "cancel")]);
            let inner = boxed(app, r, buf, nb);
            let x = put(buf, inner, inner.x + 1, inner.y, "PIN ▏", th.s_dim());
            put_line(app, buf, inner, x, inner.y, line, true, 16);
            put(
                buf,
                inner,
                inner.x + 1,
                inner.y + 2,
                "never stored or logged",
                th.s_faint(),
            );
        }
    }
}

// ── Contexts / message log ──────────────────────────────────────────────────

fn ctx_switcher(app: &App, body: Rect, buf: &mut Buffer, sel: usize) {
    let th = &app.th;
    let r = centered(body, 44, app.contexts.len() as u16 + 4);
    let nb = NotchBox::new()
        .title(Notch::new("contexts"))
        .hints(vec![Hint::new("⏎", "switch"), Hint::new("Esc", "close")]);
    let inner = boxed(app, r, buf, nb);
    if app.contexts.is_empty() {
        common::empty(app, buf, inner, &["no contexts configured"]);
        return;
    }
    let hgt = inner.height as usize;
    let off = sel.saturating_sub(hgt.saturating_sub(1));
    for (i, c) in app.contexts.iter().enumerate().skip(off).take(hgt) {
        let y = inner.y + (i - off) as u16;
        if i == sel {
            sel_row(app, buf, inner, y);
        }
        let active = *c == app.ctx_name;
        put(
            buf,
            inner,
            inner.x + 2,
            y,
            if active { "◆" } else { " " },
            th.s_accent(),
        );
        put(
            buf,
            inner,
            inner.x + 4,
            y,
            &fit(c, inner.width.saturating_sub(5) as usize),
            if active { th.s_accent() } else { th.s_text() },
        );
    }
}

fn msglog(app: &App, body: Rect, buf: &mut Buffer, scroll: usize) {
    let th = &app.th;
    let r = centered(
        body,
        body.width.saturating_sub(8).min(110),
        body.height.saturating_sub(4),
    );
    let nb = NotchBox::new()
        .title(Notch::new("messages"))
        .meta(Notch::new(format!("{}", app.msglog.len())))
        .hints(vec![Hint::new("c", "clear"), Hint::new("Esc", "close")]);
    let inner = boxed(app, r, buf, nb);
    if app.msglog.is_empty() {
        common::empty(
            app,
            buf,
            inner,
            &["no messages", "failures and reconnects are recorded here"],
        );
        return;
    }
    // newest first, two rows per message when it has a command
    let mut lines: Vec<(Style, String)> = Vec::new();
    for m in app.msglog.iter().rev() {
        let (g, st) = if m.err {
            ("✗", th.s_crit())
        } else {
            ("·", th.s_dim())
        };
        lines.push((
            st,
            format!(
                "{} {}  {} — {}",
                g,
                app.hhmmss(m.t),
                m.what,
                clean(&m.detail)
            ),
        ));
        if let Some(c) = &m.cli {
            lines.push((th.s_faint(), format!("             {}", c)));
        }
    }
    let hgt = inner.height as usize;
    let scroll = scroll.min(lines.len().saturating_sub(hgt));
    for (k, (st, l)) in lines.iter().enumerate().skip(scroll).take(hgt) {
        put(
            buf,
            inner,
            inner.x + 1,
            inner.y + (k - scroll) as u16,
            &fit(l, inner.width.saturating_sub(2) as usize),
            *st,
        );
    }
}

// ── Wiring (§5.9) ───────────────────────────────────────────────────────────

fn val_text(app: &App, uuid: &str) -> Option<String> {
    if let Some(v) = app.store.num(uuid) {
        return Some(fmt_val(&Some(Val::Num(v))));
    }
    app.store
        .text(uuid)
        .map(|t| crate::tui::text::trunc(&clean(t), 12))
}

fn wiring(app: &App, body: Rect, buf: &mut Buffer, w: &WiringState) {
    let th = &app.th;
    let h = &app.house;
    let r = centered(
        body,
        body.width.saturating_sub(4).min(120),
        body.height.saturating_sub(2).min(30),
    );
    let center_cid = h.by_uuid.get(&w.center).copied();
    let (logic, src) = match &app.wiring {
        WiringDoc::Ready(l, s) => (Some(l.clone()), s.clone()),
        _ => (None, String::new()),
    };
    let title = match (&logic, center_cid) {
        (_, Some(c)) => h.display_name(c),
        (Some(l), None) => l
            .blocks
            .get(&w.center)
            .map(|b| clean(&b.title))
            .unwrap_or_else(|| "block".into()),
        _ => "wiring".into(),
    };
    let mut nb = NotchBox::new()
        .title(Notch::new(title))
        .title(Notch::new("wiring").active(true))
        .meta(Notch::new("why is this on?"));
    nb = nb.hints(vec![
        Hint::new("⏎", "follow"),
        Hint::new("h l", "up · downstream"),
        Hint::new("b", "back"),
        Hint::new("e", "events"),
        Hint::new("Esc", "close"),
    ]);
    if !src.is_empty() {
        nb = nb.bottom_right(Notch::new(format!("lxir · {}", src)));
    }
    let inner = boxed(app, r, buf, nb);
    let Some(logic) = logic else {
        let msg: Vec<String> = match &app.wiring {
            WiringDoc::Loading | WiringDoc::None => {
                vec!["loading the Loxone Config program…".into()]
            }
            WiringDoc::Missing => vec![
                "no .Loxone config available".into(),
                "d  download it from the Miniserver via FTP (cached per context)".into(),
                "or run `lox config pull` for a versioned copy".into(),
            ],
            WiringDoc::Failed(e) => vec![
                "could not load the config".into(),
                clean(e),
                "d  retry download".into(),
            ],
            WiringDoc::Ready(..) => vec![],
        };
        let refs: Vec<&str> = msg.iter().map(|s| s.as_str()).collect();
        common::empty(app, buf, inner, &refs);
        return;
    };
    let Some(n) = logic.neighborhood(&w.center) else {
        common::empty(
            app,
            buf,
            inner,
            &[
                "this control is not in the config program",
                "(config may be stale — d re-downloads it)",
            ],
        );
        return;
    };
    let x = inner.x + 1;
    let mut y = inner.y;
    // header: current value, since when, triggered by
    if let Some(c) = center_cid {
        let v = vm::view(&app.store, h, c);
        let mut s = format!(
            "{} {}  {}",
            common::glyph(app, h.ctrls[c].kind),
            h.display_name(c),
            v.value
        );
        let states: HashSet<String> = h.ctrls[c].states.values().cloned().collect();
        let last = app.store.last_before(&states, app.now);
        if let Some(e) = last {
            s.push_str(&format!("   since {}", app.hhmmss(e.t)));
            let inputs: HashSet<String> = n.inputs.iter().map(|w| w.source.clone()).collect();
            if let Some(t) = app.store.last_before(&inputs, e.t + 0.5) {
                let who = t
                    .cid
                    .map(|c| h.display_name(c))
                    .unwrap_or_else(|| t.state.clone());
                s.push_str(&format!(
                    " · triggered by {} {}",
                    who,
                    crate::tui::ui::events::change_text(t)
                ));
            }
        }
        put(
            buf,
            inner,
            x,
            y,
            &fit(&s, inner.width.saturating_sub(2) as usize),
            th.s_text().add_modifier(Modifier::BOLD),
        );
    }
    y += 2;
    // layout: sources | wires → [block] → wires | sinks
    let rows = wiring_rows(app, w);
    let bw = 24u16;
    let bx = inner.x + (inner.width.saturating_sub(bw)) / 2;
    let lw = bx.saturating_sub(inner.x + 1);
    let name_w = (lw / 2).min(22);
    let n_in = n.inputs.len();
    let n_out = n.outputs.len();
    let bh = (n_in.max(n_out) as u16 + 2).min(inner.bottom().saturating_sub(y + 2));
    let block = Rect::new(bx, y, bw, bh);
    let bi = NotchBox::new()
        .title(Notch::new(clean(&n.center.typ)))
        .render(block, buf, th);
    let other_name = |uuid: &str| -> (String, Option<usize>) {
        match h.by_uuid.get(uuid) {
            Some(c) => (h.display_name(*c), Some(*c)),
            None => (
                logic
                    .blocks
                    .get(uuid)
                    .map(|b| clean(&b.title))
                    .filter(|t| !t.is_empty())
                    .unwrap_or_else(|| "(block)".into()),
                None,
            ),
        }
    };
    let latest = app.store.events.back().map(|e| e.uuid.clone());
    // each side scrolls on its own so the selected wire stays visible
    let last = (bi.height as usize).saturating_sub(1);
    let (off_in, off_out) = match rows.get(w.sel) {
        Some((true, _)) => (w.sel.saturating_sub(last), 0),
        Some((false, _)) => (0, (w.sel - n_in).saturating_sub(last)),
        None => (0, 0),
    };
    for (i, (is_in, wire)) in rows.iter().enumerate() {
        let k = if *is_in { i } else { i - n_in };
        let Some(k) = k.checked_sub(if *is_in { off_in } else { off_out }) else {
            continue;
        };
        let ry = bi.y + k as u16;
        if ry >= bi.bottom() {
            continue;
        }
        let selected = i == w.sel;
        let live = val_text(app, &wire.source);
        let fired = latest.as_deref() == Some(wire.source.as_str());
        let wire_st = match (&live, fired) {
            (_, true) => th.s_accent().add_modifier(Modifier::BOLD),
            (Some(_), _) => th.s_info(),
            (None, _) => th.s_faint(),
        };
        let (name, oc) = other_name(&wire.other);
        let g = oc
            .map(|c| common::glyph(app, h.ctrls[c].kind))
            .unwrap_or("·");
        let nst = if selected {
            th.s_accent().add_modifier(Modifier::BOLD)
        } else {
            th.s_text()
        };
        if *is_in {
            put(buf, bi, bi.x, ry, &fit(&wire.key, 8), th.s_dim());
            let wx0 = inner.x + 1 + name_w + 3;
            let wlen = bx.saturating_sub(wx0 + 1);
            draw_wire(buf, inner, wx0, ry, wlen, live.as_deref(), wire_st);
            if selected {
                cell(buf, inner, inner.x, ry, "▌", th.s_accent());
            }
            put(buf, inner, inner.x + 1, ry, g, nst);
            put(
                buf,
                inner,
                inner.x + 3,
                ry,
                &fit(&name, name_w as usize),
                nst,
            );
        } else {
            put(
                buf,
                bi,
                bi.right().saturating_sub(8),
                ry,
                &rfit(&wire.key, 8),
                th.s_dim(),
            );
            let wx0 = block.right() + 1;
            let wlen = (inner.right().saturating_sub(wx0 + name_w + 4)).max(4);
            draw_wire(buf, inner, wx0, ry, wlen, live.as_deref(), wire_st);
            let nx = wx0 + wlen + 1;
            if selected {
                cell(buf, inner, nx.saturating_sub(1), ry, "▌", th.s_accent());
            }
            put(buf, inner, nx, ry, g, nst);
            put(
                buf,
                inner,
                nx + 2,
                ry,
                &fit(&name, (inner.right().saturating_sub(nx + 3)) as usize),
                nst,
            );
        }
    }
    if rows.is_empty() {
        put(buf, bi, bi.x, bi.y, "no wires", th.s_faint());
    }
    let ly = inner.bottom() - 1;
    put(
        buf,
        inner,
        x,
        ly,
        "━━▶ live value from the stream    ┄┄▶ no visualization: topology only",
        th.s_faint(),
    );
}

fn draw_wire(
    buf: &mut Buffer,
    area: Rect,
    x: u16,
    y: u16,
    len: u16,
    value: Option<&str>,
    st: Style,
) {
    if len < 2 {
        return;
    }
    let dash = if value.is_some() { "━" } else { "┄" };
    for i in 0..len - 1 {
        cell(buf, area, x + i, y, dash, st);
    }
    cell(buf, area, x + len - 1, y, "▶", st);
    if let Some(v) = value {
        let v = fit(v, (len as usize).saturating_sub(4).min(12));
        let vw = width(&v) as u16;
        if vw > 0 {
            put(buf, area, x + (len - vw) / 2, y, &v, st);
        }
    }
}
