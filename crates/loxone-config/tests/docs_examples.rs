//! The docs teach by example: every ```text block of the language spec, the
//! agent guide and the README is Lox Logic that parses, and checks as far as
//! a fragment can (an excerpt may name blocks it does not declare). The
//! agent guide's sample is complete: canonical and passing `loxl check`. The
//! VS Code snippets, with their placeholders filled in, parse too.

use std::path::PathBuf;
use std::process::{Command, Output};

const DOCS: &[&str] = &["docs/language.md", "docs/agents.md", "README.md"];

/// What a check of an excerpt may fail on: a name it does not declare.
const UNDECLARED: &[&str] = &[
    "reference to undeclared",
    "undeclared slug",
    "unknown template",
    "No extern or managed block of this module has that name",
];

/// The ```text blocks of a markdown file, each with its first line.
fn examples(doc: &str) -> Vec<(usize, String)> {
    let text = std::fs::read_to_string(doc).unwrap();
    let mut out = Vec::new();
    let mut lines = text.lines().enumerate();
    while let Some((_, l)) = lines.next() {
        if l.trim() != "```text" {
            continue;
        }
        let mut body = String::new();
        let mut first = None;
        for (n, l) in lines.by_ref() {
            first.get_or_insert(n + 1);
            if l.trim() == "```" {
                break;
            }
            body.push_str(l);
            body.push('\n');
        }
        out.push((first.unwrap(), body));
    }
    out
}

fn loxl(cmd: &str, name: &str, src: &str) -> Output {
    let dir = std::env::temp_dir().join(format!("loxl-docs-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path: PathBuf = dir.join(format!("{name}.loxl"));
    std::fs::write(&path, src).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_loxl"))
        .arg(cmd)
        .arg(&path)
        .output()
        .unwrap();
    std::fs::remove_file(&path).unwrap();
    out
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

#[test]
fn doc_examples_parse_and_check() {
    let mut failures = Vec::new();
    for doc in DOCS {
        let blocks = examples(doc);
        assert!(!blocks.is_empty(), "{doc} has no examples");
        for (line, src) in blocks {
            let name = format!("{}-{line}", doc.replace(['/', '.'], "_"));
            let fmt = loxl("fmt", &name, &src);
            if !fmt.status.success() {
                failures.push(format!("{doc}:{line}: does not parse: {}", stderr(&fmt)));
                continue;
            }
            let check = loxl("check", &name, &src);
            let err = stderr(&check);
            if !check.status.success() && !UNDECLARED.iter().any(|m| err.contains(m)) {
                failures.push(format!("{doc}:{line}: check fails: {err}"));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn the_agent_guide_sample_is_canonical_and_checks() {
    let text = std::fs::read_to_string("docs/agents.md").unwrap();
    let at = text.find("## The language in 30 seconds").unwrap();
    let (_, src) = examples("docs/agents.md")
        .into_iter()
        .find(|(line, _)| text.lines().take(*line).map(|l| l.len() + 1).sum::<usize>() > at)
        .unwrap();
    let fmt = loxl("fmt", "agents-sample", &src);
    assert!(fmt.status.success(), "{}", stderr(&fmt));
    assert_eq!(String::from_utf8_lossy(&fmt.stdout), src, "not canonical");
    let check = loxl("check", "agents-sample", &src);
    assert!(check.status.success(), "{}", stderr(&check));
}

/// A snippet body with each placeholder replaced by its default:
/// `${1:name}` → `name`, `${2|a,b|}` → `a`, `$0` → nothing.
fn expand(body: &str) -> String {
    let mut out = String::new();
    let mut rest = body;
    while let Some(at) = rest.find('$') {
        out.push_str(&rest[..at]);
        rest = &rest[at + 1..];
        if let Some(inner) = rest.strip_prefix('{') {
            // Placeholders do not nest in our snippets.
            let end = inner.find('}').unwrap();
            let p = &inner[..end];
            let p = p.trim_start_matches(|c: char| c.is_ascii_digit());
            if let Some(d) = p.strip_prefix(':') {
                out.push_str(d);
            } else if let Some(c) = p.strip_prefix('|') {
                out.push_str(c.split([',', '|']).next().unwrap());
            }
            rest = &inner[end + 1..];
        } else {
            rest = rest.trim_start_matches(|c: char| c.is_ascii_digit());
        }
    }
    out.push_str(rest);
    out
}

#[test]
fn vscode_snippets_parse() {
    let json = std::fs::read_to_string("editor/vscode/snippets/loxl.json").unwrap();
    let snippets: serde_json::Value = serde_json::from_str(&json).unwrap();
    let mut failures = Vec::new();
    for (name, s) in snippets.as_object().unwrap() {
        let body = match &s["body"] {
            serde_json::Value::String(b) => b.clone(),
            b => b
                .as_array()
                .unwrap()
                .iter()
                .map(|l| l.as_str().unwrap())
                .collect::<Vec<_>>()
                .join("\n"),
        };
        let mut src = expand(&body);
        // A binding alone is an argument: give it a call.
        if name == "unit value" {
            src = format!("m = Monoflop({src})");
        }
        src.push('\n');
        let fmt = loxl("fmt", &format!("snippet-{}", name.replace(' ', "_")), &src);
        if !fmt.status.success() {
            failures.push(format!("{name}: {}\n{src}", stderr(&fmt)));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
