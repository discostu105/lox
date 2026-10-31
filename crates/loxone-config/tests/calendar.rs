//! Calendar entries from source (#211): `CalendarEntry` objects of the
//! "Betriebszeiten" folder, which switch an operating mode on dates.

use loxone_config::logic::{CompileOptions, Module, adopt_one, compile};
use loxone_config::uuid::parse_serial;
use loxone_config::{Lockfile, LoxoneDoc};

/// The house's folder with its heating period, as Loxone Config 17.2
/// writes it.
fn base() -> LoxoneDoc {
    LoxoneDoc::parse(
        "<ControlList Version=\"1\" NextObj=\"50\" NextConst=\"1\" NextNote=\"1\" NextMem=\"1\">\r\n\
         \t<C Type=\"Document\" U=\"00000001-0000-0000-ffff000000000001\" ConfigVersion=\"17020828\">\r\n\
         \t\t<C Type=\"CalendarCaption\" V=\"178\" U=\"00000002-0000-0000-ffff000000000001\" Title=\"Betriebszeiten\" WF=\"16384\">\r\n\
         \t\t\t<C Type=\"CalendarEntry\" V=\"178\" U=\"00000003-0000-0000-ffff000000000001\" Title=\"Heizperiode\" Nio=\"2\" WF=\"16384\" CalType=\"10\" Mode=\"4\" Month=\"10\" Day=\"1\" Month2=\"4\" Day2=\"30\" WeekDayInMonth=\"1\">\r\n\
         \t\t\t\t<Co K=\"I\" U=\"00000003-0000-0001-00ff000000000001\"/>\r\n\
         \t\t\t\t<Co K=\"Q\" U=\"00000003-0000-0002-01ff000000000001\"/>\r\n\
         \t\t\t</C>\r\n\
         \t\t</C>\r\n\
         \t\t<C Type=\"Page\" U=\"00000004-0000-0000-ffff000000000001\" Title=\"Logik\"></C>\r\n\
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
        page_title: Some("Logik".into()),
        allow_removals: false,
        accept_version: None,
        relayout: Vec::new(),
    }
}

fn text(doc: &LoxoneDoc) -> String {
    String::from_utf8(doc.to_bytes()).unwrap()
}

#[test]
fn a_minted_entry_is_shaped_like_loxone_configs() {
    let m = Module::parse(
        "ferien = CalendarEntry(\"Sommerferien\", CalType: 2, Mode: 3, From: 6393, To: 6456)\n\
         ostern = CalendarEntry(\"Ostermontag\", Mode: 1, EasterOffset: 1)\n",
    )
    .unwrap();
    let out = text(&compile(&base(), &m, &mut Lockfile::new(), &opts()).unwrap());
    // In the folder, after the heating period, attributes in the save's
    // order (the corpus's period and Easter entries).
    let ferien = out.find("Title=\"Sommerferien\"").unwrap();
    assert!(ferien > out.find("Title=\"Heizperiode\"").unwrap());
    assert!(ferien < out.find("Type=\"Page\"").unwrap());
    assert!(
        out.contains(
            "Title=\"Sommerferien\" Nio=\"2\" WF=\"16384\" CalType=\"2\" Mode=\"3\" Month=\"1\" \
             Day=\"1\" Month2=\"1\" Day2=\"1\" From=\"6393\" To=\"6456\" WeekDayInMonth=\"1\">"
        ),
        "{out}"
    );
    assert!(
        out.contains(
            "Title=\"Ostermontag\" Nio=\"2\" WF=\"16384\" Mode=\"1\" Month=\"1\" Day=\"1\" \
             Month2=\"1\" Day2=\"1\" EasterOffset=\"1\" WeekDayInMonth=\"1\">"
        ),
        "{out}"
    );
    assert_eq!(out.matches("<Co K=\"I\"").count(), 3, "{out}");
}

#[test]
fn an_entry_is_adopted_and_edited_from_source() {
    let mut lock = Lockfile::new();
    let adopted = adopt_one(
        &base(),
        "00000003-0000-0000-ffff000000000001",
        "heizperiode",
        &Module { items: Vec::new() },
        &mut lock,
    )
    .unwrap();
    let m = Module {
        items: adopted.items,
    };
    let src = m.to_text();
    assert_eq!(
        src,
        "heizperiode = CalendarEntry(\n\t\"Heizperiode\",\n\tCalType: 10,\n\tMode: 4,\n\tMonth: 10,\n\
         \tDay: 1,\n\tMonth2: 4,\n\tDay2: 30,\n\tWeekDayInMonth: 1,\n)\n"
    );
    // Unchanged: the same bytes.
    let same = compile(&base(), &m, &mut lock.clone(), &opts()).unwrap();
    assert_eq!(text(&same), text(&base()));
    // A shorter heating period.
    let edited = Module::parse(&src.replace("\tDay2: 30,", "\tDay2: 15,")).unwrap();
    let out = text(&compile(&base(), &edited, &mut lock, &opts()).unwrap());
    assert_eq!(out, text(&base()).replace("Day2=\"30\"", "Day2=\"15\""));
}
