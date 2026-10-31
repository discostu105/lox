//! Lossless concrete-syntax model of a `.Loxone` XML document.
//!
//! Loxone Config emits machine-regular XML, but *not* spec-conforming XML:
//! attribute names may start with digits (`12hTF="true"`), and attribute
//! values may contain raw, unescaped newlines (the `Code=` attribute of a
//! `Program` block holds multi-line PicoC source). A conforming XML parser
//! either rejects the file or silently normalizes those newlines to spaces
//! on re-serialization — which corrupts the config. This module therefore
//! parses the format directly.
//!
//! Attribute values and text nodes are stored **raw** (still entity-escaped);
//! decode on demand via [`unescape`]. The writer emits the canonical Loxone
//! serialization: UTF-8 BOM, CRLF line endings, tab indentation, a single
//! space between attributes, and `/>` for empty elements. Real Miniserver
//! output round-trips byte-for-byte (`parse` → `to_bytes` == input).

use crate::error::{Error, Result};
use std::borrow::Cow;
use std::fmt::Write as _;

/// A whole `.Loxone` file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XmlDocument {
    /// Whether the file starts with a UTF-8 byte-order mark. Loxone writes one.
    pub bom: bool,
    /// The XML declaration line, verbatim without line ending
    /// (e.g. `<?xml version="1.0" encoding="utf-8"?>`).
    pub decl: Option<String>,
    /// Line-ending convention. Loxone writes CRLF; files that passed
    /// through git newline normalization are LF. Byte fidelity means
    /// writing back whichever the input had.
    pub crlf: bool,
    /// Exact whitespace after the root's closing tag (normally one line
    /// ending, but stray extra newlines occur in the wild).
    pub trailing: String,
    pub root: Element,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Element {
    pub name: String,
    pub attrs: Vec<Attr>,
    pub children: Vec<Node>,
    /// `true` when serialized as `<Name …/>`. Elements with children are
    /// never self-closing; empty elements in Loxone output usually are,
    /// but not always — the GUI writes `<IoData></IoData>` even on fresh
    /// blocks (oracle save 2026-08-25), so the flag must be preserved
    /// per element, never normalized.
    pub self_closing: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Node {
    Element(Element),
    /// Raw (entity-escaped) character data, e.g. the hex blob in `<Key>…</Key>`.
    Text(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attr {
    pub name: String,
    /// Raw value exactly as it appears between the quotes — entities are NOT
    /// decoded, newlines are preserved.
    pub value: String,
}

impl Element {
    pub fn new(name: impl Into<String>) -> Self {
        Element {
            name: name.into(),
            attrs: Vec::new(),
            children: Vec::new(),
            self_closing: true,
        }
    }

    /// Raw (still escaped) attribute value.
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|a| a.name == name)
            .map(|a| a.value.as_str())
    }

    /// Decoded attribute value.
    pub fn attr_decoded(&self, name: &str) -> Option<Cow<'_, str>> {
        self.attr(name).map(unescape)
    }

    /// Set (or append) an attribute; the value is escaped here.
    pub fn set_attr(&mut self, name: &str, value: &str) {
        let escaped = escape(value).into_owned();
        match self.attrs.iter_mut().find(|a| a.name == name) {
            Some(a) => a.value = escaped,
            None => self.attrs.push(Attr {
                name: name.to_string(),
                value: escaped,
            }),
        }
    }

    /// Set an attribute from an already-escaped raw value.
    pub fn set_attr_raw(&mut self, name: &str, raw: &str) {
        match self.attrs.iter_mut().find(|a| a.name == name) {
            Some(a) => a.value = raw.to_string(),
            None => self.attrs.push(Attr {
                name: name.to_string(),
                value: raw.to_string(),
            }),
        }
    }

    pub fn remove_attr(&mut self, name: &str) -> Option<Attr> {
        let idx = self.attrs.iter().position(|a| a.name == name)?;
        Some(self.attrs.remove(idx))
    }

    pub fn child_elements(&self) -> impl Iterator<Item = &Element> {
        self.children.iter().filter_map(|n| match n {
            Node::Element(e) => Some(e),
            Node::Text(_) => None,
        })
    }

    pub fn child_elements_mut(&mut self) -> impl Iterator<Item = &mut Element> {
        self.children.iter_mut().filter_map(|n| match n {
            Node::Element(e) => Some(e),
            Node::Text(_) => None,
        })
    }

    pub fn push_child(&mut self, child: Element) {
        self.self_closing = false;
        self.children.push(Node::Element(child));
    }
}

impl XmlDocument {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let text = std::str::from_utf8(bytes).map_err(|e| Error::Xml {
            line: 0,
            msg: format!("not valid UTF-8: {e}"),
        })?;
        let mut doc = Parser::new(text).parse_document()?;
        // First newline decides the convention; a file without any keeps
        // the Loxone default (CRLF).
        if let Some(i) = text.find('\n') {
            doc.crlf = text[..i].ends_with('\r');
        }
        Ok(doc)
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let nl = if self.crlf { "\r\n" } else { "\n" };
        let mut out = String::with_capacity(1 << 16);
        if let Some(decl) = &self.decl {
            out.push_str(decl);
            out.push_str(nl);
        }
        write_element(&mut out, &self.root, 0, nl);
        // The root, like every element, was written with a trailing line
        // ending; the document's actual trailing whitespace replaces it.
        out.truncate(out.len() - nl.len());
        out.push_str(&self.trailing);
        let mut bytes = Vec::with_capacity(out.len() + 3);
        if self.bom {
            bytes.extend_from_slice(b"\xEF\xBB\xBF");
        }
        bytes.extend_from_slice(out.as_bytes());
        bytes
    }
}

fn write_element(out: &mut String, e: &Element, depth: usize, nl: &str) {
    for _ in 0..depth {
        out.push('\t');
    }
    out.push('<');
    out.push_str(&e.name);
    for a in &e.attrs {
        let _ = write!(out, " {}=\"{}\"", a.name, a.value);
    }
    if e.children.is_empty() {
        if e.self_closing {
            let _ = write!(out, "/>{nl}");
        } else {
            let _ = write!(out, "></{}>{nl}", e.name);
        }
        return;
    }
    // Single text child renders inline: <Key>ABCD…</Key>
    if let [Node::Text(t)] = e.children.as_slice() {
        let _ = write!(out, ">{}</{}>{nl}", t, e.name);
        return;
    }
    let _ = write!(out, ">{nl}");
    for child in &e.children {
        match child {
            Node::Element(c) => write_element(out, c, depth + 1, nl),
            // Mixed content does not occur in Loxone output; emit raw on its
            // own line so nothing is lost if it ever does.
            Node::Text(t) => {
                out.push_str(t);
                out.push_str(nl);
            }
        }
    }
    for _ in 0..depth {
        out.push('\t');
    }
    let _ = write!(out, "</{}>{nl}", e.name);
}

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

struct Parser<'a> {
    s: &'a str,
    pos: usize,
    line: usize,
}

impl<'a> Parser<'a> {
    fn new(s: &'a str) -> Self {
        Parser { s, pos: 0, line: 1 }
    }

    fn err(&self, msg: impl Into<String>) -> Error {
        Error::Xml {
            line: self.line,
            msg: msg.into(),
        }
    }

    fn rest(&self) -> &'a str {
        &self.s[self.pos..]
    }

    fn peek(&self) -> Option<char> {
        self.rest().chars().next()
    }

    fn advance(&mut self, n_bytes: usize) {
        let taken = &self.s[self.pos..self.pos + n_bytes];
        self.line += taken.matches('\n').count();
        self.pos += n_bytes;
    }

    fn eat(&mut self, prefix: &str) -> bool {
        if self.rest().starts_with(prefix) {
            self.advance(prefix.len());
            true
        } else {
            false
        }
    }

    fn skip_ws(&mut self) {
        let n = self
            .rest()
            .find(|c: char| !c.is_whitespace())
            .unwrap_or(self.rest().len());
        self.advance(n);
    }

    fn parse_document(&mut self) -> Result<XmlDocument> {
        let bom = self.eat("\u{feff}");
        self.skip_ws();
        let decl = if self.rest().starts_with("<?") {
            let end = self
                .rest()
                .find("?>")
                .ok_or_else(|| self.err("unterminated XML declaration"))?;
            let decl = self.rest()[..end + 2].to_string();
            self.advance(end + 2);
            Some(decl)
        } else {
            None
        };
        self.skip_ws();
        if self.peek() != Some('<') {
            return Err(self.err("expected root element"));
        }
        let root = self.parse_element()?;
        let trailing = self.rest().to_string();
        if !trailing.chars().all(|c| c.is_ascii_whitespace()) {
            return Err(self.err(format!(
                "trailing content after root element: {:?}…",
                &trailing[..trailing.len().min(40)]
            )));
        }
        Ok(XmlDocument {
            bom,
            decl,
            crlf: true,
            trailing,
            root,
        })
    }

    /// Parses one element; `pos` must be at `<`.
    fn parse_element(&mut self) -> Result<Element> {
        debug_assert_eq!(self.peek(), Some('<'));
        if self.rest().starts_with("<!--") {
            return Err(self.err("comments are not supported in .Loxone documents"));
        }
        self.advance(1);
        let name = self.take_name()?;
        let mut el = Element::new(name);

        loop {
            self.skip_ws();
            match self.peek() {
                Some('/') => {
                    if !self.eat("/>") {
                        return Err(self.err("expected `/>`"));
                    }
                    el.self_closing = true;
                    return Ok(el);
                }
                Some('>') => {
                    self.advance(1);
                    el.self_closing = false;
                    self.parse_children(&mut el)?;
                    return Ok(el);
                }
                Some(_) => {
                    let attr_name = self.take_name()?;
                    self.skip_ws();
                    if !self.eat("=") {
                        return Err(self.err(format!("expected `=` after attribute `{attr_name}`")));
                    }
                    self.skip_ws();
                    if !self.eat("\"") {
                        return Err(self.err(format!(
                            "expected double-quoted value for attribute `{attr_name}`"
                        )));
                    }
                    // Raw until the closing quote — a literal `"` inside a
                    // value must be `&quot;`, so this cannot over-read. The
                    // value may span lines (raw newlines are legal here).
                    let end = self
                        .rest()
                        .find('"')
                        .ok_or_else(|| self.err(format!("unterminated value for `{attr_name}`")))?;
                    let value = self.rest()[..end].to_string();
                    self.advance(end + 1);
                    el.attrs.push(Attr {
                        name: attr_name,
                        value,
                    });
                }
                None => return Err(self.err("unexpected end of file inside tag")),
            }
        }
    }

    fn parse_children(&mut self, el: &mut Element) -> Result<()> {
        loop {
            // Character data up to the next tag. Whitespace-only runs are
            // formatting (regenerated by the writer); anything else is a
            // real text node, kept raw.
            let upto = self
                .rest()
                .find('<')
                .ok_or_else(|| self.err(format!("missing closing tag for `{}`", el.name)))?;
            if upto > 0 {
                let text = &self.rest()[..upto];
                if !text.chars().all(char::is_whitespace) {
                    el.children.push(Node::Text(text.to_string()));
                }
                self.advance(upto);
            }
            if self.rest().starts_with("</") {
                self.advance(2);
                let close = self.take_name()?;
                if close != el.name {
                    return Err(self.err(format!(
                        "mismatched closing tag: expected `</{}>`, found `</{close}>`",
                        el.name
                    )));
                }
                self.skip_ws();
                if !self.eat(">") {
                    return Err(self.err("expected `>` after closing tag name"));
                }
                return Ok(());
            }
            let child = self.parse_element()?;
            el.children.push(Node::Element(child));
        }
    }

    /// Tag or attribute name. Deliberately permissive: Loxone uses names that
    /// start with digits (`12hTF`), which spec-conforming parsers reject.
    fn take_name(&mut self) -> Result<String> {
        let end = self
            .rest()
            .find(|c: char| c.is_whitespace() || matches!(c, '=' | '>' | '/' | '<' | '"'))
            .unwrap_or(self.rest().len());
        if end == 0 {
            return Err(self.err("expected a name"));
        }
        let name = self.rest()[..end].to_string();
        self.advance(end);
        Ok(name)
    }
}

// ---------------------------------------------------------------------------
// Entities
// ---------------------------------------------------------------------------

/// Decode the five XML entities. Numeric character references (`&#xA;`,
/// `&#228;`) stay verbatim, because that is how Loxone reads them: it
/// never writes one (zero in the whole corpus) and treats one it finds as
/// plain text — a program whose newlines a foreign XML writer had turned
/// into `&#xA;` came back from a Loxone Config save as `&amp;#xA;`, i.e.
/// one long line (docs/loxone-format.md). Decoding them would make the
/// semantic diff call that corruption a no-op.
pub fn unescape(raw: &str) -> Cow<'_, str> {
    if !raw.contains('&') {
        return Cow::Borrowed(raw);
    }
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let end = match rest.find(';') {
            Some(e) => e,
            None => break, // stray `&` — keep verbatim
        };
        match &rest[1..end] {
            "amp" => out.push('&'),
            "lt" => out.push('<'),
            "gt" => out.push('>'),
            "quot" => out.push('"'),
            "apos" => out.push('\''),
            // Numeric references and unknown entities — keep verbatim.
            _ => out.push_str(&rest[..end + 1]),
        }
        rest = &rest[end + 1..];
    }
    out.push_str(rest);
    Cow::Owned(out)
}

/// Escape a value for use in an attribute or text node, matching Loxone's
/// own convention (it escapes `>` and `'` too).
pub fn escape(s: &str) -> Cow<'_, str> {
    if !s.contains(['&', '<', '>', '"', '\'']) {
        return Cow::Borrowed(s);
    }
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(c),
        }
    }
    Cow::Owned(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\u{feff}<?xml version=\"1.0\" encoding=\"utf-8\"?>\r\n\
        <ControlList Version=\"1\" NextObj=\"3\">\r\n\
        \t<C Type=\"Or\" U=\"aa\" Title=\"O1\">\r\n\
        \t\t<Co K=\"I1\" Nc=\"1\" U=\"bb\">\r\n\
        \t\t\t<In Input=\"cc\"/>\r\n\
        \t\t</Co>\r\n\
        \t\t<Key>2B35</Key>\r\n\
        \t</C>\r\n\
        </ControlList>\r\n";

    #[test]
    fn roundtrip_sample() {
        let doc = XmlDocument::parse(SAMPLE.as_bytes()).unwrap();
        assert_eq!(doc.to_bytes(), SAMPLE.as_bytes());
    }

    #[test]
    fn raw_newline_in_attr_value() {
        let s = "<Root Code=\"line1\nline2\"/>\r\n";
        let doc = XmlDocument::parse(s.as_bytes()).unwrap();
        assert_eq!(doc.root.attr("Code"), Some("line1\nline2"));
        assert_eq!(doc.to_bytes(), s.as_bytes());
    }

    #[test]
    fn digit_prefixed_attr_name() {
        let s = "<Root 12hTF=\"true\"/>\r\n";
        let doc = XmlDocument::parse(s.as_bytes()).unwrap();
        assert_eq!(doc.root.attr("12hTF"), Some("true"));
        assert_eq!(doc.to_bytes(), s.as_bytes());
    }

    #[test]
    fn entities() {
        assert_eq!(unescape("a &lt;= b &amp; c"), "a <= b & c");
        // Loxone reads numeric references as text, and so does this crate.
        assert_eq!(unescape("&#228;&#xE4;"), "&#228;&#xE4;");
        assert_eq!(unescape("a&amp;#xA;b"), "a&#xA;b");
        assert_eq!(escape("a<b & 'c'"), "a&lt;b &amp; &apos;c&apos;");
        assert_eq!(unescape(&escape("x < & > \" ' y")), "x < & > \" ' y");
    }

    #[test]
    fn mismatched_close_is_error() {
        let s = "<A><B></A></A>";
        assert!(XmlDocument::parse(s.as_bytes()).is_err());
    }
}
