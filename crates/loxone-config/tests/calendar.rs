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
        "ferien = CalendarEntry(\"Sommerferien\", switches: vacation, from: 2026-07-04, to: 2026-09-05)\n\
         ostern = CalendarEntry(\"Ostermontag\", easter: 1)\n\
         frei = CalendarEntry(\"Frei\", switches: holiday, on: 05-01)\n",
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
    let out =
        build("sued = CalendarEntry(\"Süd\", switches: \"Urlaub Süd\", on: 08-01)\n").unwrap();
    assert!(out.contains("Title=\"Süd\" Nio=\"2\" WF=\"16384\" CalType=\"-1\" Month=\"8\""));
    // An extern of it: its slug.
    let out = build(
        "extern urlaub_sued = Mode(title: \"Urlaub Süd\")\n\
         sued = CalendarEntry(\"Süd\", switches: urlaub_sued, on: 08-01)\n",
    )
    .unwrap();
    assert!(out.contains("CalType=\"-1\" Month=\"8\""));
    // A mode minted in the same compile: the number it gets.
    let out = build(
        "party = Mode(\"Party\")\n\
         fest = CalendarEntry(\"Fest\", switches: party, on: 06-21)\n",
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
    let err = build("h = CalendarEntry(\"H\", CalType: 10, on: 01-01)\n").unwrap_err();
    assert!(
        err.contains("Write `switches: heating` instead of `CalType: 10`"),
        "{err}"
    );
    let m = Module::parse("s = CalendarEntry(\"S\", CalType: -1, on: 08-01)\n").unwrap();
    let err = compile(&base(), &m, &mut Lockfile::new(), &opts())
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("Write `switches: \"Urlaub Süd\"` instead of `CalType: -1`"),
        "{err}"
    );
    let m = Module::parse(
        "extern urlaub_sued = Mode(title: \"Urlaub Süd\")\n\
         s = CalendarEntry(\"S\", CalType: -1, on: 08-01)\n",
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
        "heizperiode = CalendarEntry(\n\t\"Heizperiode\",\n\tswitches: heating,\n\
         \tfrom: 10-01,\n\tto: 04-30,\n)\n"
    );
    // Unchanged: the same bytes.
    let same = compile(&base(), &m, &mut lock.clone(), &opts()).unwrap();
    assert_eq!(text(&same), text(&base()));
    // A shorter heating period.
    let edited = Module::parse(&src.replace("04-30", "04-15")).unwrap();
    let out = text(&compile(&base(), &edited, &mut lock.clone(), &opts()).unwrap());
    assert_eq!(out, text(&base()).replace("Day2=\"30\"", "Day2=\"15\""));
    // Another kind of date: `Mode=` and the attributes it reads; the
    // others stay, as in a save.
    let edited =
        Module::parse(&src.replace("\tfrom: 10-01,\n\tto: 04-30,", "\ton: 12-31,")).unwrap();
    let out = text(&compile(&base(), &edited, &mut lock.clone(), &opts()).unwrap());
    assert_eq!(
        out,
        text(&base()).replace(
            "Mode=\"4\" Month=\"10\" Day=\"1\"",
            "Month=\"12\" Day=\"31\""
        )
    );
    let edited = Module::parse(&src.replace(
        "\tfrom: 10-01,\n\tto: 04-30,",
        "\tfrom: 2026-07-04,\n\tto: 2026-09-05,",
    ))
    .unwrap();
    let out = text(&compile(&base(), &edited, &mut lock.clone(), &opts()).unwrap());
    assert!(
        out.contains(
            "CalType=\"10\" Mode=\"3\" Month=\"10\" Day=\"1\" Month2=\"4\" Day2=\"30\" \
             From=\"6393\" To=\"6456\" WeekDayInMonth=\"1\">"
        ),
        "{out}"
    );
    let edited =
        Module::parse(&src.replace("\tfrom: 10-01,\n\tto: 04-30,", "\teaster: -2,")).unwrap();
    let out = text(&compile(&base(), &edited, &mut lock.clone(), &opts()).unwrap());
    assert!(
        out.contains(
            "Mode=\"1\" Month=\"10\" Day=\"1\" Month2=\"4\" Day2=\"30\" EasterOffset=\"-2\""
        ),
        "{out}"
    );
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

#[test]
fn an_entry_whose_mode_is_gone_is_adopted_without_switches() {
    // A user mode deleted since (the corpus has one), and the cooling
    // mode, which this config lacks.
    let entry = |n: u8, cal_type: &str| {
        format!(
            "\t\t\t<C Type=\"CalendarEntry\" V=\"178\" U=\"0000000{n}-0000-0000-ffff000000000002\" \
             Title=\"E{n}\" Nio=\"2\" WF=\"16384\" CalType=\"{cal_type}\" Month=\"5\" Day=\"1\" \
             Month2=\"1\" Day2=\"1\" WeekDayInMonth=\"1\">\r\n\
             \t\t\t\t<Co K=\"I\" U=\"0000000{n}-0000-0001-00ff000000000002\"/>\r\n\
             \t\t\t\t<Co K=\"Q\" U=\"0000000{n}-0000-0002-01ff000000000002\"/>\r\n\
             \t\t\t</C>\r\n"
        )
    };
    let folder_end = "\t\t\t</C>\r\n\t\t</C>\r\n\t\t<C Type=\"Page\"";
    let xml = text(&base()).replacen(
        folder_end,
        &format!(
            "\t\t\t</C>\r\n{}{}\t\t</C>\r\n\t\t<C Type=\"Page\"",
            entry(7, "-7"),
            entry(8, "11")
        ),
        1,
    );
    let base = LoxoneDoc::parse(xml.as_bytes()).unwrap();
    for (n, cal_type) in [(7, "-7"), (8, "11")] {
        let mut lock = Lockfile::new();
        let adopted = adopt_one(
            &base,
            &format!("0000000{n}-0000-0000-ffff000000000002"),
            "e",
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
            format!(
                "e = CalendarEntry(\n\t\"E{n}\",\n\t# Its operating mode (CalType {cal_type}) is not \
                 in the config. Without `switches:` it stays; name a mode to switch that one.\n\
                 \ton: 05-01,\n)\n"
            )
        );
        // The source checks, and compiles to the same bytes.
        let parsed = Module::parse(&src).unwrap();
        parsed.validate().unwrap();
        let same = compile(&base, &parsed, &mut lock.clone(), &opts()).unwrap();
        assert_eq!(text(&same), text(&base));
        // Pointed at a mode the config has.
        let edited = Module::parse(
            &src.replace("\ton: 05-01,", "\tswitches: \"Urlaub Süd\",\n\ton: 05-01,"),
        )
        .unwrap();
        let out = text(&compile(&base, &edited, &mut lock, &opts()).unwrap());
        assert_eq!(
            out,
            text(&base).replace(
                &format!("Title=\"E{n}\" Nio=\"2\" WF=\"16384\" CalType=\"{cal_type}\""),
                &format!("Title=\"E{n}\" Nio=\"2\" WF=\"16384\" CalType=\"-1\"")
            )
        );
    }
}

/// The error a module gets from `loxl check`.
fn check_err(src: &str) -> String {
    Module::parse(src)
        .and_then(|m| m.validate())
        .unwrap_err()
        .to_string()
}

#[test]
fn the_stored_date_is_refused_with_the_date() {
    for (old, new) in [
        ("Month: 12, Day: 31", "on: 12-31"),
        ("Mode: 1, EasterOffset: 1", "easter: 1"),
        ("Mode: 1", "easter: 0"),
        (
            "Mode: 3, From: 6393, To: 6456",
            "from: 2026-07-04, to: 2026-09-05",
        ),
        (
            "Mode: 4, Month: 10, Day: 1, Month2: 4, Day2: 30, WeekDayInMonth: 1",
            "from: 10-01, to: 04-30",
        ),
    ] {
        let err = check_err(&format!("h = CalendarEntry(\"H\", {old})\n"));
        assert!(
            err.contains(&format!("Write `{new}` instead of `{old}`.")),
            "{old}: {err}"
        );
    }
    // A kind no entry has, and a day there is not.
    let err = check_err("h = CalendarEntry(\"H\", Mode: 2, Month: 1, Day: 1)\n");
    assert!(err.contains("`Mode` 2 is no kind of date"), "{err}");
    let err = check_err("h = CalendarEntry(\"H\", Month: 2, Day: 30)\n");
    assert!(err.contains("no day of the year"), "{err}");
    let err = check_err("h = CalendarEntry(\"H\", Mode: 4, Month: 10)\n");
    assert!(err.contains("it has no `Day`"), "{err}");
    let err = check_err("h = CalendarEntry(\"H\", on: 01-01, WeekDayInMonth: 1)\n");
    assert!(err.contains("Leave out `WeekDayInMonth: 1`"), "{err}");
    let err = check_err("h = CalendarEntry(\"H\", WeekDayInMonth: 1)\n");
    assert!(err.contains("leave out `WeekDayInMonth: 1`"), "{err}");
    let err = check_err("h = CalendarEntry(\"H\", WeekDayInMonth: 2, Month: 1, Day: 1)\n");
    assert!(err.contains("`WeekDayInMonth` is 2"), "{err}");
}

#[test]
fn a_date_is_one_of_four_kinds() {
    let ok = |args: &str| {
        Module::parse(&format!("h = CalendarEntry(\"H\", {args})\n"))
            .and_then(|m| m.validate())
            .map_err(|e| e.to_string())
    };
    for args in [
        "on: 02-29",
        "easter: 0",
        "easter: -48",
        "from: 2009-01-01, to: 2009-01-01",
        "from: 12-24, to: 01-06",
    ] {
        ok(args).unwrap_or_else(|e| panic!("{args}: {e}"));
    }
    for (args, msg) in [
        (
            "on: 2026-12-24",
            "write `from: 2026-12-24, to: 2026-12-24` instead of `on: 2026-12-24`",
        ),
        (
            "on: \"12-24\"",
            "Write `on: 12-24` instead of `on: \"12-24\"`",
        ),
        ("on: 12-32", "is no date"),
        ("on: 1-5", "is no date"),
        ("on: 5", "is no date"),
        ("from: 2026-07-04", "takes both `from:` and `to:`"),
        ("from: 07-04, to: 2026-09-05", "both are dates"),
        ("from: 2008-12-31, to: 2009-01-05", "2009-01-01 or later"),
        ("on: 01-01, easter: 1", "one date"),
        ("easter: \"Montag\"", "no number of days"),
    ] {
        let e = ok(args).unwrap_err();
        assert!(e.contains(msg), "{args}: {e}");
    }
    // Only a calendar entry reads a date there.
    assert!(build("a = And(on: 12-31)\n").is_err());
}

#[test]
fn fmt_writes_the_stored_date_as_a_date() {
    let mut m = Module::parse_fragment(
        "a = CalendarEntry(\"A\", Mode: 1, EasterOffset: 50, Month: 5, Day: 24)\n\
         b = CalendarEntry(\"B\", Mode: 4, Month: 5, Day: 1, Month2: 9, Day2: 30, WeekDayInMonth: 1)\n\
         c = CalendarEntry(\"C\", on: 01-06, WeekDayInMonth: 1)\n\
         e = CalendarEntry(\"E\", WeekDayInMonth: 1)\n\
         d = CalendarEntry(\"D\", Mode: 2, Day: 1)\n",
    )
    .unwrap();
    ModeSpellings::of([&m]).apply(&mut m);
    // The date where the numbers give one; an undocumented kind stays
    // for the check to refuse.
    assert_eq!(
        m.to_text(),
        "a = CalendarEntry(\"A\", easter: 50)\n\
         b = CalendarEntry(\"B\", from: 05-01, to: 09-30)\n\
         c = CalendarEntry(\"C\", on: 01-06, WeekDayInMonth: 1)\n\
         e = CalendarEntry(\"E\")\n\
         d = CalendarEntry(\"D\", Mode: 2, Day: 1)\n"
    );
}
