//! Overlays (§5.8): help, palette, menu, picker, confirm, input, contexts,
//! message log, wiring (§5.9) and the compact inspector.

use std::collections::HashSet;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use super::common;
use super::inspector;
use crate::tui::app::{
    App, BrowseState, Confirm, FacetList, FilterTarget, InputKind, Line, Overlay, PalItem, Trace,
    WiringDoc, WiringState,
};
use crate::tui::keymap::{self, BINDINGS, Ctx};
use crate::tui::lists;
use crate::tui::palette;
use crate::tui::store::{Val, fmt_val};
use crate::tui::text::{clean, fit, rfit, trunc, width};
use crate::tui::update::{
    BrowseRows, WRow, browse_rows, browse_source, contexts, wiring_doc, wiring_rows,
};
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
            Overlay::Facets { list, line, sel } => facets_box(app, body, buf, *list, line, *sel),
            Overlay::MsgLog { scroll } => msglog(app, body, buf, *scroll),
            Overlay::Wiring(w) => wiring(app, body, buf, w),
            Overlay::Browse(b) => browse(app, body, buf, b),
            Overlay::Chart(c) => super::chart::render(app, body, buf, c),
            Overlay::Value { cid, state, scroll } => {
                let text = crate::tui::update::state_text(app, *cid, state, true)
                    .unwrap_or_else(|| "—".into());
                let name = app.house.display_name(*cid);
                text_box(app, body, buf, &name, state, &text, *scroll, "copy value")
            }
            Overlay::Text {
                title,
                sub,
                text,
                scroll,
            } => text_box(app, body, buf, title, sub, text, *scroll, "copy"),
            Overlay::Inspector { cid, scroll } => {
                let w = (body.width * 2 / 3).clamp(60.min(body.width), 90.min(body.width));
                let h = body.height.saturating_sub(2).min(34);
                let r = centered(body, w, h);
                clear(buf, r, app.th.s_base());
                inspector::render(app, r, buf, *cid, top, top.then_some(*scroll));
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

// ── Value viewer ────────────────────────────────────────────────────────────

/// Wrap `text` to `w` columns (hard breaks at the width; keeps its own lines).
pub fn wrap(text: &str, w: usize) -> Vec<String> {
    let w = w.max(1);
    let mut out = Vec::new();
    for line in text.lines() {
        let chars: Vec<char> = line.chars().collect();
        if chars.is_empty() {
            out.push(String::new());
        }
        for chunk in chars.chunks(w) {
            out.push(chunk.iter().collect());
        }
    }
    out
}

/// Full text in a scrollable box: `title` plus `sub` as the active notch.
#[allow(clippy::too_many_arguments)]
fn text_box(
    app: &App,
    body: Rect,
    buf: &mut Buffer,
    title: &str,
    sub: &str,
    text: &str,
    scroll: usize,
    copy: &str,
) {
    let th = &app.th;
    let w = body.width.saturating_sub(6).min(110);
    let lines = wrap(text, w.saturating_sub(4) as usize);
    let hgt = (lines.len() as u16 + 2).clamp(5, body.height.saturating_sub(2));
    let r = centered(body, w, hgt);
    let vis = hgt.saturating_sub(2) as usize;
    let off = scroll.min(lines.len().saturating_sub(vis));
    let mut nb = NotchBox::new().title(Notch::new(title.to_string()));
    if !sub.is_empty() {
        nb = nb.title(Notch::new(sub.to_string()).active(true));
    }
    nb = nb.hints(vec![
        Hint::new("jk", "scroll"),
        Hint::new("y", copy.to_string()),
        Hint::new("Esc", "close"),
    ]);
    if lines.len() > vis {
        nb = nb.position(format!(
            "{}–{}/{}",
            off + 1,
            (off + vis).min(lines.len()),
            lines.len()
        ));
    }
    let inner = boxed(app, r, buf, nb);
    for (i, l) in lines.iter().skip(off).take(vis).enumerate() {
        put(buf, inner, inner.x + 1, inner.y + i as u16, l, th.s_text());
    }
}

// ── Facets ──────────────────────────────────────────────────────────────────

fn facets_box(app: &App, body: Rect, buf: &mut Buffer, list: FacetList, line: &Line, sel: usize) {
    let th = &app.th;
    let opts = lists::facet_options(app, list, &line.buf);
    let r = centered(body, 56, body.height.saturating_sub(4).min(26));
    let n_active = lists::facets_of(app, list).len();
    let mut nb = NotchBox::new()
        .title(Notch::new(match list {
            FacetList::Controls => "facets · controls",
            FacetList::Events => "facets · events",
        }))
        .hints(vec![
            Hint::new("␣", "toggle"),
            Hint::new("⏎", "toggle+close"),
            Hint::new("⇥", "group"),
            Hint::new("C-x", "clear"),
        ])
        .position(format!("{}", opts.len()));
    if n_active > 0 {
        nb = nb.meta(Notch::new(format!("{} active", n_active)).active(true));
    }
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
            "type to narrow — rooms, types, states…",
            th.s_faint(),
        );
    }
    let area = Rect::new(
        inner.x,
        inner.y + 2,
        inner.width,
        inner.height.saturating_sub(2),
    );
    // rows with section headers
    let mut rows: Vec<Option<usize>> = Vec::new();
    let mut heads: Vec<&str> = Vec::new();
    let mut last = None;
    for (i, o) in opts.iter().enumerate() {
        let sec = if o.suggested {
            "THIS"
        } else {
            o.facet.dim().title()
        };
        if last != Some(sec) {
            rows.push(None);
            heads.push(sec);
            last = Some(sec);
        }
        rows.push(Some(i));
    }
    let hgt = area.height as usize;
    let sel_i = rows.iter().position(|r| *r == Some(sel)).unwrap_or(0);
    let off = sel_i.saturating_sub(hgt.saturating_sub(1));
    let mut head = heads.iter();
    // headers before the scroll offset
    for r in rows.iter().take(off) {
        if r.is_none() {
            head.next();
        }
    }
    for (k, r) in rows.iter().enumerate().skip(off).take(hgt) {
        let y = area.y + (k - off) as u16;
        let Some(i) = r else {
            if let Some(h) = head.next() {
                put(
                    buf,
                    area,
                    area.x + 1,
                    y,
                    h,
                    th.s_dim().add_modifier(Modifier::BOLD),
                );
            }
            continue;
        };
        let o = &opts[*i];
        if *i == sel {
            sel_row(app, buf, area, y);
        }
        let (mark, mst) = if o.active {
            ("●", th.s_accent())
        } else {
            ("○", th.s_faint())
        };
        put(buf, area, area.x + 2, y, mark, mst);
        let cnt = o.count.to_string();
        let lw = (area.width as usize).saturating_sub(cnt.len() + 6);
        let st = if o.active {
            th.s_text().add_modifier(Modifier::BOLD)
        } else {
            th.s_text()
        };
        common::put_name(buf, area, area.x + 4, y, &o.label, lw, st, &line.buf, th);
        put(
            buf,
            area,
            area.right().saturating_sub(cnt.len() as u16 + 1),
            y,
            &cnt,
            th.s_dim(),
        );
    }
    if opts.is_empty() {
        common::empty(app, buf, area, &["no match"]);
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
        InputKind::ChartAdd => {
            let cands = lists::chart_candidates(app, &line.buf);
            let r = centered(body, 60, 10);
            let nb = NotchBox::new().title(Notch::new("add series")).hints(vec![
                Hint::new("⏎", "add best match"),
                Hint::new("Esc", "cancel"),
            ]);
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
            if let Some(e) = err {
                put(
                    buf,
                    inner,
                    inner.x + 1,
                    inner.y + 1,
                    &format!("✗ {}", e),
                    th.s_crit(),
                );
            }
            for (i, c) in cands
                .iter()
                .take(inner.height.saturating_sub(2) as usize)
                .enumerate()
            {
                let y = inner.y + 2 + i as u16;
                if i == 0 {
                    sel_row(app, buf, inner, y);
                }
                let room = app.house.room_name(*c).unwrap_or("");
                let rw = width(room).min(20);
                common::put_name(
                    buf,
                    inner,
                    inner.x + 2,
                    y,
                    &app.house.display_name(*c),
                    (inner.width as usize).saturating_sub(rw + 5),
                    th.s_text(),
                    &line.buf,
                    th,
                );
                put(
                    buf,
                    inner,
                    inner.right().saturating_sub(rw as u16 + 1),
                    y,
                    &rfit(room, rw),
                    th.s_dim(),
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
        // a control's main value in its own format (`21.5°`, `40 %`)
        if let Some((cid, st)) = app.house.state_owner.get(uuid)
            && st == "value"
            && let Some(f) = &app.house.ctrls[*cid].format
        {
            return Some(crate::tui::text::lox_format(f, v));
        }
        return Some(fmt_val(&Some(Val::Num(v))));
    }
    app.store.text(uuid).map(|t| trunc(&clean(t), 12))
}

/// Where a block lives: `device · room` for hardware, `page P · room` for
/// logic (leaving out `skip_page`, the page of the center).
fn block_place(b: &crate::logic::Block, skip_page: Option<&str>) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(d) = &b.device {
        parts.push(clean(d));
    } else if let Some(p) = b.page.as_deref().filter(|p| Some(*p) != skip_page) {
        parts.push(format!("page {}", clean(p)));
    }
    // pages are often named after their room: say it once
    if let Some(r) = &b.room
        && !parts.iter().any(|p| p == r)
        && b.page.as_deref() != Some(r.as_str())
    {
        parts.push(clean(r));
    }
    parts.join(" · ")
}

fn wiring(app: &App, body: Rect, buf: &mut Buffer, w: &WiringState) {
    let th = &app.th;
    let h = &app.house;
    let r = centered(
        body,
        body.width.saturating_sub(4).min(120),
        body.height.saturating_sub(2).min(32),
    );
    let doc = wiring_doc(app, w);
    let live = doc.as_ref().is_none_or(|d| d.live);
    let center_cid = h.by_uuid.get(&w.center).copied().filter(|_| live);
    let title = match (&doc, center_cid) {
        (_, Some(c)) => h.display_name(c),
        (Some(d), None) => d
            .logic
            .blocks
            .get(&w.center)
            .map(|b| clean(&b.title))
            .unwrap_or_else(|| "block".into()),
        _ => "wiring".into(),
    };
    let mode = match w.trace {
        Trace::Off => "wiring",
        Trace::Up => "trace ← sensors",
        Trace::Down => "trace → actuators",
    };
    let mut nb = NotchBox::new()
        .title(Notch::new(title))
        .title(Notch::new(mode).active(true));
    nb = match &doc {
        Some(d) if !d.live => nb.meta(Notch::new(format!("◷ snapshot {}", clean(&d.label)))),
        _ => nb.meta(Notch::new("why is this on?")),
    };
    nb = nb.hints(vec![
        Hint::new("⏎", "follow"),
        Hint::new("h l", "up · down"),
        Hint::new("t", "trace"),
        Hint::new("o", "page"),
        Hint::new("/", "find"),
        Hint::new("H", "history"),
        Hint::new("p", "params"),
        Hint::new("b", "back"),
        Hint::new("Esc", "close"),
    ]);
    if let Some(d) = doc.as_ref().filter(|d| d.live) {
        nb = nb.bottom_right(Notch::new(format!("lxir · {}", clean(&d.label))));
    }
    let inner = boxed(app, r, buf, nb);
    let Some(doc) = doc else {
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
    let logic = doc.logic.clone();
    let Some(n) = logic.neighborhood(&w.center) else {
        common::empty(
            app,
            buf,
            inner,
            &[
                "this control is not in the config program",
                if live {
                    "(config may be stale — d re-downloads it)"
                } else {
                    "(it does not exist in this snapshot)"
                },
            ],
        );
        return;
    };
    let center_page = n.center.page.clone();
    let x = inner.x + 1;
    let tw = inner.width.saturating_sub(2) as usize;
    let mut y = inner.y;
    // header: current value, since when, triggered by (live only)
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
            &fit(&s, tw),
            th.s_text().add_modifier(Modifier::BOLD),
        );
    } else {
        let s = if live {
            clean(&n.center.title)
        } else {
            format!(
                "{}  · values are not live in a snapshot",
                clean(&n.center.title)
            )
        };
        put(
            buf,
            inner,
            x,
            y,
            &fit(&s, tw),
            th.s_text().add_modifier(Modifier::BOLD),
        );
    }
    y += 1;
    // what the block is and where it lives
    let mut what = vec![clean(&n.center.typ)];
    let place = block_place(&n.center, None);
    if !place.is_empty() {
        what.push(place);
    }
    put(buf, inner, x, y, &fit(&what.join(" · "), tw), th.s_faint());
    y += 1;
    // the parameters someone set (p: all of them)
    let params: Vec<String> = n
        .center
        .shown_params(w.all_params)
        .map(|p| {
            format!(
                "{} {}",
                p.key,
                crate::logic::param_text(&n.center.typ, &p.key, &p.value)
            )
        })
        .collect();
    if !params.is_empty() {
        let lead = if w.all_params { "params " } else { "set " };
        let x2 = put(buf, inner, x, y, lead, th.s_faint());
        put(
            buf,
            inner,
            x2,
            y,
            &fit(&params.join(" · "), tw.saturating_sub(lead.len())),
            th.s_dim(),
        );
    } else if n.center.params.iter().any(|p| p.default) {
        put(
            buf,
            inner,
            x,
            y,
            "all parameters at defaults · p shows them",
            th.s_faint(),
        );
    }
    y += 2;
    let rows = wiring_rows(app, w);
    let foot = inner.bottom().saturating_sub(3);
    let body_r = Rect::new(inner.x, y, inner.width, foot.saturating_sub(y));
    let other_name = |uuid: &str| -> (String, Option<usize>) {
        match h.by_uuid.get(uuid).filter(|_| live) {
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
    let value_of = |src: &str| if live { val_text(app, src) } else { None };
    if w.trace != Trace::Off {
        wiring_trace(app, buf, body_r, w, &rows, &logic, &other_name, &value_of);
    } else {
        wiring_diagram(
            app,
            buf,
            inner,
            body_r,
            w,
            &rows,
            &n,
            &other_name,
            &value_of,
        );
    }
    // the selected wire in full: where the other end lives, what else it drives
    if let Some(row) = rows.get(w.sel)
        && let Some(b) = logic.blocks.get(&row.wire.other)
    {
        let dy = foot;
        let arrow = if row.input { "←" } else { "→" };
        let mut s = format!(
            "{} {} {}.{}",
            row.wire.key,
            arrow,
            clean(&b.title),
            row.wire.other_key
        );
        let place = block_place(b, center_page.as_deref());
        if !place.is_empty() {
            s.push_str(&format!("  · {}", place));
        }
        if !b.is_ref() && b.device.is_none() && b.page.is_none() {
            s.push_str(&format!("  · {}", clean(&b.typ)));
        }
        put(buf, inner, x, dy, &fit(&s, tw), th.s_text());
        if !row.wire.also.is_empty() {
            let names: Vec<String> = row
                .wire
                .also
                .iter()
                .map(|a| logic.label(a))
                .map(|t| clean(&t))
                .collect();
            let t = format!("   also drives {}", names.join(", "));
            put(buf, inner, x, dy + 1, &fit(&t, tw), th.s_dim());
        }
    }
    let ly = inner.bottom() - 1;
    let legend = if live {
        "━━▶ live value from the stream    ┄┄▶ no visualization: topology only"
    } else {
        "┄┄▶ snapshot: topology only, no live values"
    };
    put(buf, inner, x, ly, &fit(legend, tw), th.s_faint());
}

/// The one-hop view: sources | wires → [block] → wires | sinks
#[allow(clippy::too_many_arguments)]
fn wiring_diagram(
    app: &App,
    buf: &mut Buffer,
    inner: Rect,
    area: Rect,
    w: &WiringState,
    rows: &[WRow],
    n: &crate::logic::Neighborhood,
    other_name: &dyn Fn(&str) -> (String, Option<usize>),
    value_of: &dyn Fn(&str) -> Option<String>,
) {
    let th = &app.th;
    let h = &app.house;
    let y = area.y;
    let bw = 24u16;
    let bx = inner.x + (inner.width.saturating_sub(bw)) / 2;
    let lw = bx.saturating_sub(inner.x + 1);
    let name_w = (lw / 2).min(22);
    let n_in = n.inputs.len();
    let n_out = n.outputs.len();
    let bh = (n_in.max(n_out) as u16 + 2).min(area.height);
    if bh < 3 {
        return;
    }
    let block = Rect::new(bx, y, bw, bh);
    let bi = NotchBox::new()
        .title(Notch::new(clean(&n.center.typ)))
        .render(block, buf, th);
    let latest = app.store.events.back().map(|e| e.uuid.clone());
    // connector names: the whole block width when only one side is wired
    let kw = if n_in > 0 && n_out > 0 {
        (bi.width as usize).saturating_sub(1) / 2
    } else {
        bi.width as usize
    };
    // each side scrolls on its own so the selected wire stays visible
    let last = (bi.height as usize).saturating_sub(1);
    let (off_in, off_out) = match rows.get(w.sel) {
        Some(r) if r.input => (w.sel.saturating_sub(last), 0),
        Some(_) => (0, (w.sel - n_in).saturating_sub(last)),
        None => (0, 0),
    };
    for (i, row) in rows.iter().enumerate() {
        let (is_in, wire) = (row.input, &row.wire);
        let k = if is_in { i } else { i - n_in };
        let Some(k) = k.checked_sub(if is_in { off_in } else { off_out }) else {
            continue;
        };
        let ry = bi.y + k as u16;
        if ry >= bi.bottom() {
            continue;
        }
        let selected = i == w.sel;
        let live = value_of(&wire.source);
        let fired = latest.as_deref() == Some(wire.source.as_str()) && live.is_some();
        let wire_st = match (&live, fired) {
            (_, true) => th.s_accent().add_modifier(Modifier::BOLD),
            (Some(_), _) => th.s_info(),
            (None, _) => th.s_faint(),
        };
        let (mut name, oc) = other_name(&wire.other);
        // fan-out: the same source drives more blocks
        if !wire.also.is_empty() {
            name = format!("{} +{}", name, wire.also.len());
        }
        let g = oc
            .map(|c| common::glyph(app, h.ctrls[c].kind))
            .unwrap_or("·");
        let nst = if selected {
            th.s_accent().add_modifier(Modifier::BOLD)
        } else {
            th.s_text()
        };
        if is_in {
            put(buf, bi, bi.x, ry, &trunc(&wire.key, kw), th.s_dim());
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
                bi.right().saturating_sub(kw as u16),
                ry,
                &rfit(&wire.key, kw),
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
}

/// `t`: the whole path up to the sensors or down to the actuators, as a tree.
#[allow(clippy::too_many_arguments)]
fn wiring_trace(
    app: &App,
    buf: &mut Buffer,
    area: Rect,
    w: &WiringState,
    rows: &[WRow],
    logic: &crate::logic::Logic,
    other_name: &dyn Fn(&str) -> (String, Option<usize>),
    value_of: &dyn Fn(&str) -> Option<String>,
) {
    let th = &app.th;
    if rows.is_empty() {
        common::empty(
            app,
            buf,
            area,
            &[if w.trace == Trace::Up {
                "nothing is wired to its inputs"
            } else {
                "its outputs drive nothing"
            }],
        );
        return;
    }
    let hgt = area.height as usize;
    let off = w.sel.saturating_sub(hgt.saturating_sub(1));
    let tw = area.width.saturating_sub(3) as usize;
    for (i, r) in rows.iter().enumerate().skip(off).take(hgt) {
        let y = area.y + (i - off) as u16;
        let selected = i == w.sel;
        if selected {
            fill(buf, Rect::new(area.x, y, area.width, 1), th.s_selected());
            cell(buf, area, area.x, y, "▌", th.s_accent());
        }
        let arrow = if r.input { "←" } else { "→" };
        let (name, _) = other_name(&r.wire.other);
        let mut s = format!(
            "{}{} {} {}.{}",
            "   ".repeat(r.depth.saturating_sub(1)),
            r.wire.key,
            arrow,
            name,
            r.wire.other_key
        );
        if r.cycle {
            s.push_str("  ↺ loop");
        }
        let x = put(
            buf,
            area,
            area.x + 2,
            y,
            &trunc(&s, tw),
            if selected {
                th.s_accent().add_modifier(Modifier::BOLD)
            } else {
                th.s_text()
            },
        );
        let mut rest = String::new();
        if let Some(v) = value_of(&r.wire.source) {
            rest.push_str(&format!("  = {}", v));
        }
        if let Some(b) = logic.blocks.get(&r.wire.other) {
            let p = block_place(b, None);
            if !p.is_empty() {
                rest.push_str(&format!("  · {}", p));
            }
        }
        let left = area.right().saturating_sub(x + 1) as usize;
        put(buf, area, x, y, &fit(&rest, left), th.s_faint());
    }
}

// ── Config browser ──────────────────────────────────────────────────────────

fn browse(app: &App, body: Rect, buf: &mut Buffer, b: &BrowseState) {
    let th = &app.th;
    let r = centered(
        body,
        body.width.saturating_sub(4).min(120),
        body.height.saturating_sub(2).min(36),
    );
    let src = browse_source(b);
    let rows = browse_rows(b, &src);
    let crumb = match (&b.search, &b.page) {
        (Some(_), _) => "find".to_string(),
        (None, Some(p)) if b.source => format!("{} · lxir", clean(p)),
        (None, Some(p)) => clean(p),
        (None, None) => "pages".into(),
    };
    let mut nb = NotchBox::new()
        .title(Notch::new("config"))
        .title(Notch::new(crumb).active(true))
        .meta(Notch::new(if b.doc.live {
            clean(&b.doc.label)
        } else {
            format!("◷ snapshot {}", clean(&b.doc.label))
        }));
    nb = nb.hints(match (&b.search, &b.page) {
        (Some(_), _) => vec![
            Hint::new("↑↓", "select"),
            Hint::new("⏎", "wiring"),
            Hint::new("Esc", "back"),
        ],
        (None, Some(_)) => vec![
            Hint::new("⏎", "wiring"),
            Hint::new("s", if b.source { "blocks" } else { "lxir source" }),
            Hint::new("/", "find"),
            Hint::new("Esc", "pages"),
            Hint::new("q", "close"),
        ],
        (None, None) => vec![
            Hint::new("⏎", "open page"),
            Hint::new("/", "find a block"),
            Hint::new("Esc", "close"),
        ],
    });
    let inner = boxed(app, r, buf, nb);
    let tw = inner.width.saturating_sub(2) as usize;
    let mut y = inner.y;
    if let Some(q) = &b.search {
        let x = put(buf, inner, inner.x + 1, y, "/ ", th.s_accent());
        put_line(app, buf, inner, x, y, q, false, tw.saturating_sub(2));
        y += 2;
    }
    let list = Rect::new(inner.x, y, inner.width, inner.bottom().saturating_sub(y));
    let hgt = list.height as usize;
    common::set_page(app, hgt);
    let sel_row = |i: usize, y: u16| {
        if i == b.sel {
            (true, Rect::new(list.x, y, list.width, 1))
        } else {
            (false, Rect::new(list.x, y, list.width, 1))
        }
    };
    match rows {
        BrowseRows::Pages(pages) => {
            if pages.is_empty() {
                common::empty(app, buf, list, &["this config has no logic pages"]);
                return;
            }
            let nw = pages
                .iter()
                .map(|p| width(&p.title))
                .max()
                .unwrap_or(4)
                .min(48);
            let off = b.sel.saturating_sub(hgt.saturating_sub(1));
            for (i, p) in pages.iter().enumerate().skip(off).take(hgt) {
                let ry = list.y + (i - off) as u16;
                let (on, row) = sel_row(i, ry);
                if on {
                    fill(buf, row, th.s_selected());
                    cell(buf, list, list.x, ry, "▌", th.s_accent());
                }
                let x = put(
                    buf,
                    list,
                    list.x + 2,
                    ry,
                    &fit(&clean(&p.title), nw),
                    if on { th.s_accent() } else { th.s_text() },
                );
                put(
                    buf,
                    list,
                    x + 2,
                    ry,
                    &format!("{:>4} blocks  {:>4} wires", p.blocks, p.wires),
                    th.s_faint(),
                );
            }
        }
        BrowseRows::Blocks(blocks) => {
            if blocks.is_empty() {
                let msg = match &b.search {
                    Some(q) if q.buf.trim().is_empty() => {
                        "type to search names, types, devices, rooms and pages"
                    }
                    Some(_) => "no block matches",
                    None => "no blocks on this page",
                };
                common::empty(app, buf, list, &[msg]);
                return;
            }
            let off = b.sel.saturating_sub(hgt.saturating_sub(1));
            let nw = (tw / 2).min(40);
            for (i, x) in blocks.iter().enumerate().skip(off).take(hgt) {
                let ry = list.y + (i - off) as u16;
                let (on, row) = sel_row(i, ry);
                if on {
                    fill(buf, row, th.s_selected());
                    cell(buf, list, list.x, ry, "▌", th.s_accent());
                }
                let cx = put(
                    buf,
                    list,
                    list.x + 2,
                    ry,
                    &fit(&clean(&x.title), nw),
                    if on { th.s_accent() } else { th.s_text() },
                );
                let skip = b.page.as_deref().filter(|_| b.search.is_none());
                let rest = format!("{}  {}", clean(&x.typ), block_place(x, skip));
                let left = list.right().saturating_sub(cx + 3) as usize;
                put(buf, list, cx + 2, ry, &fit(&rest, left), th.s_faint());
            }
        }
        BrowseRows::Source(lines) => {
            let off = b.scroll.min(lines.len().saturating_sub(hgt));
            for (i, line) in lines.iter().enumerate().skip(off).take(hgt) {
                let ry = list.y + (i - off) as u16;
                let line = line.replace('\t', "    ");
                // comments faint, declarations' names accented
                let (code, comment) = match line.find(" #") {
                    Some(p) => (&line[..p], &line[p..]),
                    None if line.trim_start().starts_with('#') => ("", line.as_str()),
                    None => (line.as_str(), ""),
                };
                let room = |x: u16| list.right().saturating_sub(x) as usize;
                let mut x = list.x + 1;
                if let Some((name, rest)) = code.split_once(" = ") {
                    x = put(buf, list, x, ry, &trunc(name, room(x)), th.s_accent());
                    x = put(buf, list, x, ry, " = ", th.s_faint());
                    x = put(buf, list, x, ry, &trunc(rest, room(x)), th.s_text());
                } else {
                    x = put(buf, list, x, ry, &trunc(code, room(x)), th.s_text());
                }
                put(buf, list, x, ry, &trunc(comment, room(x)), th.s_faint());
            }
        }
    }
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
