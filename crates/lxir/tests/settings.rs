//! Settings of physical inputs and outputs from source (D65): claiming
//! them on an extern, the add/rewrite rules, the order an added one takes,
//! restoring the original when the claim leaves the source, and the
//! semantic diff seeing them.

use lxir::ir::{CompileOptions, Module, compile};
use lxir::uuid::parse_serial;
use lxir::{Lockfile, LoxoneDoc};

const AIR: &str = "00000010-0000-0000-ffff000000000001";
const DIN: &str = "00000020-0000-0000-ffff000000000001";

/// An Air temperature sensor and an extension's digital input, each
/// written the way a save writes them, and one logic page.
fn base() -> LoxoneDoc {
    LoxoneDoc::parse(
        "<ControlList Version=\"1\" NextObj=\"50\">\r\n\
         \t<C Type=\"Document\" U=\"00000001-0000-0000-ffff000000000001\" ConfigVersion=\"17020828\">\r\n\
         \t\t<C Type=\"LoxAIRDevice\" U=\"00000002-0000-0000-ffff000000000001\" Title=\"Fühler\">\r\n\
         \t\t\t<C Type=\"LoxAIRAsensor\" IName=\"AI2\" V=\"178\" U=\"00000010-0000-0000-ffff000000000001\" Title=\"Garten\" Cl=\"238,238,238\" Nio=\"2\" WF=\"16384\" Analog=\"true\" SourceValHigh=\"100\" DestValHigh=\"100\" MinVal=\"-40\" MaxVal=\"125\" MinChange=\"0.25\" MinTime=\"1000\" Idx=\"2\">\r\n\
         \t\t\t\t<Co K=\"AQ\" U=\"00000011-0000-0000-ffff000000000001\"/>\r\n\
         \t\t\t\t<Co K=\"Q\" U=\"00000012-0000-0000-ffff000000000001\"/>\r\n\
         \t\t\t\t<Display Type=\"2\" Unit=\"&lt;v.1&gt;°\" StateOnly=\"true\"/>\r\n\
         \t\t\t</C>\r\n\
         \t\t</C>\r\n\
         \t\t<C Type=\"DigitalIn\" IName=\"I1\" V=\"178\" U=\"00000020-0000-0000-ffff000000000001\" Title=\"Eingang 1\" Nio=\"1\" WF=\"16384\" MinTime=\"10\">\r\n\
         \t\t\t<Co K=\"Q\" U=\"00000021-0000-0000-ffff000000000001\"/>\r\n\
         \t\t\t<Display Unit=\"&lt;v&gt;\" StateOnly=\"true\"/>\r\n\
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

const EXTERNS: &str = "extern garten = LoxAIRAsensor(uuid: \"00000010-0000-0000-ffff000000000001\")\n\
     extern taster = DigitalIn(uuid: \"00000020-0000-0000-ffff000000000001\")\n";

const CLAIMS: &str = "garten.Desc = \"Bodenfühler im Beet\"\n\
     garten.Unit = \"<v.1> °C\"\n\
     garten.MinTime = 600\n\
     taster.Desc = \"Klingel\"\n";

fn element<'a>(doc: &'a LoxoneDoc, uuid: &str) -> &'a lxir::xml::Element {
    let o = doc.objects().into_iter().find(|o| o.uuid == uuid).unwrap();
    doc.element_at(&o.path).unwrap()
}

fn attr_names(el: &lxir::xml::Element) -> Vec<&str> {
    el.attrs.iter().map(|a| a.name.as_str()).collect()
}

fn display_unit(doc: &LoxoneDoc, uuid: &str) -> Option<String> {
    element(doc, uuid)
        .child_elements()
        .find(|e| e.name == "Display")?
        .attr_decoded("Unit")
        .map(|v| v.into_owned())
}

#[test]
fn settings_are_claimed_in_place_and_converge() {
    let module = Module::parse(&format!("{EXTERNS}{CLAIMS}")).unwrap();
    let mut lock = Lockfile::default();
    let out = compile(&base(), &module, &mut lock, &opts()).unwrap();

    let air = element(&out, AIR);
    // An added description goes right after `Type=`; the rest are
    // rewritten where they stand.
    assert_eq!(&attr_names(air)[..3], ["Type", "Desc", "IName"]);
    assert_eq!(
        air.attr_decoded("Desc").as_deref(),
        Some("Bodenfühler im Beet")
    );
    assert_eq!(air.attr("MinTime"), Some("600"));
    assert_eq!(
        attr_names(element(&base(), AIR)).len() + 1,
        attr_names(air).len()
    );
    assert_eq!(display_unit(&out, AIR).as_deref(), Some("<v.1> °C"));
    assert_eq!(element(&out, DIN).attr("Desc"), Some("Klingel"));

    // The originals are in the lock, keyed by object and setting.
    assert_eq!(lock.set_originals[&format!("{AIR}#Desc")], None);
    assert_eq!(
        lock.set_originals[&format!("{AIR}#MinTime")].as_deref(),
        Some("1000")
    );
    assert_eq!(
        lock.set_originals[&format!("{AIR}#Unit")].as_deref(),
        Some("<v.1>°")
    );

    // A rebuild of its own output is byte-identical.
    let mut lock2 = lock.clone();
    let again = compile(&out, &module, &mut lock2, &opts()).unwrap();
    assert!(again.to_bytes() == out.to_bytes());
    assert_eq!(lock2.set_originals, lock.set_originals);

    // The semantic diff and the fingerprint see them.
    let d = lxir::diff::diff(&base(), &out);
    let mut changed: Vec<(&str, &str)> = d
        .param_changes
        .iter()
        .map(|c| (c.object_uuid.as_str(), c.port_key.as_str()))
        .collect();
    changed.sort();
    assert_eq!(
        changed,
        [
            (AIR, "Desc"),
            (AIR, "MinTime"),
            (AIR, "Unit"),
            (DIN, "Desc")
        ]
    );
    assert_ne!(
        lxir::diff::semantic_fingerprint(&base()),
        lxir::diff::semantic_fingerprint(&out)
    );
    // The fingerprint an older lxir recorded is blind to them.
    assert_eq!(
        lxir::diff::semantic_fingerprint_v7(&base()),
        lxir::diff::semantic_fingerprint_v7(&out)
    );
}

#[test]
fn a_setting_the_object_lacks_is_added_only_where_a_save_omits_it() {
    // The low end of the correction is omitted at its default: added in
    // Loxone's order, before the high end.
    let module = Module::parse(&format!("{EXTERNS}garten.DestValLow = 5\n")).unwrap();
    let out = compile(&base(), &module, &mut Lockfile::default(), &opts()).unwrap();
    let names = attr_names(element(&out, AIR));
    let at = |n: &str| names.iter().position(|a| *a == n).unwrap();
    assert_eq!(at("DestValLow") + 1, at("SourceValHigh"));

    // A digital input has no analog correction to rewrite.
    let module = Module::parse(&format!("{EXTERNS}taster.SourceValHigh = 5\n")).unwrap();
    let err = compile(&base(), &module, &mut Lockfile::default(), &opts())
        .unwrap_err()
        .to_string();
    assert!(err.contains("never adds one"), "{err}");

    // A block on a page is not a physical input or output.
    let module = Module::parse(
        "extern online = LoxAIRDevice(uuid: \"00000002-0000-0000-ffff000000000001\")\n\
         online.MinTime = 5\n",
    )
    .unwrap();
    let err = compile(&base(), &module, &mut Lockfile::default(), &opts())
        .unwrap_err()
        .to_string();
    assert!(err.contains("not a physical input or output"), "{err}");
}

#[test]
fn a_withdrawn_setting_is_restored_and_stays_restored() {
    let mut lock = Lockfile::default();
    let claimed = Module::parse(&format!("{EXTERNS}{CLAIMS}")).unwrap();
    let deployed = compile(&base(), &claimed, &mut lock, &opts()).unwrap();

    let module = Module::parse(EXTERNS).unwrap();
    let out = compile(&deployed, &module, &mut lock, &opts()).unwrap();
    // What the base compiles to without the claims: the added description
    // is gone, unit and time are the originals again.
    let plain = compile(&base(), &module, &mut Lockfile::default(), &opts()).unwrap();
    assert!(out.to_bytes() == plain.to_bytes());
    assert!(lock.set_originals.is_empty());
    assert_eq!(lock.removed_sets.len(), 4);

    // Until it is deployed, every compile against the old base restores
    // again; against the deployed result the tombstones retire.
    let mut again = lock.clone();
    let out2 = compile(&deployed, &module, &mut again, &opts()).unwrap();
    assert!(out2.to_bytes() == out.to_bytes());
    assert_eq!(again.removed_sets, lock.removed_sets);
    let out3 = compile(&out, &module, &mut again, &opts()).unwrap();
    assert!(out3.to_bytes() == out.to_bytes());
    assert!(again.removed_sets.is_empty());
}
