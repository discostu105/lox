//! The project's and the Miniserver's settings from source (D135): the
//! location, the automatic updates, the security switches and where the
//! Miniserver is mounted, each restored when its claim leaves the source.

use loxone_config::logic::{CompileOptions, Module, adopt_one, compile};
use loxone_config::uuid::parse_serial;
use loxone_config::{Lockfile, LoxoneDoc};

const DOC: &str = "00000001-0000-0000-ffff000000000001";
const MS: &str = "00000002-0000-0000-ffff000000000001";

/// A project and its Miniserver as Loxone Config 17.2 saves them: no
/// `AUM=` (automatic updates), `CrashL="true"`, no elevation.
fn base() -> LoxoneDoc {
    LoxoneDoc::parse(
        "<ControlList Version=\"1\" NextObj=\"50\">\r\n\
         \t<C Type=\"Document\" V=\"17020828\" U=\"00000001-0000-0000-ffff000000000001\" Title=\"Haus\" ConfigVersion=\"17020828\" Street=\"\" Town=\"\" PostCode=\"\" Ctry=\"AT\" Latitude=\"48.3\" Longitude=\"14.4\" Timezone=\"Europe/Vienna\" Timeserver=\"\" UnitPrecipitation=\"1\" Currency=\"€\" UpdTime=\"120\" AUE=\"true\" DeviceTelemetry=\"true\" CrashL=\"true\" Geo=\"3\" NumO=\"10\">\r\n\
         \t\t<C Type=\"LoxLIVE\" V=\"178\" U=\"00000002-0000-0000-ffff000000000001\" Title=\"Miniserver\" Serial=\"504F94112233\" Installation=\"\" SwitchBoard=\"Schaltschrank\" SwitchBoardRow=\"1\" SwitchBoardPos=\"1\">\r\n\
         \t\t</C>\r\n\
         \t\t<C Type=\"Page\" U=\"00000005-0000-0000-ffff000000000001\" Title=\"Regeln\">\r\n\
         \t\t</C>\r\n\
         \t</C>\r\n\
         </ControlList>\r\n"
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

const EXTERNS: &str = "extern haus = Document(uuid: \"00000001-0000-0000-ffff000000000001\")\n\
     extern ms = LoxLIVE(uuid: \"00000002-0000-0000-ffff000000000001\")\n";

fn element<'a>(doc: &'a LoxoneDoc, uuid: &str) -> &'a loxone_config::xml::Element {
    let o = doc.objects().into_iter().find(|o| o.uuid == uuid).unwrap();
    doc.element_at(&o.path).unwrap()
}

fn attr_names(el: &loxone_config::xml::Element) -> Vec<&str> {
    el.attrs.iter().map(|a| a.name.as_str()).collect()
}

fn run(claims: &str) -> loxone_config::Result<LoxoneDoc> {
    let module = Module::parse(&format!("{EXTERNS}{claims}")).unwrap();
    compile(&base(), &module, &mut Lockfile::default(), &opts())
}

#[test]
fn settings_are_written_where_a_save_writes_them() {
    let claims = "haus.Altitude = 400\n\
         haus.Timeserver = \"pool.ntp.org\"\n\
         haus.Updates = \"confirm\"\n\
         haus.UpdateTime = 03:30\n\
         haus.ExtendedLogin = on\n\
         haus.AppFirewall = off\n\
         haus.PasswordReset = off\n\
         haus.LoxoneLog = off\n\
         ms.Installation = \"Technikraum\"\n";
    let module = Module::parse(&format!("{EXTERNS}{claims}")).unwrap();
    let mut lock = Lockfile::default();
    let out = compile(&base(), &module, &mut lock, &opts()).unwrap();

    let doc = element(&out, DOC);
    // The order of a Loxone Config 17.3 save (oracle session 94).
    let names: Vec<&str> = attr_names(doc)
        .into_iter()
        .filter(|n| {
            [
                "Longitude",
                "SL",
                "Timezone",
                "Currency",
                "UpdTime",
                "ExtVal",
                "AUE",
                "AUM",
                "DeviceTelemetry",
                "CrashL",
                "FwMode",
                "NoPwReset",
                "Geo",
            ]
            .contains(n)
        })
        .collect();
    assert_eq!(
        names,
        [
            "Longitude",
            "SL",
            "Timezone",
            "Currency",
            "UpdTime",
            "ExtVal",
            "AUE",
            "AUM",
            "DeviceTelemetry",
            "FwMode",
            "NoPwReset",
            "Geo"
        ]
    );
    assert_eq!(doc.attr("SL"), Some("400"));
    assert_eq!(doc.attr("Timeserver"), Some("pool.ntp.org"));
    assert_eq!(doc.attr("UpdTime"), Some("210"));
    assert_eq!(doc.attr("AUE"), Some("true"));
    assert_eq!(doc.attr("AUM"), Some("2"));
    assert_eq!(doc.attr("ExtVal"), Some("true"));
    assert_eq!(doc.attr("FwMode"), Some("0"));
    assert_eq!(doc.attr("NoPwReset"), Some("true"));
    assert_eq!(doc.attr("CrashL"), None);
    assert_eq!(element(&out, MS).attr("Installation"), Some("Technikraum"));

    // The diff sees them; a rebuild is byte-identical.
    let d = loxone_config::diff::diff(&base(), &out);
    assert_eq!(d.param_changes.len(), 9, "{:?}", d.param_changes);
    let again = compile(&out, &module, &mut lock.clone(), &opts()).unwrap();
    assert!(again.to_bytes() == out.to_bytes());
    // An older lock (fingerprint v13) is blind to them.
    assert_eq!(
        loxone_config::diff::semantic_fingerprint_v13(&base()),
        loxone_config::diff::semantic_fingerprint_v13(&out)
    );
    assert_ne!(
        loxone_config::diff::semantic_fingerprint(&base()),
        loxone_config::diff::semantic_fingerprint(&out)
    );

    // Withdrawn, the project is what it was.
    let plain = Module::parse(EXTERNS).unwrap();
    let back = compile(&out, &plain, &mut lock, &opts()).unwrap();
    let fresh = compile(&base(), &plain, &mut Lockfile::default(), &opts()).unwrap();
    assert!(back.to_bytes() == fresh.to_bytes());
}

#[test]
fn never_updating_drops_the_switch() {
    let out = run("haus.Updates = \"never\"\n").unwrap();
    let doc = element(&out, DOC);
    assert_eq!(doc.attr("AUE"), None);
    assert_eq!(doc.attr("AUM"), Some("0"));
    // `auto` on a 17.2 save's automatic leaves it as it is.
    let out = run("haus.Updates = \"auto\"\n").unwrap();
    assert!(out.to_bytes() == run("").unwrap().to_bytes());
    // From another mode, `auto` writes what a 17.3 save writes for it.
    let never = run("haus.Updates = \"never\"\n").unwrap();
    let module = Module::parse(&format!("{EXTERNS}haus.Updates = \"auto\"\n")).unwrap();
    let out = compile(&never, &module, &mut Lockfile::default(), &opts()).unwrap();
    assert_eq!(element(&out, DOC).attr("AUE"), Some("true"));
    assert_eq!(element(&out, DOC).attr("AUM"), Some("1"));
}

#[test]
fn wrong_values_are_refused() {
    let err = |claims: &str| run(claims).unwrap_err().to_string();
    assert!(err("haus.Updates = \"sometimes\"\n").contains("\"auto\", \"confirm\" or \"never\""));
    assert!(err("haus.UpdateTime = 03:30:15\n").contains("whole minutes"));
    assert!(err("haus.UpdateTime = 120\n").contains("time of day"));
    assert!(err("haus.AppFirewall = 1\n").contains("on or off"));
    // What has no pane, or no known meaning, names what the project takes.
    let e = err("haus.DeviceTelemetry = off\n");
    assert!(e.contains("LoxoneLog"), "{e}");
    let e = err("ms.IntAddr = \"10.0.0.2\"\n");
    assert!(e.contains("SwitchBoardPos"), "{e}");
    // A text setting is rewritten, never added.
    let e = err("haus.Town = \"Linz\"\nhaus.Tel = \"1\"\n");
    assert!(e.contains("Timeserver"), "{e}");
}

/// Adopt the project and the Miniserver of `doc` into an empty module:
/// the claims as source text and the lock.
fn adopt_both(doc: &LoxoneDoc) -> (String, Lockfile) {
    let mut lock = Lockfile::default();
    let mut module = Module::parse("").unwrap();
    for (uuid, slug) in [(DOC, "haus"), (MS, "ms")] {
        let adopted = adopt_one(doc, uuid, slug, &module, &mut lock).unwrap();
        module.items.extend(adopted.items);
    }
    (module.to_text(), lock)
}

#[test]
fn adopting_lifts_every_setting_as_it_stands() {
    // A 17.2 save: `AUE="true"` alone reads as automatic, and the
    // settings it has not written (`SL=`) stay unclaimed.
    let (text, lock) = adopt_both(&base());
    for line in [
        "haus.Country = \"AT\"",
        "haus.Latitude = 48.3",
        "haus.Updates = \"auto\"",
        "haus.UpdateTime = 02:00",
        "haus.AppFirewall = on",
        "haus.PasswordReset = on",
        "haus.ExtendedLogin = off",
        "haus.LoxoneLog = on",
        "ms.SwitchBoard = \"Schaltschrank\"",
        "ms.SwitchBoardRow = 1",
    ] {
        assert!(text.contains(line), "{line} missing in:\n{text}");
    }
    assert!(!text.contains("Altitude"), "{text}");
    // Compiling the adopted pair changes neither the config nor the lock.
    let module = Module::parse(&text).unwrap();
    let mut after = lock.clone();
    let out = compile(&base(), &module, &mut after, &opts()).unwrap();
    for uuid in [DOC, MS] {
        assert_eq!(element(&out, uuid).attrs, element(&base(), uuid).attrs);
    }
    assert_eq!(after.set_originals, lock.set_originals);

    // Every other value round-trips: compile claims, adopt the result,
    // and the adopted pair rebuilds it byte for byte.
    for claims in [
        "haus.Altitude = 400\nhaus.Updates = \"confirm\"\nhaus.UpdateTime = 03:30\n\
         haus.ExtendedLogin = on\nhaus.AppFirewall = off\nhaus.PasswordReset = off\n\
         haus.LoxoneLog = off\nms.Installation = \"Technikraum\"\n",
        "haus.Updates = \"never\"\n",
        "haus.Updates = \"auto\"\nhaus.UpdateTime = 23:59\n",
    ] {
        let saved = run(claims).unwrap();
        let (text, lock) = adopt_both(&saved);
        for line in claims.lines() {
            assert!(text.contains(line), "{line} missing in:\n{text}");
        }
        let mut after = lock.clone();
        let out = compile(&saved, &Module::parse(&text).unwrap(), &mut after, &opts()).unwrap();
        assert_eq!(out.to_bytes(), saved.to_bytes(), "{claims}");
        assert_eq!(after.set_originals, lock.set_originals, "{claims}");
    }
}

#[test]
fn adopting_twice_has_nothing_left() {
    let mut lock = Lockfile::default();
    let module = Module::parse("").unwrap();
    let first = adopt_one(&base(), DOC, "haus", &module, &mut lock).unwrap();
    let module = Module { items: first.items };
    let err = adopt_one(&base(), DOC, "haus", &module, &mut lock).unwrap_err();
    assert!(err.to_string().contains("nothing to adopt"), "{err}");
    let err = adopt_one(&base(), DOC, "projekt", &module, &mut lock).unwrap_err();
    assert!(err.to_string().contains("--as haus"), "{err}");
}
