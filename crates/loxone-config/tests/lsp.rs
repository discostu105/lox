//! `loxl lsp` end to end: JSON-RPC in, JSON-RPC out, on the example
//! project (`examples/project`, read only — the in-memory compile writes
//! nothing).

use serde_json::{Value, json};
use std::path::{Path, PathBuf};

fn project() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/project")
}

fn uri(p: &Path) -> String {
    loxone_config::lsp::text::path_to_uri(p)
}

/// Run a session: `initialize`, the given messages, `shutdown`, `exit`.
/// Returns everything the server sent.
fn session(messages: &[Value]) -> Vec<Value> {
    let mut all = vec![
        json!({"jsonrpc": "2.0", "id": 0, "method": "initialize", "params": {
            "capabilities": {}
        }}),
    ];
    all.extend_from_slice(messages);
    all.push(json!({"jsonrpc": "2.0", "id": 9999, "method": "shutdown"}));
    all.push(json!({"jsonrpc": "2.0", "method": "exit"}));
    let mut input = Vec::new();
    for m in &all {
        let body = m.to_string();
        input.extend_from_slice(format!("Content-Length: {}\r\n\r\n{body}", body.len()).as_bytes());
    }
    let mut output = Vec::new();
    loxone_config::lsp::serve(&mut std::io::Cursor::new(input), &mut output).unwrap();
    let text = String::from_utf8(output).unwrap();
    let mut out = Vec::new();
    let mut rest = text.as_str();
    while let Some(i) = rest.find("\r\n\r\n") {
        let len: usize = rest[..i]
            .trim()
            .strip_prefix("Content-Length: ")
            .unwrap()
            .parse()
            .unwrap();
        let body = &rest[i + 4..i + 4 + len];
        out.push(serde_json::from_str(body).unwrap());
        rest = &rest[i + 4 + len..];
    }
    out
}

fn response(out: &[Value], id: u64) -> &Value {
    &out.iter().find(|m| m["id"] == id).expect("a response")["result"]
}

/// The diagnostics last published for `file`.
fn diagnostics<'a>(out: &'a [Value], file: &Path) -> &'a Vec<Value> {
    out.iter()
        .rev()
        .filter(|m| m["method"] == "textDocument/publishDiagnostics")
        .find(|m| m["params"]["uri"] == uri(file))
        .expect("diagnostics for the file")["params"]["diagnostics"]
        .as_array()
        .unwrap()
}

fn open(file: &Path, text: &str) -> Value {
    json!({"jsonrpc": "2.0", "method": "textDocument/didOpen", "params": {
        "textDocument": {"uri": uri(file), "languageId": "loxl", "version": 1, "text": text}
    }})
}

fn change(file: &Path, text: &str) -> Value {
    json!({"jsonrpc": "2.0", "method": "textDocument/didChange", "params": {
        "textDocument": {"uri": uri(file), "version": 2},
        "contentChanges": [{"text": text}]
    }})
}

fn save(file: &Path) -> Value {
    json!({"jsonrpc": "2.0", "method": "textDocument/didSave", "params": {
        "textDocument": {"uri": uri(file)}
    }})
}

fn at(id: u64, method: &str, file: &Path, line: u32, character: u32) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "method": method, "params": {
        "textDocument": {"uri": uri(file)},
        "position": {"line": line, "character": character}
    }})
}

/// (line, column) of the first `needle` in `text`, columns in UTF-16.
fn pos(text: &str, needle: &str) -> (u32, u32) {
    let i = text.find(needle).unwrap();
    let line = text[..i].matches('\n').count();
    let start = text[..i].rfind('\n').map_or(0, |n| n + 1);
    (line as u32, text[start..i].encode_utf16().count() as u32)
}

#[test]
fn a_clean_project_publishes_empty_diagnostics_for_every_fragment() {
    let pool = project().join("pages/pool.loxl");
    let src = std::fs::read_to_string(&pool).unwrap();
    let out = session(&[open(&pool, &src)]);

    let caps = &response(&out, 0)["capabilities"];
    assert_eq!(caps["positionEncoding"], "utf-16");
    assert_eq!(
        caps["completionProvider"]["triggerCharacters"],
        json!(["."])
    );
    for f in ["_periphery.loxl", "pool.loxl", "technik.loxl"] {
        assert_eq!(
            diagnostics(&out, &project().join("pages").join(f)),
            &Vec::<Value>::new(),
            "{f}"
        );
    }
}

#[test]
fn definition_hover_and_completion_resolve_across_fragments() {
    let pool = project().join("pages/pool.loxl");
    let periphery = project().join("pages/_periphery.loxl");
    let src = std::fs::read_to_string(&pool).unwrap();
    let (l, c) = pos(&src, "wasser.Q");
    let (pl, pc) = pos(&src, "pumpe.On");
    let (tl, tc) = pos(&src, "Less(");
    // The editor's buffer while typing `pumpe.`: completion asks the
    // unsaved text.
    let typing = src.replace("pumpe.On <- heizen.Q", "pumpe.");
    let (cl, _) = pos(&typing, "pumpe.");
    let out = session(&[
        open(&pool, &src),
        at(1, "textDocument/definition", &pool, l, c + 2),
        at(2, "textDocument/hover", &pool, l, c + 1),
        at(3, "textDocument/hover", &pool, l, c + 7),
        at(4, "textDocument/hover", &pool, tl, tc + 1),
        at(5, "textDocument/hover", &pool, pl, pc + 2),
        change(&pool, &typing),
        at(6, "textDocument/completion", &pool, cl, 6),
    ]);

    let def = response(&out, 1);
    assert_eq!(def["uri"], uri(&periphery));
    let decl_line = std::fs::read_to_string(&periphery).unwrap();
    assert_eq!(
        def["range"]["start"],
        json!({"line": pos(&decl_line, "wasser").0, "character": 7})
    );

    // The extern, with the object it resolves to in the base config.
    let hover = response(&out, 2)["contents"]["value"].as_str().unwrap();
    assert!(
        hover.contains(r#"extern wasser = VirtualIn(iname: "VI1")"#),
        "{hover}"
    );
    assert!(hover.contains("`VirtualIn`"), "{hover}");
    // A port of a builtin type…
    let hover = response(&out, 3)["contents"]["value"].as_str().unwrap();
    assert!(hover.starts_with("**VirtualIn.Q** — output"), "{hover}");
    // …a block type…
    let hover = response(&out, 4)["contents"]["value"].as_str().unwrap();
    assert!(hover.starts_with("**Less**"), "{hover}");
    assert!(hover.contains("`Input1` input"), "{hover}");
    // …and an extern with a room.
    let hover = response(&out, 5)["contents"]["value"].as_str().unwrap();
    assert!(hover.contains(r#"room "Pool""#), "{hover}");

    let labels: Vec<&str> = response(&out, 6)
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["label"].as_str().unwrap())
        .collect();
    assert!(labels.contains(&"On"), "{labels:?}");
}

#[test]
fn errors_land_on_the_name_they_are_about() {
    let pool = project().join("pages/pool.loxl");
    let src = std::fs::read_to_string(&pool).unwrap();

    // A port the extern lacks: found by the in-memory compile on save.
    let bad_port = src.replace("wasser.Q,", "wasser.Qx,");
    let out = session(&[open(&pool, &src), change(&pool, &bad_port), save(&pool)]);
    let d = diagnostics(&out, &pool);
    assert_eq!(d.len(), 1, "{d:?}");
    let (l, c) = pos(&bad_port, "Qx");
    assert_eq!(d[0]["range"]["start"], json!({"line": l, "character": c}));
    assert_eq!(d[0]["severity"], 1);
    assert!(
        d[0]["message"]
            .as_str()
            .unwrap()
            .contains("did you mean `Q`")
    );

    // A misspelled argument key, while typing (no compile needed).
    let bad_key = src.replace("Input1: wasser", "Inputx: wasser");
    let out = session(&[open(&pool, &bad_key)]);
    let d = diagnostics(&out, &pool);
    let (l, c) = pos(&bad_key, "Inputx");
    assert_eq!(d[0]["range"]["start"], json!({"line": l, "character": c}));

    // An undeclared name: its use.
    let undeclared = src.replace("pumpe.On", "pumpi.On");
    let out = session(&[open(&pool, &undeclared)]);
    let d = diagnostics(&out, &pool);
    let (l, c) = pos(&undeclared, "pumpi");
    assert_eq!(d[0]["range"]["start"], json!({"line": l, "character": c}));

    // A value next to a wire on one port (D77): the port key in the call.
    let both = src.replace("\tInput1: wasser.Q,", "\tInput1: 3,\n\tInput1: wasser.Q,");
    let out = session(&[open(&pool, &both)]);
    let d = diagnostics(&out, &pool);
    assert_eq!(d.len(), 1, "{d:?}");
    let (l, c) = pos(&both, "Input1: 3");
    assert_eq!(d[0]["range"]["start"], json!({"line": l, "character": c}));
    assert!(
        d[0]["message"]
            .as_str()
            .unwrap()
            .contains("takes both a value (`Input1: 3`) and a wire (`Input1: wasser.Q`)"),
        "{d:?}"
    );

    // A duplicate argument: the same.
    let dup = src.replace("\tInput2: solltemp,", "\tInput2: solltemp,\n\tInput2: 4,");
    let out = session(&[open(&pool, &dup)]);
    let d = diagnostics(&out, &pool);
    let (l, c) = pos(&dup, "Input2: solltemp");
    assert_eq!(d[0]["range"]["start"], json!({"line": l, "character": c}));
    assert!(
        d[0]["message"]
            .as_str()
            .unwrap()
            .contains("duplicate parameter `Input2`"),
        "{d:?}"
    );

    // A syntax error: its line.
    let syntax = src.replace("pumpe.On <- heizen.Q", "pumpe.On <- <- heizen.Q");
    let out = session(&[open(&pool, &syntax)]);
    let d = diagnostics(&out, &pool);
    assert_eq!(d[0]["range"]["start"]["line"], pos(&syntax, "<- <-").0);
}

#[test]
fn unused_names_are_warnings() {
    let pool = project().join("pages/pool.loxl");
    let src = std::fs::read_to_string(&pool).unwrap() + "let spare = 3\n";
    let out = session(&[open(&pool, &src)]);
    let d = diagnostics(&out, &pool);
    assert_eq!(d.len(), 1, "{d:?}");
    assert_eq!(d[0]["severity"], 2);
    assert!(d[0]["message"].as_str().unwrap().starts_with("unused-let"));
    assert_eq!(d[0]["range"]["start"]["line"], pos(&src, "spare").0);
}

#[test]
fn formatting_and_outline() {
    let pool = project().join("pages/pool.loxl");
    let src = std::fs::read_to_string(&pool).unwrap();
    let messy = src.replace("pumpe.On <- heizen.Q", "pumpe.On   <-   heizen.Q");
    let out = session(&[
        open(&pool, &messy),
        json!({"jsonrpc": "2.0", "id": 1, "method": "textDocument/formatting", "params": {
            "textDocument": {"uri": uri(&pool)}, "options": {"tabSize": 4, "insertSpaces": false}
        }}),
        json!({"jsonrpc": "2.0", "id": 2, "method": "textDocument/documentSymbol", "params": {
            "textDocument": {"uri": uri(&pool)}
        }}),
        json!({"jsonrpc": "2.0", "id": 3, "method": "textDocument/unknown", "params": {}}),
    ]);
    let edits = response(&out, 1).as_array().unwrap();
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0]["newText"], src);

    let symbols: Vec<(&str, &str)> = response(&out, 2)
        .as_array()
        .unwrap()
        .iter()
        .map(|s| (s["name"].as_str().unwrap(), s["detail"].as_str().unwrap()))
        .collect();
    assert_eq!(symbols, [("heizen", "Less")]);

    let unknown = out.iter().find(|m| m["id"] == 3).unwrap();
    assert_eq!(unknown["error"]["code"], -32601);
}

fn with(mut msg: Value, key: &str, value: Value) -> Value {
    msg["params"][key] = value;
    msg
}

#[test]
fn references_find_a_name_across_fragments_but_not_ports_or_keys() {
    let pool = project().join("pages/pool.loxl");
    let periphery = project().join("pages/_periphery.loxl");
    let src = std::fs::read_to_string(&pool).unwrap();
    let (l, c) = pos(&src, "solltemp");
    let (kl, kc) = pos(&src, "Input1");
    let out = session(&[
        open(&pool, &src),
        at(1, "textDocument/references", &pool, l, c + 1),
        with(
            at(2, "textDocument/references", &pool, l, c + 1),
            "context",
            json!({"includeDeclaration": false}),
        ),
        at(3, "textDocument/prepareRename", &pool, l, c + 1),
        at(4, "textDocument/prepareRename", &pool, kl, kc + 1),
    ]);

    let refs = response(&out, 1).as_array().unwrap();
    let files: Vec<&str> = refs.iter().map(|r| r["uri"].as_str().unwrap()).collect();
    assert_eq!(files, [uri(&periphery), uri(&pool)], "{refs:?}");
    assert_eq!(
        refs[1]["range"]["start"],
        json!({"line": l, "character": c})
    );
    assert_eq!(response(&out, 2).as_array().unwrap().len(), 1);
    assert_eq!(response(&out, 3)["placeholder"], "solltemp");
    // An argument key is not a name rename takes.
    assert_eq!(response(&out, 4), &Value::Null);
}

#[test]
fn rename_in_a_project_rewrites_the_fragments_and_the_lockfile() {
    let pool = project().join("pages/pool.loxl");
    let lock = project().join("pool.lock.json");
    let src = std::fs::read_to_string(&pool).unwrap();
    let (l, c) = pos(&src, "heizen =");
    let out = session(&[
        open(&pool, &src),
        with(
            at(1, "textDocument/rename", &pool, l, c + 1),
            "newName",
            json!("heizung"),
        ),
        with(
            at(2, "textDocument/rename", &pool, l, c + 1),
            "newName",
            json!("solltemp"),
        ),
    ]);

    let changes = response(&out, 1)["changes"].as_object().unwrap();
    let mut files: Vec<&String> = changes.keys().collect();
    files.sort();
    assert_eq!(files, [&uri(&pool), &uri(&lock)]);
    let edited: Vec<&str> = changes[&uri(&pool)]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["newText"].as_str().unwrap())
        .collect();
    assert_eq!(edited, ["heizung = Less(", "pumpe.On <- heizung.Q"]);
    let lock_text = changes[&uri(&lock)].as_array().unwrap()[0]["newText"]
        .as_str()
        .unwrap();
    assert!(lock_text.contains("\"heizung\"") && !lock_text.contains("\"heizen\""));
    // The files on disk are untouched: the client applies the edit.
    assert_eq!(std::fs::read_to_string(&pool).unwrap(), src);

    // A name already taken is refused with a JSON-RPC error.
    let err = &out.iter().find(|m| m["id"] == 2).unwrap()["error"];
    assert!(
        err["message"].as_str().unwrap().contains("solltemp"),
        "{err}"
    );
}

#[test]
fn a_did_you_mean_diagnostic_offers_a_quick_fix() {
    let pool = project().join("pages/pool.loxl");
    let src = std::fs::read_to_string(&pool).unwrap();
    let typo = src.replace("wasser.Q,", "wasser.Qx,");
    let (l, c) = pos(&typo, "Qx");
    let out = session(&[open(&pool, &src), change(&pool, &typo), save(&pool)]);
    let diags = diagnostics(&out, &pool).clone();
    let d = diags
        .iter()
        .find(|d| d["message"].as_str().unwrap().contains("did you mean `Q`"))
        .unwrap_or_else(|| panic!("{diags:?}"));
    assert_eq!(d["range"]["start"], json!({"line": l, "character": c}));

    let out = session(&[
        open(&pool, &typo),
        json!({"jsonrpc": "2.0", "id": 1, "method": "textDocument/codeAction", "params": {
            "textDocument": {"uri": uri(&pool)},
            "range": d["range"],
            "context": {"diagnostics": [d]},
        }}),
    ]);
    let actions = response(&out, 1).as_array().unwrap();
    assert_eq!(actions.len(), 1, "{actions:?}");
    assert_eq!(actions[0]["title"], "Change to `Q`");
    assert_eq!(actions[0]["edit"]["changes"][uri(&pool)][0]["newText"], "Q");
}
