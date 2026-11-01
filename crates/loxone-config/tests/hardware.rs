//! Hardware declarations: a Tree device and its inputs and outputs taken
//! over by serial and designation, what they own written on every
//! compile, released untouched when they leave the source, refused where
//! the config lacks what they would change, and adopted as a no-op.

use loxone_config::logic::{CompileOptions, Module, adopt_one, compile};
use loxone_config::uuid::parse_serial;
use loxone_config::{Lockfile, LoxoneDoc};

const TREE: &str = "00000003-0000-0000-ffff000000000001";
const DEVICE: &str = "00000004-0000-0000-ffff000000000001";
const BUTTON: &str = "00000010-0000-0000-ffff000000000001";
const TEMP: &str = "00000020-0000-0000-ffff000000000001";
const LED: &str = "00000030-0000-0000-ffff000000000001";
const FLUR: &str = "00000040-0000-0000-ffff000000000001";
const BAD: &str = "00000041-0000-0000-ffff000000000001";
const LICHT: &str = "00000042-0000-0000-ffff000000000001";

/// A Tree extension with one device, a button, a temperature sensor with
/// statistics and an LED, written the way a save writes them, two rooms,
/// a category and one logic page.
fn base() -> LoxoneDoc {
    LoxoneDoc::parse(
        format!(
            "<ControlList Version=\"1\" NextObj=\"50\">\r\n\
             \t<C Type=\"Document\" U=\"00000001-0000-0000-ffff000000000001\" ConfigVersion=\"17020828\">\r\n\
             \t\t<C Type=\"Place\" U=\"{FLUR}\" Title=\"Flur\"/>\r\n\
             \t\t<C Type=\"Place\" U=\"{BAD}\" Title=\"Bad\"/>\r\n\
             \t\t<C Type=\"Category\" U=\"{LICHT}\" Title=\"Licht\"/>\r\n\
             \t\t<C Type=\"LoxTree\" V=\"178\" U=\"{TREE}\" Title=\"Tree\" WF=\"16384\" Serial=\"13000003\" Installation=\"\" SwitchBoard=\"Schaltschrank\">\r\n\
             \t\t\t<C Type=\"TreeDevice\" V=\"178\" U=\"{DEVICE}\" Title=\"Taster Flur\" WF=\"16384\" Serial=\"B0A1B2C3\" Installation=\"Wand\">\r\n\
             \t\t\t\t<C Type=\"TreeSensor\" IName=\"I0\" V=\"178\" U=\"{BUTTON}\" Title=\"Taste\" Cl=\"0,0,0\" Nio=\"2\" WF=\"20480\" MinTime=\"10\">\r\n\
             \t\t\t\t\t<Co K=\"Q\" U=\"00000011-0000-0000-ffff000000000001\"/>\r\n\
             \t\t\t\t\t<Co K=\"Qe\" U=\"00000012-0000-0000-ffff000000000001\"/>\r\n\
             \t\t\t\t\t<IoData Cr=\"{LICHT}\" Pr=\"{FLUR}\" St=\"2\"/>\r\n\
             \t\t\t\t\t<Display Unit=\"&lt;v&gt;\" StateOnly=\"true\"/>\r\n\
             \t\t\t\t</C>\r\n\
             \t\t\t\t<C Type=\"TreeAsensor\" IName=\"AI0\" V=\"178\" U=\"{TEMP}\" Title=\"Temperatur\" Cl=\"238,238,238\" Nio=\"2\" WF=\"16384\" Analog=\"true\" SourceValHigh=\"100\" DestValHigh=\"100\" MinChange=\"0.25\" MinTime=\"1000\" StatsType=\"11\" StatsAutoDel=\"1\">\r\n\
             \t\t\t\t\t<Co K=\"AQ\" U=\"00000021-0000-0000-ffff000000000001\"/>\r\n\
             \t\t\t\t\t<Co K=\"Q\" U=\"00000022-0000-0000-ffff000000000001\"/>\r\n\
             \t\t\t\t\t<IoData Cr=\"{LICHT}\" Pr=\"{FLUR}\" St=\"2\"/>\r\n\
             \t\t\t\t\t<Display Unit=\"&lt;v.1&gt;°\" StateOnly=\"true\"/>\r\n\
             \t\t\t\t</C>\r\n\
             \t\t\t\t<C Type=\"TreeActor\" IName=\"Q0\" V=\"178\" U=\"{LED}\" Title=\"LED\" Cl=\"238,238,238\" Nio=\"1\" WF=\"16384\">\r\n\
             \t\t\t\t\t<Co K=\"I\" U=\"00000031-0000-0000-ffff000000000001\"/>\r\n\
             \t\t\t\t\t<IoData Cr=\"{LICHT}\" Pr=\"{FLUR}\" St=\"2\"/>\r\n\
             \t\t\t\t\t<Display Unit=\"&lt;v.1&gt;\" StateOnly=\"true\"/>\r\n\
             \t\t\t\t</C>\r\n\
             \t\t\t</C>\r\n\
             \t\t</C>\r\n\
             \t\t<C Type=\"Page\" U=\"00000005-0000-0000-ffff000000000001\" Title=\"Regeln\">\r\n\
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

const DECLS: &str = r#"
taster = TreeDevice("Taster Bad", serial: "B0A1B2C3", Installation: "Neben der Tür", Desc: "Touch")
taste = TreeSensor("Klingel", device: taster, iname: "I0", Desc: "Haustür", MinTime: 20, Statistics: on)
temp = TreeAsensor("Raum", device: taster, iname: "AI0", Unit: "<v.1> °C", Statistics: off)
led = TreeActor("Status", device: taster, iname: "Q0", I: taste.Q)
tree = LoxTree("Tree", serial: "13000003", Installation: "Keller")
taste.room = "Bad"
"#;

fn attr(doc: &LoxoneDoc, uuid: &str, name: &str) -> Option<String> {
    let o = doc.objects().into_iter().find(|o| o.uuid == uuid).unwrap();
    doc.element_at(&o.path)
        .unwrap()
        .attr_decoded(name)
        .map(|v| v.into_owned())
}

fn io_data(doc: &LoxoneDoc, uuid: &str, name: &str) -> Option<String> {
    let o = doc.objects().into_iter().find(|o| o.uuid == uuid).unwrap();
    doc.element_at(&o.path)
        .unwrap()
        .child_elements()
        .find(|e| e.name == "IoData")?
        .attr(name)
        .map(str::to_string)
}

#[test]
fn a_declaration_owns_what_it_states_and_converges() {
    let module = Module::parse(DECLS).unwrap();
    let mut lock = Lockfile::default();
    let out = compile(&base(), &module, &mut lock, &opts()).unwrap();

    assert_eq!(attr(&out, DEVICE, "Title").as_deref(), Some("Taster Bad"));
    assert_eq!(
        attr(&out, DEVICE, "Installation").as_deref(),
        Some("Neben der Tür")
    );
    assert_eq!(attr(&out, DEVICE, "Desc").as_deref(), Some("Touch"));
    assert_eq!(attr(&out, BUTTON, "Title").as_deref(), Some("Klingel"));
    assert_eq!(attr(&out, BUTTON, "MinTime").as_deref(), Some("20"));
    assert_eq!(attr(&out, BUTTON, "StatsType").as_deref(), Some("7"));
    assert_eq!(attr(&out, TEMP, "StatsType"), None);
    assert_eq!(attr(&out, TEMP, "StatsAutoDel"), None);
    assert_eq!(attr(&out, TREE, "Installation").as_deref(), Some("Keller"));
    assert_eq!(io_data(&out, BUTTON, "Pr").as_deref(), Some(BAD));
    // The wire into the LED.
    assert!(
        out.wires()
            .iter()
            .any(|w| w.from_port == "00000011-0000-0000-ffff000000000001"
                && w.to_port == "00000031-0000-0000-ffff000000000001")
    );
    assert!(lock.externals.values().all(|e| e.owned));
    assert_eq!(lock.externals["taste"].matched_by, "iname");

    let again = compile(&out, &module, &mut lock, &opts()).unwrap();
    assert_eq!(out.to_bytes(), again.to_bytes());
}

#[test]
fn removing_a_declaration_leaves_the_hardware_as_it_is() {
    let module = Module::parse(DECLS).unwrap();
    let mut lock = Lockfile::default();
    let out = compile(&base(), &module, &mut lock, &opts()).unwrap();
    let released = compile(&out, &Module::parse("").unwrap(), &mut lock, &opts()).unwrap();
    // Every object stays, with the values the source wrote last; only the
    // source's wire leaves.
    assert_eq!(released.objects().len(), out.objects().len());
    assert_eq!(attr(&released, BUTTON, "Title").as_deref(), Some("Klingel"));
    assert_eq!(attr(&released, BUTTON, "StatsType").as_deref(), Some("7"));
    assert_eq!(io_data(&released, BUTTON, "Pr").as_deref(), Some(BAD));
    assert!(lock.externals.is_empty());
}

fn refusal(src: &str) -> String {
    let module = match Module::parse(src) {
        Ok(m) => m,
        Err(e) => return e.to_string(),
    };
    compile(&base(), &module, &mut Lockfile::default(), &opts())
        .unwrap_err()
        .to_string()
}

#[test]
fn refusals_name_the_fix() {
    let dev = "taster = TreeDevice(\"Taster\", uuid: \"00000004-0000-0000-ffff000000000001\")\n";
    // `iname:` needs a device matched by serial.
    let e = refusal(&format!(
        "{dev}taste = TreeSensor(\"Taste\", device: taster, iname: \"I0\")\n"
    ));
    assert!(e.contains("`serial:`"), "{e}");
    // The title is the label.
    let e = refusal(&format!("{dev}taster.title = \"X\"\n"));
    assert!(e.contains("label"), "{e}");
    // A port statement goes into the argument list.
    let e = refusal(&format!(
        "{dev}led = TreeActor(\"LED\", device: taster, uuid: \"{LED}\")\nled.I = 1\n"
    ));
    assert!(e.contains("argument list"), "{e}");
    // An I/O of another device.
    let e = refusal(
        "tree = LoxTree(\"Tree\", serial: \"13000003\")\nx = TreeSensor(\"T\", device: tree, iname: \"I0\")\n",
    );
    assert!(e.contains("TreeDevice"), "{e}");
    // The extension has no room binding to rewrite.
    let e = refusal("tree = LoxTree(\"Tree\", serial: \"13000003\")\ntree.room = \"Bad\"\n");
    assert!(e.contains("Loxone Config"), "{e}");
    // A setting the type does not have.
    let e = refusal(&format!(
        "{dev}taster2 = TreeDevice(\"T\", serial: \"B0A1B2C3\", MinTime: 5)\n"
    ));
    assert!(e.contains("MinTime"), "{e}");
}

#[test]
fn adopting_a_device_takes_its_ios_and_changes_nothing() {
    let doc = base();
    let empty = Module::parse("").unwrap();
    let mut lock = Lockfile::default();
    compile(&doc, &empty, &mut lock, &opts()).unwrap();
    let adopted = adopt_one(&doc, DEVICE, "taster", &empty, &mut lock).unwrap();
    let text = Module {
        items: adopted.items,
    }
    .to_text();
    assert!(
        text.contains(r#"taster = TreeDevice("Taster Flur", serial: "B0A1B2C3""#),
        "{text}"
    );
    assert!(text.contains(r#"iname: "AI0""#), "{text}");
    assert!(text.contains("Statistics: on"), "{text}");
    assert!(text.contains(r#"taster_taste.room = "Flur""#), "{text}");
    assert_eq!(lock.externals.len(), 4);
    assert!(lock.externals.values().all(|e| e.owned));

    let module = Module::parse(&text).unwrap();
    let out = compile(&doc, &module, &mut lock, &opts()).unwrap();
    assert!(loxone_config::diff::diff(&doc, &out).is_empty());

    // An I/O comes with its device.
    let mut lock = Lockfile::default();
    let e = adopt_one(&doc, BUTTON, "taste", &empty, &mut lock).unwrap_err();
    assert!(e.to_string().contains("TreeDevice"), "{e}");
}
