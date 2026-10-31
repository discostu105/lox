//! Settings of physical inputs and outputs from source (D65): claiming
//! them on an extern, the add/rewrite rules, the order an added one takes,
//! restoring the original when the claim leaves the source, and the
//! semantic diff seeing them.

use loxone_config::logic::{CompileOptions, Module, compile};
use loxone_config::uuid::parse_serial;
use loxone_config::{Lockfile, LoxoneDoc};

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

fn element<'a>(doc: &'a LoxoneDoc, uuid: &str) -> &'a loxone_config::xml::Element {
    let o = doc.objects().into_iter().find(|o| o.uuid == uuid).unwrap();
    doc.element_at(&o.path).unwrap()
}

fn attr_names(el: &loxone_config::xml::Element) -> Vec<&str> {
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
    let d = loxone_config::diff::diff(&base(), &out);
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
        loxone_config::diff::semantic_fingerprint(&base()),
        loxone_config::diff::semantic_fingerprint(&out)
    );
    // The fingerprint an older version of Lox Logic recorded is blind to them.
    assert_eq!(
        loxone_config::diff::semantic_fingerprint_v7(&base()),
        loxone_config::diff::semantic_fingerprint_v7(&out)
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

    // A device has settings of its own, not an input's.
    let module = Module::parse(
        "extern online = LoxAIRDevice(uuid: \"00000002-0000-0000-ffff000000000001\")\n\
         online.MinTime = 5\n",
    )
    .unwrap();
    let err = compile(&base(), &module, &mut Lockfile::default(), &opts())
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("setting of a LoxAIRDevice") && err.contains("Installation"),
        "{err}"
    );

    // A block on a page is not a physical input or output.
    let module = Module::parse(
        "extern seite = Page(uuid: \"00000005-0000-0000-ffff000000000001\")\n\
         seite.MinTime = 5\n",
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

#[test]
fn one_statement_sets_several_objects() {
    // `(a, b).Setting = value` is the statements one per object (#199).
    let bulk = Module::parse(&format!(
        "{EXTERNS}(garten, taster).Desc = \"Außen\" # beide\n\
         (garten).MinTime = 600\n"
    ))
    .unwrap();
    let single = Module::parse(&format!(
        "{EXTERNS}garten.Desc = \"Außen\"\ntaster.Desc = \"Außen\"\ngarten.MinTime = 600\n"
    ))
    .unwrap();
    let (mut l1, mut l2) = (Lockfile::default(), Lockfile::default());
    let a = compile(&base(), &bulk, &mut l1, &opts()).unwrap();
    let b = compile(&base(), &single, &mut l2, &opts()).unwrap();
    assert!(a.to_bytes() == b.to_bytes());
    assert_eq!(l1.set_originals, l2.set_originals);

    // fmt writes it back as the one line, comment included.
    let text = bulk.to_text();
    assert!(
        text.contains("(garten, taster).Desc = \"Außen\" # beide\n(garten).MinTime = 600\n"),
        "{text}"
    );
    assert_eq!(Module::parse(&text).unwrap().to_text(), text);

    // Attributes too; dropping an object keeps the rest on the line.
    let mut m = Module::parse(&format!("{EXTERNS}(garten, taster).room = \"Garten\"\n")).unwrap();
    assert!(m.to_text().contains("(garten, taster).room = \"Garten\"\n"));
    m.items
        .retain(|i| !matches!(i, loxone_config::logic::Item::Attr(a) if a.slug == "garten"));
    assert!(m.to_text().contains("(taster).room = \"Garten\"\n"));
}

#[test]
fn a_bulk_statement_is_checked() {
    for (src, msg) in [
        ("(garten, garten).Desc = \"x\"\n", "listed twice"),
        (
            "(garten taster).Desc = \"x\"\n",
            "expected `(<slug>, <slug>, …).Desc",
        ),
        (
            "(garten,).Desc = \"x\"\n",
            "expected `(<slug>, <slug>, …).Desc",
        ),
        ("().Desc = \"x\"\n", "expected `(<slug>, <slug>, …).Desc"),
        ("(garten, taster).moods = std\n", "takes one object"),
    ] {
        let err = Module::parse(&format!("{EXTERNS}{src}"))
            .unwrap_err()
            .to_string();
        assert!(err.contains(msg), "{src}: {err}");
    }
    // An object the module does not declare is an error, as in the
    // single form.
    let err = Module::parse(&format!("{EXTERNS}(garten, nirgends).Desc = \"x\"\n"))
        .unwrap_err()
        .to_string();
    assert!(err.contains("nirgends"), "{err}");
}

const AIRDEV: &str = "00000102-0000-0000-ffff000000000001";
const TREEDEV: &str = "00000103-0000-0000-ffff000000000001";

/// An Air and a Tree device as a save writes them: the Air device with
/// its place and board, the Tree device with its place only.
fn devices() -> LoxoneDoc {
    LoxoneDoc::parse(
        "<ControlList Version=\"1\" NextObj=\"50\">\r\n\
         \t<C Type=\"Document\" U=\"00000101-0000-0000-ffff000000000001\" ConfigVersion=\"17020828\">\r\n\
         \t\t<C Type=\"LoxAIRDevice\" V=\"178\" U=\"00000102-0000-0000-ffff000000000001\" Title=\"Fühler\" WF=\"16384\" IP=\"FE80000000000000524F94FFFEB9BA3D\" AirType=\"48\" SubType=\"48\" UseDimmer=\"true\" UseRGBDimmer=\"true\" PWMFrequency=\"123\" PG=\"\" Installation=\"\" SwitchBoard=\"\" SwitchBoardRow=\"1\" SwitchBoardPos=\"1\">\r\n\
         \t\t</C>\r\n\
         \t\t<C Type=\"TreeDevice\" V=\"178\" U=\"00000103-0000-0000-ffff000000000001\" Title=\"Taster\" WF=\"16384\" Serial=\"B03C7ED6\" Installation=\"\" SubType=\"32780\" TreeType=\"32780\" PG=\"\" BlinkPos=\"3\">\r\n\
         \t\t</C>\r\n\
         \t\t<C Type=\"Page\" U=\"00000105-0000-0000-ffff000000000001\" Title=\"Regeln\">\r\n\
         \t\t</C>\r\n\
         \t</C>\r\n\
         </ControlList>\r\n"
            .as_bytes(),
    )
    .unwrap()
}

const DEVICE_CLAIMS: &str = "extern fuehler = LoxAIRDevice(title: \"Fühler\", device: \"50:4F:94:FF:FE:B9:BA:3D\")\n\
     extern taster = TreeDevice(title: \"Taster\")\n\
     (fuehler, taster).Installation = \"Flur, neben der Tür\"\n\
     fuehler.SwitchBoard = \"Verteiler UG\"\n\
     fuehler.SwitchBoardPos = 4\n\
     taster.Desc = \"Tasterschnittstelle Flur\"\n";

#[test]
fn device_settings_are_claimed_and_restored() {
    let module = Module::parse(DEVICE_CLAIMS).unwrap();
    let mut lock = Lockfile::default();
    let out = compile(&devices(), &module, &mut lock, &opts()).unwrap();

    // The Air device's are rewritten where they stand.
    let air = element(&out, AIRDEV);
    assert_eq!(attr_names(air), attr_names(element(&devices(), AIRDEV)));
    assert_eq!(
        air.attr_decoded("Installation").as_deref(),
        Some("Flur, neben der Tür")
    );
    assert_eq!(air.attr("SwitchBoard"), Some("Verteiler UG"));
    assert_eq!(air.attr("SwitchBoardPos"), Some("4"));

    // The Tree device's description is added after `Type`.
    let tree = element(&out, TREEDEV);
    assert_eq!(&attr_names(tree)[..3], ["Type", "Desc", "V"]);
    assert_eq!(
        attr_names(tree).len(),
        attr_names(element(&devices(), TREEDEV)).len() + 1
    );

    // The diff sees them; a rebuild is byte-identical.
    let d = loxone_config::diff::diff(&devices(), &out);
    assert_eq!(d.param_changes.len(), 5);
    let again = compile(&out, &module, &mut lock.clone(), &opts()).unwrap();
    assert!(again.to_bytes() == out.to_bytes());
    // An older lock (fingerprint v8) is blind to them.
    assert_eq!(
        loxone_config::diff::semantic_fingerprint_v8(&devices()),
        loxone_config::diff::semantic_fingerprint_v8(&out)
    );
    assert_ne!(
        loxone_config::diff::semantic_fingerprint(&devices()),
        loxone_config::diff::semantic_fingerprint(&out)
    );

    // Withdrawn, the devices are what they were.
    let plain = Module::parse(
        "extern fuehler = LoxAIRDevice(title: \"Fühler\")\n\
         extern taster = TreeDevice(title: \"Taster\")\n",
    )
    .unwrap();
    let back = compile(&out, &plain, &mut lock, &opts()).unwrap();
    let fresh = compile(&devices(), &plain, &mut Lockfile::default(), &opts()).unwrap();
    assert!(back.to_bytes() == fresh.to_bytes());

    // What a device does not take is refused, naming what it does.
    let err = compile(
        &devices(),
        &Module::parse(
            "extern fuehler = LoxAIRDevice(title: \"Fühler\")\nfuehler.PWMFrequency = 200\n",
        )
        .unwrap(),
        &mut Lockfile::default(),
        &opts(),
    )
    .unwrap_err()
    .to_string();
    assert!(err.contains("SwitchBoardPos"), "{err}");

    // A Tree device outside a switch board has none to rewrite: a save
    // drops one added to it.
    let err = compile(
        &devices(),
        &Module::parse(
            "extern taster = TreeDevice(title: \"Taster\")\ntaster.SwitchBoard = \"EG\"\n",
        )
        .unwrap(),
        &mut Lockfile::default(),
        &opts(),
    )
    .unwrap_err()
    .to_string();
    assert!(err.contains("never adds one"), "{err}");
}
