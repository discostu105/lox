//! Automatic Designer rules from source (D57): the `rule` declaration,
//! its lift from a config, and the compile that writes `APC=` only where
//! the source changes the rule.
//!
//! The fixture is synthetic: a page with a push button `Pumpe`, a pulse
//! button `Abdeckung`, a switch `Licht`, and two rules shaped like the app
//! writes them — `Pumpe Ein` (active, weekdays in the `arrValues` form,
//! 09:00, `on` to the pump) and `Pumpe Aus` (inactive, 17:00, `off`).

use lxir::ir::{CompileOptions, DecompileOptions, Module, adopt, compile, decompile};
use lxir::uuid::parse_serial;
use lxir::{Lockfile, LoxoneDoc};

const PUMP: &str = "40000001-0000-0010-ffff504f94112233";
const COVER: &str = "40000003-0000-0030-ffff504f94112233";
const LIGHT: &str = "40000004-0000-0040-ffff504f94112233";
const RULE_ON: &str = "40000002-0000-0020-ffff504f94112233";
const RULE_OFF: &str = "40000005-0000-0050-ffff504f94112233";

fn esc(json: &str) -> String {
    json.replace('"', "&quot;")
}

fn rule_on_apc() -> String {
    esc(&format!(
        r#"{{"type":1,"id":"{RULE_ON}","active":true,"activeIfValid":false,"name":"Pumpe Ein","description":"","operator":3,"invalid":false,"eventGroups":[{{"operator":1,"events":[{{"type":3,"descID":270001,"id":270,"value":9999999999,"operator":1,"forceEdge":false,"arrValues":[0,1,2,3,4]}},{{"type":5,"descID":272001,"id":272,"value":540,"operator":1,"forceEdge":false}}]}}],"actions":[{{"type":0,"descID":342101,"id":"{PUMP}","command":"on"}}]}}"#
    ))
}

fn rule_off_apc() -> String {
    esc(&format!(
        r#"{{"type":1,"id":"{RULE_OFF}","active":false,"activeIfValid":false,"name":"Pumpe Aus","description":"","operator":3,"invalid":false,"eventGroups":[{{"operator":1,"events":[{{"type":5,"descID":272001,"id":272,"value":1020,"operator":1,"forceEdge":false}}]}}],"actions":[{{"type":0,"descID":342102,"id":"{PUMP}","command":"off"}}]}}"#
    ))
}

fn fixture_with(on_apc: &str, off_apc: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\r\n\
         <ControlList Version=\"1\" NextObj=\"9\" NextConst=\"1\" NextNote=\"1\" NextMem=\"1\">\r\n\
         \t<C Type=\"Document\" V=\"175\" U=\"40000000-0000-0001-ffff504f94112233\">\r\n\
         \t\t<C Type=\"Page\" V=\"175\" U=\"40000000-0000-0002-ffff504f94112233\" Title=\"Automatik-Regeln\" SpNo=\"1\">\r\n\
         \t\t\t<C Type=\"PushButton\" V=\"175\" U=\"{PUMP}\" Title=\"Pumpe\">\r\n\
         \t\t\t\t<Co K=\"Q\" U=\"40000001-0000-0011-01ffaaaaaaaa0001\"/>\r\n\
         \t\t\t</C>\r\n\
         \t\t\t<C Type=\"PButtonT\" V=\"175\" U=\"{COVER}\" Title=\"Abdeckung\">\r\n\
         \t\t\t\t<Co K=\"Q\" U=\"40000003-0000-0031-01ffaaaaaaaa0003\"/>\r\n\
         \t\t\t</C>\r\n\
         \t\t\t<C Type=\"Switch\" V=\"175\" U=\"{LIGHT}\" Title=\"Licht\">\r\n\
         \t\t\t\t<Co K=\"Q\" U=\"40000004-0000-0041-01ffaaaaaaaa0004\"/>\r\n\
         \t\t\t</C>\r\n\
         \t\t\t<C Type=\"AutopilotRule\" V=\"175\" U=\"{RULE_ON}\" Title=\"Pumpe Ein\" Nio=\"1\" APC=\"{on_apc}\" STA=\"1\">\r\n\
         \t\t\t\t<Co K=\"Q\" U=\"40000002-0000-0021-00ffaaaaaaaa0002\"/>\r\n\
         \t\t\t</C>\r\n\
         \t\t\t<C Type=\"AutopilotRule\" V=\"175\" U=\"{RULE_OFF}\" Title=\"Pumpe Aus\" Nio=\"1\" APC=\"{off_apc}\">\r\n\
         \t\t\t\t<Co K=\"Q\" U=\"40000005-0000-0051-00ffaaaaaaaa0005\"/>\r\n\
         \t\t\t</C>\r\n\
         \t\t</C>\r\n\
         \t</C>\r\n\
         </ControlList>\r\n"
    )
}

fn fixture() -> String {
    fixture_with(&rule_on_apc(), &rule_off_apc())
}

fn parse(xml: &str) -> LoxoneDoc {
    LoxoneDoc::parse(xml.as_bytes()).unwrap()
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

const EXTERNS: &str = "extern pumpe = PushButton(title: \"Pumpe\")\n\
                       extern abdeckung = PButtonT(title: \"Abdeckung\")\n\
                       extern licht = Switch(title: \"Licht\")\n\
                       extern pumpe_ein = AutopilotRule(title: \"Pumpe Ein\")\n\
                       extern pumpe_aus = AutopilotRule(title: \"Pumpe Aus\")\n";

/// The fixture compiled with `rules` (declarations) after the externs.
fn compile_rules(base: &str, rules: &str) -> lxir::Result<String> {
    let m = Module::parse(&format!("{EXTERNS}\n{rules}"))?;
    let out = compile(&parse(base), &m, &mut Lockfile::new(), &opts())?;
    Ok(String::from_utf8(out.to_bytes()).unwrap())
}

/// The opening `<C …>` tag of the element with `uuid`.
fn tag_of(xml: &str, uuid: &str) -> String {
    let at = xml.find(&format!("U=\"{uuid}\"")).unwrap();
    let open = xml[..at].rfind("<C ").unwrap();
    let end = at + xml[at..].find('>').unwrap();
    xml[open..=end].to_string()
}

const AS_READ: &str = "rule pumpe_ein\n\
                       \tactive\n\
                       \twhen weekday weekdays\n\
                       \twhen time 09:00\n\
                       \tthen pumpe \"on\"\n\
                       end\n\
                       \n\
                       rule pumpe_aus\n\
                       \tinactive\n\
                       \twhen time 17:00\n\
                       \tthen pumpe \"off\"\n\
                       end\n";

#[test]
fn rule_declarations_are_canonical() {
    let src = format!(
        "{EXTERNS}\n\
         rule pumpe_ein # morning\n\
         \t# on weekdays only\n\
         \tactive\n\
         \twhen weekday mon tue wed # three days\n\
         \twhen time 9:05\n\
         \tthen pumpe \"on\"\n\
         end # pump\n"
    );
    let m = Module::parse(&src).unwrap();
    // `9:05` prints as `09:05`; everything else round-trips.
    assert_eq!(m.to_text(), src.replace("9:05", "09:05"));
    let decl = m.rule_decls().next().unwrap();
    let lines: Vec<String> = decl.stmts().map(|s| s.kind.to_string()).collect();
    assert_eq!(
        lines,
        [
            "active",
            "when weekday mon tue wed",
            "when time 09:05",
            "then pumpe \"on\""
        ]
    );
    // `rule` names a declaration only in front of a slug.
    let m = Module::parse(
        "extern rule = Switch(title: \"Regel\")\nlatch = Switch(On: rule.Q)\nrule.On = 1\n",
    )
    .unwrap();
    assert_eq!(m.externs().next().unwrap().slug, "rule");
}

#[test]
fn rule_declarations_are_checked() {
    let err = |body: &str| {
        Module::parse(&format!("{EXTERNS}\n{body}"))
            .unwrap_err()
            .to_string()
    };
    let has = |body: &str, want: &str| {
        let e = err(body);
        assert!(e.contains(want), "{body:?}: {e}");
    };
    has(
        "rule pumpe_ein\n\twhen time 9:00\n\tthen pumpe \"on\"\nend\n",
        "active",
    );
    has(
        "rule pumpe_ein\n\tactive\n\tthen pumpe \"on\"\nend\n",
        "when",
    );
    has("rule pumpe_ein\n\tactive\n\twhen time 9:00\nend\n", "then");
    has(
        "rule pumpe_ein\n\tactive\n\twhen time 24:00\n\tthen pumpe \"on\"\nend\n",
        "not a time of day",
    );
    has(
        "rule pumpe_ein\n\tactive\n\twhen weekday monday\n\twhen time 9:00\n\tthen pumpe \"on\"\nend\n",
        "monday",
    );
    has(
        "rule pumpe_ein\n\tactive\n\twhen weekday mon weekdays\n\twhen time 9:00\n\tthen pumpe \"on\"\nend\n",
        "mon",
    );
    has(
        "rule pumpe_ein\n\tactive\n\twhen sunset\n\tthen pumpe \"on\"\nend\n",
        "when time",
    );
    has(
        "rule pumpe\n\tactive\n\twhen time 9:00\n\tthen pumpe \"on\"\nend\n",
        "AutopilotRule",
    );
    has(
        "rule pumpe_ein\n\tactive\n\twhen time 9:00\n\tthen nirgends \"on\"\nend\n",
        "nirgends",
    );
    has(
        "rule pumpe_ein\n\tactive\n\twhen time 9:00\n\tthen pumpe \"on\"\n",
        "unclosed `rule pumpe_ein`",
    );
}

#[test]
fn decompile_lifts_rules_and_adopt_compiles_them_back_byte_identical() {
    let base = fixture();
    let (m, _) = decompile(&parse(&base), &DecompileOptions::default()).unwrap();
    let text = m.to_text();
    assert!(text.contains(AS_READ), "{text}");

    let (m, mut lock, _) = adopt(&parse(&base)).unwrap();
    assert_eq!(m.rule_decls().count(), 2);
    let out = compile(&parse(&base), &m, &mut lock, &opts()).unwrap();
    assert_eq!(String::from_utf8(out.to_bytes()).unwrap(), base);

    // Stated by hand, as read: still no write.
    assert_eq!(compile_rules(&base, AS_READ).unwrap(), base);
}

#[test]
fn a_changed_rule_writes_only_what_the_source_states() {
    let base = fixture();
    let src = AS_READ
        .replace(
            "\tactive\n\twhen weekday weekdays",
            "\tinactive\n\twhen weekday sat sun",
        )
        .replace(
            "\tinactive\n\twhen time 17:00",
            "\tactive\n\twhen time 17:30",
        );
    let out = compile_rules(&base, &src).unwrap();

    // Deactivated: `active` false, `STA` gone, the rest of the JSON kept.
    let on = tag_of(&out, RULE_ON);
    let want = rule_on_apc()
        .replace("&quot;active&quot;:true", "&quot;active&quot;:false")
        .replace("[0,1,2,3,4]", "[5,6]");
    assert!(on.contains(&format!("APC=\"{want}\">")), "{on}");
    assert!(!on.contains("STA="), "{on}");

    // Activated: `STA="1"` right after `APC=`.
    let off = tag_of(&out, RULE_OFF);
    let want = rule_off_apc()
        .replace("&quot;active&quot;:false", "&quot;active&quot;:true")
        .replace("&quot;value&quot;:1020", "&quot;value&quot;:1050");
    assert!(off.contains(&format!("APC=\"{want}\" STA=\"1\">")), "{off}");

    // Stating the old rules again restores the original bytes.
    assert_eq!(compile_rules(&out, AS_READ).unwrap(), base);
}

#[test]
fn new_conditions_and_actions_are_built_from_evidence() {
    let base = fixture();
    // A weekday condition where there was none, a pulse to a PButtonT
    // (corpus table), and `on` to the pump copied from `Pumpe Ein`.
    let src = AS_READ.replace(
        "rule pumpe_aus\n\tinactive\n\twhen time 17:00\n\tthen pumpe \"off\"\n",
        "rule pumpe_aus\n\tinactive\n\twhen time 17:00\n\twhen weekday wed\n\
         \tthen abdeckung \"pulse\"\n\tthen pumpe \"on\"\n",
    );
    let out = compile_rules(&base, &src).unwrap();
    let off = tag_of(&out, RULE_OFF);
    let json = |s: &str| esc(s);
    assert!(
        off.contains(&json(
            r#"{"type":3,"descID":270001,"id":270,"value":9999999999,"operator":1,"forceEdge":false,"arrValues":[2]}"#
        )),
        "{off}"
    );
    assert!(
        off.contains(&json(&format!(
            r#"{{"type":0,"descID":493103,"id":"{COVER}","command":"pulse"}}"#
        ))),
        "{off}"
    );
    assert!(
        off.contains(&json(&format!(
            r#"{{"type":0,"descID":342101,"id":"{PUMP}","command":"on"}}"#
        ))),
        "{off}"
    );
    // The result reads back as the source.
    let (m, _) = decompile(&parse(&out), &DecompileOptions::default()).unwrap();
    assert!(
        m.to_text().contains(
            "rule pumpe_aus\n\tinactive\n\twhen time 17:00\n\twhen weekday wed\n\
             \tthen abdeckung \"pulse\"\n\tthen pumpe \"on\"\nend\n"
        ),
        "{}",
        m.to_text()
    );
}

#[test]
fn unknown_actions_and_shapes_are_refused() {
    let base = fixture();
    // No action code is known for a Switch.
    let src = AS_READ.replace("then pumpe \"off\"", "then licht \"on\"");
    let e = compile_rules(&base, &src).unwrap_err().to_string();
    assert!(e.contains("descID") && e.contains("Switch"), "{e}");

    // A rule with a condition lxir does not know is neither lifted nor
    // written.
    let odd = rule_off_apc().replace(
        "&quot;type&quot;:5,&quot;descID&quot;:272001,&quot;id&quot;:272",
        "&quot;type&quot;:7,&quot;descID&quot;:271001,&quot;id&quot;:271",
    );
    let base = fixture_with(&rule_on_apc(), &odd);
    let (m, _) = decompile(&parse(&base), &DecompileOptions::default()).unwrap();
    assert_eq!(m.rule_decls().count(), 1);
    let e = compile_rules(&base, AS_READ).unwrap_err().to_string();
    assert!(e.contains("`rule pumpe_aus`"), "{e}");
}
