//! Shared render pieces: control rows, hint notches, scrolling, tones.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use crate::tui::app::{App, Hit, PendState};
use crate::tui::keymap::{self, Cmd, Ctx};
use crate::tui::lists;
use crate::tui::model::{Cid, Kind};
use crate::tui::text::{fit, width};
use crate::tui::theme::{Grad, Theme};
use crate::tui::vm::{self, Tone, Verb};
use crate::tui::widgets::meter::meter;
use crate::tui::widgets::notchbox::Hint;
use crate::tui::widgets::{cell, put};

pub fn tone(th: &Theme, t: Tone) -> Style {
    match t {
        Tone::Text => th.s_text(),
        Tone::Dim => th.s_dim(),
        Tone::Faint => th.s_faint(),
        Tone::Ok => th.s_ok(),
        Tone::Warn => th.s_warn(),
        Tone::Crit => th.s_crit(),
        Tone::Info => th.s_info(),
    }
}

pub fn glyph(app: &App, k: Kind) -> &'static str {
    if app.opts.nerd {
        k.nerd_glyph()
    } else {
        k.glyph()
    }
}

const SPIN: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

/// `⋯` (animated) while sent, `?` unconfirmed, `✗` failed.
pub fn pending_mark(app: &App, cid: Cid) -> Option<(&'static str, Style)> {
    let p = app.pending.get(&cid)?;
    let th = &app.th;
    Some(match &p.state {
        PendState::Sent => {
            let s = if app.opts.motion {
                SPIN[(app.frame as usize) % SPIN.len()]
            } else {
                "⋯"
            };
            (s, th.s_accent())
        }
        PendState::Unconfirmed => ("?", th.s_warn()),
        PendState::Failed(_) => ("✗", th.s_crit()),
    })
}

/// Register a mouse hit area.
pub fn hit(app: &App, r: Rect, h: Hit) {
    app.ui.borrow_mut().hits.push((r, h));
}

/// Scroll offset that keeps `sel` visible, stable across frames.
pub fn offset(app: &App, id: u8, sel: usize, len: usize, height: usize) -> usize {
    let mut ui = app.ui.borrow_mut();
    let off = ui.offsets.entry(id).or_insert(0);
    if height == 0 {
        return 0;
    }
    if sel < *off {
        *off = sel;
    } else if sel >= *off + height {
        *off = sel + 1 - height;
    }
    *off = (*off).min(len.saturating_sub(height));
    *off
}

/// Remember the page size of the focused list (for Ctrl-d / Ctrl-u).
pub fn set_page(app: &App, rows: usize) {
    app.ui.borrow_mut().page = rows;
}

/// Name with fuzzy-match highlighting.
pub fn put_name(
    buf: &mut Buffer,
    area: Rect,
    x: u16,
    y: u16,
    name: &str,
    w: usize,
    st: Style,
    filter: &str,
    th: &Theme,
) -> u16 {
    let idx = lists::fuzzy_indices(filter, name);
    put_marked(buf, area, x, y, name, w, st, &idx, th)
}

/// Like [`put_name`], but marks every case-insensitive occurrence of `needle`
/// (substring search, e.g. the log).
#[allow(clippy::too_many_arguments)]
pub fn put_found(
    buf: &mut Buffer,
    area: Rect,
    x: u16,
    y: u16,
    text: &str,
    w: usize,
    st: Style,
    needle: &str,
    th: &Theme,
) -> u16 {
    let idx = substring_indices(needle, text);
    put_marked(buf, area, x, y, text, w, st, &idx, th)
}

/// Char indices of every case-insensitive occurrence of `needle` in `text`.
pub fn substring_indices(needle: &str, text: &str) -> Vec<usize> {
    let low = |c: char| c.to_lowercase().next().unwrap_or(c);
    let n: Vec<char> = needle.trim().chars().map(low).collect();
    let t: Vec<char> = text.chars().map(low).collect();
    let mut out = Vec::new();
    if n.is_empty() || n.len() > t.len() {
        return out;
    }
    let mut i = 0;
    while i + n.len() <= t.len() {
        if t[i..i + n.len()] == n[..] {
            out.extend(i..i + n.len());
            i += n.len();
        } else {
            i += 1;
        }
    }
    out
}

/// Text fitted to `w` with the chars at `idx` in the match style.
#[allow(clippy::too_many_arguments)]
fn put_marked(
    buf: &mut Buffer,
    area: Rect,
    x: u16,
    y: u16,
    text: &str,
    w: usize,
    st: Style,
    idx: &[usize],
    th: &Theme,
) -> u16 {
    let s = fit(text, w);
    if idx.is_empty() {
        return put(buf, area, x, y, &s, st);
    }
    let mut cx = x;
    for (i, ch) in s.chars().enumerate() {
        let cs = if idx.contains(&i) {
            th.s_accent()
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
        } else {
            st
        };
        cx = put(buf, area, cx, y, &ch.to_string(), cs);
    }
    cx
}

pub struct RowOpts<'a> {
    pub selected: bool,
    pub marked: bool,
    pub depth: u8,
    pub name_w: usize,
    pub show_room: bool,
    pub filter: &'a str,
    pub focus: bool,
}

/// One control row: `▌✓ ● Name   ▕meter▏  value   extra   ⋯`.
pub fn ctrl_row(app: &App, buf: &mut Buffer, area: Rect, y: u16, cid: Cid, o: &RowOpts) {
    let th = &app.th;
    let h = &app.house;
    let row = Rect::new(area.x, y, area.width, 1);
    if o.selected {
        let st = if o.focus {
            th.s_selected()
        } else {
            Style::default()
        };
        crate::tui::widgets::fill(buf, row, st);
        if o.focus {
            cell(buf, area, area.x, y, "▌", th.s_accent());
        }
    }
    let v = vm::view(&app.store, h, cid);
    let c = &h.ctrls[cid];
    let mut x = area.x + 1;
    if o.marked {
        cell(
            buf,
            area,
            x,
            y,
            "✓",
            th.s_accent().add_modifier(Modifier::BOLD),
        );
    }
    x += 1 + o.depth as u16 * 2;
    let gst = match v.on {
        Some(true) => th.s_on(),
        Some(false) => th.s_faint(),
        None => tone(th, v.tone),
    };
    let gst = if v.attention { th.s_warn() } else { gst };
    put(buf, area, x, y, glyph(app, c.kind), gst);
    x += 2;
    let name = if o.show_room {
        match h.room_name(cid) {
            Some(r) => format!("{} · {}", c.name, r),
            None => c.name.clone(),
        }
    } else {
        c.name.clone()
    };
    let name_w = o.name_w.saturating_sub(o.depth as usize * 2);
    let nst = if o.selected && o.focus {
        th.s_text().add_modifier(Modifier::BOLD)
    } else {
        th.s_text()
    };
    put_name(buf, area, x, y, &name, name_w, nst, o.filter, th);
    x += name_w as u16 + 1;
    let rest = area.right().saturating_sub(x) as usize;
    // meter (only when there's room and the control has a level)
    let mw: u16 = if rest >= 44 {
        12
    } else if rest >= 34 {
        8
    } else {
        0
    };
    if mw > 0 {
        if v.frac.is_some() {
            cell(buf, area, x, y, "▕", th.s_faint());
            meter(buf, area, x + 1, y, mw - 2, v.frac, v.grad, th);
            cell(buf, area, x + mw - 1, y, "▏", th.s_faint());
        }
        x += mw + 1;
    }
    let vw = 14usize.min(area.right().saturating_sub(x) as usize);
    let vst = tone(th, v.tone);
    let vst = if !app.live() && !app.opts.demo {
        th.s_dim()
    } else {
        vst
    };
    put(buf, area, x, y, &fit(&v.value, vw), vst);
    x += vw as u16 + 1;
    let pend = pending_mark(app, cid);
    let right_reserved = if pend.is_some() || v.locked { 9 } else { 0 };
    let ew = area.right().saturating_sub(x + right_reserved) as usize;
    if ew > 0 && !v.extra.is_empty() {
        put(buf, area, x, y, &fit(&v.extra, ew), tone(th, v.extra_tone));
    }
    let mut rx = area.right().saturating_sub(2);
    if let Some((m, st)) = pend {
        cell(buf, area, rx, y, m, st);
        rx = rx.saturating_sub(2);
    }
    if v.locked {
        put(buf, area, rx.saturating_sub(5), y, "locked", th.s_warn());
    }
}

/// Hint notches for a control (computed from the live state, §5.1).
pub fn item_hints(app: &App, cid: Cid) -> Vec<Hint> {
    let s = &app.store;
    let h = &app.house;
    let mut out = Vec::new();
    if app.opts.read_only {
        out.push(Hint::new("⏎", "inspect"));
        out.push(Hint::new("w", "wiring"));
        return out;
    }
    let opt = app.pending.get(&cid).and_then(|p| p.optimistic);
    let last = app.last_dir.get(&cid).copied();
    let plan = |v: Verb| vm::verb(s, h, cid, v, opt, last);
    if let Some(p) = plan(Verb::Primary) {
        out.push(Hint::new("␣", p.label));
    }
    if plan(Verb::Plus).is_some() {
        let lab = match h.ctrls[cid].kind {
            Kind::LightCtl | Kind::CentralLight => "mood",
            _ => "step",
        };
        out.push(Hint::new("+-", lab));
    }
    if let (Some(a), Some(b)) = (plan(Verb::Min), plan(Verb::Max)) {
        out.push(Hint::new(
            "<>",
            format!("{} · {}", short(&a.label), short(&b.label)),
        ));
    }
    if vm::set_spec(s, h, cid).is_some() {
        out.push(Hint::new("=", "set"));
    }
    if plan(Verb::Stop).is_some() && plan(Verb::Primary).is_none_or(|p| p.label != "stop") {
        out.push(Hint::new("s", "stop"));
    }
    if vm::modes(s, h, cid).is_some_and(|m| !m.is_empty()) {
        let lab = match h.ctrls[cid].kind {
            Kind::LightCtl | Kind::CentralLight => "mood",
            Kind::Blind => "shade",
            _ => "mode",
        };
        out.push(Hint::new("m", lab));
    }
    if !h.ctrls[cid].kind.is_actuator() {
        out.push(Hint::new("⏎", "inspect"));
    }
    out.push(Hint::new("w", "wiring"));
    out.push(Hint::new("a", "all"));
    out
}

fn short(s: &str) -> String {
    s.trim_start_matches(['▲', '▼', ' ']).to_string()
}

/// The way out of a popup: `Esc` goes back one step (`back` names it) and
/// `q` closes, or `Esc` closes when there is no step left.
pub fn close_hints(back: Option<&str>) -> Vec<Hint> {
    match back {
        Some(b) => vec![Hint::new("Esc", b.to_string()), Hint::new("q", "close")],
        None => vec![Hint::new("Esc", "close")],
    }
}

/// Hints from the keymap for a context (static labels).
pub fn ctx_hints(ctx: Ctx, cmds: &[Cmd]) -> Vec<Hint> {
    cmds.iter()
        .filter_map(|c| {
            let b = keymap::BINDINGS
                .iter()
                .find(|b| b.ctx == ctx && b.cmd == *c)?;
            let short = b.help.split(':').next().unwrap_or(b.help);
            Some(Hint::new(keymap::display(b.keys[0]), short.to_string()))
        })
        .collect()
}

/// The state whose history best represents the control (sparklines, graphs).
pub fn main_state(app: &App, cid: Cid) -> Option<String> {
    let c = &app.house.ctrls[cid];
    let pick = |names: &[&str]| names.iter().find_map(|n| c.state(n)).map(|s| s.to_string());
    match c.kind {
        Kind::Climate => pick(&["tempActual"]),
        Kind::Meter => pick(&["actual", "actualProduction", "actualConsumption"]),
        Kind::Efm => pick(&["Gpwr", "Ppwr"]),
        Kind::Blind | Kind::Gate => pick(&["position"]),
        Kind::Dimmer => pick(&["position", "value"]),
        Kind::Charger => pick(&["power", "actualPower"]),
        Kind::Switch | Kind::Digital | Kind::Presence => pick(&["active"]),
        _ => pick(&["value", "position"]),
    }
}

/// Grad for a control's sparkline.
pub fn spark_grad(app: &App, cid: Cid) -> Grad {
    match app.house.ctrls[cid].kind {
        Kind::Climate | Kind::Analog
            if app.house.ctrls[cid].is_temperature()
                || app.house.ctrls[cid].kind == Kind::Climate =>
        {
            Grad::Temp
        }
        Kind::Meter | Kind::Efm | Kind::Charger => Grad::Use,
        Kind::Dimmer | Kind::LightCtl | Kind::ColorPicker => Grad::Lamp,
        _ => Grad::Info,
    }
}

/// Section header row inside a list.
pub fn header_row(app: &App, buf: &mut Buffer, area: Rect, y: u16, label: &str) {
    let th = &app.th;
    put(
        buf,
        area,
        area.x + 1,
        y,
        label,
        th.s_dim().add_modifier(Modifier::BOLD),
    );
}

/// Centered empty-state text.
pub fn empty(app: &App, buf: &mut Buffer, area: Rect, lines: &[&str]) {
    let th = &app.th;
    let y0 = area.y + area.height.saturating_sub(lines.len() as u16) / 2;
    for (i, l) in lines.iter().enumerate() {
        let w = width(l) as u16;
        let x = area.x + area.width.saturating_sub(w) / 2;
        put(
            buf,
            area,
            x,
            y0 + i as u16,
            l,
            if i == 0 { th.s_text() } else { th.s_dim() },
        );
    }
}

/// Age text for polled values older than two intervals (§6.3).
pub fn age(app: &App, t: f64, interval: f64) -> Option<String> {
    let a = app.now - t;
    (a > interval * 2.0).then(|| format!("{} ago", crate::tui::text::fmt_age(a as u64)))
}
