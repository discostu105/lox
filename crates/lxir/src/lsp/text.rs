//! Positions and URIs as the protocol has them: a column counts UTF-16
//! code units unless the client agrees to UTF-8 (`positionEncoding`), and
//! a document is a percent-encoded `file://` URI.

use std::path::{Path, PathBuf};

/// How the client counts columns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encoding {
    Utf8,
    Utf16,
}

impl Encoding {
    /// The protocol column of byte offset `byte` in `line`.
    pub fn column(self, line: &str, byte: usize) -> u32 {
        let byte = floor_char_boundary(line, byte);
        match self {
            Encoding::Utf8 => byte as u32,
            Encoding::Utf16 => line[..byte].encode_utf16().count() as u32,
        }
    }

    /// The byte offset in `line` of protocol column `col` (clamped to the
    /// line's end).
    pub fn byte(self, line: &str, col: u32) -> usize {
        match self {
            Encoding::Utf8 => floor_char_boundary(line, col as usize),
            Encoding::Utf16 => {
                let mut units = 0u32;
                for (i, c) in line.char_indices() {
                    if units >= col {
                        return i;
                    }
                    units += c.len_utf16() as u32;
                }
                line.len()
            }
        }
    }
}

fn floor_char_boundary(s: &str, mut i: usize) -> usize {
    if i >= s.len() {
        return s.len();
    }
    while !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// Line `n` (0-based) of `text`, without its line ending.
pub fn line(text: &str, n: usize) -> &str {
    let l = text.split('\n').nth(n).unwrap_or("");
    l.strip_suffix('\r').unwrap_or(l)
}

/// The path of a `file://` URI.
pub fn uri_to_path(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    // `file://host/path` is not a local file; `file:///path` is.
    let rest = rest.strip_prefix("localhost").unwrap_or(rest);
    if !rest.starts_with('/') {
        return None;
    }
    let bytes = rest.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(b) = bytes
                .get(i + 1..i + 3)
                .and_then(|h| std::str::from_utf8(h).ok())
                .and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(b);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    let s = String::from_utf8(out).ok()?;
    // Windows: `file:///C:/x` → `C:/x`.
    let s = match s.as_bytes() {
        [b'/', d, b':', ..] if d.is_ascii_alphabetic() => s[1..].to_string(),
        _ => s,
    };
    Some(PathBuf::from(s))
}

/// The `file://` URI of `path`.
pub fn path_to_uri(path: &Path) -> String {
    let s = path.to_string_lossy();
    // Windows verbatim paths (`canonicalize`): `\\?\C:\x` → `C:/x`.
    let s = s.strip_prefix(r"\\?\").unwrap_or(&s).replace('\\', "/");
    let mut out = String::from("file://");
    if !s.starts_with('/') {
        out.push('/');
    }
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                out.push(b as char)
            }
            b':' if out.len() == "file:///".len() + 1 => out.push(':'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf16_columns_count_code_units() {
        let l = "a = Or(\"Küche 🔥\", I1: b.Q)";
        let byte = l.find("I1").unwrap();
        let col = Encoding::Utf16.column(l, byte);
        // ü is one unit, 🔥 two; bytes: ü 2, 🔥 4.
        assert_eq!(col as usize, byte - 1 - 2);
        assert_eq!(Encoding::Utf16.byte(l, col), byte);
        assert_eq!(Encoding::Utf8.column(l, byte) as usize, byte);
        assert_eq!(Encoding::Utf16.byte(l, 999), l.len());
    }

    #[test]
    fn uris_round_trip() {
        let p = PathBuf::from("/home/x/Haus Küche/a.lxir");
        let uri = path_to_uri(&p);
        assert_eq!(uri, "file:///home/x/Haus%20K%C3%BCche/a.lxir");
        assert_eq!(uri_to_path(&uri), Some(p));
        assert_eq!(
            uri_to_path("file:///c%3A/x/a.lxir"),
            Some(PathBuf::from("c:/x/a.lxir"))
        );
        assert_eq!(uri_to_path("untitled:1"), None);
        assert_eq!(
            path_to_uri(Path::new(r"\\?\C:\x\a.lxir")),
            "file:///C:/x/a.lxir"
        );
    }

    #[test]
    fn lines_drop_their_ending() {
        assert_eq!(line("a\r\nb\n", 0), "a");
        assert_eq!(line("a\r\nb\n", 1), "b");
        assert_eq!(line("a", 5), "");
    }
}
