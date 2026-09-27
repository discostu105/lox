//! NotchBox — the only pane primitive (§5.1, §6.3).
//!
//! ```text
//! ╭┐²rooms┌┐by room┌────────────── ┐14┌─╮
//! │                                      │
//! ╰┘␣ stop└┘+- step└┘= set└──────┘5/14└─╯
//! ```
//! Title/tab notches on top (hotkey letter highlighted), right-aligned meta notches,
//! hint notches at the bottom of the focused pane only, and a position notch.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use super::{cell, put_clip};
use crate::tui::text;
use crate::tui::theme::Theme;

/// One notch in a border.
#[derive(Debug, Clone, Default)]
pub struct Notch {
    pub text: String,
    /// Byte index of the hotkey character to highlight, if any
    pub hot: Option<usize>,
    /// Emphasized (active tab / pane title)
    pub active: bool,
    /// Optional custom style for the whole text
    pub style: Option<Style>,
}

impl Notch {
    pub fn new(text: impl Into<String>) -> Self {
        Notch {
            text: text.into(),
            ..Default::default()
        }
    }
    /// A notch whose hotkey is the first occurrence of `key` in the text
    /// (or a leading key prefix when it doesn't occur).
    pub fn hot(text: impl Into<String>, key: char) -> Self {
        let text = text.into();
        match text.find(key) {
            Some(i) => Notch {
                text,
                hot: Some(i),
                ..Default::default()
            },
            None => Notch {
                text: format!("{} {}", key, text),
                hot: Some(0),
                ..Default::default()
            },
        }
    }
    pub fn active(mut self, on: bool) -> Self {
        self.active = on;
        self
    }
    pub fn styled(mut self, s: Style) -> Self {
        self.style = Some(s);
        self
    }
    fn width(&self) -> usize {
        text::width(&self.text) + 2
    }
}

/// A key hint in the bottom border: `┘key label└`.
#[derive(Debug, Clone)]
pub struct Hint {
    pub key: String,
    pub label: String,
}

impl Hint {
    pub fn new(key: impl Into<String>, label: impl Into<String>) -> Self {
        Hint {
            key: key.into(),
            label: label.into(),
        }
    }
    fn width(&self) -> usize {
        text::width(&self.key) + 1 + text::width(&self.label) + 2
    }
}

#[derive(Default)]
pub struct NotchBox {
    pub title: Vec<Notch>,
    pub meta: Vec<Notch>,
    pub hints: Vec<Hint>,
    pub position: Option<String>,
    pub bottom_right: Vec<Notch>,
    pub focus: bool,
}

struct Glyphs {
    tl: &'static str,
    tr: &'static str,
    bl: &'static str,
    br: &'static str,
    h: &'static str,
    v: &'static str,
    // notch edges: top-left-of-notch, top-right-of-notch, bottom-left, bottom-right
    nl: &'static str,
    nr: &'static str,
    bnl: &'static str,
    bnr: &'static str,
}

const ROUND: Glyphs = Glyphs {
    tl: "╭",
    tr: "╮",
    bl: "╰",
    br: "╯",
    h: "─",
    v: "│",
    nl: "┐",
    nr: "┌",
    bnl: "┘",
    bnr: "└",
};

const HEAVY: Glyphs = Glyphs {
    tl: "┏",
    tr: "┓",
    bl: "┗",
    br: "┛",
    h: "━",
    v: "┃",
    nl: "┓",
    nr: "┏",
    bnl: "┛",
    bnr: "┗",
};

impl NotchBox {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn title(mut self, n: Notch) -> Self {
        self.title.push(n);
        self
    }
    pub fn meta(mut self, n: Notch) -> Self {
        self.meta.push(n);
        self
    }
    pub fn hints(mut self, h: Vec<Hint>) -> Self {
        self.hints = h;
        self
    }
    pub fn position(mut self, p: impl Into<String>) -> Self {
        self.position = Some(p.into());
        self
    }
    pub fn bottom_right(mut self, n: Notch) -> Self {
        self.bottom_right.push(n);
        self
    }
    pub fn focus(mut self, f: bool) -> Self {
        self.focus = f;
        self
    }

    /// Draw the box and return the inner area.
    pub fn render(&self, area: Rect, buf: &mut Buffer, th: &Theme) -> Rect {
        if area.width < 4 || area.height < 2 {
            return Rect::new(area.x, area.y, 0, 0);
        }
        // Mono: focus must survive without color → heavy borders on the focused pane.
        let g = if th.is_mono() && self.focus {
            &HEAVY
        } else {
            &ROUND
        };
        let bs = th.s_border(self.focus);
        let (x0, y0) = (area.x, area.y);
        let (x1, y1) = (area.right() - 1, area.bottom() - 1);
        for x in x0 + 1..x1 {
            cell(buf, area, x, y0, g.h, bs);
            cell(buf, area, x, y1, g.h, bs);
        }
        for y in y0 + 1..y1 {
            cell(buf, area, x0, y, g.v, bs);
            cell(buf, area, x1, y, g.v, bs);
        }
        cell(buf, area, x0, y0, g.tl, bs);
        cell(buf, area, x1, y0, g.tr, bs);
        cell(buf, area, x0, y1, g.bl, bs);
        cell(buf, area, x1, y1, g.br, bs);

        let inner_w = area.width.saturating_sub(2) as usize;

        // right-aligned meta notches on top
        let meta_w: usize = self.meta.iter().map(|n| n.width()).sum();
        let mut right_start = x1 as usize;
        if meta_w + 2 <= inner_w {
            let mut x = (x1 as usize - 1 - meta_w) as u16;
            right_start = x as usize;
            for n in &self.meta {
                x = self.draw_notch(buf, area, x, y0, n, g.nl, g.nr, th, false);
            }
        }
        // title notches from the left, clipped before the meta notches
        let mut x = x0 + 1;
        for (i, n) in self.title.iter().enumerate() {
            if x as usize + n.width() + 1 > right_start {
                // try a truncated version of the first notch only
                if i == 0 {
                    let room = right_start.saturating_sub(x as usize + 3);
                    if room >= 3 {
                        let short = Notch {
                            text: text::trunc(&n.text, room),
                            hot: None,
                            active: n.active,
                            style: n.style,
                        };
                        self.draw_notch(buf, area, x, y0, &short, g.nl, g.nr, th, i == 0);
                    }
                }
                break;
            }
            x = self.draw_notch(buf, area, x, y0, n, g.nl, g.nr, th, i == 0);
        }

        // bottom: position (+ custom) notches right-aligned
        let mut br: Vec<Notch> = self.bottom_right.clone();
        if let Some(p) = &self.position {
            br.push(Notch::new(p.clone()));
        }
        let br_w: usize = br.iter().map(|n| n.width()).sum();
        let mut hint_limit = x1 as usize;
        if br_w + 2 <= inner_w {
            let mut x = (x1 as usize - 1 - br_w) as u16;
            hint_limit = x as usize;
            for n in &br {
                x = self.draw_notch(buf, area, x, y1, n, g.bnl, g.bnr, th, false);
            }
        }
        // hints only in the focused pane
        if self.focus && !self.hints.is_empty() {
            let more = Hint::new("?", "more");
            let mut x = x0 + 1;
            let n = self.hints.len();
            for (i, h) in self.hints.iter().enumerate() {
                let need = h.width() + if i + 1 < n { more.width() } else { 0 };
                if x as usize + need + 1 > hint_limit {
                    if x as usize + more.width() < hint_limit {
                        self.draw_hint(buf, area, x, y1, &more, g, th);
                    }
                    break;
                }
                x = self.draw_hint(buf, area, x, y1, h, g, th);
            }
        }

        Rect::new(
            area.x + 1,
            area.y + 1,
            area.width.saturating_sub(2),
            area.height.saturating_sub(2),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_notch(
        &self,
        buf: &mut Buffer,
        area: Rect,
        x: u16,
        y: u16,
        n: &Notch,
        l: &str,
        r: &str,
        th: &Theme,
        first: bool,
    ) -> u16 {
        let bs = th.s_border(self.focus);
        cell(buf, area, x, y, l, bs);
        let mut base = n.style.unwrap_or_else(|| {
            if n.active || (first && self.focus) {
                th.s_accent().add_modifier(Modifier::BOLD)
            } else if first {
                th.s_text().add_modifier(Modifier::BOLD)
            } else {
                th.s_dim()
            }
        });
        if n.active && first {
            base = base.add_modifier(Modifier::BOLD);
        }
        let mut cx = x + 1;
        match n.hot {
            Some(h) if h < n.text.len() => {
                let (a, rest) = n.text.split_at(h);
                let mut it = rest.chars();
                let k = it.next().map(|c| c.to_string()).unwrap_or_default();
                let b: String = it.collect();
                cx = put_clip(buf, area, cx, y, a, base);
                cx = put_clip(
                    buf,
                    area,
                    cx,
                    y,
                    &k,
                    th.s_accent().add_modifier(Modifier::BOLD),
                );
                cx = put_clip(buf, area, cx, y, &b, base);
            }
            _ => {
                cx = put_clip(buf, area, cx, y, &n.text, base);
            }
        }
        cell(buf, area, cx, y, r, bs);
        cx + 1
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_hint(
        &self,
        buf: &mut Buffer,
        area: Rect,
        x: u16,
        y: u16,
        h: &Hint,
        g: &Glyphs,
        th: &Theme,
    ) -> u16 {
        let bs = th.s_border(true);
        cell(buf, area, x, y, g.bnl, bs);
        let mut cx = put_clip(
            buf,
            area,
            x + 1,
            y,
            &h.key,
            th.s_accent().add_modifier(Modifier::BOLD),
        );
        cx = put_clip(buf, area, cx, y, " ", th.s_dim());
        cx = put_clip(buf, area, cx, y, &h.label, th.s_text());
        cell(buf, area, cx, y, g.bnr, bs);
        cx + 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::theme::{Depth, ThemeName};

    fn render(b: &NotchBox, w: u16, h: u16, th: &Theme) -> Vec<String> {
        let area = Rect::new(0, 0, w, h);
        let mut buf = Buffer::empty(area);
        b.render(area, &mut buf, th);
        (0..h)
            .map(|y| (0..w).map(|x| buf[(x, y)].symbol().to_string()).collect())
            .collect()
    }

    #[test]
    fn notches_and_hints() {
        let th = Theme::new(ThemeName::Night, Depth::TrueColor);
        let b = NotchBox::new()
            .title(Notch::new("²rooms"))
            .title(Notch::hot("by room", 'o'))
            .meta(Notch::new("14"))
            .hints(vec![Hint::new("␣", "stop"), Hint::new("=", "set")])
            .position("5/14")
            .focus(true);
        let lines = render(&b, 40, 4, &th);
        insta::assert_snapshot!(lines.join("\n"), @r"
        ╭┐²rooms┌┐by room┌────────────────┐14┌─╮
        │                                      │
        │                                      │
        ╰┘␣ stop└┘= set└────────────────┘5/14└─╯
        ");
    }

    #[test]
    fn unfocused_has_no_hints_and_mono_focus_is_heavy() {
        let th = Theme::new(ThemeName::Mono, Depth::Mono);
        let b = NotchBox::new()
            .title(Notch::new("events"))
            .hints(vec![Hint::new("x", "mute")])
            .position("1/3");
        let lines = render(&b, 24, 3, &th);
        assert!(!lines[2].contains("mute"));
        assert!(lines[0].starts_with('╭'));
        let lines = render(&b.focus(true), 24, 3, &th);
        assert!(lines[0].starts_with('┏'));
        assert!(lines[2].contains("mute"));
    }

    #[test]
    fn hints_drop_with_more_marker_when_narrow() {
        let th = Theme::new(ThemeName::Night, Depth::TrueColor);
        let b = NotchBox::new()
            .title(Notch::new("x"))
            .hints(vec![
                Hint::new("␣", "toggle"),
                Hint::new("+-", "step"),
                Hint::new("=", "set"),
                Hint::new("m", "mood"),
            ])
            .focus(true);
        let lines = render(&b, 30, 3, &th);
        assert!(lines[2].contains("? more"), "{}", lines[2]);
        assert!(lines[2].contains("␣ toggle"));
    }

    #[test]
    fn tiny_area_does_not_panic() {
        let th = Theme::new(ThemeName::Night, Depth::TrueColor);
        let b = NotchBox::new()
            .title(Notch::new("a very long title"))
            .focus(true);
        for (w, h) in [(0, 0), (1, 1), (3, 2), (4, 2), (6, 3)] {
            render(&b, w, h, &th);
        }
    }
}
