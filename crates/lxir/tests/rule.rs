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
        m.to_text().contains("rule pumpe_ein\n\tinactive\n"),
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
                        \tthen pumpe \"on\"\n\
                        end\n";

fn compile_with(base: &str, src: &str, lock: &mut Lockfile) -> lxir::Result<String> {
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
            .contains("\tactive\n\twhen time 12:30\n\tthen pumpe \"on\"\nend\n"),
        "{}",
        m.to_text()
    );
}

#[test]
fn a_named_rule_finds_the_base_rule_and_owns_its_name() {
    let base = placed_fixture();
    let mut lock = Lockfile::new();
    let as_read = "rule abend \"Pumpe Aus\"\n\tinactive\n\twhen time 17:00\n\
                   \tthen pumpe \"off\"\nend\n";
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

    // `lxir rename` carries the pin to the new slug.
    let old = Module::parse(&format!("{EXTERNS}\n{renamed}")).unwrap();
    let mut new = old.clone();
    lxir::ir::rename_slug(&mut new, "abend", "spaet");
    let rekeys = lxir::ir::lock_rekeys(&old, &new).unwrap();
    assert_eq!(lxir::ir::apply_rekeys(&mut lock, &rekeys).unwrap(), 1);
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
            "{}\nrule pumpe_aus\n\tinactive\n\twhen time 17:00\n\tthen pumpe \"off\"\nend\n",
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
    assert!(e.contains("create one in the app"), "{e}");
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
