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
    let both = src.replace("Input1: wasser.Q,", "Input1: 3, Input1: wasser.Q,");
    let out = session(&[open(&pool, &both)]);
    let d = diagnostics(&out, &pool);
    assert_eq!(d.len(), 1, "{d:?}");
    let (l, c) = pos(&both, "Input1: 3");
    assert_eq!(d[0]["range"]["start"], json!({"line": l, "character": c}));
    assert!(
        d[0]["message"]
            .as_str()
            .unwrap()
            .contains("has both a value (`Input1: 3`) and a wire (`Input1: wasser.Q`)"),
        "{d:?}"
    );

    // A duplicate argument: the same.
    let dup = src.replace("Input2: solltemp)", "Input2: solltemp, Input2: 4)");
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
    assert_eq!(
        edited,
        [
            "heizung = Less(\"Wasser unter Soll\", Input1: wasser.Q, Input2: solltemp)",
            "pumpe.On <- heizung.Q"
        ]
    );
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

#[test]
fn a_v1_spelling_offers_its_replacement_as_a_quick_fix() {
    let pool = project().join("pages/pool.loxl");
    let src = std::fs::read_to_string(&pool).unwrap();
    let v1 = format!("{src}\ntest \"uhr\"\n\tclock \"7:30\"\n\ttick 1\nend\n");
    let out = session(&[open(&pool, &src), change(&pool, &v1), save(&pool)]);
    let diags = diagnostics(&out, &pool).clone();
    let d = diags
        .iter()
        .find(|d| d["message"].as_str().unwrap().contains("instead of"))
        .unwrap_or_else(|| panic!("{diags:?}"));

    let out = session(&[
        open(&pool, &v1),
        json!({"jsonrpc": "2.0", "id": 1, "method": "textDocument/codeAction", "params": {
            "textDocument": {"uri": uri(&pool)},
            "range": d["range"],
            "context": {"diagnostics": [d]},
        }}),
    ]);
    let actions = response(&out, 1).as_array().unwrap();
    assert_eq!(actions.len(), 1, "{actions:?}");
    assert_eq!(actions[0]["title"], "Change to `clock 07:30`");
    let edit = &actions[0]["edit"]["changes"][uri(&pool)][0];
    assert_eq!(edit["newText"], "clock 07:30");
    // Only the old spelling is replaced, not the line's indentation.
    let (l, c) = pos(&v1, "clock \"7:30\"");
    assert_eq!(edit["range"]["start"], json!({"line": l, "character": c}));
    assert_eq!(
        edit["range"]["end"],
        json!({"line": l, "character": c + 12})
    );
}

#[test]
fn a_mode_title_offers_its_slug_as_a_quick_fix() {
    let pool = project().join("pages/pool.loxl");
    let src = std::fs::read_to_string(&pool).unwrap();
    let titled =
        format!("{src}\nparty = Mode(\"Party\")\n\nschedule abend\n\t\"Party\" 18:00-24:00\nend\n");
    let out = session(&[open(&pool, &src), change(&pool, &titled), save(&pool)]);
    let diags = diagnostics(&out, &pool).clone();
    let d = diags
        .iter()
        .find(|d| d["message"].as_str().unwrap().contains("instead of"))
        .unwrap_or_else(|| panic!("{diags:?}"));

    let out = session(&[
        open(&pool, &titled),
        json!({"jsonrpc": "2.0", "id": 1, "method": "textDocument/codeAction", "params": {
            "textDocument": {"uri": uri(&pool)},
            "range": d["range"],
            "context": {"diagnostics": [d]},
        }}),
    ]);
    let actions = response(&out, 1).as_array().unwrap();
    assert_eq!(actions.len(), 1, "{actions:?}");
    assert_eq!(actions[0]["title"], "Change to `party 18:00-24:00`");
    let edit = &actions[0]["edit"]["changes"][uri(&pool)][0];
    let (l, c) = pos(&titled, "\"Party\" 18:00");
    assert_eq!(edit["range"]["start"], json!({"line": l, "character": c}));
}

#[test]
fn an_access_section_on_the_block_offers_the_named_set_as_a_quick_fix() {
    let pool = project().join("pages/pool.loxl");
    let src = std::fs::read_to_string(&pool).unwrap();
    let v1 = format!("{src}\naccess heizen\n\tuser \"christoph\"\nend\n");
    let out = session(&[open(&pool, &src), change(&pool, &v1), save(&pool)]);
    let diags = diagnostics(&out, &pool).clone();
    let d = diags
        .iter()
        .find(|d| d["message"].as_str().unwrap().contains("instead of"))
        .unwrap_or_else(|| panic!("{diags:?}"));

    let out = session(&[
        open(&pool, &v1),
        json!({"jsonrpc": "2.0", "id": 1, "method": "textDocument/codeAction", "params": {
            "textDocument": {"uri": uri(&pool)},
            "range": d["range"],
            "context": {"diagnostics": [d]},
        }}),
    ]);
    let actions = response(&out, 1).as_array().unwrap();
    assert_eq!(actions.len(), 1, "{actions:?}");
    assert_eq!(actions[0]["title"], "Change to `access heizen_access`");
    let edits = actions[0]["edit"]["changes"][uri(&pool)]
        .as_array()
        .unwrap();
    assert_eq!(edits.len(), 2, "{edits:?}");
    assert_eq!(edits[0]["newText"], "access heizen_access");
    // The claim goes on its own line after the section's `end`.
    assert_eq!(edits[1]["newText"], "\nheizen.access = heizen_access\n");
    let after_end = v1.lines().count() as u32;
    assert_eq!(
        edits[1]["range"]["start"],
        json!({"line": after_end, "character": 0})
    );
}

#[test]
fn an_expanded_instance_name_offers_the_scoped_one_as_a_quick_fix() {
    let pool = project().join("pages/pool.loxl");
    let src = std::fs::read_to_string(&pool).unwrap();
    let v1 =
        format!("{src}\ntemplate paar()\n\tein = Not()\nend\npx = paar()\nqx = Not(I: px_ein.Q)\n");
    let out = session(&[open(&pool, &src), change(&pool, &v1), save(&pool)]);
    let diags = diagnostics(&out, &pool).clone();
    let d = diags
        .iter()
        .find(|d| d["message"].as_str().unwrap().contains("instead of"))
        .unwrap_or_else(|| panic!("{diags:?}"));
    let (l, c) = pos(&v1, "px_ein.Q");
    assert_eq!(d["range"]["start"], json!({"line": l, "character": c}));

    let out = session(&[
        open(&pool, &v1),
        json!({"jsonrpc": "2.0", "id": 1, "method": "textDocument/codeAction", "params": {
            "textDocument": {"uri": uri(&pool)},
            "range": d["range"],
            "context": {"diagnostics": [d]},
        }}),
    ]);
    let actions = response(&out, 1).as_array().unwrap();
    assert_eq!(actions.len(), 1, "{actions:?}");
    assert_eq!(
        actions[0]["edit"]["changes"][uri(&pool)][0]["newText"],
        "px.Q"
    );
}

#[test]
fn completion_after_an_instance_offers_its_blocks_and_primary_ports() {
    let pool = project().join("pages/pool.loxl");
    let src = std::fs::read_to_string(&pool).unwrap();
    let typing = format!(
        "{src}\ntemplate paar() -> ein\n\tein = Not()\n\taus = Or()\nend\npx = paar()\npx.\n"
    );
    let (cl, _) = pos(&typing, "px.\n");
    let out = session(&[
        open(&pool, &src),
        change(&pool, &typing),
        at(1, "textDocument/completion", &pool, cl, 3),
    ]);
    let labels: Vec<&str> = response(&out, 1)
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["label"].as_str().unwrap())
        .collect();
    // `aus` is a block of the instance; `Q` and `I` ports of its primary
    // block `ein` (a Not).
    assert!(labels.contains(&"aus"), "{labels:?}");
    assert!(labels.contains(&"Q"), "{labels:?}");
    assert!(!labels.contains(&"ein"), "{labels:?}");
}

/// The labels of a completion response.
fn labels(out: &[Value], id: u64) -> Vec<String> {
    response(out, id)
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["label"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn scoped_instance_names_hover_and_go_to_the_template_body() {
    let pool = project().join("pages/pool.loxl");
    let src = std::fs::read_to_string(&pool).unwrap();
    let text = format!(
        "{src}\ntemplate paar() -> ein\n\tein = Not()\n\taus = Or()\nend\npx = paar()\n\
         lampe.On <- px.aus.Q\npumpe.Off <- px.Q\n"
    );
    let (bl, bc) = pos(&text, "px.aus.Q");
    let (ql, qc) = pos(&text, "px.Q\n");
    let (dl, dc) = pos(&text, "aus = Or");
    let typing = format!("{text}x = Not(I: px.aus.\n");
    let (cl, cc) = pos(&typing, "px.aus.\n");
    let out = session(&[
        open(&pool, &text),
        at(1, "textDocument/definition", &pool, bl, bc + 4),
        at(2, "textDocument/hover", &pool, bl, bc + 4),
        at(3, "textDocument/definition", &pool, bl, bc + 7),
        at(4, "textDocument/hover", &pool, bl, bc + 7),
        at(5, "textDocument/hover", &pool, ql, qc + 3),
        change(&pool, &typing),
        at(6, "textDocument/completion", &pool, cl, cc + 7),
    ]);
    // `px.aus` and `px.aus.Q` lead to `aus` in the template body.
    for id in [1, 3] {
        let def = response(&out, id);
        assert_eq!(def["uri"], uri(&pool));
        assert_eq!(
            def["range"]["start"],
            json!({"line": dl, "character": dc}),
            "{id}"
        );
    }
    let hover = response(&out, 2)["contents"]["value"].as_str().unwrap();
    assert!(hover.contains("aus = Or()"), "{hover}");
    assert!(hover.contains("compiled as `px_aus`"), "{hover}");
    let hover = response(&out, 4)["contents"]["value"].as_str().unwrap();
    assert!(hover.starts_with("**Or.Q** — output"), "{hover}");
    // The port's line from Loxone's documentation.
    assert!(
        hover.contains("Output: on while any input is on."),
        "{hover}"
    );
    // `px.Q` is a port of the primary block, a Not.
    let hover = response(&out, 5)["contents"]["value"].as_str().unwrap();
    assert!(hover.starts_with("**Not.Q** — output"), "{hover}");
    let got = labels(&out, 6);
    assert!(got.contains(&"Q".to_string()), "{got:?}");
    assert!(got.contains(&"I1".to_string()), "{got:?}");
    let q = response(&out, 6)
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["label"] == "Q")
        .unwrap()
        .clone();
    assert_eq!(
        q["detail"], "output (O) — Output: on while any input is on.",
        "{q}"
    );
}

#[test]
fn completion_offers_argument_keys_and_signal_names() {
    let pool = project().join("pages/pool.loxl");
    let src = std::fs::read_to_string(&pool).unwrap();
    let typing = format!(
        "{src}\nwp_ok = heizen.Q and wasser.Q\ndefaults Monoflop(\n\t\n)\n\
         m = Monoflop(Time: 5s, )\nt = Not(I: )\nn = Not(I: wp_ok.)\n"
    );
    let (dl, _) = pos(&typing, "defaults Monoflop(");
    let (ml, mc) = pos(&typing, ", )");
    let (tl, tc) = pos(&typing, "I: )");
    let (sl, sc) = pos(&typing, "wp_ok.)");
    let out = session(&[
        open(&pool, &src),
        change(&pool, &typing),
        at(1, "textDocument/completion", &pool, dl + 1, 1),
        at(2, "textDocument/completion", &pool, ml, mc + 2),
        at(3, "textDocument/completion", &pool, tl, tc + 3),
        at(4, "textDocument/completion", &pool, sl, sc + 6),
    ]);
    // `defaults` takes parameters only; a block also its inputs.
    let got = labels(&out, 1);
    assert!(got.contains(&"Time".to_string()), "{got:?}");
    assert!(!got.contains(&"InputTrigger".to_string()), "{got:?}");
    assert!(!got.contains(&"Q".to_string()), "{got:?}");
    let got = labels(&out, 2);
    assert!(got.contains(&"InputTrigger".to_string()), "{got:?}");
    assert!(!got.contains(&"Q".to_string()), "{got:?}");
    // A named signal is a name like a block's, with its top block's ports.
    let got = labels(&out, 3);
    assert!(got.contains(&"wp_ok".to_string()), "{got:?}");
    let got = labels(&out, 4);
    assert!(got.contains(&"Q".to_string()), "{got:?}");
}

#[test]
fn a_bare_duration_offers_its_unit_suffix_as_a_quick_fix() {
    let pool = project().join("pages/pool.loxl");
    let src = std::fs::read_to_string(&pool).unwrap();
    let text = format!("{src}\nm = Monoflop(InputTrigger: heizen.Q, Time: 300)\n");
    let out = session(&[open(&pool, &src), change(&pool, &text), save(&pool)]);
    let diags = diagnostics(&out, &pool).clone();
    let d = diags
        .iter()
        .find(|d| d["message"].as_str().unwrap().starts_with("unit-suffix"))
        .unwrap_or_else(|| panic!("{diags:?}"));
    let (l, c) = pos(&text, "Time: 300");
    assert_eq!(d["range"]["start"], json!({"line": l, "character": c}));

    let out = session(&[
        open(&pool, &text),
        json!({"jsonrpc": "2.0", "id": 1, "method": "textDocument/codeAction", "params": {
            "textDocument": {"uri": uri(&pool)},
            "range": d["range"],
            "context": {"diagnostics": [d]},
        }}),
    ]);
    let actions = response(&out, 1).as_array().unwrap();
    // The fix first, then the allow comment that keeps the finding.
    assert_eq!(actions.len(), 2, "{actions:?}");
    assert!(actions[1]["title"].as_str().unwrap().starts_with("Allow"));
    assert_eq!(
        actions[0]["edit"]["changes"][uri(&pool)][0]["newText"],
        "Time: 5min"
    );
}

#[test]
fn a_file_on_the_default_page_offers_the_previous_page_as_a_quick_fix() {
    let technik = project().join("pages/technik.loxl");
    let src = std::fs::read_to_string(&technik).unwrap();
    let text = src.replace("page \"Technik\"\n", "");
    let out = session(&[
        open(&technik, &src),
        change(&technik, &text),
        save(&technik),
    ]);
    let diags = diagnostics(&out, &technik).clone();
    let d = diags
        .iter()
        .find(|d| d["message"].as_str().unwrap().starts_with("default-page"))
        .unwrap_or_else(|| panic!("{diags:?}"));

    let out = session(&[
        open(&technik, &text),
        json!({"jsonrpc": "2.0", "id": 1, "method": "textDocument/codeAction", "params": {
            "textDocument": {"uri": uri(&technik)},
            "range": d["range"],
            "context": {"diagnostics": [d]},
        }}),
    ]);
    let actions = response(&out, 1).as_array().unwrap();
    assert_eq!(actions.len(), 2, "{actions:?}");
    let edit = &actions[0]["edit"]["changes"][uri(&technik)][0];
    assert_eq!(edit["newText"], "page \"Pool\"\n\n");
    // Below the comments that head the file.
    let first = text
        .lines()
        .position(|l| !l.starts_with('#') && !l.trim().is_empty())
        .unwrap();
    assert_eq!(
        edit["range"]["start"],
        json!({"line": first, "character": 0})
    );
}

/// The code actions for diagnostic `d` of `file` holding `text`.
fn actions_for(file: &Path, text: &str, d: &Value) -> Vec<Value> {
    let out = session(&[
        open(file, text),
        json!({"jsonrpc": "2.0", "id": 1, "method": "textDocument/codeAction", "params": {
            "textDocument": {"uri": uri(file)},
            "range": d["range"],
            "context": {"diagnostics": [d]},
        }}),
    ]);
    response(&out, 1).as_array().unwrap().clone()
}

/// The diagnostics of `file` after saving it with `text`.
fn saved(file: &Path, src: &str, text: &str) -> Vec<Value> {
    let out = session(&[open(file, src), change(file, text), save(file)]);
    diagnostics(&out, file).clone()
}

fn with_prefix<'a>(diags: &'a [Value], prefix: &str) -> Option<&'a Value> {
    diags
        .iter()
        .find(|d| d["message"].as_str().unwrap().starts_with(prefix))
}

#[test]
fn a_lint_finding_offers_an_allow_comment_above_its_declaration() {
    let pool = project().join("pages/pool.loxl");
    let src = std::fs::read_to_string(&pool).unwrap();
    // Without its wire `heizen` drives nothing.
    let text = src.replace("pumpe.On <- heizen.Q\n", "");
    let diags = saved(&pool, &src, &text);
    let d = with_prefix(&diags, "dead-outputs").unwrap_or_else(|| panic!("{diags:?}"));
    let (l, _) = pos(&text, "heizen =");
    assert_eq!(d["data"], json!({"allow": "dead-outputs", "line": l}));

    let actions = actions_for(&pool, &text, d);
    assert_eq!(actions.len(), 1, "{actions:?}");
    assert_eq!(
        actions[0]["title"],
        "Allow this finding (`# lint: allow dead-outputs`)"
    );
    let edit = &actions[0]["edit"]["changes"][uri(&pool)][0];
    assert_eq!(edit["newText"], "# lint: allow dead-outputs\n");
    assert_eq!(edit["range"]["start"], json!({"line": l, "character": 0}));

    // With the comment in place the finding is gone, and the comment is used.
    let allowed = text.replace("heizen =", "# lint: allow dead-outputs\nheizen =");
    let diags = saved(&pool, &src, &allowed);
    assert!(diags.is_empty(), "{diags:?}");
}

#[test]
fn an_allow_comment_already_above_takes_the_new_kind() {
    let pool = project().join("pages/pool.loxl");
    let src = std::fs::read_to_string(&pool).unwrap();
    let text = src.replace("pumpe.On <- heizen.Q\n", "").replace(
        "heizen =",
        "# lint: allow unit-suffix kept for later\nheizen =",
    );
    let diags = saved(&pool, &src, &text);
    let d = with_prefix(&diags, "dead-outputs").unwrap_or_else(|| panic!("{diags:?}"));
    let actions = actions_for(&pool, &text, d);
    let allow = actions
        .iter()
        .find(|a| a["title"].as_str().unwrap().starts_with("Allow"))
        .unwrap_or_else(|| panic!("{actions:?}"));
    let edit = &allow["edit"]["changes"][uri(&pool)][0];
    assert_eq!(edit["newText"], " dead-outputs");
    let (l, c) = pos(&text, " unit-suffix kept");
    assert_eq!(edit["range"]["start"], json!({"line": l, "character": c}));
}

#[test]
fn an_unused_allow_shows_on_its_comment_and_offers_to_remove_it() {
    let pool = project().join("pages/pool.loxl");
    let src = std::fs::read_to_string(&pool).unwrap();
    let text = src.replace("heizen = ", "# lint: allow dead-outputs\nheizen = ");
    let diags = saved(&pool, &src, &text);
    let d = with_prefix(&diags, "unused-allow").unwrap_or_else(|| panic!("{diags:?}"));
    let (l, _) = pos(&text, "# lint: allow");
    assert_eq!(d["range"]["start"], json!({"line": l, "character": 0}));
    assert!(d.get("data").is_none(), "{d:?}");

    let actions = actions_for(&pool, &text, d);
    assert_eq!(actions.len(), 1, "{actions:?}");
    assert_eq!(actions[0]["title"], "Remove this allow");
    let edit = &actions[0]["edit"]["changes"][uri(&pool)][0];
    assert_eq!(edit["newText"], "");
    assert_eq!(
        edit["range"],
        json!({"start": {"line": l, "character": 0}, "end": {"line": l + 1, "character": 0}})
    );
}
