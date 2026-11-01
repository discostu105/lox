//! A PulseAt's fire time from source (#248): `At: 19:00` claims `Sec=`
//! (seconds since midnight) — canonical as `HH:MM[:SS]`, carried verbatim
//! while unclaimed, written where the base has it while claimed, refused
//! on a block that fires on a time function, lifted by decompile, and
//! seen by the diff and the fingerprint as `At`.

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

fn module(src: &str) -> Module {
    Module::parse(src).unwrap()
}

/// The attributes of the block titled `Abends`, in element order.
fn attrs(doc: &LoxoneDoc) -> Vec<(String, String)> {
    let o = doc
        .objects()
        .into_iter()
        .find(|o| o.title.as_deref() == Some("Abends"))
        .unwrap();
    doc.element_at(&o.path)
        .unwrap()
        .attrs
        .iter()
        .map(|a| (a.name.clone(), a.value.clone()))
        .collect()
}

fn attr(doc: &LoxoneDoc, name: &str) -> Option<String> {
    attrs(doc)
        .into_iter()
        .find(|(n, _)| n == name)
        .map(|(_, v)| v)
}

/// `doc` with the block's attribute set as the GUI would have it.
fn with_attr(doc: &LoxoneDoc, name: &str, value: &str) -> LoxoneDoc {
    let mut doc = doc.clone();
    let o = doc
        .objects()
        .into_iter()
        .find(|o| o.title.as_deref() == Some("Abends"))
        .unwrap();
    doc.element_at_mut(&o.path)
        .unwrap()
        .set_attr_raw(name, value);
    doc
}

const CLAIMED: &str = "abends = PulseAt(\"Abends\", At: 19:00, Time: 1s)\n";
const UNCLAIMED: &str = "abends = PulseAt(\"Abends\", Time: 1s)\n";

#[test]
fn a_time_of_day_has_one_spelling() {
    for (written, canonical) in [
        ("6:58", "06:58"),
        ("19:00:00", "19:00"),
        ("07:05:30", "07:05:30"),
        ("0:00", "00:00"),
        ("23:59:59", "23:59:59"),
    ] {
        let m = module(&format!("p = PulseAt(At: {written})\n"));
        assert_eq!(m.to_text(), format!("p = PulseAt(At: {canonical})\n"));
    }
    for bad in ["24:00", "7:5", "12:60", "12:00:60", "123:00"] {
        let err = Module::parse(&format!("p = PulseAt(At: {bad})\n")).unwrap_err();
        assert!(
            err.to_string().contains("not a time of day"),
            "{bad}: {err}"
        );
    }
    // A constant holds one too.
    let m = module("let abends = 19:00\np = PulseAt(\"Abends\", At: abends)\n");
    let out = compile(&base(), &m, &mut Lockfile::new(), &opts()).unwrap();
    assert_eq!(attr(&out, "Sec").as_deref(), Some("68400"));
}

#[test]
fn at_takes_a_time_of_day_and_nothing_else_takes_one() {
    for (src, want) in [
        ("p = PulseAt(At: 68400)\n", "takes a time of day"),
        ("p = PulseAt(At: \"19:00\")\n", "takes a time of day"),
        ("let n = 5\np = PulseAt(At: n)\n", "takes a time of day"),
        ("m = Monoflop(On: 19:00)\n", "only the fire time"),
        ("let t = 19:00\nm = Monoflop(On: t)\n", "only the fire time"),
        ("a = And()\np = PulseAt(At: a.Q)\n", "never a wire"),
    ] {
        let err = compile(&base(), &module(src), &mut Lockfile::new(), &opts()).unwrap_err();
        assert!(err.to_string().contains(want), "{src}: {err}");
    }
}

#[test]
fn a_minted_block_fires_at_its_time() {
    let out = compile(&base(), &module(CLAIMED), &mut Lockfile::new(), &opts()).unwrap();
    assert_eq!(attr(&out, "Sec").as_deref(), Some("68400"));
    // `Typ=` is the save's to add; the source writes only the time.
    assert_eq!(attr(&out, "Typ"), None);
    let bare = compile(&base(), &module(UNCLAIMED), &mut Lockfile::new(), &opts()).unwrap();
    assert_eq!(attr(&bare, "Sec"), None);
}

#[test]
fn unclaimed_time_is_carried_and_a_claim_rewrites_it_in_place() {
    let mut lock = Lockfile::new();
    let first = compile(&base(), &module(UNCLAIMED), &mut lock, &opts()).unwrap();
    // The GUI sets the time: `Sec=` before `Typ=`, as a save writes them.
    let gui = with_attr(&with_attr(&first, "Sec", "68280"), "Typ", "0");
    let order = |doc: &LoxoneDoc| -> Vec<String> { attrs(doc).into_iter().map(|a| a.0).collect() };

    let carried = compile(&gui, &module(UNCLAIMED), &mut lock, &opts()).unwrap();
    assert_eq!(attr(&carried, "Sec").as_deref(), Some("68280"));
    assert_eq!(order(&carried), order(&gui));

    let claimed = compile(&gui, &module(CLAIMED), &mut lock, &opts()).unwrap();
    assert_eq!(attr(&claimed, "Sec").as_deref(), Some("68400"));
    assert_eq!(attr(&claimed, "Typ").as_deref(), Some("0"));
    assert_eq!(
        order(&claimed),
        order(&gui),
        "the claim keeps the base's order"
    );
}

#[test]
fn a_time_function_refuses_a_fixed_time() {
    let mut lock = Lockfile::new();
    let first = compile(&base(), &module(UNCLAIMED), &mut lock, &opts()).unwrap();
    let sunrise = with_attr(&with_attr(&first, "Sec", "0"), "Typ", "2");
    let err = compile(&sunrise, &module(CLAIMED), &mut lock, &opts()).unwrap_err();
    assert!(err.to_string().contains("time function"), "{err}");
    // Unclaimed, the GUI's timing is carried as it is.
    let carried = compile(&sunrise, &module(UNCLAIMED), &mut lock, &opts()).unwrap();
    assert_eq!(attr(&carried, "Typ").as_deref(), Some("2"));

    // Decompile lifts only a fixed time.
    let (m, _) = decompile(&sunrise, &DecompileOptions::default()).unwrap();
    assert!(!m.to_text().contains("At:"), "{}", m.to_text());
}

#[test]
fn decompile_lifts_the_fire_time() {
    let out = compile(&base(), &module(CLAIMED), &mut Lockfile::new(), &opts()).unwrap();
    let out = with_attr(&out, "Sec", "25110");
    for scope in [DecompileScope::Full, DecompileScope::ManagedOnly] {
        let (m, _) = decompile(
            &out,
            &DecompileOptions {
                scope,
                ..Default::default()
            },
        )
        .unwrap();
        let text = m.to_text();
        assert!(text.contains("At: 06:58:30,"), "{text}");
    }
}

#[test]
fn the_diff_and_the_fingerprint_see_the_time() {
    let evening = compile(&base(), &module(CLAIMED), &mut Lockfile::new(), &opts()).unwrap();
    let morning = compile(
        &base(),
        &module("abends = PulseAt(\"Abends\", At: 06:58, Time: 1s)\n"),
        &mut Lockfile::new(),
        &opts(),
    )
    .unwrap();
    let d = loxone_config::diff::diff(&evening, &morning);
    assert_eq!(d.param_changes.len(), 1, "{:?}", d.param_changes);
    let c = &d.param_changes[0];
    assert_eq!(
        (c.port_key.as_str(), c.from.as_deref(), c.to.as_deref()),
        ("At", Some("19:00"), Some("06:58"))
    );

    use loxone_config::diff::{semantic_fingerprint, semantic_fingerprint_v9};
    assert_ne!(
        semantic_fingerprint(&evening),
        semantic_fingerprint(&morning)
    );
    // An older lock's fingerprint is blind to the time, so an upgrade
    // alone never reads as drift.
    assert_eq!(
        semantic_fingerprint_v9(&evening),
        semantic_fingerprint_v9(&morning)
    );
}
