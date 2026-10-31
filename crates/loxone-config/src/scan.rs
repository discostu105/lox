//! Personal-data scan of a `.Loxone` config (`loxl scan`).
//!
//! A real config identifies a household: the installation address and
//! coordinates on the Document, the users' names, mail addresses, phone
//! numbers and access codes, credentials, the remote-access address,
//! hardware serial numbers. None of that may reach a repository (the
//! corpus stays untracked, fixtures are synthetic). The scan lists where
//! such data sits — element and attribute, never the value — so a CI
//! step can refuse a commit that carries it.
//!
//! The rules come from the attributes of the house config and the web
//! corpus. They look at *where* a value sits and whether it is set, not
//! at the value itself, except for two shapes that are personal anywhere:
//! a mail address, and a public IPv4 address.

use crate::doc::LoxoneDoc;
use crate::xml::{Element, unescape};
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Finding {
    /// Rule id: `location`, `person`, `access-code`, `credential`,
    /// `remote-access`, `mail`, `network`, `hardware-serial`.
    pub rule: &'static str,
    /// The element: its type and, unless the title is itself personal,
    /// its title.
    pub object: String,
    pub attr: String,
    /// Length of the value — enough to judge a finding, nothing to leak.
    pub len: usize,
}

/// Installation address and position (Document). The country alone
/// identifies nobody.
const LOCATION: &[&str] = &["Street", "Town", "PostCode", "Latitude", "Longitude", "Geo"];
/// A user's name, mail address, phone number (User).
const PERSON: &[&str] = &["FiN", "LaN", "Ema", "Phn"];
/// A user's keypad codes, NFC tags, iButtons (User).
const ACCESS: &[&str] = &["CodeArr", "NFCArr", "NFCNArr", "iBtnArr", "EAIdArr"];

pub fn scan(doc: &LoxoneDoc) -> Vec<Finding> {
    let mut out = Vec::new();
    let mut users = 0;
    walk(&doc.xml.root, &mut out, &mut users);
    out
}

fn walk(el: &Element, out: &mut Vec<Finding>, users: &mut usize) {
    for child in el.child_elements() {
        let ty = child.attr("Type").unwrap_or(&child.name);
        // A user's title is their login: name the user by number.
        let object = if ty == "User" {
            *users += 1;
            format!("User #{users}")
        } else {
            match child.attr("Title") {
                Some(t) if child.name == "C" => format!("{ty} \"{}\"", unescape(t)),
                _ => ty.to_string(),
            }
        };
        let mut hit = |rule, attr: &str, len| {
            out.push(Finding {
                rule,
                object: object.clone(),
                attr: attr.to_string(),
                len,
            })
        };
        for a in &child.attrs {
            let value = unescape(&a.value);
            let v = value.trim();
            if let Some(rule) = rule_of(ty, &a.name, v) {
                hit(rule, &a.name, v.chars().count());
            }
        }
        walk(child, out, users);
    }
}

/// The rule an attribute value falls under, if any.
fn rule_of(ty: &str, name: &str, v: &str) -> Option<&'static str> {
    if is_unset(v) || is_redacted(v) {
        return None;
    }
    match (ty, name) {
        ("Document", n) if LOCATION.contains(&n) => Some("location"),
        ("User", "Title") => Some("person"),
        ("User", n) if PERSON.contains(&n) => Some("person"),
        ("User", n) if ACCESS.contains(&n) => Some("access-code"),
        (_, "ExtAddr") => Some("remote-access"),
        (_, "MAC") => Some("network"),
        (_, "Serial") => Some("hardware-serial"),
        // An Air device's id: its `IP=` is not an IP address.
        ("LoxAIRDevice", "IP") => Some("hardware-serial"),
        (_, n) if is_credential(n) => Some("credential"),
        _ if is_mail(v) => Some("mail"),
        _ if is_public_ipv4(v) => Some("network"),
        _ => None,
    }
}

/// Replace every finding of [`scan`] with a placeholder of its kind, in
/// place: names become `redacted`, mail addresses
/// `redacted@example.invalid`, public addresses `192.0.2.1`, codes and
/// credentials empty, coordinates `0`, MAC addresses zeros. A hardware
/// serial becomes a counter of its length (`000000000001`), and so does
/// every occurrence of it elsewhere — the Miniserver's serial ends every
/// UUID it minted, so the copy keeps its identities apart. Returns the
/// number of values replaced; [`scan`] of the result finds nothing.
pub fn redact(doc: &mut LoxoneDoc) -> usize {
    let mut serials: Vec<(String, String)> = Vec::new();
    let mut n = 0;
    redact_walk(&mut doc.xml.root, &mut serials, &mut n);
    if !serials.is_empty() {
        serials.sort_by_key(|(s, _)| std::cmp::Reverse(s.len()));
        rename_serials(&mut doc.xml.root, &serials);
    }
    n
}

fn redact_walk(el: &mut Element, serials: &mut Vec<(String, String)>, n: &mut usize) {
    for child in el.children.iter_mut() {
        let crate::xml::Node::Element(child) = child else {
            continue;
        };
        let ty = child.attr("Type").unwrap_or(&child.name).to_string();
        for a in child.attrs.iter_mut() {
            let value = unescape(&a.value).trim().to_string();
            let Some(rule) = rule_of(&ty, &a.name, &value) else {
                continue;
            };
            *n += 1;
            a.value = match (rule, a.name.as_str()) {
                ("location", "Latitude" | "Longitude") => "0".into(),
                ("location" | "person", _) => "redacted".into(),
                ("network", "MAC") => value
                    .chars()
                    .map(|c| if c.is_ascii_hexdigit() { '0' } else { c })
                    .collect(),
                ("network", _) => "192.0.2.1".into(),
                ("mail", _) => value
                    .split(' ')
                    .map(|w| {
                        if is_mail(w) {
                            "redacted@example.invalid"
                        } else {
                            w
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(" "),
                ("hardware-serial", _) => {
                    let known = serials.iter().find(|(s, _)| s.eq_ignore_ascii_case(&value));
                    match known {
                        Some((_, r)) => r.clone(),
                        None => {
                            let r = format!("{:0>w$}", serials.len() + 1, w = value.len());
                            serials.push((value.to_ascii_uppercase(), r.clone()));
                            r
                        }
                    }
                }
                _ => String::new(),
            };
        }
        redact_walk(child, serials, n);
    }
}

/// Replace the serials wherever else they occur (UUIDs, references), in
/// the case they are written in.
fn rename_serials(el: &mut Element, serials: &[(String, String)]) {
    for a in el.attrs.iter_mut() {
        for (s, r) in serials {
            if s.len() < 8 {
                continue;
            }
            for form in [s.clone(), s.to_ascii_lowercase()] {
                if a.value.contains(&form) {
                    a.value = a.value.replace(&form, r);
                }
            }
        }
    }
    for child in el.children.iter_mut() {
        if let crate::xml::Node::Element(child) = child {
            rename_serials(child, serials);
        }
    }
}

/// A placeholder [`redact`] writes.
fn is_redacted(v: &str) -> bool {
    v == "redacted"
        || v.split(' ').all(|w| w == "redacted@example.invalid")
        || v == "192.0.2.1"
        || v.chars().all(|c| matches!(c, '0' | ':' | '-' | '.'))
        || (v.len() >= 8
            && v.trim_start_matches('0').len() <= 4
            && v.chars().all(|c| c.is_ascii_digit()))
}

fn is_unset(v: &str) -> bool {
    v.is_empty() || v.chars().all(|c| matches!(c, '0' | '.' | ',' | '-'))
}

fn is_credential(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    [
        "pwd", "password", "passwd", "hash", "secret", "token", "psk",
    ]
    .iter()
    .any(|s| n.ends_with(s))
}

fn is_mail(v: &str) -> bool {
    v.split([' ', ',', ';']).any(|w| {
        w.split_once('@').is_some_and(|(user, host)| {
            !user.is_empty()
                && host.contains('.')
                && !host.starts_with('.')
                && !host.ends_with('.')
                && host
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
        })
    })
}

fn is_public_ipv4(v: &str) -> bool {
    let host = v.split([':', '/']).next().unwrap_or(v);
    let Ok(ip) = host.parse::<std::net::Ipv4Addr>() else {
        return false;
    };
    !(ip.is_private()
        || ip.is_loopback()
        || ip.is_link_local()
        || ip.is_unspecified()
        || ip.is_broadcast()
        || ip.is_documentation()
        || ip.is_multicast())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(body: &str) -> LoxoneDoc {
        LoxoneDoc::parse(format!("<ControlList>{body}</ControlList>").as_bytes()).unwrap()
    }

    #[test]
    fn personal_data_is_located_never_quoted() {
        let d = doc(concat!(
            r#"<C Type="Document" Title="Haus" Street="Hauptstraße 1" Latitude="0" Geo=""/>"#,
            r#"<C Type="UserCaption"><C Type="User" Title="anna" FiN="Anna" Ema="" CodeArr="x"/></C>"#,
            r#"<C Type="LoxLIVE" Title="MS" ExtAddr="dyn.example.org" Serial="504F94A1B2C3"/>"#,
            r#"<C Type="Mailer" Title="Mail" SmtpReceiver="a@example.org" Cloud="true"/>"#,
            r#"<C Type="NetworkDevice" Title="TV" IP="192.168.1.20" Addr="8.8.4.4"/>"#,
            r#"<C Type="Page" Title="a@b"><IoData VisuPwd="geheim"/></C>"#,
            r#"<C Type="LoxAIRDevice" Title="Rauchmelder" IP="0123456789ABCDEF0123456789ABCDEF"/>"#,
        ));
        let got: Vec<(&str, String, String)> = scan(&d)
            .into_iter()
            .map(|f| (f.rule, f.object, f.attr))
            .collect();
        let want = [
            ("location", "Document \"Haus\"", "Street"),
            ("person", "User #1", "Title"),
            ("person", "User #1", "FiN"),
            ("access-code", "User #1", "CodeArr"),
            ("remote-access", "LoxLIVE \"MS\"", "ExtAddr"),
            ("hardware-serial", "LoxLIVE \"MS\"", "Serial"),
            ("mail", "Mailer \"Mail\"", "SmtpReceiver"),
            ("network", "NetworkDevice \"TV\"", "Addr"),
            ("credential", "IoData", "VisuPwd"),
            ("hardware-serial", "LoxAIRDevice \"Rauchmelder\"", "IP"),
        ];
        let want: Vec<(&str, String, String)> = want
            .iter()
            .map(|(r, o, a)| (*r, o.to_string(), a.to_string()))
            .collect();
        assert_eq!(got, want);
        let text = format!("{:?}", scan(&d));
        assert!(!text.contains("Hauptstraße") && !text.contains("geheim"));
    }

    #[test]
    fn a_redacted_copy_has_nothing_left() {
        let mut d = doc(concat!(
            r#"<C Type="Document" Title="Haus" Street="Hauptstraße 1" Latitude="48.3"/>"#,
            r#"<C Type="UserCaption"><C Type="User" Title="anna" Ema="a@example.org" NFCArr="x"/></C>"#,
            r#"<C Type="LoxLIVE" Title="MS" Serial="504F94A1B2C3" MAC="50:4F:94:A1:B2:C3"/>"#,
            r#"<C Type="Page" U="20975331-01c6-a67e-ffff504f94a1b2c3" Title="a"><IoData Pwd="geheim"/></C>"#,
            r#"<C Type="NetworkDevice" Title="TV" Addr="8.8.4.4"/>"#,
        ));
        let before = scan(&d).len();
        assert_eq!(redact(&mut d), before);
        assert_eq!(scan(&d), []);
        let text = String::from_utf8(d.to_bytes()).unwrap();
        for gone in [
            "Hauptstraße",
            "anna",
            "a@example.org",
            "504f94a1b2c3",
            "504F94A1B2C3",
            "geheim",
            "8.8.4.4",
        ] {
            assert!(!text.contains(gone), "{gone}: {text}");
        }
        assert!(
            text.contains(r#"U="20975331-01c6-a67e-ffff000000000001""#),
            "{text}"
        );
        assert!(text.contains(r#"Serial="000000000001""#), "{text}");
        assert!(text.contains(r#"Title="redacted""#) && text.contains(r#"Latitude="0""#));
    }

    #[test]
    fn value_shapes() {
        assert!(is_mail("x@y.de") && !is_mail("@y.de") && !is_mail("a@b"));
        assert!(is_public_ipv4("93.184.216.34:80"));
        assert!(!is_public_ipv4("10.0.0.1") && !is_public_ipv4("1.2.3"));
    }
}
