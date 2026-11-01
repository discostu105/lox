//! Calendar entries from source (#211): `CalendarEntry` objects of the
//! "Betriebszeiten" folder, which switch an operating mode on dates.
//! `switches:` names the mode (#326).

use loxone_config::logic::{CompileOptions, ModeSpellings, Module, adopt_one, compile};
use loxone_config::uuid::parse_serial;
use loxone_config::{Lockfile, LoxoneDoc};

/// The house's folder with its heating period, as Loxone Config 17.2
/// writes it, and the modes it can switch: the holiday mode, the heating
/// period and a user mode.
fn base() -> LoxoneDoc {
    LoxoneDoc::parse(
        "<ControlList Version=\"1\" NextObj=\"50\" NextConst=\"1\" NextNote=\"1\" NextMem=\"1\">\r\n\
         \t<C Type=\"Document\" U=\"00000001-0000-0000-ffff000000000001\" ConfigVersion=\"17020828\">\r\n\
         \t\t<C Type=\"ModeCaption\" U=\"00000005-0000-0000-ffff000000000001\" Title=\"Betriebsmodi\">\r\n\
         \t\t\t<C Type=\"Mode\" V=\"178\" U=\"00000000-0000-0001-1500000000000000\" Title=\"Feiertag\" Nio=\"2\" WF=\"16384\">\r\n\
         \t\t\t\t<Co K=\"I\" U=\"00000010-0000-0001-00ff000000000001\"/>\r\n\
         \t\t\t\t<Co K=\"Q\" U=\"00000010-0000-0001-01ff000000000001\"/>\r\n\
         \t\t\t</C>\r\n\
         \t\t\t<C Type=\"Mode\" V=\"178\" U=\"00000000-0000-000b-1500000000000000\" Title=\"Heizperiode\" Nio=\"2\" WF=\"16384\" Mode=\"10\">\r\n\
         \t\t\t\t<Co K=\"I\" U=\"00000010-0000-000b-00ff000000000001\"/>\r\n\
         \t\t\t\t<Co K=\"Q\" U=\"00000010-0000-000b-01ff000000000001\"/>\r\n\
         \t\t\t</C>\r\n\
         \t\t\t<C Type=\"Mode\" V=\"178\" U=\"00000006-0000-0000-ffff000000000001\" Title=\"Urlaub Süd\" Nio=\"2\" WF=\"16384\" Mode=\"-1\">\r\n\
         \t\t\t\t<Co K=\"I\" U=\"00000006-0000-0001-00ff000000000001\"/>\r\n\
         \t\t\t\t<Co K=\"Q\" U=\"00000006-0000-0002-01ff000000000001\"/>\r\n\
         \t\t\t</C>\r\n\
         \t\t</C>\r\n\
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

fn build(src: &str) -> Result<String, String> {
    let m = Module::parse(src).map_err(|e| e.to_string())?;
    compile(&base(), &m, &mut Lockfile::new(), &opts())
        .map(|d| text(&d))
        .map_err(|e| e.to_string())
}

#[test]
fn a_minted_entry_is_shaped_like_loxone_configs() {
    let out = build(
        "ferien = CalendarEntry(\"Sommerferien\", switches: vacation, Mode: 3, From: 6393, To: 6456)\n\
         ostern = CalendarEntry(\"Ostermontag\", Mode: 1, EasterOffset: 1)\n\
         frei = CalendarEntry(\"Frei\", switches: holiday, Month: 5, Day: 1)\n",
    )
    .unwrap();
    // In the folder, after the heating period, attributes in the save's
    // order (the corpus's period and Easter entries).
    let ferien = out.find("Title=\"Sommerferien\"").unwrap();
    assert!(
        ferien
            > out
                .find("Title=\"Heizperiode\" Nio=\"2\" WF=\"16384\" CalType")
                .unwrap()
    );
    assert!(ferien < out.find("Type=\"Page\"").unwrap());
    assert!(
        out.contains(
            "Title=\"Sommerferien\" Nio=\"2\" WF=\"16384\" CalType=\"1\" Mode=\"3\" Month=\"1\" \
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
    // The holiday mode is 0: no attribute.
    assert!(
        out.contains("Title=\"Frei\" Nio=\"2\" WF=\"16384\" Month=\"5\" Day=\"1\""),
        "{out}"
    );
    assert_eq!(out.matches("<Co K=\"I\" U=\"").count(), 3 + 4, "{out}");
}

#[test]
fn a_user_mode_is_named_by_slug_or_title() {
    // A mode of the config the module does not name: its title.
    let out = build("sued = CalendarEntry(\"Süd\", switches: \"Urlaub Süd\", Month: 8, Day: 1)\n")
        .unwrap();
    assert!(out.contains("Title=\"Süd\" Nio=\"2\" WF=\"16384\" CalType=\"-1\" Month=\"8\""));
    // An extern of it: its slug.
    let out = build(
        "extern urlaub_sued = Mode(title: \"Urlaub Süd\")\n\
         sued = CalendarEntry(\"Süd\", switches: urlaub_sued, Month: 8, Day: 1)\n",
    )
    .unwrap();
    assert!(out.contains("CalType=\"-1\" Month=\"8\""));
    // A mode minted in the same compile: the number it gets.
    let out = build(
        "party = Mode(\"Party\")\n\
         fest = CalendarEntry(\"Fest\", switches: party, Month: 6, Day: 21)\n",
    )
    .unwrap();
    assert!(
        out.contains("Title=\"Party\" Nio=\"2\" WF=\"147456\" Mode=\"-2\""),
        "{out}"
    );
    assert!(
        out.contains("Title=\"Fest\" Nio=\"2\" WF=\"16384\" CalType=\"-2\""),
        "{out}"
    );
}

#[test]
fn each_mode_has_one_spelling() {
    // The raw number: check names the day word, compile the slug or title.
    let err = build("h = CalendarEntry(\"H\", CalType: 10, Mode: 4)\n").unwrap_err();
    assert!(
        err.contains("Write `switches: heating` instead of `CalType: 10`"),
        "{err}"
    );
    let m = Module::parse("s = CalendarEntry(\"S\", CalType: -1, Month: 8, Day: 1)\n").unwrap();
    let err = compile(&base(), &m, &mut Lockfile::new(), &opts())
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("Write `switches: \"Urlaub Süd\"` instead of `CalType: -1`"),
        "{err}"
    );
    let m = Module::parse(
        "extern urlaub_sued = Mode(title: \"Urlaub Süd\")\n\
         s = CalendarEntry(\"S\", CalType: -1, Month: 8, Day: 1)\n",
    )
    .unwrap();
    let err = compile(&base(), &m, &mut Lockfile::new(), &opts())
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("Write `switches: urlaub_sued` instead of `CalType: -1`"),
        "{err}"
    );
    // A mode the module names: its slug; a built-in mode: its day word.
    let err = build(
        "party = Mode(\"Party\")\n\
         fest = CalendarEntry(\"Fest\", switches: \"Party\")\n",
    )
    .unwrap_err();
    assert!(
        err.contains("Write `switches: party` instead of `switches: \"Party\"`"),
        "{err}"
    );
    let err = build("h = CalendarEntry(\"H\", switches: \"Heizperiode\")\n").unwrap_err();
    assert!(
        err.contains("Write `switches: heating` instead of `switches: \"Heizperiode\"`"),
        "{err}"
    );
    let err = build(
        "extern hp = Mode(uuid: \"00000000-0000-000b-1500000000000000\")\n\
         h = CalendarEntry(\"H\", switches: hp)\n",
    )
    .unwrap_err();
    assert!(
        err.contains("Write `switches: heating` instead of `switches: hp`"),
        "{err}"
    );
    // The date kind's name, and the spelling that reads like it.
    let err = build("h = CalendarEntry(\"H\", mode: heating)\n").unwrap_err();
    assert!(
        err.contains("Write `switches: heating` instead of `mode: heating`"),
        "{err}"
    );
    let err = build("h = CalendarEntry(\"H\", Mode: heating)\n").unwrap_err();
    assert!(
        err.contains("Write `switches: heating` instead of `Mode: heating`"),
        "{err}"
    );
    // One mode, and a mode at all.
    let err = build("h = CalendarEntry(\"H\", switches: weekdays)\n").unwrap_err();
    assert!(err.contains("names several modes"), "{err}");
    let err = build("h = CalendarEntry(\"H\", switches: heatng)\n").unwrap_err();
    assert!(err.contains("did you mean `heating`?"), "{err}");
    let err = build("h = CalendarEntry(\"H\", switches: \"Partymodus\")\n").unwrap_err();
    assert!(
        err.contains("no operating mode titled \"Partymodus\""),
        "{err}"
    );
    let err = build("a = And(switches: heating)\n").unwrap_err();
    assert!(err.contains("is no calendar entry"), "{err}");
}

#[test]
fn fmt_writes_a_built_in_number_as_its_day_word() {
    let mut m = Module::parse_fragment(
        "h = CalendarEntry(\"H\", CalType: 10, Mode: 4)\n\
         s = CalendarEntry(\"S\", CalType: -1)\n\
         party = Mode(\"Party\")\n\
         f = CalendarEntry(\"F\", switches: \"Party\")\n",
    )
    .unwrap();
    ModeSpellings::of([&m]).apply(&mut m);
    assert_eq!(
        m.to_text(),
        "h = CalendarEntry(\"H\", switches: heating, Mode: 4)\n\
         s = CalendarEntry(\"S\", CalType: -1)\n\
         party = Mode(\"Party\")\n\
         f = CalendarEntry(\"F\", switches: party)\n"
    );
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
        "heizperiode = CalendarEntry(\n\t\"Heizperiode\",\n\tswitches: heating,\n\tMode: 4,\n\
         \tMonth: 10,\n\tDay: 1,\n\tMonth2: 4,\n\tDay2: 30,\n\tWeekDayInMonth: 1,\n)\n"
    );
    // Unchanged: the same bytes.
    let same = compile(&base(), &m, &mut lock.clone(), &opts()).unwrap();
    assert_eq!(text(&same), text(&base()));
    // A shorter heating period.
    let edited = Module::parse(&src.replace("\tDay2: 30,", "\tDay2: 15,")).unwrap();
    let out = text(&compile(&base(), &edited, &mut lock.clone(), &opts()).unwrap());
    assert_eq!(out, text(&base()).replace("Day2=\"30\"", "Day2=\"15\""));
    // Another mode, and the holiday mode (no attribute).
    let edited = Module::parse(&src.replace("heating", "\"Urlaub Süd\"")).unwrap();
    let out = text(&compile(&base(), &edited, &mut lock.clone(), &opts()).unwrap());
    assert_eq!(
        out,
        text(&base()).replace("CalType=\"10\"", "CalType=\"-1\"")
    );
    let edited = Module::parse(&src.replace("heating", "holiday")).unwrap();
    let out = text(&compile(&base(), &edited, &mut lock, &opts()).unwrap());
    assert_eq!(out, text(&base()).replace(" CalType=\"10\"", ""));
}
