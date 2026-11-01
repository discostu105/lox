//! `loxl lsp`: a language server for `.loxl` modules over stdio
//! (JSON-RPC with `Content-Length` framing, LSP 3.17).
//!
//! - Diagnostics: what `loxl check` reports, plus the lints. In a
//!   `lox.toml` project, opening or saving a file also compiles the module
//!   against the base config in memory (extern resolution, lockfile, dead
//!   outputs); nothing is written.
//! - Go to definition and hover for names, with the base object an extern
//!   resolves to; hover on a port shows its direction and documented
//!   range.
//! - Completion of ports after `name.`, of block types after `name =`.
//! - Formatting (`loxl fmt`) and the outline of declarations.
//! - References and rename of module-level names (constants, externs,
//!   blocks, templates, instances) across the module's files; in a
//!   project, rename is `loxl rename`'s verified plan and also rewrites
//!   the lockfile. Quick fixes for "did you mean `X`?" diagnostics,
//!   and `# lint: allow <kind>` comments added or removed.
//!
//! Synchronous and single-threaded: requests are answered in order.

pub mod analysis;
pub mod text;

use analysis::{Severity, Workspace};
use serde_json::{Value, json};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use text::{Encoding, path_to_uri, uri_to_path};

/// Serve on stdin/stdout until the client sends `exit`.
pub fn run_stdio() -> std::io::Result<()> {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    serve(&mut stdin.lock(), &mut stdout.lock())
}

/// Serve on `input`/`output` until `exit` or the end of the input.
pub fn serve(input: &mut impl BufRead, output: &mut impl Write) -> std::io::Result<()> {
    let mut server = Server {
        ws: Workspace::default(),
        enc: Encoding::Utf16,
        out: output,
    };
    while let Some(msg) = read_message(input)? {
        if msg.get("method").and_then(Value::as_str) == Some("exit") {
            break;
        }
        server.handle(&msg)?;
    }
    Ok(())
}

fn read_message(input: &mut impl BufRead) -> std::io::Result<Option<Value>> {
    let mut len: Option<usize> = None;
    loop {
        let mut header = String::new();
        if input.read_line(&mut header)? == 0 {
            return Ok(None);
        }
        let header = header.trim_end();
        if header.is_empty() {
            if len.is_some() {
                break;
            }
            continue;
        }
        if let Some(v) = header
            .split_once(':')
            .filter(|(k, _)| k.eq_ignore_ascii_case("content-length"))
        {
            len = v.1.trim().parse().ok();
        }
    }
    let mut body = vec![0; len.unwrap_or(0)];
    input.read_exact(&mut body)?;
    Ok(Some(serde_json::from_slice(&body).unwrap_or(Value::Null)))
}

fn write_message(out: &mut impl Write, msg: &Value) -> std::io::Result<()> {
    let body = msg.to_string();
    write!(out, "Content-Length: {}\r\n\r\n{body}", body.len())?;
    out.flush()
}

struct Server<'o, W: Write> {
    ws: Workspace,
    enc: Encoding,
    out: &'o mut W,
}

impl<W: Write> Server<'_, W> {
    fn handle(&mut self, msg: &Value) -> std::io::Result<()> {
        let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
        let params = msg.get("params").cloned().unwrap_or(Value::Null);
        let id = msg.get("id").cloned();
        let result = match method {
            "initialize" => Some(self.initialize(&params)),
            "shutdown" => Some(Value::Null),
            "textDocument/didOpen" => {
                let doc = &params["textDocument"];
                if let Some(path) = path_of(doc) {
                    let text = doc["text"].as_str().unwrap_or("").to_string();
                    self.ws.open(path.clone(), text);
                    self.publish(&path, true)?;
                }
                None
            }
            "textDocument/didChange" => {
                if let Some(path) = path_of(&params["textDocument"])
                    && let Some(text) = params["contentChanges"]
                        .as_array()
                        .and_then(|c| c.last())
                        .and_then(|c| c["text"].as_str())
                {
                    self.ws.open(path.clone(), text.to_string());
                    self.publish(&path, false)?;
                }
                None
            }
            "textDocument/didSave" => {
                if let Some(path) = path_of(&params["textDocument"]) {
                    self.publish(&path, true)?;
                }
                None
            }
            "textDocument/didClose" => {
                if let Some(path) = path_of(&params["textDocument"]) {
                    self.ws.close(&path);
                }
                None
            }
            "textDocument/definition" => Some(self.at(&params, |s, p, l, b| {
                s.ws.definition(p, l, b)
                    .map_or(Value::Null, |d| s.location(&d))
            })),
            "textDocument/hover" => Some(self.at(&params, |s, p, l, b| {
                s.ws.hover(p, l, b).map_or(Value::Null, |(md, span)| {
                    json!({
                        "contents": { "kind": "markdown", "value": md },
                        "range": s.range(&span),
                    })
                })
            })),
            "textDocument/completion" => Some(self.at(&params, |s, p, l, b| {
                let items: Vec<Value> =
                    s.ws.completion(p, l, b)
                        .into_iter()
                        .map(|c| {
                            json!({
                                "label": c.label,
                                "kind": completion_kind(c.kind),
                                "detail": c.detail,
                            })
                        })
                        .collect();
                Value::Array(items)
            })),
            "textDocument/references" => Some(self.at(&params, |s, p, l, b| {
                let all = s.ws.references(p, l, b);
                let skip_decl = params["context"]["includeDeclaration"] == false;
                let decl = skip_decl.then(|| s.ws.definition(p, l, b)).flatten();
                Value::Array(
                    all.iter()
                        .filter(|r| Some(*r) != decl.as_ref())
                        .map(|r| s.location(r))
                        .collect(),
                )
            })),
            "textDocument/prepareRename" => Some(self.at(&params, |s, p, l, b| {
                s.ws.name_at(p, l, b).map_or(
                    Value::Null,
                    |(name, span)| json!({ "range": s.range(&span), "placeholder": name }),
                )
            })),
            "textDocument/rename" => {
                let new = params["newName"].as_str().unwrap_or("").to_string();
                let edits = self.at(&params, |s, p, l, b| match s.ws.rename(p, l, b, &new) {
                    Ok(files) => s.workspace_edit(&files),
                    Err(e) => json!({ "error": e }),
                });
                if let (Some(id), Some(e)) = (&id, edits.get("error")) {
                    return write_message(
                        self.out,
                        &json!({ "jsonrpc": "2.0", "id": id, "error": {
                            "code": -32803, "message": e,
                        }}),
                    );
                }
                Some(edits)
            }
            "textDocument/codeAction" => Some(self.code_actions(&params)),
            "textDocument/formatting" => Some(self.formatting(&params)),
            "textDocument/documentSymbol" => Some(self.symbols(&params)),
            _ => {
                // Unknown requests get an error, notifications nothing.
                if let Some(id) = id {
                    return write_message(
                        self.out,
                        &json!({ "jsonrpc": "2.0", "id": id, "error": {
                            "code": -32601, "message": format!("method not found: {method}"),
                        }}),
                    );
                }
                None
            }
        };
        match (id, result) {
            (Some(id), Some(result)) => write_message(
                self.out,
                &json!({ "jsonrpc": "2.0", "id": id, "result": result }),
            ),
            _ => Ok(()),
        }
    }

    fn initialize(&mut self, params: &Value) -> Value {
        let utf8 = params["capabilities"]["general"]["positionEncodings"]
            .as_array()
            .is_some_and(|e| e.iter().any(|e| e == "utf-8"));
        self.enc = if utf8 {
            Encoding::Utf8
        } else {
            Encoding::Utf16
        };
        json!({
            "capabilities": {
                "positionEncoding": if utf8 { "utf-8" } else { "utf-16" },
                "textDocumentSync": { "openClose": true, "change": 1, "save": true },
                "definitionProvider": true,
                "hoverProvider": true,
                "completionProvider": { "triggerCharacters": ["."] },
                "documentFormattingProvider": true,
                "documentSymbolProvider": true,
                "referencesProvider": true,
                "renameProvider": { "prepareProvider": true },
                "codeActionProvider": { "codeActionKinds": ["quickfix"] },
            },
            "serverInfo": { "name": "loxl", "version": env!("CARGO_PKG_VERSION") },
        })
    }

    /// Run `f` on the document position of a request.
    fn at(&self, params: &Value, f: impl Fn(&Self, &Path, usize, usize) -> Value) -> Value {
        let Some(path) = path_of(&params["textDocument"]) else {
            return Value::Null;
        };
        let line = params["position"]["line"].as_u64().unwrap_or(0) as usize;
        let col = params["position"]["character"].as_u64().unwrap_or(0) as u32;
        let text = self.ws.text(&path).unwrap_or_default();
        let byte = self.enc.byte(text::line(&text, line), col);
        f(self, &path, line, byte)
    }

    fn publish(&mut self, path: &Path, deep: bool) -> std::io::Result<()> {
        let unit = self.ws.unit(path);
        for (file, diags) in self.ws.diagnostics(&unit, deep) {
            let text = self.ws.text(&file).unwrap_or_default();
            let items: Vec<Value> = diags
                .iter()
                .map(|d| {
                    let l = text::line(&text, d.line);
                    let mut item = json!({
                        "range": {
                            "start": { "line": d.line, "character": self.enc.column(l, d.start) },
                            "end": { "line": d.line, "character": self.enc.column(l, d.end) },
                        },
                        "severity": match d.severity { Severity::Error => 1, Severity::Warning => 2 },
                        "source": "loxl",
                        "message": d.message,
                    });
                    // Sent back with a code action request: where the
                    // allow comment for the finding goes.
                    if let Some((kind, line)) = d.allow {
                        item["data"] = json!({ "allow": kind.label(), "line": line });
                    }
                    item
                })
                .collect();
            write_message(
                self.out,
                &json!({
                    "jsonrpc": "2.0",
                    "method": "textDocument/publishDiagnostics",
                    "params": { "uri": path_to_uri(&file), "diagnostics": items },
                }),
            )?;
        }
        Ok(())
    }

    fn range(&self, span: &analysis::Span) -> Value {
        let text = self.ws.text(&span.file).unwrap_or_default();
        let l = text::line(&text, span.line);
        json!({
            "start": { "line": span.line, "character": self.enc.column(l, span.start) },
            "end": { "line": span.line, "character": self.enc.column(l, span.end) },
        })
    }

    fn location(&self, span: &analysis::Span) -> Value {
        json!({ "uri": path_to_uri(&span.file), "range": self.range(span) })
    }

    /// A `WorkspaceEdit` for the files a rename rewrites: the lines that
    /// change, or the whole file when its line count changes.
    fn workspace_edit(&self, files: &[analysis::FileEdit]) -> Value {
        let mut changes = serde_json::Map::new();
        for f in files {
            changes.insert(
                path_to_uri(&f.file),
                Value::Array(self.line_edits(&f.old, &f.new)),
            );
        }
        json!({ "changes": changes })
    }

    fn line_edits(&self, old: &str, new: &str) -> Vec<Value> {
        let (a, b): (Vec<&str>, Vec<&str>) = (old.split('\n').collect(), new.split('\n').collect());
        if a.len() != b.len() {
            return vec![json!({
                "range": {
                    "start": { "line": 0, "character": 0 },
                    "end": { "line": a.len(), "character": 0 },
                },
                "newText": new,
            })];
        }
        a.iter()
            .zip(&b)
            .enumerate()
            .filter(|(_, (x, y))| x != y)
            .map(|(n, (x, y))| {
                json!({
                    "range": {
                        "start": { "line": n, "character": 0 },
                        "end": { "line": n, "character": self.enc.column(x, x.len()) },
                    },
                    "newText": y,
                })
            })
            .collect()
    }

    /// Quick fixes: a diagnostic's "did you mean `X`?" on a misspelled
    /// name, and its "write `X` instead of `Y`" (old spellings, the
    /// `unit-suffix` lint), replace the text with `X`; the `default-page`
    /// lint inserts the page line it names at the top of the file. A lint
    /// finding on a declaration can be kept with `# lint: allow <kind>`
    /// above it, and an `unused-allow` comment removed.
    fn code_actions(&self, params: &Value) -> Value {
        let Some(path) = path_of(&params["textDocument"]) else {
            return Value::Null;
        };
        let text = self.ws.text(&path).unwrap_or_default();
        let uri = path_to_uri(&path);
        let mut out = Vec::new();
        let mut allows = Vec::new();
        for d in params["context"]["diagnostics"]
            .as_array()
            .into_iter()
            .flatten()
        {
            let msg = d["message"].as_str().unwrap_or("");
            if let Some(edit) = self.allow_edit(&text, &d["data"]) {
                let kind = d["data"]["allow"].as_str().unwrap_or("");
                allows.push(json!({
                    "title": format!("Allow this finding (`# lint: allow {kind}`)"),
                    "kind": "quickfix",
                    "diagnostics": [d],
                    "edit": { "changes": { uri.clone(): [edit] } },
                }));
            }
            // An allow that covers nothing: the diagnostic is on the
            // comment line, which goes.
            if msg.starts_with("unused-allow:")
                && let Some(n) = d["range"]["start"]["line"].as_u64()
                && text::line(&text, n as usize)
                    .trim_start()
                    .strip_prefix('#')
                    .is_some_and(crate::logic::is_allow_comment)
            {
                out.push(json!({
                    "title": "Remove this allow",
                    "kind": "quickfix",
                    "diagnostics": [d],
                    "isPreferred": true,
                    "edit": { "changes": { uri.clone(): [{
                        "range": {
                            "start": { "line": n, "character": 0 },
                            "end": { "line": n + 1, "character": 0 },
                        },
                        "newText": "",
                    }] } },
                }));
                continue;
            }
            let (start, end) = (&d["range"]["start"], &d["range"]["end"]);
            let line = start["line"].as_u64().unwrap_or(0) as usize;
            // A list of arguments over several lines ("write `from:
            // 10-01, to: 04-30` instead of `Mode: 4, Month: 10, …`"):
            // replaced from its first argument to its last.
            if let Some((new, old)) = analysis::replacement(msg)
                && old.contains(", ")
                && !text::line(&text, line).contains(old)
            {
                let line_start: usize = text.split('\n').take(line).map(|l| l.len() + 1).sum();
                if let Some((from, to)) = analysis::find_arguments(&text, line_start, old) {
                    let at = |b: usize| {
                        let n = text[..b].matches('\n').count();
                        let start = text[..b].rfind('\n').map_or(0, |i| i + 1);
                        json!({ "line": n, "character": self.enc.column(&text[start..], b - start) })
                    };
                    out.push(json!({
                        "title": format!("Change to `{new}`"),
                        "kind": "quickfix",
                        "diagnostics": [d],
                        "isPreferred": true,
                        "edit": { "changes": { uri.clone(): [{
                            "range": { "start": at(from), "end": at(to) },
                            "newText": new,
                        }] } },
                    }));
                    continue;
                }
            }
            if end["line"].as_u64() != Some(line as u64) {
                continue;
            }
            let l = text::line(&text, line);
            let s = self
                .enc
                .byte(l, start["character"].as_u64().unwrap_or(0) as u32);
            let e = self
                .enc
                .byte(l, end["character"].as_u64().unwrap_or(0) as u32);
            // A v1 spelling whose error names its replacement:
            // "write `clock 07:30` instead of `clock \"07:30\"`".
            if let Some((new, old)) = analysis::replacement(msg)
                && let Some(at) = l.get(s..e).and_then(|r| r.find(old))
            {
                let (from, to) = (s + at, s + at + old.len());
                let range = json!({
                    "start": { "line": line, "character": self.enc.column(l, from) },
                    "end": { "line": line, "character": self.enc.column(l, to) },
                });
                let mut edits = vec![json!({ "range": range, "newText": new })];
                // A set renamed off its block: "… and claim it with
                // `x.access = x_access`" goes after the set's `end`.
                if let Some(claim) = analysis::claim_to_add(msg)
                    && let Some(end) = text
                        .split('\n')
                        .enumerate()
                        .skip(line + 1)
                        .find(|(_, t)| t.split('#').next().unwrap_or("").trim() == "end")
                        .map(|(n, _)| n)
                {
                    let at = json!({ "line": end + 1, "character": 0 });
                    edits.push(json!({
                        "range": { "start": at, "end": at },
                        "newText": format!("\n{claim}\n"),
                    }));
                }
                out.push(json!({
                    "title": format!("Change to `{new}`"),
                    "kind": "quickfix",
                    "diagnostics": [d],
                    "isPreferred": true,
                    "edit": { "changes": { uri.clone(): edits } },
                }));
                continue;
            }
            // A file whose blocks take the default page: its `page` line
            // above the first statement (the comments heading the file
            // stay on top).
            if let Some(page) = analysis::page_to_insert(msg) {
                let first = text
                    .split('\n')
                    .position(|t| !t.split('#').next().unwrap_or("").trim().is_empty())
                    .unwrap_or(0);
                let at = json!({ "line": first, "character": 0 });
                out.push(json!({
                    "title": format!("Insert `{page}`"),
                    "kind": "quickfix",
                    "diagnostics": [d],
                    "isPreferred": true,
                    "edit": { "changes": { uri.clone(): [{
                        "range": { "start": at, "end": at },
                        "newText": format!("{page}\n\n"),
                    }] } },
                }));
                continue;
            }
            let Some(fix) = analysis::suggestion(msg) else {
                continue;
            };
            if !analysis::is_name(&l[s..e]) {
                continue;
            }
            out.push(json!({
                "title": format!("Change to `{fix}`"),
                "kind": "quickfix",
                "diagnostics": [d],
                "isPreferred": true,
                "edit": { "changes": { uri.clone(): [{ "range": d["range"], "newText": fix }] } },
            }));
        }
        out.extend(allows);
        Value::Array(out)
    }

    /// The edit that keeps a finding: `# lint: allow <kind>` on its own
    /// line above the declaration at `data.line`, indented like it, or the
    /// kind added to the allow comment already there.
    fn allow_edit(&self, text: &str, data: &Value) -> Option<Value> {
        let kind = data["allow"].as_str()?;
        let n = data["line"].as_u64()? as usize;
        let decl = text::line(text, n);
        let indent = &decl[..decl.len() - decl.trim_start().len()];
        if n > 0 {
            let above = text::line(text, n - 1);
            let comment = above.trim_start();
            if comment
                .strip_prefix('#')
                .is_some_and(crate::logic::is_allow_comment)
            {
                // Right after `allow`, before any reason.
                let at = above.len() - comment.len() + comment.find("allow")? + "allow".len();
                let at = json!({ "line": n - 1, "character": self.enc.column(above, at) });
                return Some(json!({
                    "range": { "start": at, "end": at },
                    "newText": format!(" {kind}"),
                }));
            }
        }
        let at = json!({ "line": n, "character": 0 });
        Some(json!({
            "range": { "start": at, "end": at },
            "newText": format!("{indent}# lint: allow {kind}\n"),
        }))
    }

    fn formatting(&self, params: &Value) -> Value {
        let Some(path) = path_of(&params["textDocument"]) else {
            return Value::Null;
        };
        let text = self.ws.text(&path).unwrap_or_default();
        match analysis::format(&self.ws, &path) {
            Some(canonical) if canonical != text => {
                let lines = text.split('\n').count();
                json!([{
                    "range": {
                        "start": { "line": 0, "character": 0 },
                        "end": { "line": lines, "character": 0 },
                    },
                    "newText": canonical,
                }])
            }
            Some(_) => json!([]),
            None => Value::Null,
        }
    }

    fn symbols(&self, params: &Value) -> Value {
        let Some(path) = path_of(&params["textDocument"]) else {
            return Value::Null;
        };
        let items: Vec<Value> = self
            .ws
            .symbols(&path)
            .iter()
            .map(|d| {
                let range = self.range(&d.span);
                json!({
                    "name": d.slug,
                    "detail": d.ty.clone().unwrap_or_else(|| d.kind.to_string()),
                    "kind": symbol_kind(d.kind),
                    "range": range,
                    "selectionRange": range,
                })
            })
            .collect();
        Value::Array(items)
    }
}

fn path_of(doc: &Value) -> Option<PathBuf> {
    uri_to_path(doc["uri"].as_str()?)
}

/// LSP `CompletionItemKind`.
fn completion_kind(kind: &str) -> u32 {
    match kind {
        "port" => 5,              // Field
        "type" | "template" => 7, // Class
        "let" => 21,              // Constant
        "extern" => 6,            // Variable
        _ => 18,                  // Reference
    }
}

/// LSP `SymbolKind`.
fn symbol_kind(kind: &str) -> u32 {
    match kind {
        "let" => 14,                           // Constant
        "template" => 5,                       // Class
        "extern" => 13,                        // Variable
        "block" | "signal" | "instance" => 19, // Object
        _ => 2,                                // Module
    }
}
