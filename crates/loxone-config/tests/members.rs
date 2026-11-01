//! Central block members from source: `<central>.members = (a, b)` writes a
//! CentralLight's or CentralShade's `rec=` in the order written, an empty
//! list removes it, decompile lifts it, and the diff sees it.

use loxone_config::logic::{
    CompileOptions, DecompileOptions, DecompileScope, Module, compile, decompile,
};
use loxone_config::uuid::parse_serial;
use loxone_config::{Lockfile, LoxoneDoc};

fn base() -> LoxoneDoc {
    LoxoneDoc::parse(&std::fs::read("examples/configs/pool.Loxone").unwrap()).unwrap()
}

fn opts() -> CompileOptions {
    CompileOptions {
        machine: parse_serial("504F94112233").unwrap(),
        mint_time_unix: 1_767_225_600,
        page_title: Some("Pool".into()),
        allow_removals: false,
        accept_version: None,
        relayout: Vec::new(),
    }
}

fn try_build(doc: &LoxoneDoc, src: &str, lock: &mut Lockfile) -> Result<LoxoneDoc, String> {
    let m = Module::parse(src).map_err(|e| e.to_string())?;
    compile(doc, &m, lock, &opts()).map_err(|e| e.to_string())
}

fn build(doc: &LoxoneDoc, src: &str, lock: &mut Lockfile) -> LoxoneDoc {
    try_build(doc, src, lock).unwrap()
}

fn uuid_of(doc: &LoxoneDoc, title: &str) -> String {
    doc.objects()
        .into_iter()
        .find(|o| o.title.as_deref() == Some(title))
        .unwrap()
        .uuid
}

/// The element attributes of the block titled `title`, in order.
fn attrs(doc: &LoxoneDoc, title: &str) -> Vec<(String, String)> {
    let o = doc
        .objects()
        .into_iter()
        .find(|o| o.title.as_deref() == Some(title))
        .unwrap();
    doc.element_at(&o.path)
        .unwrap()
        .attrs
        .iter()
        .map(|a| (a.name.clone(), a.value.clone()))
        .collect()
}

fn rec(doc: &LoxoneDoc, title: &str) -> Option<String> {
    attrs(doc, title)
        .into_iter()
        .find(|(n, _)| n == "rec")
        .map(|(_, v)| v)
}

const LIGHTS: &str = "\
l1 = LightController2(\"Licht 1\")
l2 = LightController2(\"Licht 2\")
l3 = LightController2(\"Licht 3\")
zentral = CentralLight(\"Zentral\")
";

#[test]
fn members_are_written_in_the_order_given() {
    let mut lock = Lockfile::default();
    let src = format!("{LIGHTS}zentral.members = (l3, l1)\n");
    let out = build(&base(), &src, &mut lock);
    let want = format!("{},{}", uuid_of(&out, "Licht 3"), uuid_of(&out, "Licht 1"));
    assert_eq!(rec(&out, "Zentral"), Some(want));

    // A new order, against the compiled config.
    let src = format!("{LIGHTS}zentral.members = (l1, l2, l3)\n");
    let again = build(&out, &src, &mut lock);
    let want = ["Licht 1", "Licht 2", "Licht 3"].map(|t| uuid_of(&again, t));
    assert_eq!(rec(&again, "Zentral"), Some(want.join(",")));
    let d = loxone_config::diff::diff(&out, &again);
    assert!(d.param_changes.iter().any(|c| c.port_key == "rec"), "{d:?}");

    // Compiling again changes nothing.
    let same = build(&again, &src, &mut lock);
    assert_eq!(same.to_bytes(), again.to_bytes());
}

#[test]
fn an_empty_list_leaves_the_attribute_out() {
    let mut lock = Lockfile::default();
    let out = build(
        &base(),
        &format!("{LIGHTS}zentral.members = (l1)\n"),
        &mut lock,
    );
    assert!(rec(&out, "Zentral").is_some());
    let out = build(&out, &format!("{LIGHTS}zentral.members = ()\n"), &mut lock);
    assert_eq!(rec(&out, "Zentral"), None);
}

#[test]
fn a_new_list_goes_before_the_special_states() {
    let mut lock = Lockfile::default();
    let src = "\
j1 = AutoJalousie(\"Jalousie 1\")
j2 = AutoJalousie(\"Jalousie 2\")
alle = CentralShade(\"Alle\")
alle.members = (j2, j1)
";
    let out = build(&base(), src, &mut lock);
    let names: Vec<String> = attrs(&out, "Alle").into_iter().map(|a| a.0).collect();
    let at = |n: &str| names.iter().position(|x| x == n);
    assert!(at("rec").is_some(), "{names:?}");
    if let Some(sp) = at("SpStates") {
        assert!(at("rec") < Some(sp), "{names:?}");
    }
}

#[test]
fn decompile_lifts_the_members() {
    let mut lock = Lockfile::default();
    let out = build(
        &base(),
        &format!("{LIGHTS}zentral.members = (l2, l1)\n"),
        &mut lock,
    );
    let (m, _) = decompile(
        &out,
        &DecompileOptions {
            scope: DecompileScope::Full,
            ..DecompileOptions::default()
        },
    )
    .unwrap();
    let text = m.to_text();
    assert!(
        text.contains("zentral.members = (licht_2, licht_1)"),
        "{text}"
    );
}

#[test]
fn wrong_members_are_refused() {
    let refused = |claim: &str, want: &str| {
        let mut lock = Lockfile::default();
        let src = format!("{LIGHTS}j = AutoJalousie(\"J\")\n{claim}\n");
        let e = try_build(&base(), &src, &mut lock).unwrap_err();
        assert!(e.contains(want), "{claim}: {e}");
    };
    refused(
        "zentral.members = (l1, j)",
        "the members of a CentralLight are `LightController2` blocks, but `j` is a `AutoJalousie`",
    );
    refused(
        "l1.members = (l2)",
        "members belong to a `CentralLight` or a `CentralShade`",
    );
    refused("zentral.members = (l1, nix)", "undeclared slug `nix`");
    refused("zentral.members = (l1, l1)", "`l1` is listed twice");
    refused(
        "zentral.members = (l1)\nzentral.members = (l2)",
        "duplicate assignment `zentral.members = …`",
    );
    refused(
        "zentral.members <- l1.Q",
        "takes a list of blocks (`zentral.members = (a, b)`)",
    );
    refused(
        "zentral.members = l1",
        "expected `zentral.members = (<slug>, <slug>, …)`",
    );
    // The attribute itself is no parameter.
    let mut lock = Lockfile::default();
    let e = try_build(&base(), "z = CentralLight(\"Z\", rec: \"x\")\n", &mut lock).unwrap_err();
    assert!(e.contains("`<slug>.members = (<slug>, …)`"), "{e}");
}

#[test]
fn the_claim_prints_as_written() {
    let src = format!("{LIGHTS}z2 = CentralLight(\"Z2\")\n");
    let claims = "zentral.members = (l1, l2) # all lights\nz2.members = ()\n";
    let text = Module::parse(&format!("{src}{claims}")).unwrap().to_text();
    assert!(text.ends_with(claims), "{text}");
}
