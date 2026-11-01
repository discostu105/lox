//! LightController2 circuits from source: `Circuit<n>: "switch"` or
//! `"dimmer"` names the circuit after the actuator `AQ<n>` drives and
//! sets its kind in every mood, as Loxone Config's mood editor does.

use loxone_config::logic::{CompileOptions, Module, compile, lint_circuits};
use loxone_config::uuid::parse_serial;
use loxone_config::{Lockfile, LoxoneDoc};

fn base() -> LoxoneDoc {
    LoxoneDoc::parse(&std::fs::read("examples/library/lighting/lighting.Loxone").unwrap()).unwrap()
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

const HEAD: &str = "page \"Probe\" (room: \"Wohnzimmer\")\n\
                    extern relais2 = Actor(iname: \"Q2\")\n\
                    extern relais3 = Actor(iname: \"Q3\")\n\
                    extern led = TreeAactor(title: \"LED-Streifen\")\n";

fn build(doc: &LoxoneDoc, body: &str, lock: &mut Lockfile) -> Result<LoxoneDoc, String> {
    let module = Module::parse(&format!("{HEAD}{body}")).unwrap();
    compile(doc, &module, lock, &opts()).map_err(|e| e.to_string())
}

fn controller<'a>(doc: &'a LoxoneDoc, title: &str) -> &'a loxone_config::xml::Element {
    let o = doc
        .objects()
        .into_iter()
        .find(|o| o.title.as_deref() == Some(title))
        .unwrap();
    doc.element_at(&o.path).unwrap()
}

fn names(doc: &LoxoneDoc, title: &str) -> Vec<String> {
    controller(doc, title)
        .attrs
        .iter()
        .map(|a| a.name.clone())
        .collect()
}

/// `Q<n>` of every mood, in order.
fn column(doc: &LoxoneDoc, title: &str, n: usize) -> Vec<u32> {
    loxone_config::moods::Moods::read(controller(doc, title))
        .unwrap()
        .moods
        .iter()
        .map(|m| m.qs[n - 1])
        .collect()
}

const DIM: &str = "l1 = LightController2(\"L1\", Circuit1: \"dimmer\", Circuit2: \"switch\")\n\
                   relais2.I <- l1.AQ1\n\
                   relais3.I <- l1.AQ2\n";

#[test]
fn a_circuit_is_named_after_its_actuator() {
    let mut lock = Lockfile::new();
    let out = build(&base(), DIM, &mut lock).unwrap();
    let el = controller(&out, "L1");
    assert_eq!(el.attr("NameAI0"), Some("Aktor (Relais) 2"));
    assert_eq!(el.attr("PresM0"), Some("65535"));
    assert_eq!(el.attr("NameAI1"), Some("Aktor (Relais) 3"));
    assert_eq!(el.attr("CapAI0"), None);
    // In index order after `Nio=`, where the mood editor writes them (a
    // minted controller has no sequencing lists yet).
    let n = names(&out, "L1");
    let at = |k: &str| n.iter().position(|a| a == k).unwrap();
    assert!(at("Nio") < at("NameAI0"));
    assert!(at("NameAI0") < at("PresM0") && at("PresM0") < at("NameAI1"));
    assert!(at("NameAI1") < at("PresM1"));
    // The dimmer's moods carry prefix 1, the switch's prefix 0. The stock
    // "Viel Licht" goes to 0 % on the dimmer, as the editor's *Typ* does.
    assert_eq!(column(&out, "L1", 1), [1 << 28, 1 << 28]);
    assert_eq!(column(&out, "L1", 2), [1, 0]);

    // Compiled again onto its own output, nothing changes.
    let again = build(&out, DIM, &mut lock).unwrap();
    assert_eq!(again.to_bytes(), out.to_bytes());
}

#[test]
fn a_dimmer_takes_percentages() {
    let src = format!(
        "{DIM}moods abend\n\t\"Viel Licht\": AQ1 80%, AQ2 on\n\t\"Kino\": AQ1 20%\nend\n\
         l1.moods = abend\n"
    );
    let mut lock = Lockfile::new();
    let out = build(&base(), &src, &mut lock).unwrap();
    assert_eq!(
        column(&out, "L1", 1),
        [(1 << 28) + 80, (1 << 28) + 20, 1 << 28]
    );
    let module = Module::parse(&format!("{HEAD}{src}")).unwrap();
    assert!(lint_circuits(&module, &lock, &out).unwrap().is_empty());

    // Back to a switch: the prefix goes, and the moods are written anew.
    let switch = src
        .replace("\"dimmer\"", "\"switch\"")
        .replace("80%", "on")
        .replace("20%", "on");
    let out = build(&out, &switch, &mut lock).unwrap();
    assert_eq!(column(&out, "L1", 1), [1, 1, 0]);
    // The name stays: it is the controller's now.
    assert_eq!(
        controller(&out, "L1").attr("NameAI0"),
        Some("Aktor (Relais) 2")
    );
}

#[test]
fn a_name_loxone_config_gave_is_kept() {
    let mut lock = Lockfile::new();
    let out = build(&base(), DIM, &mut lock).unwrap();
    let mut gui = out.clone();
    let o = gui
        .objects()
        .into_iter()
        .find(|o| o.title.as_deref() == Some("L1"))
        .unwrap();
    gui.element_at_mut(&o.path)
        .unwrap()
        .set_attr("NameAI1", "Flurlicht");
    let again = build(&gui, DIM, &mut lock).unwrap();
    assert_eq!(controller(&again, "L1").attr("NameAI1"), Some("Flurlicht"));
}

#[test]
fn what_the_mood_editor_was_never_seen_to_do_is_refused() {
    for (body, want) in [
        (
            "l1 = LightController2(\"L1\", Circuit1: \"dimmer\")\n",
            "drives no actuator",
        ),
        (
            "l1 = LightController2(\"L1\", Circuit1: \"dimmer\")\n\
             relais2.I <- l1.AQ1\nrelais3.I <- l1.AQ1\n",
            "drives 2 objects",
        ),
        (
            "l1 = LightController2(\"L1\", Circuit1: \"dimmer\")\nled.I <- l1.AQ1\n",
            "colour actuator",
        ),
        (
            "l1 = LightController2(\"L1\", Circuit1: \"rgb\")\n",
            "not a circuit kind",
        ),
        (
            "l1 = LightController2(\"L1\", Circuit6: \"dimmer\")\n",
            "Circuit1 … Circuit5",
        ),
    ] {
        let e = build(&base(), body, &mut Lockfile::new()).unwrap_err();
        assert!(e.contains(want), "{body}: {e}");
    }
}

#[test]
fn decompile_lifts_the_circuits() {
    use loxone_config::logic::{DecompileOptions, DecompileScope, decompile};
    let mut lock = Lockfile::new();
    let out = build(&base(), DIM, &mut lock).unwrap();
    let (m, _) = decompile(
        &out,
        &DecompileOptions {
            scope: DecompileScope::ManagedOnly,
            ..Default::default()
        },
    )
    .unwrap();
    let text = m.to_text();
    assert!(
        text.contains("Circuit1: \"dimmer\"") && text.contains("Circuit2: \"switch\""),
        "{text}"
    );
}
