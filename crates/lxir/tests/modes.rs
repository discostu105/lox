//! Operating modes from source (D69): minting a user-defined mode, and
//! schedule lines that name modes by title.

use lxir::ir::{CompileOptions, Module, compile};
use lxir::uuid::parse_serial;
use lxir::{Lockfile, LoxoneDoc};

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

fn element<'a>(doc: &'a LoxoneDoc, ty: &str, title: &str) -> &'a lxir::xml::Element {
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
	"Party" 18:00-24:00
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
