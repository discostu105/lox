//! Themes, gradients and terminal color capability handling (§6.1).
//!
//! Widgets never hard-code colors: they ask the theme for a semantic slot
//! (`text`, `dim`, `accent`, …) or a gradient lookup (`grad(Grad::Load, 0.8)`).

use ratatui::style::{Color, Modifier, Style};

/// Color capability of the terminal, resolved once at startup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Depth {
    TrueColor,
    Ansi256,
    Ansi16,
    /// No color at all: bold/dim/reverse only
    Mono,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ThemeName {
    Night,
    Day,
    Neon,
    Mono,
}

impl ThemeName {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "night" | "loxone-night" | "dark" => Some(Self::Night),
            "day" | "light" => Some(Self::Day),
            "neon" => Some(Self::Neon),
            "mono" | "none" => Some(Self::Mono),
            _ => None,
        }
    }
}

/// Named gradients (§6.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Grad {
    Load,
    Temp,
    Pv,
    Use,
    Grid,
    Batt,
    Info,
    Lamp,
    Acc,
    Fade,
}

const GRADS: usize = 10;

type Rgb = (u8, u8, u8);

struct Palette {
    bg: Rgb,
    surface: Rgb,
    border: Rgb,
    text: Rgb,
    dim: Rgb,
    faint: Rgb,
    accent: Rgb,
    on: Rgb,
    ok: Rgb,
    warn: Rgb,
    crit: Rgb,
    info: Rgb,
    pv: Rgb,
    grid: Rgb,
    batt: Rgb,
    load: Rgb,
    track: Rgb,
    grads: [&'static [Rgb]; GRADS],
}

fn hex(s: &str) -> Rgb {
    let s = s.trim_start_matches('#');
    let p = |i: usize| u8::from_str_radix(&s[i..i + 2], 16).unwrap_or(0);
    (p(0), p(2), p(4))
}

const NIGHT_GRADS: [&[Rgb]; GRADS] = [
    &[(0x69, 0xc3, 0x50), (0xe8, 0xc3, 0x3c), (0xef, 0x5f, 0x5f)],
    &[
        (0x4a, 0xa3, 0xdf),
        (0x69, 0xc3, 0x50),
        (0xf0, 0xa3, 0x3a),
        (0xef, 0x5f, 0x5f),
    ],
    &[(0x6b, 0x52, 0x12), (0xe0, 0xa9, 0x3a), (0xff, 0xe0, 0x8a)],
    &[(0x6b, 0x34, 0x13), (0xc8, 0x6a, 0x2c), (0xfb, 0xab, 0x6c)],
    &[(0x3d, 0x34, 0x70), (0xa7, 0x8b, 0xfa)],
    &[(0x0f, 0x4d, 0x3a), (0x34, 0xd3, 0x99)],
    &[(0x1d, 0x46, 0x60), (0x5c, 0xb8, 0xe6)],
    &[(0x6b, 0x52, 0x12), (0xf5, 0xc4, 0x51), (0xff, 0xf1, 0xc2)],
    &[(0x27, 0x49, 0x1f), (0x69, 0xc3, 0x50), (0xb8, 0xf5, 0x9b)],
    &[(0xd7, 0xdd, 0xe5), (0x4b, 0x55, 0x63)],
];

const DAY_GRADS: [&[Rgb]; GRADS] = [
    &[(0x2f, 0x8a, 0x1f), (0xb8, 0x86, 0x00), (0xc6, 0x28, 0x28)],
    &[
        (0x1f, 0x6f, 0xb2),
        (0x2f, 0x8a, 0x1f),
        (0xc2, 0x6a, 0x00),
        (0xc6, 0x28, 0x28),
    ],
    &[(0xe2, 0xc2, 0x6a), (0xb8, 0x86, 0x00), (0x8a, 0x5a, 0x00)],
    &[(0xe8, 0xa8, 0x78), (0xc8, 0x6a, 0x2c), (0x8f, 0x3d, 0x0e)],
    &[(0xb9, 0xa8, 0xf0), (0x6d, 0x4c, 0xd1)],
    &[(0x8f, 0xd8, 0xbb), (0x0f, 0x8a, 0x5f)],
    &[(0x9c, 0xc8, 0xe4), (0x1f, 0x6f, 0xb2)],
    &[(0xe2, 0xc2, 0x6a), (0xc2, 0x8e, 0x00), (0x8a, 0x5a, 0x00)],
    &[(0x9f, 0xd4, 0x8e), (0x2f, 0x8a, 0x1f), (0x1d, 0x5c, 0x12)],
    &[(0x1f, 0x25, 0x2d), (0xa0, 0xa8, 0xb3)],
];

const NEON_GRADS: [&[Rgb]; GRADS] = [
    &[(0x50, 0xfa, 0x7b), (0xf1, 0xfa, 0x8c), (0xff, 0x63, 0x63)],
    &[
        (0x80, 0xff, 0xea),
        (0x50, 0xfa, 0x7b),
        (0xff, 0xb8, 0x6c),
        (0xff, 0x63, 0x63),
    ],
    &[(0x6a, 0x5a, 0x10), (0xf1, 0xfa, 0x8c), (0xff, 0xff, 0xd0)],
    &[(0x5a, 0x10, 0x4a), (0xff, 0x6a, 0xc1), (0xff, 0xb3, 0xe6)],
    &[(0x3a, 0x1f, 0x70), (0xbd, 0x93, 0xf9)],
    &[(0x0f, 0x4d, 0x4a), (0x80, 0xff, 0xea)],
    &[(0x1f, 0x3a, 0x70), (0x80, 0xff, 0xea)],
    &[(0x6a, 0x5a, 0x10), (0xf1, 0xfa, 0x8c), (0xff, 0xff, 0xd0)],
    &[(0x4a, 0x10, 0x5a), (0xe1, 0x35, 0xff), (0xff, 0xb3, 0xff)],
    &[(0xf8, 0xf8, 0xf2), (0x4a, 0x4a, 0x6a)],
];

fn palette(name: ThemeName) -> Palette {
    match name {
        ThemeName::Night | ThemeName::Mono => Palette {
            bg: hex("#0f1216"),
            surface: hex("#1a2029"),
            border: hex("#2f3744"),
            text: hex("#d7dde5"),
            dim: hex("#7d8896"),
            faint: hex("#4b5563"),
            accent: hex("#69c350"),
            on: hex("#f5c451"),
            ok: hex("#69c350"),
            warn: hex("#f0a33a"),
            crit: hex("#ef5f5f"),
            info: hex("#5cb8e6"),
            pv: hex("#f5c451"),
            grid: hex("#a78bfa"),
            batt: hex("#34d399"),
            load: hex("#fb923c"),
            track: hex("#232a34"),
            grads: NIGHT_GRADS,
        },
        ThemeName::Day => Palette {
            bg: hex("#f6f7f9"),
            surface: hex("#e4e8ee"),
            border: hex("#c3c9d2"),
            text: hex("#1f252d"),
            dim: hex("#5b6572"),
            faint: hex("#a0a8b3"),
            accent: hex("#2f8a1f"),
            on: hex("#b88600"),
            ok: hex("#2f8a1f"),
            warn: hex("#c26a00"),
            crit: hex("#c62828"),
            info: hex("#1f6fb2"),
            pv: hex("#b88600"),
            grid: hex("#6d4cd1"),
            batt: hex("#0f8a5f"),
            load: hex("#c8561c"),
            track: hex("#dde2e8"),
            grads: DAY_GRADS,
        },
        ThemeName::Neon => Palette {
            bg: hex("#0d0b16"),
            surface: hex("#1c1830"),
            border: hex("#3a2f5c"),
            text: hex("#f8f8f2"),
            dim: hex("#9a8fc0"),
            faint: hex("#4a4a6a"),
            accent: hex("#e135ff"),
            on: hex("#f1fa8c"),
            ok: hex("#50fa7b"),
            warn: hex("#ffb86c"),
            crit: hex("#ff6363"),
            info: hex("#80ffea"),
            pv: hex("#f1fa8c"),
            grid: hex("#bd93f9"),
            batt: hex("#80ffea"),
            load: hex("#ff6ac1"),
            track: hex("#221d38"),
            grads: NEON_GRADS,
        },
    }
}

/// A resolved theme: semantic colors already quantized for the terminal.
#[derive(Debug, Clone)]
pub struct Theme {
    pub depth: Depth,
    pub bg: Color,
    pub surface: Color,
    pub border: Color,
    pub border_focus: Color,
    pub text: Color,
    pub dim: Color,
    pub faint: Color,
    pub accent: Color,
    pub on: Color,
    pub ok: Color,
    pub warn: Color,
    pub crit: Color,
    pub info: Color,
    pub pv: Color,
    pub grid: Color,
    pub batt: Color,
    pub load: Color,
    pub track: Color,
    luts: Vec<Vec<Color>>,
}

impl Theme {
    pub fn new(name: ThemeName, depth: Depth) -> Self {
        let depth = if name == ThemeName::Mono {
            Depth::Mono
        } else {
            depth
        };
        let p = palette(name);
        let q = |c: Rgb| quantize(c, depth);
        let luts = p
            .grads
            .iter()
            .map(|stops| {
                (0..=100)
                    .map(|i| q(lerp_stops(stops, i as f64 / 100.0)))
                    .collect::<Vec<_>>()
            })
            .collect();
        Theme {
            depth,
            bg: q(p.bg),
            surface: q(p.surface),
            border: q(p.border),
            border_focus: q(p.accent),
            text: q(p.text),
            dim: q(p.dim),
            faint: q(p.faint),
            accent: q(p.accent),
            on: q(p.on),
            ok: q(p.ok),
            warn: q(p.warn),
            crit: q(p.crit),
            info: q(p.info),
            pv: q(p.pv),
            grid: q(p.grid),
            batt: q(p.batt),
            load: q(p.load),
            track: q(p.track),
            luts,
        }
    }

    pub fn is_mono(&self) -> bool {
        self.depth == Depth::Mono
    }

    /// Gradient color at position `t` (0.0..=1.0).
    pub fn grad(&self, g: Grad, t: f64) -> Color {
        if self.is_mono() {
            return Color::Reset;
        }
        let t = if t.is_finite() {
            t.clamp(0.0, 1.0)
        } else {
            0.0
        };
        self.luts[g as usize][(t * 100.0).round() as usize]
    }

    /// Style for a gradient cell. In mono, the gradient becomes dim/normal/bold bands.
    pub fn grad_style(&self, g: Grad, t: f64) -> Style {
        if self.is_mono() {
            return if t < 0.34 {
                Style::default().add_modifier(Modifier::DIM)
            } else if t < 0.67 {
                Style::default()
            } else {
                Style::default().add_modifier(Modifier::BOLD)
            };
        }
        Style::default().fg(self.grad(g, t))
    }

    fn fg(&self, c: Color) -> Style {
        if self.is_mono() {
            Style::default()
        } else {
            Style::default().fg(c)
        }
    }

    pub fn s_text(&self) -> Style {
        self.fg(self.text)
    }
    pub fn s_dim(&self) -> Style {
        if self.is_mono() {
            Style::default().add_modifier(Modifier::DIM)
        } else {
            self.fg(self.dim)
        }
    }
    pub fn s_faint(&self) -> Style {
        if self.is_mono() {
            Style::default().add_modifier(Modifier::DIM)
        } else {
            self.fg(self.faint)
        }
    }
    pub fn s_accent(&self) -> Style {
        if self.is_mono() {
            Style::default().add_modifier(Modifier::BOLD)
        } else {
            self.fg(self.accent)
        }
    }
    pub fn s_on(&self) -> Style {
        if self.is_mono() {
            Style::default().add_modifier(Modifier::BOLD)
        } else {
            self.fg(self.on)
        }
    }
    pub fn s_ok(&self) -> Style {
        self.fg(self.ok)
    }
    pub fn s_warn(&self) -> Style {
        if self.is_mono() {
            Style::default().add_modifier(Modifier::BOLD)
        } else {
            self.fg(self.warn)
        }
    }
    pub fn s_crit(&self) -> Style {
        if self.is_mono() {
            Style::default().add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
        } else {
            self.fg(self.crit)
        }
    }
    pub fn s_info(&self) -> Style {
        self.fg(self.info)
    }
    pub fn s_color(&self, c: Color) -> Style {
        self.fg(c)
    }
    pub fn s_border(&self, focus: bool) -> Style {
        if self.is_mono() {
            if focus {
                Style::default().add_modifier(Modifier::BOLD)
            } else {
                Style::default().add_modifier(Modifier::DIM)
            }
        } else if focus {
            self.fg(self.border_focus)
        } else {
            self.fg(self.border)
        }
    }
    /// Selected row: surface background (reverse in mono).
    pub fn s_selected(&self) -> Style {
        if self.is_mono() {
            Style::default().add_modifier(Modifier::REVERSED)
        } else {
            Style::default().bg(self.surface)
        }
    }
    /// Key-cap / badge style: inverted accent.
    pub fn s_badge(&self, c: Color) -> Style {
        if self.is_mono() {
            Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD)
        } else {
            Style::default()
                .fg(self.bg)
                .bg(c)
                .add_modifier(Modifier::BOLD)
        }
    }
    pub fn s_base(&self) -> Style {
        if self.is_mono() {
            Style::default()
        } else {
            Style::default().fg(self.text).bg(self.bg)
        }
    }
}

fn lerp(a: u8, b: u8, t: f64) -> u8 {
    (a as f64 + (b as f64 - a as f64) * t).round() as u8
}

fn lerp_stops(stops: &[Rgb], t: f64) -> Rgb {
    if stops.len() == 1 {
        return stops[0];
    }
    let seg = (stops.len() - 1) as f64;
    let x = (t * seg).clamp(0.0, seg);
    let i = (x.floor() as usize).min(stops.len() - 2);
    let f = x - i as f64;
    let (a, b) = (stops[i], stops[i + 1]);
    (lerp(a.0, b.0, f), lerp(a.1, b.1, f), lerp(a.2, b.2, f))
}

/// Map an RGB color to what the terminal can show.
pub fn quantize(c: Rgb, depth: Depth) -> Color {
    match depth {
        Depth::TrueColor => Color::Rgb(c.0, c.1, c.2),
        Depth::Ansi256 => Color::Indexed(rgb_to_xterm256(c)),
        Depth::Ansi16 => rgb_to_ansi16(c),
        Depth::Mono => Color::Reset,
    }
}

fn rgb_to_xterm256((r, g, b): Rgb) -> u8 {
    let lvl = |v: u8| -> (u8, u8) {
        const STEPS: [u8; 6] = [0, 95, 135, 175, 215, 255];
        let mut best = 0;
        for (i, s) in STEPS.iter().enumerate() {
            if (*s as i32 - v as i32).abs() < (STEPS[best] as i32 - v as i32).abs() {
                best = i;
            }
        }
        (best as u8, STEPS[best])
    };
    let (ri, rv) = lvl(r);
    let (gi, gv) = lvl(g);
    let (bi, bv) = lvl(b);
    let cube = 16 + 36 * ri + 6 * gi + bi;
    let cube_err = dist((r, g, b), (rv, gv, bv));
    let avg = (r as u32 + g as u32 + b as u32) / 3;
    let gi2 = if avg < 8 {
        0
    } else {
        ((avg - 8) / 10).min(23) as u8
    };
    let gray_v = 8 + gi2 * 10;
    let gray_err = dist((r, g, b), (gray_v, gray_v, gray_v));
    if gray_err < cube_err { 232 + gi2 } else { cube }
}

fn dist(a: Rgb, b: Rgb) -> u32 {
    let d = |x: u8, y: u8| (x as i32 - y as i32).pow(2) as u32;
    d(a.0, b.0) + d(a.1, b.1) + d(a.2, b.2)
}

fn rgb_to_ansi16(c: Rgb) -> Color {
    const TABLE: [(Rgb, Color); 16] = [
        ((0, 0, 0), Color::Black),
        ((205, 49, 49), Color::Red),
        ((13, 188, 121), Color::Green),
        ((229, 229, 16), Color::Yellow),
        ((36, 114, 200), Color::Blue),
        ((188, 63, 188), Color::Magenta),
        ((17, 168, 205), Color::Cyan),
        ((229, 229, 229), Color::Gray),
        ((102, 102, 102), Color::DarkGray),
        ((241, 76, 76), Color::LightRed),
        ((35, 209, 139), Color::LightGreen),
        ((245, 245, 67), Color::LightYellow),
        ((59, 142, 234), Color::LightBlue),
        ((214, 112, 214), Color::LightMagenta),
        ((41, 184, 219), Color::LightCyan),
        ((255, 255, 255), Color::White),
    ];
    TABLE
        .iter()
        .min_by_key(|(rgb, _)| dist(*rgb, c))
        .map(|(_, col)| *col)
        .unwrap_or(Color::Reset)
}

/// Where the theme choice came from, for §6.1's resolution order.
pub struct ColorChoice {
    pub theme: ThemeName,
    pub depth: Depth,
}

/// Resolve theme and color depth: explicit flags/config first, then `NO_COLOR`,
/// then capability detection.
pub fn resolve(
    flag_theme: Option<ThemeName>,
    no_color_flag: bool,
    config_theme: Option<ThemeName>,
    env: &dyn Fn(&str) -> Option<String>,
) -> ColorChoice {
    let detect = || -> Depth {
        let colorterm = env("COLORTERM").unwrap_or_default().to_lowercase();
        let term = env("TERM").unwrap_or_default().to_lowercase();
        if colorterm.contains("truecolor") || colorterm.contains("24bit") {
            Depth::TrueColor
        } else if term.contains("256") || term.contains("kitty") || term.contains("alacritty") {
            // 256-color terms: most modern ones do truecolor too, but only claim what we know.
            if term.contains("kitty") || term.contains("alacritty") || term.contains("wezterm") {
                Depth::TrueColor
            } else {
                Depth::Ansi256
            }
        } else if term == "dumb" {
            Depth::Mono
        } else {
            Depth::Ansi16
        }
    };
    // 1. explicit flag wins (a theme flag can opt back into color despite NO_COLOR)
    if let Some(t) = flag_theme {
        return ColorChoice {
            theme: t,
            depth: detect(),
        };
    }
    if no_color_flag {
        return ColorChoice {
            theme: ThemeName::Mono,
            depth: Depth::Mono,
        };
    }
    if let Some(t) = config_theme {
        return ColorChoice {
            theme: t,
            depth: detect(),
        };
    }
    // 2. NO_COLOR (non-empty) before any detection
    if env("NO_COLOR").is_some_and(|v| !v.is_empty()) {
        return ColorChoice {
            theme: ThemeName::Mono,
            depth: Depth::Mono,
        };
    }
    // 3. detection
    let depth = detect();
    ColorChoice {
        theme: if depth == Depth::Mono {
            ThemeName::Mono
        } else {
            ThemeName::Night
        },
        depth,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env_of(pairs: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> {
        move |k| {
            pairs
                .iter()
                .find(|(n, _)| *n == k)
                .map(|(_, v)| v.to_string())
        }
    }

    #[test]
    fn resolution_order() {
        let e = env_of(&[("NO_COLOR", "1"), ("COLORTERM", "truecolor")]);
        // NO_COLOR beats detection
        assert_eq!(resolve(None, false, None, &e).theme, ThemeName::Mono);
        // an explicit theme flag opts back into color
        let c = resolve(Some(ThemeName::Night), false, None, &e);
        assert_eq!((c.theme, c.depth), (ThemeName::Night, Depth::TrueColor));
        // config theme also beats NO_COLOR (explicit user choice)
        assert_eq!(
            resolve(None, false, Some(ThemeName::Day), &e).theme,
            ThemeName::Day
        );
        // --no-color flag beats config
        assert_eq!(
            resolve(None, true, Some(ThemeName::Day), &e).theme,
            ThemeName::Mono
        );
        // empty NO_COLOR is ignored
        let e2 = env_of(&[("NO_COLOR", ""), ("TERM", "xterm-256color")]);
        let c = resolve(None, false, None, &e2);
        assert_eq!((c.theme, c.depth), (ThemeName::Night, Depth::Ansi256));
    }

    #[test]
    fn gradient_endpoints() {
        let t = Theme::new(ThemeName::Night, Depth::TrueColor);
        assert_eq!(t.grad(Grad::Load, 0.0), Color::Rgb(0x69, 0xc3, 0x50));
        assert_eq!(t.grad(Grad::Load, 1.0), Color::Rgb(0xef, 0x5f, 0x5f));
        assert_eq!(t.grad(Grad::Load, 7.0), t.grad(Grad::Load, 1.0));
        assert_eq!(t.grad(Grad::Load, f64::NAN), t.grad(Grad::Load, 0.0));
    }

    #[test]
    fn quantization() {
        assert_eq!(rgb_to_xterm256((0, 0, 0)), 16);
        assert_eq!(rgb_to_xterm256((255, 255, 255)), 231);
        assert_eq!(rgb_to_ansi16((0xef, 0x5f, 0x5f)), Color::LightRed);
        let t = Theme::new(ThemeName::Night, Depth::Ansi256);
        assert!(matches!(t.accent, Color::Indexed(_)));
        let m = Theme::new(ThemeName::Mono, Depth::TrueColor);
        assert!(m.is_mono());
    }
}
