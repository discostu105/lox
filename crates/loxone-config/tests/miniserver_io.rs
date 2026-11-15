//! The I/O of the Miniserver and of what hangs below it: its onboard
//! outputs (by `uuid:` below the Miniserver's extern), a Relay
//! Extension's (by designation) and a network device's, declared and
//! adopted as a no-op, alone where the device is declared already.

use loxone_config::logic::{CompileOptions, Module, adopt_one, compile};
use loxone_config::uuid::parse_serial;
use loxone_config::{Lockfile, LoxoneDoc};

const MS: &str = "00000002-0000-0000-ffff000000000001";
const MS_Q1: &str = "00000010-0000-0000-ffff000000000001";
const MS_I1: &str = "00000011-0000-0000-ffff000000000001";
const MS_TSD: &str = "00000012-0000-0000-ffff000000000001";
const REL: &str = "00000020-0000-0000-ffff000000000001";
const REL_Q1: &str = "00000021-0000-0000-ffff000000000001";
const REL_TSD: &str = "00000022-0000-0000-ffff000000000001";
const ICOM: &str = "00000030-0000-0000-ffff000000000001";
const ICOM_I1: &str = "00000031-0000-0000-ffff000000000001";
const ICOM_API: &str = "00000032-0000-0000-ffff000000000001";

/// A Miniserver with an input, an output and its overtemperature
/// shutdown, a Relay Extension and an Intercom below it, whose
/// designations repeat the Miniserver's.
fn base() -> LoxoneDoc {
    LoxoneDoc::parse(
        format!(
            "<ControlList Version=\"1\" NextObj=\"50\">\r\n\
             \t<C Type=\"Document\" U=\"00000001-0000-0000-ffff000000000001\" ConfigVersion=\"17020828\">\r\n\
             \t\t<C Type=\"LoxLIVE\" V=\"178\" U=\"{MS}\" Title=\"Loxone Miniserver\" Serial=\"504F94112233\">\r\n\
             \t\t\t<C Type=\"OvertempShutdown\" IName=\"TSD\" V=\"178\" U=\"{MS_TSD}\" Title=\"Temperaturabschaltung\" Nio=\"1\" WF=\"20496\" MinTime=\"10\">\r\n\
             \t\t\t\t<Co K=\"Q\" U=\"00000013-0000-0000-ffff000000000001\"/>\r\n\
             \t\t\t</C>\r\n\
             \t\t\t<C Type=\"InputCaption\" V=\"178\" U=\"00000003-0000-0000-ffff000000000001\" Title=\"Digitale Eingänge\">\r\n\
             \t\t\t\t<C Type=\"DigitalIn\" IName=\"I1\" V=\"178\" U=\"{MS_I1}\" Title=\"Eingang 1\" Nio=\"2\" WF=\"16384\" MinTime=\"10\">\r\n\
             \t\t\t\t\t<Co K=\"Q\" U=\"00000014-0000-0000-ffff000000000001\"/>\r\n\
             \t\t\t\t</C>\r\n\
             \t\t\t</C>\r\n\
             \t\t\t<C Type=\"OutputCaption\" V=\"178\" U=\"00000004-0000-0000-ffff000000000001\" Title=\"Digitale Ausgänge\">\r\n\
             \t\t\t\t<C Type=\"Actor\" IName=\"Q1\" V=\"178\" U=\"{MS_Q1}\" Title=\"Aktor (Relais) 1\" Nio=\"1\" WF=\"16400\">\r\n\
             \t\t\t\t\t<Co K=\"I\" U=\"00000015-0000-0000-ffff000000000001\"/>\r\n\
             \t\t\t\t</C>\r\n\
             \t\t\t\t<C Type=\"NetworkDevice\" V=\"178\" U=\"{ICOM}\" Title=\"Intercom Haustür\">\r\n\
             \t\t\t\t\t<C Type=\"DigitalIn\" IName=\"I1\" V=\"178\" U=\"{ICOM_I1}\" Title=\"Klingeltaste\" Nio=\"2\" WF=\"16400\" MinTime=\"10\">\r\n\
             \t\t\t\t\t\t<Co K=\"Q\" U=\"00000033-0000-0000-ffff000000000001\"/>\r\n\
             \t\t\t\t\t</C>\r\n\
             \t\t\t\t\t<C Type=\"ApiActor\" V=\"178\" U=\"{ICOM_API}\" Title=\"API Connector\" Nio=\"1\" WF=\"8405008\">\r\n\
             \t\t\t\t\t\t<Co K=\"I\" U=\"00000034-0000-0000-ffff000000000001\"/>\r\n\
             \t\t\t\t\t</C>\r\n\
             \t\t\t\t</C>\r\n\
             \t\t\t</C>\r\n\
             \t\t\t<C Type=\"LoxREL\" V=\"178\" U=\"{REL}\" Title=\"Relay Extension 1\" Serial=\"0BDDC807\">\r\n\
             \t\t\t\t<C Type=\"OvertempShutdown\" IName=\"TSD\" V=\"178\" U=\"{REL_TSD}\" Title=\"Temperaturabschaltung\" Nio=\"1\" WF=\"20480\" MinTime=\"10\">\r\n\
             \t\t\t\t\t<Co K=\"Q\" U=\"00000023-0000-0000-ffff000000000001\"/>\r\n\
             \t\t\t\t</C>\r\n\
             \t\t\t\t<C Type=\"Actor\" IName=\"Q1\" V=\"178\" U=\"{REL_Q1}\" Title=\"Aktor (Relais) 1\" Nio=\"1\" WF=\"16400\">\r\n\
             \t\t\t\t\t<Co K=\"I\" U=\"00000024-0000-0000-ffff000000000001\"/>\r\n\
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

fn title(doc: &LoxoneDoc, uuid: &str) -> String {
    doc.objects()
        .into_iter()
        .find(|o| o.uuid == uuid)
        .and_then(|o| o.title)
        .unwrap()
}

fn refusal(src: &str) -> String {
    let module = Module::parse(src).unwrap();
    compile(&base(), &module, &mut Lockfile::default(), &opts())
        .unwrap_err()
        .to_string()
}

#[test]
fn declarations_own_the_miniservers_io_and_converge() {
    let src = format!(
        "extern ms = LoxLIVE(uuid: \"{MS}\")\n\
         tueroeffner = Actor(\"Türöffner\", device: ms, uuid: \"{MS_Q1}\", Desc: \"Haustür\")\n\
         rel = LoxREL(\"Relais Keller\", serial: \"0BDDC807\")\n\
         rollo = Actor(\"Rollo auf\", device: rel, iname: \"Q1\", I: klingel.Q)\n\
         rel_tsd = OvertempShutdown(\"Übertemperatur Keller\", device: rel, iname: \"TSD\")\n\
         icom = NetworkDevice(\"Intercom\", uuid: \"{ICOM}\")\n\
         klingel = DigitalIn(\"Klingel\", device: icom, uuid: \"{ICOM_I1}\")\n"
    );
    let module = Module::parse(&src).unwrap();
    let mut lock = Lockfile::default();
    let out = compile(&base(), &module, &mut lock, &opts()).unwrap();
    assert_eq!(title(&out, MS_Q1), "Türöffner");
    assert_eq!(title(&out, REL), "Relais Keller");
    assert_eq!(title(&out, REL_Q1), "Rollo auf");
    assert_eq!(title(&out, REL_TSD), "Übertemperatur Keller");
    assert_eq!(title(&out, ICOM_I1), "Klingel");
    // The Miniserver's own Q1 and TSD are untouched.
    assert_eq!(title(&out, MS_TSD), "Temperaturabschaltung");
    assert_eq!(lock.externals["rollo"].uuid, REL_Q1);
    assert_eq!(lock.externals["rollo"].matched_by, "iname");
    assert_eq!(lock.externals["rel_tsd"].uuid, REL_TSD);
    assert!(!lock.externals["ms"].owned);

    let again = compile(&out, &module, &mut lock, &opts()).unwrap();
    assert_eq!(out.to_bytes(), again.to_bytes());
}

#[test]
fn refusals_name_the_fix() {
    let ms = format!("extern ms = LoxLIVE(uuid: \"{MS}\")\n");
    // Extensions below the Miniserver repeat its designations.
    let e = refusal(&format!(
        "{ms}q = Actor(\"Q\", device: ms, iname: \"Q1\")\n"
    ));
    assert!(e.contains(r#"with `uuid: "…"`"#), "{e}");
    // An API connector is no Miniserver I/O.
    let e = refusal(&format!(
        "{ms}a = ApiActor(\"A\", device: ms, uuid: \"{ICOM_API}\")\n"
    ));
    assert!(e.contains("not the Miniserver"), "{e}");
    // A relay output hangs below a Relay Extension, not a DI Extension.
    let e = refusal(
        "di = LoxDigin(\"DI\", serial: \"0BDDC807\")\nq = Actor(\"Q\", device: di, iname: \"Q1\")\n",
    );
    assert!(e.contains("LoxREL or LoxLIVE"), "{e}");
}

/// A declaration on one line, however `to_text` breaks it.
fn flat(text: &str) -> String {
    text.replace("(\n\t", "(")
        .replace(",\n\t", ", ")
        .replace(",\n)", ")")
}

/// Adopt `uuid` as `slug` into `module`, check the next compile changes
/// nothing, and return the source with the adopted lines.
fn adopt(doc: &LoxoneDoc, src: &str, uuid: &str, slug: &str, lock: &mut Lockfile) -> String {
    let mut module = Module::parse(src).unwrap();
    let adopted = adopt_one(doc, uuid, slug, &module, lock).unwrap();
    for x in &adopted.promoted {
        assert!(module.strip_extern(x));
    }
    module.items.extend(adopted.items);
    let text = flat(&module.to_text());
    let out = compile(doc, &Module::parse(&text).unwrap(), lock, &opts()).unwrap();
    assert!(loxone_config::diff::diff(doc, &out).is_empty(), "{text}");
    assert_eq!(out.to_bytes(), doc.to_bytes(), "{text}");
    text
}

#[test]
fn adopting_an_extension_takes_its_outputs_by_designation() {
    let mut lock = Lockfile::default();
    let doc = compile(&base(), &Module::parse("").unwrap(), &mut lock, &opts()).unwrap();
    let text = adopt(&doc, "", REL, "rel", &mut lock);
    assert!(
        text.contains(r#"rel = LoxREL("Relay Extension 1", serial: "0BDDC807")"#),
        "{text}"
    );
    assert!(text.contains(r#"device: rel, iname: "Q1""#), "{text}");
    assert!(text.contains(r#"device: rel, iname: "TSD""#), "{text}");

    // A network device has no serial: its I/O goes by uuid.
    let text = adopt(&doc, &text, ICOM, "icom", &mut lock);
    assert!(
        text.contains(&format!(r#"device: icom, uuid: "{ICOM_API}""#)),
        "{text}"
    );
}

#[test]
fn an_io_adopts_alone_below_its_declared_device_or_the_miniserver() {
    let doc = base();
    let mut lock = Lockfile::default();
    // The device declared, its outputs still externs.
    let src = format!(
        "rel = LoxREL(\"Relay Extension 1\", serial: \"0BDDC807\")\n\
         extern rollo = Actor(uuid: \"{REL_Q1}\")\n\
         extern ms = LoxLIVE(uuid: \"{MS}\")\n\
         extern tor = Actor(uuid: \"{MS_Q1}\")\n\
         extern ein = DigitalIn(uuid: \"{MS_I1}\")\n\
         rollo.I <- ein.Q\n"
    );
    let first = compile(&doc, &Module::parse(&src).unwrap(), &mut lock, &opts()).unwrap();
    let doc = first;

    let text = adopt(&doc, &src, REL_Q1, "rollo", &mut lock);
    assert!(
        text.contains(r#"rollo = Actor("Aktor (Relais) 1", device: rel, iname: "Q1", I: ein.Q)"#),
        "{text}"
    );
    let text = adopt(&doc, &text, MS_Q1, "tor", &mut lock);
    assert!(
        text.contains(&format!(
            r#"tor = Actor("Aktor (Relais) 1", device: ms, uuid: "{MS_Q1}")"#
        )),
        "{text}"
    );

    // Without the Miniserver's extern, or below an undeclared device, the
    // refusal names what to adopt first.
    let empty = Module::parse("").unwrap();
    let e = adopt_one(&doc, MS_I1, "i1", &empty, &mut Lockfile::default()).unwrap_err();
    assert!(
        e.to_string().contains(&format!("loxl adopt --uuid {MS}")),
        "{e}"
    );
    let e = adopt_one(&doc, ICOM_I1, "k", &empty, &mut Lockfile::default()).unwrap_err();
    assert!(
        e.to_string().contains(&format!("loxl adopt --uuid {ICOM}")),
        "{e}"
    );
}
