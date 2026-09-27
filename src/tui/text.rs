//! Untrusted-text handling and cell-width helpers (§7.3 "Untrusted text").
//!
//! Every string that comes from the Miniserver or the config (names, log lines,
//! text states) goes through [`clean`] before it is drawn, so a crafted name can't
//! move the cursor, set the window title or trigger OSC 52.

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// Strip control characters and escape sequences; collapse newlines/tabs to spaces.
pub fn clean(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\u{1b}' => {
                // ESC: skip a CSI/OSC/other escape sequence
                match chars.peek() {
                    Some('[') => {
                        chars.next();
                        // CSI: parameters until a final byte in 0x40..=0x7e
                        for n in chars.by_ref() {
                            if ('\u{40}'..='\u{7e}').contains(&n) {
                                break;
                            }
                        }
                    }
                    Some(']') | Some('P') | Some('_') | Some('^') => {
                        chars.next();
                        // OSC/DCS/APC/PM: until BEL or ST (ESC \)
                        while let Some(n) = chars.next() {
                            if n == '\u{7}' {
                                break;
                            }
                            if n == '\u{1b}' {
                                if chars.peek() == Some(&'\\') {
                                    chars.next();
                                }
                                break;
                            }
                        }
                    }
                    Some(_) => {
                        chars.next();
                    }
                    None => {}
                }
            }
            '\n' | '\r' | '\t' => out.push(' '),
            c if c.is_control() => {}
            // C1 controls and bidi overrides are invisible-but-dangerous
            '\u{80}'..='\u{9f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' => {}
            c => out.push(c),
        }
    }
    out
}

/// Display width in terminal cells.
pub fn width(s: &str) -> usize {
    UnicodeWidthStr::width(s)
}

/// Truncate to at most `max` cells, adding `…` when cut.
pub fn trunc(s: &str, max: usize) -> String {
    if width(s) <= max {
        return s.to_string();
    }
    if max == 0 {
        return String::new();
    }
    let mut out = String::new();
    let mut w = 0;
    for c in s.chars() {
        let cw = UnicodeWidthChar::width(c).unwrap_or(0);
        if w + cw > max - 1 {
            break;
        }
        out.push(c);
        w += cw;
    }
    out.push('…');
    out
}

/// Truncate or pad with spaces to exactly `w` cells.
pub fn fit(s: &str, w: usize) -> String {
    let t = trunc(s, w);
    let pad = w.saturating_sub(width(&t));
    format!("{}{}", t, " ".repeat(pad))
}

/// Right-align in exactly `w` cells.
pub fn rfit(s: &str, w: usize) -> String {
    let t = trunc(s, w);
    let pad = w.saturating_sub(width(&t));
    format!("{}{}", " ".repeat(pad), t)
}

/// Natural sort key: digits compare numerically ("Room 2" < "Room 10").
pub fn natural_key(s: &str) -> Vec<(u8, u64, String)> {
    let mut out = Vec::new();
    let mut num = String::new();
    let mut txt = String::new();
    for c in s.chars() {
        if c.is_ascii_digit() {
            if !txt.is_empty() {
                out.push((1, 0, std::mem::take(&mut txt).to_lowercase()));
            }
            num.push(c);
        } else {
            if !num.is_empty() {
                out.push((0, num.parse().unwrap_or(u64::MAX), String::new()));
                num.clear();
            }
            txt.push(c);
        }
    }
    if !num.is_empty() {
        out.push((0, num.parse().unwrap_or(u64::MAX), String::new()));
    }
    if !txt.is_empty() {
        out.push((1, 0, txt.to_lowercase()));
    }
    out
}

/// Format a number with a C-style Loxone format string such as `%.1f°C`, `%.3fkW`, `%i%%`.
pub fn lox_format(fmt: &str, v: f64) -> String {
    let Some(pos) = fmt.find('%') else {
        return format!("{} {}", trim_num(v, 1), fmt).trim().to_string();
    };
    let (pre, rest) = fmt.split_at(pos);
    let rest = &rest[1..];
    let mut chars = rest.char_indices();
    let mut prec: Option<usize> = None;
    let mut end = 0;
    let mut spec = 'f';
    let mut digits = String::new();
    let mut seen_dot = false;
    for (i, c) in chars.by_ref() {
        if c == '.' {
            seen_dot = true;
        } else if c.is_ascii_digit() {
            if seen_dot {
                digits.push(c);
            }
        } else if c == '-' || c == '+' || c == ' ' || c == '0' || c == 'l' {
            // flags / length modifiers
        } else {
            spec = c;
            end = i + c.len_utf8();
            break;
        }
    }
    if seen_dot {
        prec = digits.parse().ok();
    }
    let suffix = rest.get(end..).unwrap_or("").replace("%%", "%");
    let num = match spec {
        'i' | 'd' | 'u' => format!("{}", v.round() as i64),
        '%' => return format!("{}%{}", pre, suffix),
        's' => trim_num(v, 2),
        _ => match prec {
            Some(p) => format!("{:.*}", p, v),
            None => format!("{:.6}", v),
        },
    };
    format!("{}{}{}", pre, num, suffix)
}

fn trim_num(v: f64, prec: usize) -> String {
    let s = format!("{:.*}", prec, v);
    if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        s
    }
}

/// Compact power: "6.2 kW", "850 W".
pub fn fmt_kw(kw: f64) -> String {
    if kw.abs() < 1.0 {
        format!("{:.0} W", kw * 1000.0)
    } else if kw.abs() < 100.0 {
        format!("{:.1} kW", kw)
    } else {
        format!("{:.0} kW", kw)
    }
}

/// Compact energy: "14.2 kWh", "1.3 MWh".
pub fn fmt_kwh(kwh: f64) -> String {
    if kwh.abs() >= 10_000.0 {
        format!("{:.1} MWh", kwh / 1000.0)
    } else if kwh.abs() >= 100.0 {
        format!("{:.0} kWh", kwh)
    } else {
        format!("{:.1} kWh", kwh)
    }
}

/// "3 s", "4 min", "2 h", "5 d"
pub fn fmt_age(secs: u64) -> String {
    match secs {
        0..=59 => format!("{} s", secs),
        60..=3599 => format!("{} min", secs / 60),
        3600..=86_399 => format!("{} h", secs / 3600),
        _ => format!("{} d", secs / 86_400),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_escape_sequences() {
        assert_eq!(clean("Kitchen\u{1b}[2J\u{1b}[H light"), "Kitchen light");
        assert_eq!(clean("a\u{1b}]52;c;ZXZpbA==\u{7}b"), "ab");
        assert_eq!(clean("a\u{1b}]0;title\u{1b}\\b"), "ab");
        assert_eq!(clean("line1\nline2\tx\r"), "line1 line2 x ");
        assert_eq!(clean("bell\u{7}\u{0}"), "bell");
        assert_eq!(clean("rtl\u{202e}evil"), "rtlevil");
        assert_eq!(clean("Küche 温度"), "Küche 温度");
    }

    #[test]
    fn width_aware_truncation() {
        assert_eq!(trunc("Living room", 20), "Living room");
        assert_eq!(trunc("Living room", 6), "Livin…");
        assert_eq!(width(&trunc("温度温度温度", 5)), 5);
        assert_eq!(fit("ab", 4), "ab  ");
        assert_eq!(rfit("ab", 4), "  ab");
        assert_eq!(fit("温度温度", 3), "温…");
        assert_eq!(trunc("x", 0), "");
    }

    #[test]
    fn natural_sorting() {
        let mut v = vec!["Room 10", "Room 2", "room 1", "EG Office", "OG Bath"];
        v.sort_by_key(|s| natural_key(s));
        assert_eq!(v, ["EG Office", "OG Bath", "room 1", "Room 2", "Room 10"]);
    }

    #[test]
    fn loxone_formats() {
        assert_eq!(lox_format("%.1f°C", 21.456), "21.5°C");
        assert_eq!(lox_format("%.3fkW", 1.5), "1.500kW");
        assert_eq!(lox_format("%i%%", 42.4), "42%");
        assert_eq!(lox_format("%.0f%%", 74.6), "75%");
        assert_eq!(lox_format("%.1f°", 22.0), "22.0°");
        assert_eq!(lox_format("Lux", 300.0), "300 Lux");
        assert_eq!(lox_format("%.2f ppm", 400.0), "400.00 ppm");
    }

    #[test]
    fn compact_units() {
        assert_eq!(fmt_kw(0.85), "850 W");
        assert_eq!(fmt_kw(6.21), "6.2 kW");
        assert_eq!(fmt_kwh(14.23), "14.2 kWh");
        assert_eq!(fmt_kwh(12_345.0), "12.3 MWh");
        assert_eq!(fmt_age(125), "2 min");
    }
}
