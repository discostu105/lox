//! Network Periphery plugins: created by `gid:` into the "Network
//! Periphery" folder, taken over by `uuid:`, their settings the
//! package's parameters in `<SET>`.

use loxone_config::logic::{CompileOptions, Module, adopt_one, compile};
use loxone_config::uuid::parse_serial;
use loxone_config::{Lockfile, LoxoneDoc};

const FOLDER: &str = "00000003-0000-0000-ffff000000000001";
const MATTER: &str = "00000004-0000-0000-ffff000000000001";

/// The "Network Periphery" folder with a Matter plugin made in Loxone
/// Config, as the house's config holds it.
fn base() -> LoxoneDoc {
    LoxoneDoc::parse(
        format!(
            "<ControlList Version=\"1\" NextObj=\"50\">\r\n\
             \t<C Type=\"Document\" U=\"00000001-0000-0000-ffff000000000001\" ConfigVersion=\"17020828\">\r\n\
             \t\t<C Type=\"WeatherCaption\" V=\"178\" U=\"{FOLDER}\" Title=\"Netzwerkperipherie\" WF=\"16384\" CaptionType=\"27\" SubType=\"27\">\r\n\
             \t\t\t<C Type=\"Plugin\" gid=\"Matter\" V=\"178\" U=\"{MATTER}\" Title=\"Matter\" Cl=\"0,0,0\" WF=\"16384\" Addr=\"\" MAC=\"\">\r\n\
             \t\t\t\t<SET>\r\n\
             \t\t\t\t\t<mx_add_roomnames_to_endpoint_names t=\"1\" v=\"true\"/>\r\n\
             \t\t\t\t</SET>\r\n\
             \t\t\t\t<IoData></IoData>\r\n\
             \t\t\t</C>\r\n\
             \t\t</C>\r\n\
             \t</C>\r\n\
             </ControlList>\r\n"
        )
        .as_bytes(),
    )
    .unwrap()
}

fn opts() -> CompileOptions {
    CompileOptions {
        machine: parse_serial("504F94112233").unwrap(),
        mint_time_unix: 1_767_225_600,
        page_title: Some("Regeln".into()),
        allow_removals: false,
        accept_version: None,
        relayout: Vec::new(),
    }
}

/// The element with `uuid`, and the uuid of its parent.
fn element(doc: &LoxoneDoc, uuid: &str) -> Option<(loxone_config::xml::Element, String)> {
    let objects = doc.objects();
    let o = objects.iter().find(|o| o.uuid == uuid)?;
    let parent = objects
        .iter()
        .find(|p| p.path.len() + 1 == o.path.len() && o.path.starts_with(&p.path))
        .map(|p| p.uuid.clone())
        .unwrap_or_default();
    Some((doc.element_at(&o.path)?.clone(), parent))
}

/// `v=` of a `<SET>` entry, `None` when the entry has none.
fn set_value(el: &loxone_config::xml::Element, id: &str) -> Option<String> {
    let set = el.child_elements().find(|c| c.name == "SET").unwrap();
    let entry = set.child_elements().find(|c| c.name == id).unwrap();
    entry.attr("v").map(str::to_string)
}

const MCP: &str =
    r#"mcp = Plugin("MCP Server", gid: "McpServer", custom_local_url: "https://loxone.local/mcp")"#;

#[test]
fn a_created_plugin_is_added_once_and_converges() {
    let mut lock = Lockfile::default();
    let out = compile(&base(), &Module::parse(MCP).unwrap(), &mut lock, &opts()).unwrap();
    let uuid = lock.hardware["mcp"].clone();
    let (el, parent) = element(&out, &uuid).unwrap();
    assert_eq!(parent, FOLDER);
    assert_eq!(el.attr("gid"), Some("McpServer"));
    assert_eq!(el.attr("Title"), Some("MCP Server"));
    assert_eq!(el.attr("V"), Some("178"));
    assert_eq!(
        set_value(&el, "custom-local-url").as_deref(),
        Some("https://loxone.local/mcp")
    );
    assert_eq!(set_value(&el, "custom-remote-url"), None);
    let online = el.child_elements().find(|c| c.name == "C").unwrap();
    assert_eq!(online.attr("Type"), Some("Online"));
    assert_eq!(online.attr("Title"), Some("Online Status MCP Server"));

    // Compiling the pushed config again changes nothing.
    let again = compile(&out, &Module::parse(MCP).unwrap(), &mut lock, &opts()).unwrap();
    assert_eq!(again.to_bytes(), out.to_bytes());
    // A base without it gets it back under the same UUID.
    let back = compile(&base(), &Module::parse(MCP).unwrap(), &mut lock, &opts()).unwrap();
    let (el, parent) = element(&back, &uuid).unwrap();
    assert_eq!(parent, FOLDER);
    assert_eq!(el.attr("gid"), Some("McpServer"));
}

#[test]
fn a_setting_is_written_and_cleared() {
    let mut lock = Lockfile::default();
    let out = compile(&base(), &Module::parse(MCP).unwrap(), &mut lock, &opts()).unwrap();
    let uuid = lock.hardware["mcp"].clone();
    let cleared = r#"mcp = Plugin("MCP Server", gid: "McpServer", custom_local_url: "")"#;
    let out = compile(&out, &Module::parse(cleared).unwrap(), &mut lock, &opts()).unwrap();
    let (el, _) = element(&out, &uuid).unwrap();
    assert_eq!(set_value(&el, "custom-local-url"), None);
}

#[test]
fn a_plugin_taken_over_by_uuid_owns_its_settings() {
    let src = format!(
        "let rooms = off\nmatter = Plugin(\"Matter Bridge\", uuid: \"{MATTER}\", \
         mx_add_roomnames_to_endpoint_names: rooms)"
    );
    let mut lock = Lockfile::default();
    let out = compile(&base(), &Module::parse(&src).unwrap(), &mut lock, &opts()).unwrap();
    let (el, _) = element(&out, MATTER).unwrap();
    assert_eq!(el.attr("Title"), Some("Matter Bridge"));
    // An unticked box has no `v=`, as a save writes it.
    assert_eq!(set_value(&el, "mx_add_roomnames_to_endpoint_names"), None);
    assert!(lock.hardware.is_empty());

    let wrong = format!("matter = Plugin(\"M\", uuid: \"{MATTER}\", nope: on)");
    let err = compile(&base(), &Module::parse(&wrong).unwrap(), &mut lock, &opts())
        .unwrap_err()
        .to_string();
    assert!(err.contains("no setting `nope`"), "{err}");
    assert!(err.contains("mx_add_roomnames_to_endpoint_names"), "{err}");
    let typed = format!(
        "matter = Plugin(\"M\", uuid: \"{MATTER}\", mx_add_roomnames_to_endpoint_names: \"x\")"
    );
    let err = compile(&base(), &Module::parse(&typed).unwrap(), &mut lock, &opts())
        .unwrap_err()
        .to_string();
    assert!(err.contains("`on` or `off`"), "{err}");
}

#[test]
fn one_more_than_a_miniserver_takes_is_refused() {
    let src = r#"m = Plugin("Matter", gid: "Matter")"#;
    let err = compile(
        &base(),
        &Module::parse(src).unwrap(),
        &mut Lockfile::default(),
        &opts(),
    )
    .unwrap_err()
    .to_string();
    assert!(err.contains("at most 1"), "{err}");
    assert!(err.contains(&format!("uuid: \"{MATTER}\"")), "{err}");
}

#[test]
fn a_created_plugin_is_deleted_only_by_removed() {
    let mut lock = Lockfile::default();
    let out = compile(&base(), &Module::parse(MCP).unwrap(), &mut lock, &opts()).unwrap();
    let uuid = lock.hardware["mcp"].clone();
    let err = compile(
        &out,
        &Module::parse("").unwrap(),
        &mut lock.clone(),
        &opts(),
    )
    .unwrap_err()
    .to_string();
    assert!(err.contains("created the plugin"), "{err}");
    assert!(err.contains("add `removed mcp`"), "{err}");
    let gone = compile(
        &out,
        &Module::parse("removed mcp").unwrap(),
        &mut lock,
        &opts(),
    )
    .unwrap();
    assert!(element(&gone, &uuid).is_none());
    assert!(element(&gone, MATTER).is_some());
}

#[test]
fn adopting_a_plugin_changes_nothing() {
    let doc = base();
    let empty = Module::parse("").unwrap();
    let mut lock = Lockfile::default();
    compile(&doc, &empty, &mut lock, &opts()).unwrap();
    let adopted = adopt_one(&doc, MATTER, "matter", &empty, &mut lock).unwrap();
    let text = Module {
        items: adopted.items,
    }
    .to_text();
    assert!(text.contains("matter = Plugin("), "{text}");
    assert!(text.contains(&format!("uuid: \"{MATTER}\"")), "{text}");
    assert!(
        text.contains("mx_add_roomnames_to_endpoint_names: on"),
        "{text}"
    );
    let out = compile(&doc, &Module::parse(&text).unwrap(), &mut lock, &opts()).unwrap();
    assert!(loxone_config::diff::diff(&doc, &out).is_empty());
    assert_eq!(element(&out, MATTER), element(&doc, MATTER));
}

#[test]
fn refusals_name_the_fix() {
    let err = |src: &str| {
        compile(
            &base(),
            &Module::parse(src).unwrap(),
            &mut Lockfile::default(),
            &opts(),
        )
        .unwrap_err()
        .to_string()
    };
    assert!(err(r#"x = Plugin("X")"#).contains("gid: "));
    assert!(err(r#"x = Plugin("X", gid: "Exo")"#).contains("cannot create a `Exo`"));
    assert!(err(r#"x = Plugin("X", gid: "McpServer", foo: 1)"#).contains("custom_local_url"));
    assert!(err(r#"x = Plugin("X", serial: "1")"#).contains("does not apply"));
    assert!(
        err(r#"x = Plugin("X", gid: "McpServer", custom_local_url: on)"#).contains("quoted string")
    );
}

#[test]
fn diff_and_drift_see_a_plugin_parameter() {
    use loxone_config::diff;
    let a = base();
    let edited = String::from_utf8(a.to_bytes())
        .unwrap()
        .replace("t=\"1\" v=\"true\"", "t=\"1\" v=\"false\"");
    let b = LoxoneDoc::parse(edited.as_bytes()).unwrap();
    let d = diff::diff(&a, &b);
    assert_eq!(d.param_changes.len(), 1, "{d:#?}");
    assert_eq!(
        d.param_changes[0].port_key,
        "param mx_add_roomnames_to_endpoint_names"
    );
    assert_ne!(
        diff::semantic_fingerprint(&a),
        diff::semantic_fingerprint(&b)
    );
    // The form before plugin parameters joined is blind to it.
    assert_eq!(
        diff::semantic_fingerprint_v14(&a),
        diff::semantic_fingerprint_v14(&b)
    );
    // `v="false"` as a save drops it: no change.
    let dropped = edited.replace(" v=\"false\"", "");
    let c = LoxoneDoc::parse(dropped.as_bytes()).unwrap();
    assert!(diff::diff(&b, &c).is_empty());
    assert_eq!(
        diff::semantic_fingerprint(&b),
        diff::semantic_fingerprint(&c)
    );
}
