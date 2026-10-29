//! Repairing numeric-character-reference text (D44): `lxir repair` and
//! `LoxoneDoc::repair_char_refs` over examples/configs/pool.Loxone, with
//! multi-line attributes added the way Loxone writes them (raw line
//! breaks) and damaged the way a foreign writer leaves them.

use lxir::LoxoneDoc;
use lxir::doc::CharRefRepair;
use std::path::Path;
use std::process::Command;

const PUMPE: &str = "30000005-0000-0050-ffff504f94112233";
const STOERUNG: &str = "30000009-0000-0090-ffff504f94112233";
const NETZ: &str = "3000000b-0000-00b0-ffff504f94112233";

/// pool.Loxone with one extra attribute on each of three objects.
fn with_attrs(ndoc: &str, apc: &str, code: &str) -> String {
    let mut xml =
        String::from_utf8(std::fs::read("examples/configs/pool.Loxone").unwrap()).unwrap();
    for (uuid, attr) in [(PUMPE, ndoc), (STOERUNG, apc), (NETZ, code)] {
        let at = format!("U=\"{uuid}\"");
        assert_eq!(xml.matches(&at).count(), 1);
        xml = xml.replacen(&at, &format!("{at} {attr}"), 1);
    }
    xml
}

/// As Loxone writes them: line breaks and tabs raw, named entities only.
fn native() -> String {
    with_attrs(
        "NDOC=\"Pumpe &amp; Filter\r\nnur tagsüber\"",
        "APC=\"{&quot;a&quot;:1},\r\n{&quot;b&quot;:2}\"",
        "Code=\"int x;\nwhile(TRUE) {\n\tsleep(100);\n}\n\"",
    )
}

/// After a foreign writer: the NDOC and Code through a later GUI save
/// (ampersand re-escaped), the APC straight from the writer.
fn damaged() -> String {
    with_attrs(
        "NDOC=\"Pumpe &amp; Filter&amp;#xD;&amp;#xA;nur tagsüber\"",
        "APC=\"{&quot;a&quot;:1},&#xD;&#xA;{&quot;b&quot;:2}\"",
        "Code=\"int x;&amp;#xA;while(TRUE) {&amp;#xA;&amp;#9;sleep(100);&amp;#xA;}&amp;#10;\"",
    )
}

fn parse(xml: &str) -> LoxoneDoc {
    LoxoneDoc::parse(xml.as_bytes()).unwrap()
}

#[test]
fn repair_restores_what_loxone_wrote_byte_for_byte() {
    let mut doc = parse(&damaged());
    assert_eq!(doc.char_ref_attrs().len(), 3);
    let repairs = doc.repair_char_refs();
    let entry = |object: &str, uuid: &str, attr: &str, replaced| CharRefRepair {
        object: object.into(),
        uuid: uuid.into(),
        attr: attr.into(),
        replaced,
    };
    assert_eq!(
        repairs,
        [
            entry("Netzbezug", NETZ, "Code", 5),
            entry("Freigabe", PUMPE, "NDOC", 2),
            entry("Störmeldung", STOERUNG, "APC", 2),
        ]
    );
    let want = native();
    assert_eq!(String::from_utf8(doc.to_bytes()).unwrap(), want);
    assert!(doc.char_ref_attrs().is_empty());

    // The repaired document round-trips and repairs to itself.
    let again = parse(&want);
    assert_eq!(again.to_bytes(), want.as_bytes());
    let mut twice = again.clone();
    assert!(twice.repair_char_refs().is_empty());
}

#[test]
fn repair_writes_special_characters_as_named_entities() {
    let mut doc = parse(&with_attrs(
        "NDOC=\"a &#38; b &amp;#60;c&amp;#x3E; &#34;d&#39;\"",
        "APC=\"x\"",
        "Code=\"y\"",
    ));
    let repairs = doc.repair_char_refs();
    assert_eq!(repairs.len(), 1);
    assert_eq!(repairs[0].replaced, 5);
    let xml = String::from_utf8(doc.to_bytes()).unwrap();
    assert!(
        xml.contains("NDOC=\"a &amp; b &lt;c&gt; &quot;d&apos;\""),
        "{xml}"
    );
}

#[test]
fn repair_leaves_what_is_no_character_and_what_is_no_reference() {
    // `&#0;` names nothing an attribute can hold; `&amp;amp;#xA;` decodes
    // to the text `&amp;#xA;`, which holds no reference; `&#xA` lacks its
    // semicolon. All three stay byte for byte.
    let src = with_attrs(
        "NDOC=\"a&amp;#0;b\"",
        "APC=\"c&amp;amp;#xA;d\"",
        "Code=\"e&amp;#xAf\"",
    );
    let mut doc = parse(&src);
    assert!(doc.repair_char_refs().is_empty());
    assert_eq!(doc.to_bytes(), src.as_bytes());
    assert_eq!(
        doc.char_ref_attrs(),
        [("Freigabe".to_string(), "NDOC".to_string())]
    );

    // A clean config is copied unchanged.
    let clean = std::fs::read("examples/configs/pool.Loxone").unwrap();
    let mut doc = LoxoneDoc::parse(&clean).unwrap();
    assert!(doc.repair_char_refs().is_empty());
    assert_eq!(doc.to_bytes(), clean);
}

#[test]
fn repair_command_lists_each_attribute_and_writes_the_copy() {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("repair-cli");
    std::fs::create_dir_all(&dir).unwrap();
    let input = dir.join("damaged.Loxone");
    let output = dir.join("repaired.Loxone");
    std::fs::write(&input, damaged()).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_lxir"))
        .arg("repair")
        .arg(&input)
        .arg("--out")
        .arg(&output)
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(
        stdout.contains(&format!(
            "repaired \"Netzbezug\".Code ({NETZ}): 5 reference(s)"
        )),
        "{stdout}"
    );
    assert!(stdout.contains("3 attribute(s) repaired"), "{stdout}");
    let repaired = std::fs::read(&output).unwrap();
    let mut doc = parse(&damaged());
    doc.repair_char_refs();
    assert_eq!(repaired, doc.to_bytes());
    // The input is never touched.
    assert_eq!(std::fs::read(&input).unwrap(), damaged().as_bytes());
}
