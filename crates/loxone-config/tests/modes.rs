//! Operating modes from source (D69): minting a user-defined mode, and
//! schedule lines that name modes by title.

use loxone_config::logic::{CompileOptions, Module, compile};
use loxone_config::uuid::parse_serial;
use loxone_config::{Lockfile, LoxoneDoc};

fn base() -> LoxoneDoc {
    let mut modes = String::new();
    let titles = [
        "Feiertag",
        "Urlaub",
        "Freier Tag",
        "Montag",
        "Dienstag",
        "Mittwoch",
        "Donnerstag",
        "Freitag",
        "Samstag",
        "Sonntag",
        "Heizperiode",
        "Kühlperiode",
    ];
    for (i, t) in titles.iter().enumerate() {
        let n = i + 1;
        let number = if n == 1 {
            String::new()
        } else {
            format!(" Mode=\"{}\"", n - 1)
        };
        modes.push_str(&format!(
            "\t\t\t<C Type=\"Mode\" V=\"178\" U=\"00000000-0000-{n:04x}-1500000000000000\" Title=\"{t}\" Nio=\"2\" WF=\"16384\"{number}>\r\n\
             \t\t\t\t<Co K=\"I\" U=\"00000010-0000-{n:04x}-00ff000000000001\"/>\r\n\
             \t\t\t\t<Co K=\"Q\" U=\"00000010-0000-{n:04x}-01ff000000000001\"/>\r\n\
             \t\t\t</C>\r\n"
        ));
    }
    let xml = format!(
        "<ControlList Version=\"1\" NextObj=\"50\">\r\n\
         \t<C Type=\"Document\" U=\"00000001-0000-0000-ffff000000000001\" ConfigVersion=\"17020828\">\r\n\
         \t\t<C Type=\"ModeCaption\" U=\"00000002-0000-0000-ffff000000000001\" Title=\"Betriebsmodi\">\r\n\
         {modes}\
         \t\t\t<C Type=\"Mode\" V=\"178\" U=\"00000003-0000-0000-ffff000000000001\" Title=\"Tiefschlaf\" Nio=\"2\" WF=\"16384\" Mode=\"-1\">\r\n\
         \t\t\t\t<Co K=\"I\" U=\"00000003-0000-0001-00ff000000000001\"/>\r\n\
         \t\t\t\t<Co K=\"Q\" U=\"00000003-0000-0002-01ff000000000001\"/>\r\n\
         \t\t\t</C>\r\n\
         \t\t</C>\r\n\
         \t\t<C Type=\"Page\" U=\"00000004-0000-0000-ffff000000000001\" Title=\"Logik\">\r\n\
         \t\t</C>\r\n\
         \t</C>\r\n\
         </ControlList>\r\n"
    );
    LoxoneDoc::parse(xml.as_bytes()).unwrap()
}

fn build(source: &str) -> Result<LoxoneDoc, String> {
    let opts = CompileOptions {
        machine: parse_serial("504F94112233").unwrap(),
        mint_time_unix: 1_767_225_600,
        page_title: Some("Logik".into()),
        allow_removals: false,
        accept_version: None,
        relayout: Vec::new(),
    };
    let module = Module::parse(source).map_err(|e| e.to_string())?;
    compile(&base(), &module, &mut Lockfile::new(), &opts).map_err(|e| e.to_string())
}

fn element<'a>(doc: &'a LoxoneDoc, ty: &str, title: &str) -> &'a loxone_config::xml::Element {
    let o = doc
        .objects()
        .into_iter()
        .find(|o| o.block_type == ty && o.title.as_deref() == Some(title))
        .unwrap();
    doc.element_at(&o.path).unwrap()
}

const SOURCE: &str = r#"
party = Mode("Party")

schedule abend
	weekdays 18:00-22:00
	party 18:00-24:00
	"Tiefschlaf" off
	heating 06:00-07:00
end

timer = DayTimer("Abendlicht")
timer.schedule = abend
licht = And(I1: party.Q)
"#;

#[test]
fn a_minted_mode_is_numbered_below_the_lowest_and_shaped_like_a_save() {
    let doc = build(SOURCE).unwrap();
    let mode = element(&doc, "Mode", "Party");
    assert_eq!(mode.attr("Mode"), Some("-2"));
    assert_eq!(mode.attr("WF"), Some("147456"));
    assert_eq!(
        mode.attr("Px"),
        None,
        "an object of the tree has no position"
    );
    let o = doc
        .objects()
        .into_iter()
        .find(|o| o.title.as_deref() == Some("Party"))
        .unwrap();
    let parent = doc.element_at(&o.path[..o.path.len() - 1]).unwrap();
    assert_eq!(parent.attr("Type"), Some("ModeCaption"));
}

#[test]
fn schedule_columns_follow_the_mode_numbers() {
    let doc = build(SOURCE).unwrap();
    let party = element(&doc, "Mode", "Party")
        .attr("U")
        .unwrap()
        .to_string();
    let timer = element(&doc, "DayTimer", "Abendlicht");
    let modes: Vec<&str> = timer.attr("Modes").unwrap().split(',').collect();
    assert_eq!(modes[0], party);
    assert_eq!(modes[1], "00000003-0000-0000-ffff000000000001");
    assert_eq!(modes[2], "00000000-0000-0004-1500000000000000");
    assert_eq!(modes[9], "00000000-0000-000b-1500000000000000");
    assert_eq!(modes.len(), 10);
    // Two columns before Monday; heating after Sunday does not count.
    assert_eq!(timer.attr("UserModes"), Some("2"));
}

#[test]
fn an_unknown_mode_title_is_refused_with_the_titles() {
    let err = build(
        "schedule s\n\t\"Partymodus\" 18:00-24:00\nend\ntimer = DayTimer(\"T\")\ntimer.schedule = s\n",
    )
    .unwrap_err();
    assert!(
        err.contains("no operating mode titled \"Partymodus\"") && err.contains("\"Tiefschlaf\""),
        "{err}"
    );
}

// --- One spelling per mode: day word, else slug, else title.

/// `SOURCE` with the schedule's party line as `line`.
fn with_party_line(line: &str) -> String {
    SOURCE.replace("\tparty 18:00-24:00\n", &format!("\t{line}\n"))
}

#[test]
fn a_declared_mode_is_named_by_its_slug() {
    // Check refuses the title of a mode the module declares, naming the
    // line as it should read.
    let err = Module::parse(&with_party_line("\"Party\" 18:00-24:00 # Fest"))
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("Write `party 18:00-24:00` instead of `\"Party\" 18:00-24:00`"),
        "{err}"
    );
    // An extern by UUID: only the compile knows its title.
    let by_uuid =
        format!("extern tief = Mode(uuid: \"00000003-0000-0000-ffff000000000001\")\n{SOURCE}");
    let err = build(&by_uuid).unwrap_err();
    assert!(
        err.contains("Write `tief off` instead of `\"Tiefschlaf\" off`"),
        "{err}"
    );
    // The slug compiles to the same bytes the title did.
    let fixed = build(&by_uuid.replace("\t\"Tiefschlaf\" off\n", "\ttief off\n")).unwrap();
    assert_eq!(fixed.to_bytes(), build(SOURCE).unwrap().to_bytes());
}

#[test]
fn a_builtin_mode_is_named_by_its_day_word() {
    let err = build(&with_party_line("\"Montag\" 18:00-24:00")).unwrap_err();
    assert!(
        err.contains("Write `mon 18:00-24:00` instead of `\"Montag\" 18:00-24:00`"),
        "{err}"
    );
    // An extern of a built-in mode: by UUID check knows it, by title the
    // compile.
    let by_uuid = format!(
        "extern montag = Mode(uuid: \"00000000-0000-0004-1500000000000000\")\n{}",
        with_party_line("montag 18:00-24:00")
    );
    let err = Module::parse(&by_uuid).unwrap_err().to_string();
    assert!(
        err.contains("Write `mon 18:00-24:00` instead of `montag 18:00-24:00`"),
        "{err}"
    );
    let by_title = by_uuid.replace(
        "uuid: \"00000000-0000-0004-1500000000000000\"",
        "title: \"Montag\"",
    );
    let err = build(&by_title).unwrap_err();
    assert!(
        err.contains("Write `mon 18:00-24:00` instead of `montag 18:00-24:00`"),
        "{err}"
    );
}

#[test]
fn a_day_slug_must_name_a_mode() {
    let err = Module::parse(&with_party_line("mondy 18:00-24:00"))
        .unwrap_err()
        .to_string();
    assert!(err.contains("unknown day `mondy`"), "{err}");
    let err = Module::parse(&with_party_line("pary 18:00-24:00"))
        .unwrap_err()
        .to_string();
    assert!(err.contains("did you mean `party`?"), "{err}");
    let err = Module::parse(&with_party_line("timer 18:00-24:00"))
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("`timer` is a `DayTimer`, not a `Mode`"),
        "{err}"
    );
}

#[test]
fn fmt_writes_the_one_spelling() {
    let page = Module::parse_fragment(
        "party = Mode(\"Party\")\n\
         extern montag = Mode(uuid: \"00000000-0000-0004-1500000000000000\")\n",
    )
    .unwrap();
    let mut other = Module::parse_fragment(
        "schedule s\n\t\"Party\" montag 18:00-24:00\n\t\"Tiefschlaf\" off\nend\n\n\
         rule r\n\tactive\n\twhen mode \"Party\"\n\tthen mode \"Tiefschlaf\" on\nend\n",
    )
    .unwrap();
    loxone_config::logic::ModeSpellings::of([&page, &other]).apply(&mut other);
    assert_eq!(
        other.to_text(),
        "schedule s\n\tparty mon 18:00-24:00\n\t\"Tiefschlaf\" off\nend\n\n\
         rule r\n\tactive\n\twhen mode party\n\tthen mode \"Tiefschlaf\" on\nend\n"
    );
}
