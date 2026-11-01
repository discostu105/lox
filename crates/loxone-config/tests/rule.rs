//! Automatic Designer rules from source (D57): the `rule` declaration,
//! its lift from a config, and the compile that writes `APC=` only where
//! the source changes the rule.
//!
//! The fixture is synthetic: a page with a push button `Pumpe`, a pulse
//! button `Abdeckung`, a switch `Licht`, and two rules shaped like the app
//! writes them — `Pumpe Ein` (active, weekdays in the `arrValues` form,
//! 09:00, `on` to the pump) and `Pumpe Aus` (inactive, 17:00, `off`).

use loxone_config::logic::{
    CompileOptions, DecompileOptions, Item, Module, adopt, adopt_one, compile, decompile,
};
use loxone_config::uuid::parse_serial;
use loxone_config::{Lockfile, LoxoneDoc};

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
        r#"{{"type":1,"id":"{RULE_ON}","active":true,"activeIfValid":true,"name":"Pumpe Ein","description":"","operator":3,"invalid":false,"eventGroups":[{{"operator":1,"events":[{{"type":3,"descID":270001,"id":270,"value":9999999999,"operator":1,"forceEdge":false,"arrValues":[0,1,2,3,4]}},{{"type":5,"descID":272001,"id":272,"value":540,"operator":1,"forceEdge":false}}]}}],"actions":[{{"type":0,"descID":342101,"id":"{PUMP}","command":"on"}}]}}"#
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
         \t\t\t<C Type=\"PushButton\" V=\"175\" U=\"{PUMP}\" Title=\"Pumpe\" SpStates=\"40000001-0000-0013-ffffaaaaaaaa0001,40000001-0000-0014-ffffaaaaaaaa0001,40000001-0000-0015-ffffaaaaaaaa0001\">\r\n\
         \t\t\t\t<Co K=\"Reset\" U=\"40000001-0000-0012-00ffaaaaaaaa0001\"/>\r\n\
         \t\t\t\t<Co K=\"Q\" U=\"40000001-0000-0011-01ffaaaaaaaa0001\"/>\r\n\
         \t\t\t\t<IoData Visu=\"true\" Pr=\"40000000-0000-0003-ffff504f94112233\" Cr=\"40000000-0000-0004-ffff504f94112233\"/>\r\n\
         \t\t\t\t<Display Type=\"1\"/>\r\n\
         \t\t\t</C>\r\n\
         \t\t\t<C Type=\"PButtonT\" V=\"175\" U=\"{COVER}\" Title=\"Abdeckung\">\r\n\
         \t\t\t\t<Co K=\"Reset\" U=\"40000003-0000-0032-00ffaaaaaaaa0003\"/>\r\n\
         \t\t\t\t<Co K=\"Q\" U=\"40000003-0000-0031-01ffaaaaaaaa0003\"/>\r\n\
         \t\t\t\t<IoData Visu=\"true\" Pr=\"40000000-0000-0003-ffff504f94112233\" Cr=\"40000000-0000-0004-ffff504f94112233\"/>\r\n\
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
fn compile_rules(base: &str, rules: &str) -> loxone_config::Result<String> {
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
                       \tthen pumpe on\n\
                       end\n\
                       \n\
                       rule pumpe_aus\n\
                       \tinactive\n\
                       \twhen time 17:00\n\
                       \tthen pumpe off\n\
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
         \tthen pumpe on\n\
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
            "then pumpe on"
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
        "rule pumpe_ein\n\twhen time 9:00\n\tthen pumpe on\nend\n",
        "active",
    );
    has("rule pumpe_ein\n\tactive\n\tthen pumpe on\nend\n", "when");
    has("rule pumpe_ein\n\tactive\n\twhen time 9:00\nend\n", "then");
    has(
        "rule pumpe_ein\n\tactive\n\twhen time 24:00\n\tthen pumpe on\nend\n",
        "not a time of day",
    );
    has(
        "rule pumpe_ein\n\tactive\n\twhen weekday monday\n\twhen time 9:00\n\tthen pumpe on\nend\n",
        "monday",
    );
    has(
        "rule pumpe_ein\n\tactive\n\twhen weekday mon weekdays\n\twhen time 9:00\n\tthen pumpe on\nend\n",
        "mon",
    );
    has(
        "rule pumpe_ein\n\tactive\n\twhen rain\n\tthen pumpe on\nend\n",
        "when time",
    );
    has(
        "rule pumpe\n\tactive\n\twhen time 9:00\n\tthen pumpe on\nend\n",
        "AutopilotRule",
    );
    has(
        "rule pumpe_ein\n\tactive\n\twhen time 9:00\n\tthen nirgends on\nend\n",
        "nirgends",
    );
    has(
        "rule pumpe_ein\n\tactive\n\twhen time 9:00\n\tthen pumpe on\n",
        "unclosed `rule pumpe_ein`",
    );
}

#[test]
fn decompile_lifts_rules_and_adopt_compiles_them_back_byte_identical() {
    let base = fixture();
    let (m, _) = decompile(&parse(&base), &DecompileOptions::default()).unwrap();
    let text = m.to_text();
    // Each rule has a name of its own: declared by it, no extern (#192).
    let named = AS_READ
        .replace("rule pumpe_ein\n", "rule pumpe_ein \"Pumpe Ein\"\n")
        .replace("rule pumpe_aus\n", "rule pumpe_aus \"Pumpe Aus\"\n");
    assert!(text.contains(&named), "{text}");
    assert!(!text.contains("AutopilotRule"), "{text}");

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
        .replace(
            "&quot;activeIfValid&quot;:true",
            "&quot;activeIfValid&quot;:false",
        )
        .replace("[0,1,2,3,4]", "[5,6]");
    assert!(on.contains(&format!("APC=\"{want}\">")), "{on}");
    assert!(!on.contains("STA="), "{on}");

    // Activated: `STA="1"` right after `APC=`.
    let off = tag_of(&out, RULE_OFF);
    let want = rule_off_apc()
        .replace("&quot;active&quot;:false", "&quot;active&quot;:true")
        .replace(
            "&quot;activeIfValid&quot;:false",
            "&quot;activeIfValid&quot;:true",
        )
        .replace("&quot;value&quot;:1020", "&quot;value&quot;:1050");
    assert!(off.contains(&format!("APC=\"{want}\" STA=\"1\">")), "{off}");

    // Stating the old rules again restores the original bytes.
    assert_eq!(compile_rules(&out, AS_READ).unwrap(), base);
}

#[test]
fn active_alone_is_not_active() {
    // `active` without `activeIfValid` never ran (live): the rule reads
    // as inactive, and stating that writes nothing.
    let half = rule_on_apc().replace(
        "&quot;activeIfValid&quot;:true",
        "&quot;activeIfValid&quot;:false",
    );
    let base = fixture_with(&half, &rule_off_apc());
    let (m, _) = decompile(&parse(&base), &DecompileOptions::default()).unwrap();
    assert!(
        m.to_text()
            .contains("rule pumpe_ein \"Pumpe Ein\"\n\tinactive\n"),
        "{}",
        m.to_text()
    );
    let inactive = AS_READ.replacen("\tactive\n", "\tinactive\n", 1);
    assert_eq!(compile_rules(&base, &inactive).unwrap(), base);
    // Activating it sets both flags.
    let out = compile_rules(&base, AS_READ).unwrap();
    assert!(tag_of(&out, RULE_ON).contains(&rule_on_apc()));
}

#[test]
fn new_conditions_and_actions_are_built_from_evidence() {
    let base = fixture();
    // A weekday condition where there was none, a pulse to a PButtonT
    // (corpus table), and `on` to the pump copied from `Pumpe Ein`.
    let src = AS_READ.replace(
        "rule pumpe_aus\n\tinactive\n\twhen time 17:00\n\tthen pumpe off\n",
        "rule pumpe_aus\n\tinactive\n\twhen time 17:00\n\twhen weekday wed\n\
         \tthen abdeckung pulse\n\tthen pumpe on\n",
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
            "rule pumpe_aus \"Pumpe Aus\"\n\tinactive\n\twhen time 17:00\n\twhen weekday wed\n\
             \tthen abdeckung pulse\n\tthen pumpe on\nend\n"
        ),
        "{}",
        m.to_text()
    );
}

#[test]
fn unknown_actions_and_shapes_are_refused() {
    let base = fixture();
    // No action code is known for a Switch.
    let src = AS_READ.replace("then pumpe off", "then licht on");
    let e = compile_rules(&base, &src).unwrap_err().to_string();
    assert!(e.contains("descID") && e.contains("Switch"), "{e}");

    // A rule with a condition Lox Logic does not know is neither lifted nor
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

// --- Named rules (D60): `rule <slug> "Name"` finds or creates the rule.

/// The fixture with `Pumpe Aus` placed and filed like the house's rules:
/// a position, colour, flags and the `ref=` to the `AutoPilot` container.
fn placed_fixture() -> String {
    fixture().replace(
        &format!("U=\"{RULE_OFF}\" Title=\"Pumpe Aus\" Nio=\"1\""),
        &format!(
            "U=\"{RULE_OFF}\" Title=\"Pumpe Aus\" Px=\"5952\" Py=\"768\" Px2=\"8640\" \
             Py2=\"1656\" Cl=\"141,255,112\" Nio=\"7\" WF=\"16384\" \
             ref=\"40000000-0000-0003-ffff504f94112233\""
        ),
    )
}

const NEW_RULE: &str = "rule mittag \"Pumpe Mittag\"\n\
                        \tactive\n\
                        \twhen time 12:30\n\
                        \tthen pumpe on\n\
                        end\n";

fn compile_with(base: &str, src: &str, lock: &mut Lockfile) -> loxone_config::Result<String> {
    let m = Module::parse(&format!("{EXTERNS}\n{src}"))?;
    let out = compile(&parse(base), &m, lock, &opts())?;
    Ok(String::from_utf8(out.to_bytes()).unwrap())
}

#[test]
fn a_named_rule_is_created_next_to_the_others() {
    let base = placed_fixture();
    let mut lock = Lockfile::new();
    let out = compile_with(&base, NEW_RULE, &mut lock).unwrap();
    let uuid = lock.rules["mittag"].clone();
    assert!(uuid.ends_with("-ffff504f94112233"), "{uuid}");

    // Below the last rule, sized, coloured and filed like it; the payload
    // names the rule and holds what the source states.
    let tag = tag_of(&out, &uuid);
    for want in [
        "Type=\"AutopilotRule\" V=\"175\"",
        "Title=\"Pumpe Mittag\" Px=\"5952\" Py=\"1824\" Px2=\"8640\" Py2=\"2712\"",
        "Cl=\"141,255,112\" Nio=\"7\" WF=\"16384\"",
        "STA=\"1\" ref=\"40000000-0000-0003-ffff504f94112233\">",
    ] {
        assert!(tag.contains(want), "{want}: {tag}");
    }
    let apc = esc(&format!(
        r#"{{"type":1,"id":"{uuid}","active":true,"activeIfValid":true,"name":"Pumpe Mittag","description":"","operator":3,"invalid":false,"eventGroups":[{{"operator":1,"events":[{{"type":5,"descID":272001,"id":272,"value":750,"operator":1,"forceEdge":false}}]}}],"actions":[{{"type":0,"descID":342101,"id":"{PUMP}","command":"on"}}]}}"#
    ));
    assert!(tag.contains(&apc), "{tag}");

    // Seven connectors minted right before the rule, then `<IoData>`.
    let at = out.find(&tag).unwrap() + tag.len();
    let body = &out[at..at + out[at..].find("</C>").unwrap()];
    let seq = u32::from_str_radix(&uuid[14..18], 16).unwrap();
    for (i, k) in ["InputTrigger", "Reset", "On", "run", "Q", "A", "OutputAPI"]
        .iter()
        .enumerate()
    {
        let port = format!(
            "{}{:04x}-{:02x}ff504f94112233",
            &uuid[..14],
            seq - 7 + i as u32,
            i
        );
        assert!(
            body.contains(&format!("<Co K=\"{k}\" U=\"{port}\"/>")),
            "{k}: {body}"
        );
    }
    assert!(body.contains("<IoData></IoData>"), "{body}");

    // A fixpoint: the same base and lock give the same bytes, and the
    // compiled file as base creates nothing more.
    let again = compile_with(&base, NEW_RULE, &mut lock).unwrap();
    assert_eq!(again, out);
    assert_eq!(compile_with(&out, NEW_RULE, &mut lock).unwrap(), out);
    assert_eq!(lock.rules["mittag"], uuid);

    // The created rule reads back as an extern rule.
    let (m, _) = decompile(&parse(&out), &DecompileOptions::default()).unwrap();
    assert!(
        m.to_text()
            .contains("\tactive\n\twhen time 12:30\n\tthen pumpe on\nend\n"),
        "{}",
        m.to_text()
    );
}

#[test]
fn a_named_rule_finds_the_base_rule_and_owns_its_name() {
    let base = placed_fixture();
    let mut lock = Lockfile::new();
    let as_read = "rule abend \"Pumpe Aus\"\n\tinactive\n\twhen time 17:00\n\
                   \tthen pumpe off\nend\n";
    // Found by name: nothing written, the UUID pinned.
    assert_eq!(compile_with(&base, as_read, &mut lock).unwrap(), base);
    assert_eq!(lock.rules["abend"], RULE_OFF);

    // Renamed in source: the pinned rule takes the new name.
    let renamed = as_read.replace("Pumpe Aus", "Pumpe Abend");
    let out = compile_with(&base, &renamed, &mut lock).unwrap();
    let tag = tag_of(&out, RULE_OFF);
    assert!(tag.contains("Title=\"Pumpe Abend\""), "{tag}");
    assert!(
        tag.contains("&quot;name&quot;:&quot;Pumpe Abend&quot;"),
        "{tag}"
    );
    assert_eq!(out.matches("Type=\"AutopilotRule\"").count(), 2);

    // `loxl rename` carries the pin to the new slug.
    let old = Module::parse(&format!("{EXTERNS}\n{renamed}")).unwrap();
    let mut new = old.clone();
    loxone_config::logic::rename_slug(&mut new, "abend", "spaet");
    let rekeys = loxone_config::logic::lock_rekeys(&old, &new).unwrap();
    assert_eq!(
        loxone_config::logic::apply_rekeys(&mut lock, &rekeys).unwrap(),
        1
    );
    assert_eq!(lock.rules["spaet"], RULE_OFF);
}

#[test]
fn named_rules_are_checked() {
    let err = |src: &str| {
        let mut lock = Lockfile::new();
        compile_with(&placed_fixture(), src, &mut lock)
            .unwrap_err()
            .to_string()
    };
    let has = |src: &str, want: &str| {
        let e = err(src);
        assert!(e.contains(want), "{src:?}: {e}");
    };
    // The slug of an extern is not a new rule's.
    has(
        &NEW_RULE.replace("rule mittag", "rule pumpe_ein"),
        "without the name",
    );
    // A name is one rule.
    has(
        &format!("{NEW_RULE}\n{}", NEW_RULE.replace("mittag", "noch_einer")),
        "another rule is named",
    );
    // An extern and a name for the same rule.
    has(
        &format!(
            "{}\nrule pumpe_aus\n\tinactive\n\twhen time 17:00\n\tthen pumpe off\nend\n",
            NEW_RULE.replace("Pumpe Mittag", "Pumpe Aus")
        ),
        "already declares this rule",
    );
    // Where rules live is the app's choice: with none, nothing is created.
    let no_rules = {
        let base = placed_fixture();
        let start = base.find("\t\t\t<C Type=\"AutopilotRule\"").unwrap();
        let end = base.rfind("\t\t</C>").unwrap();
        format!("{}{}", &base[..start], &base[end..])
    };
    let m = Module::parse(&format!(
        "extern pumpe = PushButton(title: \"Pumpe\")\n\n{NEW_RULE}"
    ))
    .unwrap();
    let e = compile(&parse(&no_rules), &m, &mut Lockfile::new(), &opts())
        .unwrap_err()
        .to_string();
    assert!(e.contains("Create one in the app"), "{e}");
}

#[test]
fn a_removed_rule_is_deleted_until_the_miniserver_runs_without_it() {
    let base = placed_fixture();
    let mut lock = Lockfile::new();
    let created = compile_with(&base, NEW_RULE, &mut lock).unwrap();
    let uuid = lock.rules["mittag"].clone();

    // `removed` deletes it with its connectors, and the lock keeps a
    // tombstone instead of the pin.
    let out = compile_with(&created, "removed mittag\n", &mut lock).unwrap();
    assert!(!out.contains(&uuid[..18]), "{out}");
    assert_eq!(out, compile_with(&base, "", &mut Lockfile::new()).unwrap());
    assert!(lock.rules.is_empty());
    assert_eq!(lock.removed[&uuid].slug, "mittag");

    // A base from before the deployment loses it again; the deployed one
    // retires the tombstone, and the statement is then an error.
    let mut again = lock.clone();
    assert_eq!(
        compile_with(&created, "removed mittag\n", &mut again).unwrap(),
        out
    );
    assert_eq!(
        compile_with(&out, "removed mittag\n", &mut again).unwrap(),
        out
    );
    assert!(again.removed.is_empty());
    let e = compile_with(&out, "removed mittag\n", &mut again).unwrap_err();
    assert!(e.to_string().contains("delete the `removed` line"), "{e}");

    // A rule that drives a block stays until that wire is gone.
    let seq = u32::from_str_radix(&uuid[14..18], 16).unwrap();
    let port = format!("{}{:04x}-04ff504f94112233", &uuid[..14], seq - 3);
    assert!(
        created.contains(&format!("<Co K=\"Q\" U=\"{port}\"/>")),
        "{port}"
    );
    let light = "<Co K=\"Q\" U=\"40000004-0000-0041-01ffaaaaaaaa0004\"/>";
    assert!(created.contains(light));
    let wired = created.replace(
        light,
        &format!(
            "<Co K=\"Q\" Nc=\"1\" U=\"40000004-0000-0041-01ffaaaaaaaa0004\"><In Input=\"{port}\"/></Co>"
        ),
    );
    let mut l = Lockfile::new();
    compile_with(&wired, NEW_RULE, &mut l).unwrap();
    let e = compile_with(&wired, "removed mittag\n", &mut l).unwrap_err();
    assert!(e.to_string().contains("drives Licht"), "{e}");
}

#[test]
fn adopting_an_extern_rule_declares_it_by_name() {
    // A project that edits both rules as externs (D57).
    let base = fixture();
    let module = Module::parse(&format!("{EXTERNS}\n{AS_READ}")).unwrap();
    let mut lock = Lockfile::new();
    compile(&parse(&base), &module, &mut lock, &opts()).unwrap();

    let err = adopt_one(
        &parse(&base),
        RULE_ON,
        "morgens",
        &module,
        &mut lock.clone(),
    )
    .unwrap_err()
    .to_string();
    assert!(err.contains("--as pumpe_ein"), "{err}");

    let adopted = adopt_one(&parse(&base), RULE_ON, "pumpe_ein", &module, &mut lock).unwrap();
    assert_eq!(adopted.promoted, ["pumpe_ein"]);
    assert_eq!(adopted.page_title, "Automatik-Regeln");
    let [Item::Rule(decl)] = &adopted.items[..] else {
        panic!("{:?}", adopted.items)
    };
    assert_eq!(decl.title.as_deref(), Some("Pumpe Ein"));
    assert_eq!(lock.rules["pumpe_ein"], RULE_ON);
    assert!(!lock.externals.contains_key("pumpe_ein"));

    // The extern and its edits leave the source; the named rule compiles
    // to the same bytes.
    let mut m = module.clone();
    assert!(m.strip_extern("pumpe_ein"));
    assert_eq!(m.rule_decls().count(), 1);
    m.items.extend(adopted.items);
    let out = compile(&parse(&base), &m, &mut lock, &opts()).unwrap();
    assert_eq!(String::from_utf8(out.to_bytes()).unwrap(), base);

    let err = adopt_one(&parse(&base), RULE_ON, "pumpe_ein", &m, &mut lock)
        .unwrap_err()
        .to_string();
    assert!(err.contains("already rule `pumpe_ein`"), "{err}");
}

#[test]
fn a_rule_is_adopted_only_with_a_name_of_its_own() {
    let twins = fixture().replace("Title=\"Pumpe Aus\"", "Title=\"Pumpe Ein\"");
    let module = Module::parse("extern pumpe = PushButton(title: \"Pumpe\")\n").unwrap();
    let mut lock = Lockfile::new();
    compile(&parse(&fixture()), &module, &mut lock, &opts()).unwrap();
    let err = adopt_one(
        &parse(&twins),
        RULE_ON,
        "morgens",
        &module,
        &mut lock.clone(),
    )
    .unwrap_err()
    .to_string();
    assert!(err.contains("2 rules of this name"), "{err}");
    // Decompile keeps such twins as extern rules.
    let (m, _) = decompile(&parse(&twins), &DecompileOptions::default()).unwrap();
    assert!(m.to_text().contains("= AutopilotRule("), "{}", m.to_text());

    // A fresh slug for a rule the source does not know yet; its target
    // is in the lock.
    let adopted = adopt_one(&parse(&fixture()), RULE_ON, "morgens", &module, &mut lock).unwrap();
    assert!(adopted.promoted.is_empty());
    assert_eq!(lock.rules["morgens"], RULE_ON);
    let err = adopt_one(&parse(&fixture()), RULE_OFF, "pumpe", &module, &mut lock)
        .unwrap_err()
        .to_string();
    assert!(err.contains("is taken or not a valid slug"), "{err}");
}

// --- More shapes (D73): control states, operating modes, `or` groups.

/// The fixture with an operating mode `Party` (`Mode="-5"`).
fn mode_fixture() -> String {
    fixture().replace(
        "\t\t</C>\r\n\t</C>\r\n</ControlList>",
        "\t\t\t<C Type=\"Mode\" V=\"175\" U=\"40000006-0000-0060-ffff504f94112233\" Title=\"Party\" Nio=\"2\" WF=\"16384\" Mode=\"-5\">\r\n\
         \t\t\t\t<Co K=\"I\" U=\"40000006-0000-0061-00ffaaaaaaaa0006\"/>\r\n\
         \t\t\t\t<Co K=\"Q\" U=\"40000006-0000-0062-01ffaaaaaaaa0006\"/>\r\n\
         \t\t\t</C>\r\n\
         \t\t</C>\r\n\t</C>\r\n</ControlList>",
    )
}

const SHAPES: &str = "rule pumpe_aus\n\
                      \tinactive\n\
                      \twhen pumpe.active == on\n\
                      \twhen mode \"Party\"\n\
                      \tor\n\
                      \twhen time 17:00\n\
                      \tthen mode \"Party\" off\n\
                      \tthen abdeckung pulse\n\
                      end\n";

#[test]
fn states_modes_and_groups_compile_and_read_back() {
    let base = mode_fixture();
    let src = AS_READ.replace(
        "rule pumpe_aus\n\tinactive\n\twhen time 17:00\n\tthen pumpe off\nend\n",
        SHAPES,
    );
    let out = compile_rules(&base, &src).unwrap();
    let off = tag_of(&out, RULE_OFF);
    let json = |s: &str| esc(s);
    // Two groups: the state and the mode, then the old time event.
    assert!(
        off.contains(&json(
            r#""eventGroups":[{"operator":1,"events":[{"type":0,"descID":342001,"#
        )),
        "{off}"
    );
    assert!(
        off.contains(&json(
            r#"{"type":1,"descID":21001,"id":-5,"value":1,"operator":1,"forceEdge":false}]},{"operator":1,"events":[{"type":5,"descID":272001,"id":272,"value":1020"#
        )),
        "{off}"
    );
    assert!(
        off.contains(&json(
            r#"{"type":1,"descID":21101,"id":-5,"command":"0","arrValues":[0]}"#
        )),
        "{off}"
    );
    // It reads back as the source, the mode by the slug the decompiled
    // module gives it, and compiling either again writes the same bytes.
    let (m, _) = decompile(&parse(&out), &DecompileOptions::default()).unwrap();
    let text = m.to_text();
    let by_slug = SHAPES
        .replace("rule pumpe_aus\n", "rule pumpe_aus \"Pumpe Aus\"\n")
        .replace("mode \"Party\"", "mode party");
    assert!(
        text.contains("extern party = Mode(title: \"Party\")"),
        "{text}"
    );
    assert!(text.contains(&by_slug), "{text}");
    // Split into pages, the rule's page declares the mode it names.
    let (pages, _) =
        loxone_config::logic::decompile_pages(&parse(&out), &DecompileOptions::default()).unwrap();
    let page = pages
        .iter()
        .map(|p| p.module.to_text())
        .find(|t| t.contains("rule pumpe_aus"))
        .unwrap();
    assert!(
        page.contains("extern party = Mode(title: \"Party\")"),
        "{page}"
    );
    assert!(page.contains("\twhen mode party\n"), "{page}");
    assert_eq!(compile_rules(&out, &src).unwrap(), out);
    let declared = format!(
        "extern party = Mode(title: \"Party\")\n{}",
        src.replace("mode \"Party\"", "mode party")
    );
    assert_eq!(compile_rules(&out, &declared).unwrap(), out);
    // With the mode declared, its title is refused, naming the slug.
    let e = compile_rules(
        &out,
        &format!("extern party = Mode(title: \"Party\")\n{src}"),
    )
    .unwrap_err()
    .to_string();
    assert!(
        e.contains("Write `when mode party` instead of `when mode \"Party\"`"),
        "{e}"
    );
    // Back to the old rule: the original bytes.
    assert_eq!(compile_rules(&out, AS_READ).unwrap(), base);
}

#[test]
fn states_and_modes_are_checked() {
    let base = mode_fixture();
    let with = |rule: &str| AS_READ.replace("\twhen time 17:00\n", rule);
    // A state the app does not offer for a Switch-shown push button.
    let e = compile_rules(&base, &with("\twhen pumpe.level > 3\n"))
        .unwrap_err()
        .to_string();
    assert!(e.contains("level"), "{e}");
    // A mode the config does not have.
    let e = compile_rules(&base, &with("\twhen mode \"Urlaub\"\n"))
        .unwrap_err()
        .to_string();
    assert!(e.contains("Urlaub") && e.contains("Party"), "{e}");
    // `or` needs conditions before it.
    let e = Module::parse(&format!(
        "{EXTERNS}\nrule pumpe_aus\n\tinactive\n\tor\n\twhen time 17:00\n\tthen pumpe off\nend\n"
    ))
    .unwrap_err()
    .to_string();
    assert!(e.contains("`or`"), "{e}");
    // A state of an undeclared slug.
    let e = Module::parse(&format!(
        "{EXTERNS}\nrule pumpe_aus\n\tinactive\n\twhen nope.active == on\n\tthen pumpe off\nend\n"
    ))
    .unwrap_err()
    .to_string();
    assert!(e.contains("nope"), "{e}");
}

#[test]
fn a_page_module_declares_the_rule_targets_of_other_pages() {
    // The rules on a page of their own, the blocks they drive elsewhere:
    // the rules' module names those blocks as externs.
    let split = fixture().replace(
        "\t\t\t<C Type=\"AutopilotRule\" V=\"175\" U=\"40000002",
        "\t\t</C>\r\n\
         \t\t<C Type=\"Page\" V=\"175\" U=\"40000000-0000-0005-ffff504f94112233\" Title=\"Regeln\" SpNo=\"2\">\r\n\
         \t\t\t<C Type=\"AutopilotRule\" V=\"175\" U=\"40000002",
    );
    let (pages, _) =
        loxone_config::logic::decompile_pages(&parse(&split), &DecompileOptions::default())
            .unwrap();
    let rules = pages.iter().find(|p| p.title == "Regeln").unwrap();
    let text = rules.module.to_text();
    assert!(
        text.contains("extern pumpe = PushButton(title: \"Pumpe\") # page: Automatik-Regeln"),
        "{text}"
    );
    assert_eq!(rules.module.rule_decls().count(), 2, "{text}");
}

#[test]
fn the_container_watches_what_the_rules_watch() {
    // Loxone Config wires every source a rule watches into the AutoPilot
    // container's `Input` (oracle session 31): the state, the mode's `Q`;
    // a weekday the `DayOfWeek` block's `Q`; a time condition none. In
    // reverse document order.
    let base = mode_fixture().replace(
        "\t\t</C>\r\n\t</C>\r\n</ControlList>",
        "\t\t\t<C Type=\"DayOfWeek\" V=\"175\" U=\"40000008-0000-0080-ffff504f94112233\">\r\n\
         \t\t\t\t<Co K=\"Q\" U=\"40000008-0000-0081-01ffaaaaaaaa0008\"/>\r\n\
         \t\t\t</C>\r\n\
         \t\t\t<C Type=\"AutoPilot\" V=\"175\" U=\"40000007-0000-0070-ffff504f94112233\">\r\n\
         \t\t\t\t<Co K=\"Input\" U=\"40000007-0000-0071-00ffaaaaaaaa0007\"/>\r\n\
         \t\t\t</C>\r\n\
         \t\t</C>\r\n\t</C>\r\n</ControlList>",
    );
    let src = AS_READ.replace(
        "rule pumpe_aus\n\tinactive\n\twhen time 17:00\n\tthen pumpe off\nend\n",
        SHAPES,
    );
    let out = compile_rules(&base, &src).unwrap();
    let at = out.find("<Co K=\"Input\"").unwrap();
    let co = &out[at..at + out[at..].find("</Co>").unwrap()];
    assert!(co.contains("Nc=\"3\""), "{co}");
    let mode_q = co.find("40000006-0000-0062-01ffaaaaaaaa0006").unwrap();
    let state = co.find("Input=\"40000001-").unwrap();
    let weekday = co.find("40000008-0000-0081-01ffaaaaaaaa0008").unwrap();
    assert!(mode_q < state && state < weekday, "{co}");
    assert!(co.contains("FLG=\"1\""), "{co}");
    // Stable; back to the old rules, only the weekday stays watched.
    assert_eq!(compile_rules(&out, &src).unwrap(), out);
    // Rules the compile does not change leave the list as the base has
    // it, even none at all (a corpus config, #224); a changed rule
    // derives it again.
    let bare = out.replace(
        co,
        "<Co K=\"Input\" U=\"40000007-0000-0071-00ffaaaaaaaa0007\">",
    );
    let bare = bare.replace(
        "<Co K=\"Input\" U=\"40000007-0000-0071-00ffaaaaaaaa0007\"></Co>",
        "<Co K=\"Input\" U=\"40000007-0000-0071-00ffaaaaaaaa0007\"/>",
    );
    assert!(!bare.contains("FLG=\"1\""), "{bare}");
    assert_eq!(compile_rules(&bare, &src).unwrap(), bare);
    assert!(compile_rules(&bare, AS_READ).unwrap().contains("FLG=\"1\""));
    let back = compile_rules(&out, AS_READ).unwrap();
    assert!(
        back.contains(
            "<Co K=\"Input\" Nc=\"1\" U=\"40000007-0000-0071-00ffaaaaaaaa0007\">\r\n\
             \t\t\t\t\t<In Input=\"40000008-0000-0081-01ffaaaaaaaa0008\" FLG=\"1\"/>"
        ),
        "{back}"
    );
}

// --- Weather, clock and date conditions; notification, mail, pause and
// system message actions (D76).

const MORE: &str = "rule pumpe_aus\n\
                    \tinactive\n\
                    \twhen weather temperature > -2.5\n\
                    \twhen weather sunshine == on\n\
                    \twhen day >= 15\n\
                    \twhen daylight off\n\
                    \tor\n\
                    \twhen sunset\n\
                    \twhen date 2026-07-01\n\
                    \tthen notify \"Pumpe\" \"läuft aus\"\n\
                    \tthen wait 2.5s\n\
                    \tthen mail \"a@example.invalid\" \"Pumpe\" \"aus\"\n\
                    \tthen message warning \"Pumpe\" \"aus\" admins\n\
                    \tthen pumpe off\n\
                    end\n";

#[test]
fn weather_clock_and_messages_compile_and_read_back() {
    let base = fixture().replace(
        "<C Type=\"Document\" V=\"175\"",
        "<C Type=\"Document\" V=\"175\" Timezone=\"Europe/Vienna\"",
    );
    let src = AS_READ.replace(
        "rule pumpe_aus\n\tinactive\n\twhen time 17:00\n\tthen pumpe off\nend\n",
        MORE,
    );
    let out = compile_rules(&base, &src).unwrap();
    let off = tag_of(&out, RULE_OFF);
    for part in [
        r#"{"type":2,"descID":37001,"id":1,"value":-2.5,"operator":3,"forceEdge":false}"#,
        r#"{"type":2,"descID":37003,"id":34,"value":1,"operator":1,"forceEdge":false}"#,
        r#"{"type":3,"descID":269001,"id":269,"value":15,"operator":5,"forceEdge":false}"#,
        r#"{"type":3,"descID":281001,"id":281,"value":0,"operator":1,"forceEdge":false}"#,
        r#"{"type":3,"descID":295001,"id":295,"value":1,"operator":1,"forceEdge":false}"#,
        // 2026-07-01 00:00 CEST.
        r#"{"type":4,"descID":233001,"id":233,"value":1782856800,"operator":1,"forceEdge":false}"#,
        r#"{"type":4,"descID":96001,"arrValues":["Pumpe","läuft aus"]}"#,
        r#"{"type":5,"descID":341801,"command":"2500","arrValues":[2500]}"#,
        r#"{"type":2,"descID":92001,"arrValues":["a@example.invalid","Pumpe","aus"]}"#,
        r#"{"type":7,"descID":98101,"command":"autopilotMessage/2/Pumpe/aus","arrValues":[2,"Pumpe","aus",4]}"#,
    ] {
        assert!(off.contains(&esc(part)), "{part}\n{off}");
    }
    let (m, _) = decompile(&parse(&out), &DecompileOptions::default()).unwrap();
    let text = m.to_text();
    let lifted = MORE
        .replace("rule pumpe_aus\n", "rule pumpe_aus \"Pumpe Aus\"\n")
        .replace("2.5s", "2500ms");
    assert!(text.contains(&lifted), "{text}");
    assert_eq!(compile_rules(&out, &src).unwrap(), out);
    assert_eq!(compile_rules(&out, AS_READ).unwrap(), base);
}

#[test]
fn weather_clock_and_messages_are_checked() {
    let base = fixture();
    let with = |rule: &str| AS_READ.replace("\twhen time 17:00\n", rule);
    let e = compile_rules(&base, &with("\twhen weather humidity > 80\n"))
        .unwrap_err()
        .to_string();
    assert!(e.contains("temperature"), "{e}");
    let e = compile_rules(&base, &with("\twhen weather sunshine > 1\n"))
        .unwrap_err()
        .to_string();
    assert!(e.contains("== on"), "{e}");
    let e = compile_rules(&base, &with("\twhen day > 32\n"))
        .unwrap_err()
        .to_string();
    assert!(e.contains("1 … 31"), "{e}");
    // The fixture states no time zone.
    let e = compile_rules(&base, &with("\twhen date 2026-07-01\n"))
        .unwrap_err()
        .to_string();
    assert!(e.contains("not stated"), "{e}");
    let parse_err = |line: &str| Module::parse(&with(line)).unwrap_err().to_string();
    assert!(parse_err("\twhen date 2026-02-30\n").contains("not a date"));
    let then = |line: &str| with(&format!("\twhen time 17:00\n{line}"));
    let parse_then = |line: &str| Module::parse(&then(line)).unwrap_err().to_string();
    assert!(parse_then("\tthen wait 0.05s\n").contains("0.1 s"));
    assert!(parse_then("\tthen message loud \"a\" \"b\"\n").contains("severity"));
    let e = compile_rules(&base, &then("\tthen message info \"a/b\" \"c\"\n"))
        .unwrap_err()
        .to_string();
    assert!(e.contains("`/`"), "{e}");
}

#[test]
fn the_container_watches_the_weather_day_and_daylight() {
    // Oracle session 32: a weather condition wires the `AQ` of its
    // field's `WeatherData` block, a day of month or daylight condition
    // the `Day` or `Daylight` block's `Q`, a date none. A sun time is not
    // observed: the wires stay as they are.
    let base = fixture()
        .replace(
            "<C Type=\"Document\" V=\"175\"",
            "<C Type=\"Document\" V=\"175\" Timezone=\"Europe/Vienna\"",
        )
        .replace(
            "\t\t</C>\r\n\t</C>\r\n</ControlList>",
            "\t\t\t<C Type=\"DayOfWeek\" V=\"175\" U=\"4000000c-0000-00c0-ffff504f94112233\">\r\n\
             \t\t\t\t<Co K=\"Q\" U=\"4000000c-0000-00c1-01ffaaaaaaaa000c\"/>\r\n\
             \t\t\t</C>\r\n\
             \t\t\t<C Type=\"Day\" V=\"175\" U=\"40000008-0000-0080-ffff504f94112233\">\r\n\
             \t\t\t\t<Co K=\"Q\" U=\"40000008-0000-0081-01ffaaaaaaaa0008\"/>\r\n\
             \t\t\t</C>\r\n\
             \t\t\t<C Type=\"Daylight\" V=\"175\" U=\"40000009-0000-0090-ffff504f94112233\">\r\n\
             \t\t\t\t<Co K=\"Q\" U=\"40000009-0000-0091-01ffaaaaaaaa0009\"/>\r\n\
             \t\t\t</C>\r\n\
             \t\t\t<C Type=\"WeatherData\" V=\"175\" U=\"4000000a-0000-00a0-ffff504f94112233\" WType=\"2\">\r\n\
             \t\t\t\t<Co K=\"AQ\" U=\"4000000a-0000-00a1-01ffaaaaaaaa000a\"/>\r\n\
             \t\t\t</C>\r\n\
             \t\t\t<C Type=\"WeatherData\" V=\"175\" U=\"4000000b-0000-00b0-ffff504f94112233\" WType=\"1\">\r\n\
             \t\t\t\t<Co K=\"AQ\" U=\"4000000b-0000-00b1-01ffaaaaaaaa000b\"/>\r\n\
             \t\t\t</C>\r\n\
             \t\t\t<C Type=\"AutoPilot\" V=\"175\" U=\"40000007-0000-0070-ffff504f94112233\">\r\n\
             \t\t\t\t<Co K=\"Input\" U=\"40000007-0000-0071-00ffaaaaaaaa0007\"/>\r\n\
             \t\t\t</C>\r\n\
             \t\t</C>\r\n\t</C>\r\n</ControlList>",
        );
    let with = |conds: &str| {
        AS_READ.replace(
            "rule pumpe_aus\n\tinactive\n\twhen time 17:00\n",
            &format!("rule pumpe_aus\n\tinactive\n{conds}"),
        )
    };
    let src = with(
        "\twhen time 17:00\n\twhen weather temperature > 0\n\twhen day >= 15\n\
         \twhen daylight\n\twhen date 2026-07-01\n",
    );
    let out = compile_rules(&base, &src).unwrap();
    let at = out.find("<Co K=\"Input\"").unwrap();
    let co = &out[at..at + out[at..].find("</Co>").unwrap()];
    assert!(co.contains("Nc=\"4\""), "{co}");
    let daylight = co.find("40000009-0000-0091-").unwrap();
    let day = co.find("40000008-0000-0081-").unwrap();
    let weather = co.find("4000000b-0000-00b1-").unwrap();
    let weekday = co.find("4000000c-0000-00c1-").unwrap();
    assert!(daylight < day && day < weather && weather < weekday, "{co}");
    assert_eq!(compile_rules(&out, &src).unwrap(), out);
    // A sun time leaves the wires alone.
    let sun = compile_rules(&out, &with("\twhen sunset\n\twhen day >= 15\n")).unwrap();
    let input = |xml: &str| {
        let at = xml.find("<Co K=\"Input\"").unwrap();
        xml[at..at + xml[at..].find("</Co>").unwrap()].to_string()
    };
    assert_eq!(input(&sun), input(&out));
}
