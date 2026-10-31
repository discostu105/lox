//! Page creation from source (D56): `page "T" (room: "…")` creates the
//! page a base lacks, the lock keeps its identity, and a page Loxone
//! Config owns is never touched.

use loxone_config::logic::{CompileOptions, Module, compile};
use loxone_config::uuid::parse_serial;
use loxone_config::{Lockfile, LoxoneDoc};

const BASE: &str = "\u{feff}<?xml version=\"1.0\" encoding=\"utf-8\"?>\r\n\
<ControlList Version=\"1\" NextObj=\"50\" NextConst=\"1\" NextNote=\"1\" NextMem=\"1\">\r\n\
\t<C Type=\"Document\" V=\"17020000\" U=\"20000000-0000-0001-ffff504f94112233\" Title=\"Haus\" ConfigVersion=\"17020000\">\r\n\
\t\t<C Type=\"CategoryCaption\" V=\"178\" U=\"20000000-0000-0002-ffff504f94112233\" Title=\"Kategorien\">\r\n\
\t\t\t<C Type=\"Category\" V=\"178\" U=\"20000000-0000-0003-ffff504f94112233\" Title=\"Nicht zugeordnet\" First=\"true\"/>\r\n\
\t\t\t<C Type=\"Category\" V=\"178\" U=\"20000000-0000-0004-ffff504f94112233\" Title=\"Beleuchtung\"/>\r\n\
\t\t</C>\r\n\
\t\t<C Type=\"PlaceCaption\" V=\"178\" U=\"20000000-0000-0005-ffff504f94112233\" Title=\"R\u{e4}ume\">\r\n\
\t\t\t<C Type=\"Place\" V=\"178\" U=\"20000000-0000-0006-ffff504f94112233\" Title=\"Nicht zugeordnet\" First=\"true\"/>\r\n\
\t\t\t<C Type=\"Place\" V=\"178\" U=\"20000000-0000-0007-ffff504f94112233\" Title=\"K\u{fc}che\"/>\r\n\
\t\t</C>\r\n\
\t\t<C Type=\"Program\" V=\"178\" U=\"20000000-0000-0008-ffff504f94112233\" Title=\"Loxone Miniserver\">\r\n\
\t\t\t<C Type=\"Page\" V=\"178\" U=\"20000000-0000-0009-ffff504f94112233\" Title=\"Automatik-Regeln\" SpNo=\"1\">\r\n\
\t\t\t\t<IoData Cr=\"20000000-0000-0003-ffff504f94112233\" Pr=\"20000000-0000-0006-ffff504f94112233\"/>\r\n\
\t\t\t</C>\r\n\
\t\t\t<C Type=\"Page\" V=\"178\" U=\"20000000-0000-000a-ffff504f94112233\" Title=\"K\u{fc}che\">\r\n\
\t\t\t\t<IoData Cr=\"20000000-0000-0003-ffff504f94112233\" Pr=\"20000000-0000-0007-ffff504f94112233\"/>\r\n\
\t\t\t</C>\r\n\
\t\t\t<C Type=\"Page\" V=\"178\" U=\"20000000-0000-000b-ffff504f94112233\" Title=\"Zentral\">\r\n\
\t\t\t\t<IoData Cr=\"20000000-0000-0003-ffff504f94112233\" Pr=\"20000000-0000-0007-ffff504f94112233\"/>\r\n\
\t\t\t</C>\r\n\
\t\t</C>\r\n\
\t</C>\r\n\
</ControlList>";

const SRC: &str = "page \"K\u{fc}che\"\n\n\
                   a = Not()\n\n\
                   page \"K\u{fc}che Logik\" (room: \"K\u{fc}che\")\n\n\
                   b = Not(I: a.Q)\n";

fn base() -> LoxoneDoc {
    LoxoneDoc::parse(BASE.as_bytes()).unwrap()
}

fn opts() -> CompileOptions {
    CompileOptions {
        machine: parse_serial("504F94112233").unwrap(),
        mint_time_unix: 1_767_225_600,
        page_title: None,
        allow_removals: false,
        accept_version: None,
        relayout: Vec::new(),
    }
}

/// Program's pages in document order, as (title, uuid).
fn pages(doc: &LoxoneDoc) -> Vec<(String, String)> {
    doc.objects()
        .into_iter()
        .filter(|o| o.block_type == "Page")
        .map(|o| (o.title.unwrap_or_default(), o.uuid))
        .collect()
}

fn page_of(doc: &LoxoneDoc, lock: &Lockfile, slug: &str) -> String {
    let objs = doc.objects();
    let block = objs
        .iter()
        .find(|o| o.uuid == lock.objects[slug].uuid)
        .unwrap();
    objs.iter()
        .filter(|o| o.block_type == "Page" && block.path.starts_with(&o.path))
        .max_by_key(|o| o.path.len())
        .and_then(|o| o.title.clone())
        .unwrap()
}

fn text(doc: &LoxoneDoc) -> String {
    String::from_utf8(doc.to_bytes()).unwrap()
}

#[test]
fn a_missing_page_with_a_room_is_created_sorted_in() {
    let module = Module::parse(SRC).unwrap();
    let mut lock = Lockfile::new();
    let out = compile(&base(), &module, &mut lock, &opts()).unwrap();

    let titles: Vec<String> = pages(&out).into_iter().map(|(t, _)| t).collect();
    assert_eq!(
        titles,
        [
            "Automatik-Regeln",
            "K\u{fc}che",
            "K\u{fc}che Logik",
            "Zentral"
        ]
    );
    let uuid = &lock.pages["K\u{fc}che Logik"];
    assert!(uuid.ends_with("-ffff504f94112233"), "minted: {uuid}");
    assert_eq!(page_of(&out, &lock, "a"), "K\u{fc}che");
    assert_eq!(page_of(&out, &lock, "b"), "K\u{fc}che Logik");
    // The element Loxone Config's "Neue Seite" writes: room and the
    // unassigned category through IoData.
    let xml = text(&out);
    assert!(
        xml.contains(&format!(
            "<C Type=\"Page\" V=\"178\" U=\"{uuid}\" Title=\"K\u{fc}che Logik\" WF=\"16384\">\r\n\
             \t\t\t\t<IoData Cr=\"20000000-0000-0003-ffff504f94112233\" \
             Pr=\"20000000-0000-0007-ffff504f94112233\"/>\r\n"
        )),
        "{xml}"
    );

    // A recompile against the same (undeployed) base: same page, same
    // bytes, same lock.
    let mut again = lock.clone();
    let out2 = compile(&base(), &module, &mut again, &opts()).unwrap();
    assert_eq!(out2.to_bytes(), out.to_bytes());
    assert_eq!(again, lock);

    // Against the deployed output: nothing more is created.
    let mut deployed = lock.clone();
    let out3 = compile(&out, &module, &mut deployed, &opts()).unwrap();
    assert_eq!(pages(&out3), pages(&out));
    assert_eq!(deployed.pages, lock.pages);
}

/// The GUI does not re-sort, so a base's page list need not be sorted: a
/// new page goes behind the last page sorting at or before it, and never
/// ahead of the special pages.
#[test]
fn a_created_page_follows_its_predecessor_in_title_order() {
    let unsorted = BASE.replace("Title=\"Zentral\"", "Title=\"Diele\"");
    let unsorted = LoxoneDoc::parse(unsorted.as_bytes()).unwrap();
    for (title, want) in [
        (
            "Abstellraum",
            ["Automatik-Regeln", "Abstellraum", "K\u{fc}che", "Diele"],
        ),
        // Diele is out of order; what sorts after it follows it.
        (
            "Keller",
            ["Automatik-Regeln", "K\u{fc}che", "Diele", "Keller"],
        ),
        (
            "Zimmer",
            ["Automatik-Regeln", "K\u{fc}che", "Diele", "Zimmer"],
        ),
    ] {
        let src = SRC.replace("K\u{fc}che Logik", title);
        let out = compile(
            &unsorted,
            &Module::parse(&src).unwrap(),
            &mut Lockfile::new(),
            &opts(),
        )
        .unwrap();
        let titles: Vec<String> = pages(&out).into_iter().map(|(t, _)| t).collect();
        assert_eq!(titles, want, "{title}");
    }
}

#[test]
fn a_page_loxone_config_has_is_used_not_created() {
    let module = Module::parse(&SRC.replace("K\u{fc}che Logik", "Zentral")).unwrap();
    let mut lock = Lockfile::new();
    let out = compile(&base(), &module, &mut lock, &opts()).unwrap();
    assert_eq!(pages(&out), pages(&base()));
    assert!(lock.pages.is_empty());
    assert_eq!(page_of(&out, &lock, "b"), "Zentral");
}

#[test]
fn without_a_room_a_missing_page_stays_an_error() {
    let module = Module::parse(&SRC.replace(" (room: \"K\u{fc}che\")", "")).unwrap();
    let e = compile(&base(), &module, &mut Lockfile::new(), &opts()).unwrap_err();
    assert!(e.to_string().contains("(room: \"…\")"), "{e}");
}

#[test]
fn room_and_category_are_checked() {
    let compile_src = |src: String| {
        compile(
            &base(),
            &Module::parse(&src).unwrap(),
            &mut Lockfile::new(),
            &opts(),
        )
    };
    let e = compile_src(SRC.replace("room: \"K\u{fc}che\"", "room: \"Keller\"")).unwrap_err();
    assert!(e.to_string().contains("no room titled \"Keller\""), "{e}");
    let e =
        compile_src(SRC.replace("room: \"K\u{fc}che\"", "room: \"Nicht zugeordnet\"")).unwrap_err();
    assert!(e.to_string().contains("needs a real room"), "{e}");
    let out = compile_src(SRC.replace(
        "room: \"K\u{fc}che\"",
        "room: \"K\u{fc}che\", category: \"Beleuchtung\"",
    ))
    .unwrap();
    assert!(text(&out).contains("<IoData Cr=\"20000000-0000-0004-ffff504f94112233\""));
    let e = compile_src(format!(
        "{SRC}page \"K\u{fc}che Logik\" (room: \"Nicht zugeordnet\")\n"
    ))
    .unwrap_err();
    assert!(e.to_string().contains("two different"), "{e}");
}

#[test]
fn a_created_page_renamed_in_loxone_config_is_refused() {
    let module = Module::parse(SRC).unwrap();
    let mut lock = Lockfile::new();
    let out = compile(&base(), &module, &mut lock, &opts()).unwrap();
    let renamed = text(&out).replace("Title=\"K\u{fc}che Logik\"", "Title=\"Logik\"");
    let renamed = LoxoneDoc::parse(renamed.as_bytes()).unwrap();
    let e = compile(&renamed, &module, &mut lock.clone(), &opts()).unwrap_err();
    assert!(e.to_string().contains("titles it \"Logik\""), "{e}");

    // Following the rename in source releases the page to Loxone Config.
    let followed = Module::parse(&SRC.replace(
        "page \"K\u{fc}che Logik\" (room: \"K\u{fc}che\")",
        "page \"Logik\"",
    ))
    .unwrap();
    let out = compile(&renamed, &followed, &mut lock, &opts()).unwrap();
    assert!(lock.pages.is_empty());
    assert_eq!(page_of(&out, &lock, "b"), "Logik");
}

#[test]
fn page_arguments_parse_and_print() {
    let m = Module::parse("page \"P\" (room: \"R\", category: \"C\")  # x\n").unwrap();
    assert_eq!(
        m.to_text(),
        "page \"P\" (room: \"R\", category: \"C\") # x\n"
    );
    for bad in [
        "page \"P\" (category: \"C\")\n",
        "page \"P\" (room: R)\n",
        "page \"P\" (floor: \"R\")\n",
        "page \"P\" (room: \"R\", room: \"S\")\n",
    ] {
        assert!(Module::parse(bad).is_err(), "{bad}");
    }
}
