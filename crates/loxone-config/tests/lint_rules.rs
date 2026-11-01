//! What `dead-outputs` counts as used (#302), and the `# lint: allow`
//! comment that keeps a deliberate finding out of the report.
//!
//! The base holds the folders the periphery lives in, one operating mode,
//! a notification and a logic page. Every UUID is made up.

use loxone_config::logic::{
    CompileOptions, LintFinding, LintKind, Module, compile, finish_lint, lint_dead_outputs,
    lint_source,
};
use loxone_config::uuid::parse_serial;
use loxone_config::{Lockfile, LoxoneDoc};

fn base() -> LoxoneDoc {
    LoxoneDoc::parse(
        "<ControlList Version=\"1\" NextObj=\"50\">\r\n\
         \t<C Type=\"Document\" U=\"00000001-0000-0000-ffff000000000001\" ConfigVersion=\"17020828\">\r\n\
         \t\t<C Type=\"Notification\" V=\"178\" U=\"00000006-0000-0000-ffff000000000001\" Title=\"Frost\" WF=\"16384\" Entries=\"10\">\r\n\
         \t\t</C>\r\n\
         \t\t<C Type=\"ModeCaption\" U=\"00000007-0000-0000-ffff000000000001\" Title=\"Betriebsmodi\">\r\n\
         \t\t\t<C Type=\"Mode\" V=\"178\" U=\"00000008-0000-0000-ffff000000000001\" Title=\"Feiertag\" Nio=\"2\" WF=\"16384\">\r\n\
         \t\t\t\t<Co K=\"I\" U=\"00000008-0000-0001-00ff000000000001\"/>\r\n\
         \t\t\t\t<Co K=\"Q\" U=\"00000008-0000-0002-01ff000000000001\"/>\r\n\
         \t\t\t</C>\r\n\
         \t\t</C>\r\n\
         \t\t<C Type=\"VirtualInCaption\" U=\"00000002-0000-0000-ffff000000000001\" Title=\"Virtuelle Eingänge\">\r\n\
         \t\t</C>\r\n\
         \t\t<C Type=\"VirtualOutCaption\" U=\"00000003-0000-0000-ffff000000000001\" Title=\"Virtuelle Ausgänge\">\r\n\
         \t\t</C>\r\n\
         \t\t<C Type=\"Page\" U=\"00000005-0000-0000-ffff000000000001\" Title=\"Logik\">\r\n\
         \t\t</C>\r\n\
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

/// Compiles `src`, lets `edit` change the compiled XML (what a GUI save
/// would add), and runs the project lints as `loxl lint` does.
fn lint_with(src: &str, edit: impl Fn(&Lockfile, String) -> String) -> Vec<LintFinding> {
    let module = Module::parse(src).unwrap();
    let mut lock = Lockfile::new();
    let doc = compile(&base(), &module, &mut lock, &opts()).unwrap();
    let xml = edit(&lock, String::from_utf8(doc.to_bytes()).unwrap());
    let doc = LoxoneDoc::parse(xml.as_bytes()).unwrap();
    let mut findings = lint_source(&module).unwrap();
    findings.extend(lint_dead_outputs(&module, &lock, &doc).unwrap());
    finish_lint(&module, findings, true).unwrap()
}

fn lint(src: &str) -> Vec<LintFinding> {
    lint_with(src, |_, xml| xml)
}

fn of(findings: &[LintFinding], kind: LintKind) -> Vec<String> {
    findings
        .iter()
        .filter(|f| f.kind == kind)
        .map(|f| f.slug.clone())
        .collect()
}

/// Adds attributes to the element of `slug` (right after its `U=`).
fn with_attrs(lock: &Lockfile, xml: &str, slug: &str, attrs: &str) -> String {
    let u = format!("U=\"{}\"", lock.objects[slug].uuid);
    assert!(xml.contains(&u), "{slug} not in the compiled config");
    xml.replacen(&u, &format!("{u} {attrs}"), 1)
}

/// Adds a child element to the element of `slug`.
fn with_child(lock: &Lockfile, xml: &str, slug: &str, child: &str) -> String {
    let u = format!("U=\"{}\"", lock.objects[slug].uuid);
    let at = xml.find(&u).expect("slug in the compiled config");
    let gt = at + xml[at..].find('>').unwrap();
    // A self-closing element opens up to take the child.
    if xml[..gt].ends_with('/') {
        let close = "</C>";
        format!("{}>{child}{close}{}", &xml[..gt - 1], &xml[gt + 1..])
    } else {
        format!("{}>{child}{}", &xml[..gt], &xml[gt + 1..])
    }
}

const SWITCH: &str = "schalter = VirtualIn(\"Schalter\")\n";

/// A source the app shows as a value: used, and so never a finding.
const QUELLE: &str = "quelle = VirtualIn(\"Quelle\")\n";

#[test]
fn a_value_the_app_shows_is_used() {
    // A minted digital virtual input is an info-only value in the app
    // (`Visu="true"`, `<Display StateOnly="true">`).
    let findings = lint(SWITCH);
    assert!(
        of(&findings, LintKind::DeadOutputs).is_empty(),
        "{findings:?}"
    );
}

#[test]
fn a_switch_nothing_reads_is_dead_even_in_the_app() {
    // The same input as a switch, as Loxone Config shows it once the
    // display is no longer state-only.
    let findings = lint_with(SWITCH, |_, xml| xml.replacen(" StateOnly=\"true\"", "", 1));
    assert_eq!(of(&findings, LintKind::DeadOutputs), ["schalter"]);
    assert!(
        findings[0].detail.contains("in the app"),
        "{}",
        findings[0].detail
    );
}

#[test]
fn a_controller_that_drives_nothing_names_what_to_wire() {
    // The type is changed after compiling, as a GUI save of the
    // controller would leave it: in the app, its outputs unwired.
    let src = format!("{QUELLE}regler = And(I1: quelle.Q)\n");
    for (ty, fix) in [
        (
            "HeatIRoomController2",
            "wire AQh (heating), AQc (cooling) or AQhc",
        ),
        ("TpfController", "link its OutputAPI to the API Connector"),
    ] {
        let findings = lint_with(&src, |lock, xml| {
            with_child(lock, &xml, "regler", "<IoData Visu=\"true\"/>").replacen(
                "Type=\"And\"",
                &format!("Type=\"{ty}\""),
                1,
            )
        });
        let dead: Vec<_> = findings
            .iter()
            .filter(|f| f.kind == LintKind::DeadOutputs)
            .collect();
        assert_eq!(dead.len(), 1, "{findings:?}");
        assert!(
            dead[0].detail.contains("in the app") && dead[0].detail.contains(fix),
            "{ty}: {}",
            dead[0].detail
        );
    }
    // A gate has no expected use to name.
    let findings = lint(&src);
    assert!(!findings[0].detail.contains("To use it"), "{findings:?}");
}

#[test]
fn a_statistic_is_a_use() {
    let src = format!("{QUELLE}verwaist = And(I1: quelle.Q)\n");
    let src = src.as_str();
    assert_eq!(of(&lint(src), LintKind::DeadOutputs), ["verwaist"]);
    let findings = lint_with(src, |lock, xml| {
        with_attrs(lock, &xml, "verwaist", "StatsType=\"7\"")
    });
    assert!(
        of(&findings, LintKind::DeadOutputs).is_empty(),
        "{findings:?}"
    );
}

#[test]
fn a_block_an_energy_flow_monitor_reads_is_used() {
    let src = format!("{QUELLE}zaehler = And(I1: quelle.Q)\nefm = Or(I1: quelle.Q)\n");
    let src = src.as_str();
    assert_eq!(of(&lint(src), LintKind::DeadOutputs), ["zaehler", "efm"]);
    // `efm` becomes a monitor whose node reads `zaehler`.
    let findings = lint_with(src, |lock, xml| {
        let meter = &lock.objects["zaehler"].uuid;
        with_child(
            lock,
            &xml,
            "efm",
            &format!("<root t=\"5\"><nd t=\"1\" ctrlUuid=\"{meter}\"/></root>"),
        )
        .replacen("Type=\"Or\"", "Type=\"EFM\"", 1)
    });
    assert_eq!(
        of(&findings, LintKind::DeadOutputs),
        ["efm"],
        "{findings:?}"
    );
}

#[test]
fn modes_and_messages_count_as_used() {
    let src = "\
extern frost = Notification(title: \"Frost\")
quelle = VirtualIn(\"Quelle\")
party = Mode(\"Party\")
kalt = And(I1: quelle.Q)
warnung = OutputRefLM(mirrors: frost, AI: kalt.Q, On: \"kalt\")
";
    let findings = lint(src);
    assert!(
        of(&findings, LintKind::DeadOutputs).is_empty(),
        "{findings:?}"
    );
}

const DEVICES: &str = r#"
http = VirtualHttpIn("Zähler", Address: "http://192.0.2.10/status", Poll: 600)
http_a = VirtualHttpInCmd("A", device: http, Check: "a:\\v")
http_b = VirtualHttpInCmd("B", device: http, Check: "b:\\v")
http_c = VirtualHttpInCmd("C", device: http, Check: "c:\\v")
udp = VirtualUdpIn("Kamera", Port: 45999)
udp_person = VirtualUdpInCmd("Person", device: udp, Check: "person:\\v")
out = VirtualOut("Aktor", Address: "http://192.0.2.11")
ein = VirtualOutCmd("Ein", device: out, CmdOn: "/on", I: http_c.Q)
aus = VirtualOutCmd("Aus", device: out, CmdOn: "/off")
"#;

#[test]
fn a_device_reports_its_unused_inputs_once_and_outputs_by_their_drive() {
    let findings = lint(DEVICES);
    let dead: Vec<(&str, &str)> = findings
        .iter()
        .filter(|f| f.kind == LintKind::DeadOutputs)
        .map(|f| (f.slug.as_str(), f.detail.as_str()))
        .collect();
    let slugs: Vec<&str> = dead.iter().map(|(s, _)| *s).collect();
    // Two of three HTTP commands unused: one finding on the device. The
    // lone UDP command keeps its own. `ein` is driven, `aus` is not.
    assert_eq!(slugs, ["http", "udp_person", "aus"], "{dead:?}");
    assert!(
        dead[0].1.starts_with("2 of its 3 inputs feed nothing") && dead[0].1.contains("`http_b`"),
        "{}",
        dead[0].1
    );
    assert!(dead[2].1.contains("nothing drives"), "{}", dead[2].1);
}

#[test]
fn an_allow_above_a_declaration_keeps_its_finding_out() {
    // On the device: covers its inputs. On a single block: that block.
    let src = format!(
        "{QUELLE}# lint: allow dead-outputs read by Home Assistant\nverwaist = And(I1: quelle.Q)\n{}",
        DEVICES.replace(
            "http = VirtualHttpIn",
            "# Shelly: every value it reports\n# lint: allow dead-outputs\nhttp = VirtualHttpIn"
        )
    );
    let findings = lint(&src);
    assert_eq!(
        of(&findings, LintKind::DeadOutputs),
        ["udp_person", "aus"],
        "{findings:?}"
    );
    assert!(
        of(&findings, LintKind::UnusedAllow).is_empty(),
        "{findings:?}"
    );
}

#[test]
fn an_allow_that_covers_nothing_is_reported() {
    let src = "\
vi = VirtualIn(\"Eins\")
# lint: allow dead-outputs
gate = And(I1: vi.Q)
# lint: allow unused-extern
ok = VirtualIn(\"Zwei\")
# lint: allow dead-output
x = Or(I1: gate.Q, I2: ok.Q)
ein = VirtualOutCmd(\"Ein\", device: out, CmdOn: \"/on\", I: x.Q)
out = VirtualOut(\"Aktor\", Address: \"http://192.0.2.11\")
# lint: allow dead-outputs
";
    let findings = lint(src);
    let unused: Vec<(&str, &str)> = findings
        .iter()
        .filter(|f| f.kind == LintKind::UnusedAllow)
        .map(|f| (f.slug.as_str(), f.detail.as_str()))
        .collect();
    assert_eq!(unused.len(), 4, "{unused:?}");
    assert_eq!(unused[0].0, "gate");
    assert!(unused[0].1.contains("covers no finding"), "{}", unused[0].1);
    assert_eq!(unused[1].0, "ok");
    // A misspelt kind names no kind.
    assert_eq!(unused[2].0, "x");
    assert!(
        unused[2].1.contains("names no lint kind"),
        "{}",
        unused[2].1
    );
    assert!(
        unused[3].1.contains("above no declaration"),
        "{}",
        unused[3].1
    );
}

#[test]
fn an_allow_outside_a_project_judges_source_lints_only() {
    let m = Module::parse(
        "# lint: allow dead-outputs\ngate = And(I1: 1)\n\
         # lint: allow unused-let\nlet schwelle = 3\n# lint: allow unused-let\nlet b = 2\nx = And(I1: 1, I2: b)\n",
    )
    .unwrap();
    let findings = finish_lint(&m, lint_source(&m).unwrap(), false).unwrap();
    // `schwelle` is allowed; the allow above `b` covers nothing; the
    // dead-outputs allow is not judged without the compiled config.
    let got: Vec<(&str, &str)> = findings
        .iter()
        .map(|f| (f.kind.label(), f.slug.as_str()))
        .collect();
    assert_eq!(got, [("unused-allow", "b")], "{findings:?}");
}

#[test]
fn an_allow_reaches_template_blocks() {
    let src = "\
quelle = VirtualIn(\"Quelle\")

template paar()
\t# lint: allow dead-outputs
\ta = And(I1: quelle.Q)
\tb = Or(I1: quelle.Q)
end

# lint: allow dead-outputs
eins = paar()
zwei = paar()
";
    let dead = of(&lint(src), LintKind::DeadOutputs);
    // `eins` is covered whole; in `zwei` only the body's allow on `a`.
    assert_eq!(dead, ["zwei_b"]);
}

#[test]
fn fmt_keeps_an_allow_right_above_its_statement() {
    let src = "\
extern a = VirtualIn(iname: \"VI1\")
# lint: allow unused-extern
extern b = VirtualIn(iname: \"VI2\")
# a plain comment
extern c = VirtualIn(iname: \"VI3\")
";
    let text = Module::parse(src).unwrap().to_text();
    assert!(
        text.contains(
            "extern a = VirtualIn(iname: \"VI1\")\n# lint: allow unused-extern\nextern b"
        ),
        "{text}"
    );
    // A plain comment keeps its blank lines.
    assert!(text.contains("\n\n# a plain comment\n\nextern c"), "{text}");
    assert_eq!(Module::parse(&text).unwrap().to_text(), text);
}
