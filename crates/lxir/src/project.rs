//! Project files: a `lox.toml` marks a directory as one deployment target
//! — base config, module sources, lockfile, compiled output — so the CLI
//! runs without flags inside it (`lxir compile`) and scripts stop
//! threading four paths around (Stufe 4; decision D25: the file split is
//! source ergonomics, there is no `import` statement — every fragment
//! shares one namespace and one lockfile, so the project file is the only
//! project-level construct needed).
//!
//! The format is a strict, flat subset of TOML: `key = "string"` pairs
//! and `#` comments. Tables, arrays, and non-string values are refused
//! with a pointed error — better a small language parsed exactly than a
//! big one parsed approximately. Paths are relative to the file's
//! directory.

use crate::error::{Error, Result};
use crate::uuid::parse_serial;
use std::path::{Path, PathBuf};

/// The file name that marks a project root.
pub const PROJECT_FILE: &str = "lox.toml";

const KNOWN_KEYS: [&str; 6] = ["base", "module", "lock", "out", "serial", "page"];

/// A parsed `lox.toml` with all paths resolved against its directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    /// The directory holding the `lox.toml`.
    pub root: PathBuf,
    /// The deployed `.Loxone` config compiles read (`base = …`, required).
    pub base: PathBuf,
    /// Module source: one `.lxir` file or a directory of fragments,
    /// searched recursively (`module = …`, required — deliberately no
    /// default, so a stray `.lxir` view or backup next to the project
    /// file can never be compiled by accident).
    pub module: PathBuf,
    /// The identity lockfile (`lock = …`, default `lxir.lock.json`).
    pub lock: PathBuf,
    /// Compile output (`out = …`, default `out.Loxone`).
    pub out: PathBuf,
    /// Miniserver serial, 12 hex digits (`serial = …`, optional — after
    /// the first compile the lockfile records it anyway).
    pub serial: Option<String>,
    /// Page title for newly placed blocks (`page = …`, optional).
    pub page: Option<String>,
}

impl Project {
    /// `dir/lox.toml`, if present.
    pub fn find(dir: &Path) -> Option<PathBuf> {
        let file = dir.join(PROJECT_FILE);
        file.is_file().then_some(file)
    }

    /// Load a project file — the file itself, or a directory holding one.
    pub fn load(path: &Path) -> Result<Project> {
        let file = if path.is_dir() {
            let file = path.join(PROJECT_FILE);
            if !file.is_file() {
                return Err(Error::Project(format!(
                    "`{}` has no {PROJECT_FILE}",
                    path.display()
                )));
            }
            file
        } else {
            path.to_path_buf()
        };
        let src = std::fs::read_to_string(&file)?;
        let root = file.parent().unwrap_or(Path::new(".")).to_path_buf();
        Self::parse(&src, &root)
    }

    /// Parse project-file text; relative paths resolve against `root`.
    pub fn parse(src: &str, root: &Path) -> Result<Project> {
        let mut values: Vec<(&str, String)> = Vec::new();
        for (i, raw) in src.lines().enumerate() {
            let lineno = i + 1;
            let line = raw.trim_start_matches('\u{feff}').trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if line.starts_with('[') {
                return Err(err(
                    lineno,
                    "tables are not part of the project format — flat `key = \"value\"` lines only",
                ));
            }
            let Some((key, rest)) = line.split_once('=') else {
                return Err(err(lineno, "expected `key = \"value\"`"));
            };
            let key = key.trim();
            let Some(&key) = KNOWN_KEYS.iter().find(|k| **k == key) else {
                return Err(err(
                    lineno,
                    &format!("unknown key `{key}` (known: {})", KNOWN_KEYS.join(", ")),
                ));
            };
            if values.iter().any(|(k, _)| *k == key) {
                return Err(err(lineno, &format!("`{key}` is set twice")));
            }
            let value = parse_string(rest.trim(), lineno)?;
            if value.is_empty() {
                return Err(err(lineno, &format!("`{key}` is empty")));
            }
            values.push((key, value));
        }
        let mut take = |key: &str| {
            values
                .iter()
                .position(|(k, _)| *k == key)
                .map(|i| values.remove(i).1)
        };
        let require = |v: Option<String>, key: &str| {
            v.ok_or_else(|| Error::Project(format!("missing required key `{key}`")))
        };
        // Joining against a `.` root would render every path as `./x` in
        // messages; strip the no-op prefix.
        let path = |v: String| {
            let p = root.join(v);
            p.strip_prefix(".").map(Path::to_path_buf).unwrap_or(p)
        };
        let base = path(require(take("base"), "base")?);
        let module = path(require(take("module"), "module")?);
        let lock = path(take("lock").unwrap_or_else(|| "lxir.lock.json".into()));
        let out = path(take("out").unwrap_or_else(|| "out.Loxone".into()));
        let serial = take("serial");
        if let Some(s) = &serial {
            parse_serial(s).map_err(|_| {
                Error::Project(format!(
                    "serial \"{s}\" is not a 12-hex-digit Miniserver serial"
                ))
            })?;
        }
        let page = take("page");
        Ok(Project {
            root: root.to_path_buf(),
            base,
            module,
            lock,
            out,
            serial,
            page,
        })
    }
}

fn err(lineno: usize, msg: &str) -> Error {
    Error::Project(format!("line {lineno}: {msg}"))
}

/// A TOML basic string with the common escapes, optionally followed by a
/// `#` comment. Anything else — literal strings, multiline strings, bare
/// values — is refused: project values are quoted strings, full stop.
fn parse_string(rest: &str, lineno: usize) -> Result<String> {
    let mut chars = rest.chars();
    if chars.next() != Some('"') {
        return Err(err(
            lineno,
            "values are quoted strings: `key = \"value\"` (a `\"` must open the value)",
        ));
    }
    let mut out = String::new();
    loop {
        match chars.next() {
            None => return Err(err(lineno, "unterminated string (missing closing `\"`)")),
            Some('"') => break,
            Some('\\') => match chars.next() {
                Some('"') => out.push('"'),
                Some('\\') => out.push('\\'),
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('r') => out.push('\r'),
                c => {
                    return Err(err(
                        lineno,
                        &format!(
                            "unsupported escape `\\{}` (supported: \\\" \\\\ \\n \\t \\r)",
                            c.map(String::from).unwrap_or_default()
                        ),
                    ));
                }
            },
            Some(c) => out.push(c),
        }
    }
    let tail = chars.as_str().trim();
    if !tail.is_empty() && !tail.starts_with('#') {
        return Err(err(
            lineno,
            &format!("unexpected `{tail}` after the closing `\"`"),
        ));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_full_project_file() {
        let src = r#"
            # r50 — Wohnhaus
            base = "r50.Loxone"     # the deployed config
            module = "pages"
            lock = "r50.lock.json"
            out = "out/r50.Loxone"
            serial = "504F94A26236"
            page = "lxir"
        "#;
        let p = Project::parse(src, Path::new("/haus")).unwrap();
        assert_eq!(p.base, Path::new("/haus/r50.Loxone"));
        assert_eq!(p.module, Path::new("/haus/pages"));
        assert_eq!(p.lock, Path::new("/haus/r50.lock.json"));
        assert_eq!(p.out, Path::new("/haus/out/r50.Loxone"));
        assert_eq!(p.serial.as_deref(), Some("504F94A26236"));
        assert_eq!(p.page.as_deref(), Some("lxir"));
    }

    #[test]
    fn defaults_lock_and_out() {
        let p = Project::parse("base = \"a.Loxone\"\nmodule = \".\"\n", Path::new("d")).unwrap();
        assert_eq!(p.lock, Path::new("d/lxir.lock.json"));
        assert_eq!(p.out, Path::new("d/out.Loxone"));
        assert_eq!(p.module, Path::new("d/."));
        assert_eq!(p.serial, None);
    }

    #[test]
    fn refusals_are_pointed() {
        let bad = [
            ("module = \"m\"\n", "missing required key `base`"),
            ("base = \"b\"\n", "missing required key `module`"),
            ("[compile]\n", "tables are not part"),
            ("base b\n", "expected `key = \"value\"`"),
            ("bases = \"b\"\n", "unknown key `bases`"),
            ("base = \"b\"\nbase = \"c\"\n", "`base` is set twice"),
            ("base = b.Loxone\n", "a `\"` must open the value"),
            ("base = \"b\" trailing\n", "unexpected `trailing`"),
            ("base = \"b\nmodule = \"m\"\n", "unterminated string"),
            ("base = \"\\q\"\n", "unsupported escape `\\q`"),
            ("base = \"\"\n", "`base` is empty"),
            (
                "base = \"b\"\nmodule = \"m\"\nserial = \"xyz\"\n",
                "not a 12-hex-digit",
            ),
        ];
        for (src, want) in bad {
            let e = Project::parse(src, Path::new(".")).unwrap_err().to_string();
            assert!(e.contains(want), "{src:?}: got {e:?}, want {want:?}");
        }
    }

    #[test]
    fn escapes_and_comments_in_strings() {
        let src = "base = \"a b.Loxone\"\nmodule = \"with \\\"quotes\\\"\" # trailing\n";
        let p = Project::parse(src, Path::new(".")).unwrap();
        assert_eq!(p.base, Path::new("a b.Loxone"));
        assert_eq!(p.module, Path::new("with \"quotes\""));
    }
}
