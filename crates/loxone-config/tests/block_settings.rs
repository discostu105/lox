//! Block settings from source: the app name (`Desc:`), a memory flag's
//! type, a PulseAt's time function, an automatic blind's sunshine flag,
//! a DayTimer's on/off texts and a meter's display fields. Each is
//! carried from the base until the source binds it, written where a
//! save writes it, lifted by decompile and seen by the diff.

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

fn build(doc: &LoxoneDoc, src: &str, lock: &mut Lockfile) -> LoxoneDoc {
    compile(doc, &module(src), lock, &opts()).unwrap()
}

/// The attributes of the block titled `title`, in element order.
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

fn attr(doc: &LoxoneDoc, title: &str, name: &str) -> Option<String> {
    attrs(doc, title)
        .into_iter()
        .find(|(n, _)| n == name)
        .map(|(_, v)| v)
}

fn names(doc: &LoxoneDoc, title: &str) -> Vec<String> {
    attrs(doc, title).into_iter().map(|a| a.0).collect()
}

/// `doc` with the block's attribute set as a save would have it.
fn with_attr(doc: &LoxoneDoc, title: &str, name: &str, value: Option<&str>) -> LoxoneDoc {
    let mut doc = doc.clone();
    let o = doc
        .objects()
        .into_iter()
        .find(|o| o.title.as_deref() == Some(title))
        .unwrap();
    let el = doc.element_at_mut(&o.path).unwrap();
    match value {
        Some(v) => el.set_attr_raw(name, v),
        None => {
            el.remove_attr(name);
        }
    }
    doc
}

fn err(src: &str) -> String {
    compile(&base(), &module(src), &mut Lockfile::new(), &opts())
        .unwrap_err()
        .to_string()
}

#[test]
fn the_app_name_sits_right_after_the_type() {
    let src = "m = Memory(\"Merker\", Desc: \"Haustür offen\")\n";
    let mut lock = Lockfile::new();
    let out = build(&base(), src, &mut lock);
    assert_eq!(
        attr(&out, "Merker", "Desc").as_deref(),
        Some("Haustür offen")
    );
    assert_eq!(names(&out, "Merker")[..2], ["Type", "Desc"]);

    // Unclaimed, the base's name is carried; claimed, it is rewritten in place.
    let bare = "m = Memory(\"Merker\")\n";
    let gui = with_attr(
        &build(&base(), bare, &mut lock),
        "Merker",
        "Desc",
        Some("Alt"),
    );
    let carried = build(&gui, bare, &mut lock);
    assert_eq!(attr(&carried, "Merker", "Desc").as_deref(), Some("Alt"));
    let claimed = build(&gui, src, &mut lock);
    assert_eq!(
        attr(&claimed, "Merker", "Desc").as_deref(),
        Some("Haustür offen")
    );
    assert_eq!(names(&claimed, "Merker"), names(&gui, "Merker"));

    assert!(err("m = Memory(Desc: \"\")\n").contains("empty"));
    assert!(err("m = Memory(Desc: on)\n").contains("takes text"));
}

#[test]
fn a_memory_flag_has_a_type() {
    let mut lock = Lockfile::new();
    let out = build(
        &base(),
        "m = Memory(\"Merker\", Type: \"text\")\n",
        &mut lock,
    );
    assert_eq!(attr(&out, "Merker", "Tp").as_deref(), Some("2"));
    let e = err("m = Memory(Type: \"string\")\n");
    assert!(e.contains("\"digital\", \"analog\" or \"text\""), "{e}");
}

#[test]
fn a_pulse_fires_on_a_time_function() {
    let mut lock = Lockfile::new();
    let out = build(
        &base(),
        "p = PulseAt(\"P\", Pulse: \"sunrise\")\n",
        &mut lock,
    );
    assert_eq!(attr(&out, "P", "Typ").as_deref(), Some("11"));

    let e = err("p = PulseAt(At: 19:00, Pulse: \"hour\")\n");
    assert!(e.contains("time function"), "{e}");

    // A block Loxone Config set to dusk takes a fixed time with `Pulse: "time"`.
    let dusk = with_attr(&out, "P", "Typ", Some("8"));
    let fixed = build(
        &dusk,
        "p = PulseAt(\"P\", At: 19:00, Pulse: \"time\")\n",
        &mut lock,
    );
    assert_eq!(attr(&fixed, "P", "Typ").as_deref(), Some("0"));
    assert_eq!(attr(&fixed, "P", "Sec").as_deref(), Some("68400"));
}

#[test]
fn sunshine_off_is_left_out() {
    let mut lock = Lockfile::new();
    let on = build(&base(), "j = AutoJalousie(\"J\", Sun: on)\n", &mut lock);
    assert_eq!(attr(&on, "J", "Sun").as_deref(), Some("true"));
    let off = build(&on, "j = AutoJalousie(\"J\", Sun: off)\n", &mut lock);
    assert_eq!(attr(&off, "J", "Sun"), None);
    // Unclaimed, the base's flag stays.
    let carried = build(&on, "j = AutoJalousie(\"J\")\n", &mut lock);
    assert_eq!(attr(&carried, "J", "Sun").as_deref(), Some("true"));
}

#[test]
fn texts_and_units_are_written_as_given() {
    let mut lock = Lockfile::new();
    let out = build(
        &base(),
        "t = DayTimer(\"T\", On: \"Tag\", Off: \"\")\n\
         z = MeterAbsSt(\"Z\", PwrN: \"Leistung\", Un1: \"kW\", Un2: \"kWh\", UnSt: \"kWh\", MaxLvl: 13.5)\n",
        &mut lock,
    );
    assert_eq!(attr(&out, "T", "On").as_deref(), Some("Tag"));
    assert_eq!(attr(&out, "T", "Off").as_deref(), Some(""));
    for (n, v) in [
        ("PwrN", "Leistung"),
        ("Un1", "kW"),
        ("Un2", "kWh"),
        ("UnSt", "kWh"),
        ("MaxLvl", "13.5"),
    ] {
        assert_eq!(attr(&out, "Z", n).as_deref(), Some(v), "{n}");
    }
    assert!(err("z = MeterDig(Un1: \"\")\n").contains("empty"));
}

const ALL: &str = "m = Memory(\"M\", Desc: \"Merker\", Type: \"analog\")\n\
                   p = PulseAt(\"P\", Pulse: \"dusk\")\n\
                   j = AutoJalousie(\"J\", Sun: on)\n\
                   t = DayTimer(\"T\", On: \"Tag\", Off: \"Nacht\")\n\
                   z = MeterDig(\"Z\", PwrN: \"Leistung\", Un1: \"kW\", Un2: \"kWh\")\n";

#[test]
fn decompile_lifts_the_settings() {
    let mut lock = Lockfile::new();
    let out = build(&base(), ALL, &mut lock);
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
        for want in [
            "Desc: \"Merker\"",
            "Type: \"analog\"",
            "Pulse: \"dusk\"",
            "Sun: on",
            "On: \"Tag\"",
            "Off: \"Nacht\"",
            "PwrN: \"Leistung\"",
            "Un2: \"kWh\"",
        ] {
            assert!(text.contains(want), "{want}: {text}");
        }
    }
    // Defaults a save writes are not lifted.
    let plain = build(
        &base(),
        "m = Memory(\"M\")\np = PulseAt(\"P\")\n",
        &mut Lockfile::new(),
    );
    let plain = with_attr(
        &with_attr(&plain, "M", "Tp", Some("0")),
        "P",
        "Typ",
        Some("0"),
    );
    let (m, _) = decompile(&plain, &DecompileOptions::default()).unwrap();
    let text = m.to_text();
    assert!(
        !text.contains("Type:") && !text.contains("Pulse:"),
        "{text}"
    );
}

#[test]
fn the_diff_sees_the_settings() {
    let mut lock = Lockfile::new();
    let a = build(&base(), ALL, &mut lock);
    let b = build(&a, &ALL.replace("\"Merker\"", "\"Flag\""), &mut lock);
    let d = loxone_config::diff::diff(&a, &b);
    assert!(
        d.param_changes
            .iter()
            .any(|c| c.port_key == "Desc" && c.to.as_deref() == Some("Flag")),
        "{:?}",
        d.param_changes
    );
    use loxone_config::diff::{semantic_fingerprint, semantic_fingerprint_v10};
    assert_ne!(semantic_fingerprint(&a), semantic_fingerprint(&b));
    // An older lock's fingerprint is blind to them, so an upgrade alone
    // never reads as drift.
    assert_eq!(semantic_fingerprint_v10(&a), semantic_fingerprint_v10(&b));
}

#[test]
fn settings_only_loxone_config_knows_are_refused_by_name() {
    for (src, want) in [
        ("s = SmokeAlarm(Objects: \"x\")\n", "member list"),
        ("c = CentralShade(rec: \"x\")\n", "member list"),
        ("v = VirtualIn(MinTime: 1)\n", "no field"),
        ("p = Code16(Task: 1)\n", "every save"),
    ] {
        let e = err(src);
        assert!(
            e.contains("cannot be set from source") && e.contains(want),
            "{src}: {e}"
        );
    }
}
