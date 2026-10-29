//! End-to-end tests for mirror routing (D34): a wire that names the
//! mirrored object directly is drawn through a same-page ref the base
//! already carries — and stays direct when no such ref exists.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn lxir(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_lxir"))
        .current_dir(dir)
        .args(args)
        .output()
        .expect("run lxir")
}

const PAGE_BESCHATTUNG: &str =
    "<C Type=\"Page\" V=\"175\" U=\"20000004-0000-0040-ffff504f94112233\" Title=\"Beschattung\">";
const PAGE_WERKSTATT: &str =
    "<C Type=\"Page\" V=\"175\" U=\"20000009-0000-0090-ffff504f94112233\" Title=\"Werkstatt\"></C>";

fn make_project(name: &str, page: &str, module: &str, base: Option<&Path>) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    if dir.exists() {
        std::fs::remove_dir_all(&dir).unwrap();
    }
    std::fs::create_dir_all(dir.join("src")).unwrap();
    match base {
        Some(b) => {
            std::fs::copy(b, dir.join("haus.Loxone")).unwrap();
        }
        None => {
            // A second, empty page so cross-page behavior is testable.
            let xml = std::fs::read_to_string(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/examples/configs/haus.Loxone"
            ))
            .unwrap();
            let xml = xml.replace(
                PAGE_BESCHATTUNG,
                &format!("{PAGE_WERKSTATT}{PAGE_BESCHATTUNG}"),
            );
            std::fs::write(dir.join("haus.Loxone"), xml).unwrap();
        }
    }
    std::fs::write(
        dir.join("lox.toml"),
        format!(
            "base = \"haus.Loxone\"\n\
             module = \"src\"\n\
             lock = \"haus.lock.json\"\n\
             serial = \"504F94112233\"\n\
             page = \"{page}\"\n"
        ),
    )
    .unwrap();
    std::fs::write(dir.join("src/main.lxir"), module).unwrap();
    dir
}

/// Port uuid of connector `key` on the element whose tag carries `marker`.
fn port_of(xml: &str, marker: &str, key: &str) -> String {
    let at = xml.find(marker).unwrap_or_else(|| panic!("no {marker}"));
    let el_end = xml[at..].find("</C>").map_or(xml.len(), |e| at + e);
    let seg = &xml[at..el_end];
    let co = seg
        .find(&format!("<Co K=\"{key}\""))
        .unwrap_or_else(|| panic!("no Co {key} in {marker}"));
    let useg = &seg[co..];
    let u = useg.find(" U=\"").unwrap() + 4;
    useg[u..u + useg[u..].find('"').unwrap()].to_string()
}

/// The `<In Input=…>` sources of connector `key` on the marked element.
fn inputs_of(xml: &str, marker: &str, key: &str) -> Vec<String> {
    let at = xml.find(marker).unwrap_or_else(|| panic!("no {marker}"));
    let el_end = xml[at..].find("</C>").map_or(xml.len(), |e| at + e);
    let seg = &xml[at..el_end];
    let co = match seg.find(&format!("<Co K=\"{key}\"")) {
        Some(c) => c,
        None => return vec![],
    };
    let co_end = seg[co..]
        .find("</Co>")
        .map_or_else(|| seg[co..].find("/>").unwrap() + co, |e| e + co);
    regex_inputs(&seg[co..co_end])
}

fn regex_inputs(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = s;
    while let Some(i) = rest.find("<In Input=\"") {
        let v = &rest[i + 11..];
        let end = v.find('"').unwrap();
        out.push(v[..end].to_string());
        rest = &v[end..];
    }
    out
}

/// A base with a GUI-style mirror: mint an InputRef of VI1 and an
/// OutputRef of VI3 (its `Qm` fed from the OutputRef's `AQ`, the way the
/// GUI wires actors), then use the *output* as the next project's base —
/// there the refs are unmanaged, exactly like GUI-created ones.
fn base_with_mirrors(name: &str) -> PathBuf {
    let dir = make_project(
        name,
        "Beschattung",
        "extern quelle = VirtualIn(iname: \"VI1\")\n\
         extern aktor = VirtualIn(iname: \"VI3\")\n\
         \n\
         spiegel = InputRef(\n\
         \tmirrors: quelle,\n\
         \tAI: quelle.Q,\n\
         \tI: quelle.Qm,\n\
         )\n\
         \n\
         halter = Memory(\n\
         \tInput: spiegel.Q,\n\
         )\n\
         \n\
         aus_spiegel = OutputRef(\n\
         \tmirrors: aktor,\n\
         \tAI: halter.Q,\n\
         )\n\
         \n\
         aktor.Qm <- aus_spiegel.AQ\n",
        None,
    );
    let out = lxir(&dir, &["compile"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    dir.join("out.Loxone")
}

#[test]
fn wires_route_through_same_page_mirrors() {
    let base = base_with_mirrors("routing-base-reuse");
    let dir = make_project(
        "routing-reuse",
        "Beschattung",
        "extern quelle = VirtualIn(iname: \"VI1\")\n\
         extern aktor = VirtualIn(iname: \"VI3\")\n\
         \n\
         anzeige = And(\n\
         \tI1: quelle.Q,\n\
         \tI2: quelle.Qm,\n\
         )\n\
         \n\
         aktor.Qm <- anzeige.Q\n",
        Some(&base),
    );
    let out = lxir(&dir, &["compile"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let xml = std::fs::read_to_string(dir.join("out.Loxone")).unwrap();
    // The consumer reads through the mirror: AI-fed serves AQ, I-fed
    // serves Q — not the VirtualIn's ports directly.
    let spiegel_aq = port_of(&xml, "<C Type=\"InputRef\"", "AQ");
    let spiegel_q = port_of(&xml, "<C Type=\"InputRef\"", "Q");
    assert_eq!(
        inputs_of(&xml, "Title=\"anzeige\"", "I1"),
        vec![spiegel_aq],
        "I1 must read the mirror's AQ"
    );
    assert_eq!(
        inputs_of(&xml, "Title=\"anzeige\"", "I2"),
        vec![spiegel_q],
        "I2 must read the mirror's Q"
    );
    // The actor write lands on the OutputRef's AI, not on the actor.
    let aus_ai = port_of(&xml, "<C Type=\"OutputRef\"", "AI");
    let anzeige_q = port_of(&xml, "Title=\"anzeige\"", "Q");
    let out_ref_at = xml.find("<C Type=\"OutputRef\"").unwrap();
    let out_ref_seg = &xml[out_ref_at..out_ref_at + xml[out_ref_at..].find("</C>").unwrap()];
    assert!(
        out_ref_seg.contains(&format!("<In Input=\"{anzeige_q}\"")),
        "anzeige.Q must land on the OutputRef's AI ({aus_ai}): {out_ref_seg}"
    );
    // Deterministic recompile.
    let before = std::fs::read(dir.join("out.Loxone")).unwrap();
    assert!(lxir(&dir, &["compile"]).status.success());
    assert_eq!(before, std::fs::read(dir.join("out.Loxone")).unwrap());
}

#[test]
fn twin_mirrors_route_by_the_drawn_wire_or_refuse() {
    // Two InputRef mirrors of one target on one page each carry their own
    // ports — every corpus config does; a push tool once made them share
    // (docs/loxone-format.md). The wire already drawn through one twin
    // keeps it; a consumer without such a wire is refused.
    let module = "extern quelle = VirtualIn(iname: \"VI1\")\n\
         extern aktor = VirtualIn(iname: \"VI3\")\n\
         \n\
         anzeige = And(\n\
         \tI1: quelle.Q,\n\
         \tI2: quelle.Qm,\n\
         )\n\
         \n\
         aktor.Qm <- anzeige.Q\n";
    let base = base_with_mirrors("routing-base-twins");
    let first = make_project("routing-twins-first", "Beschattung", module, Some(&base));
    let out = lxir(&first, &["compile"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    // The GUI adds a second mirror next to the first: fresh object and
    // port uuids throughout, the same feed wires.
    let add_twin = |xml: &str| -> String {
        let at = xml.find("<C Type=\"InputRef\"").unwrap();
        let end = at + xml[at..].find("</C>").unwrap() + 4;
        let mut clone = xml[at..end].to_string();
        let mut olds = Vec::new();
        let mut rest = clone.as_str();
        while let Some(i) = rest.find(" U=\"") {
            let v = &rest[i + 4..];
            olds.push(v[..v.find('"').unwrap()].to_string());
            rest = v;
        }
        for (n, old) in olds.iter().enumerate() {
            clone = clone.replace(old, &format!("3000000a-0000-00a{n}-ffff504f94112233"));
        }
        format!("{}{}{}", &xml[..end], clone, &xml[end..])
    };
    let xml = std::fs::read_to_string(first.join("out.Loxone")).unwrap();
    let spiegel_aq = port_of(&xml, "<C Type=\"InputRef\"", "AQ");
    let spiegel_q = port_of(&xml, "<C Type=\"InputRef\"", "Q");
    let base2 = first.join("haus-twinned.Loxone");
    std::fs::write(&base2, add_twin(&xml)).unwrap();

    let again = make_project("routing-twins-again", "Beschattung", module, Some(&base2));
    std::fs::copy(first.join("haus.lock.json"), again.join("haus.lock.json")).unwrap();
    let out = lxir(&again, &["compile"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out_xml = std::fs::read_to_string(again.join("out.Loxone")).unwrap();
    assert_eq!(
        inputs_of(&out_xml, "Title=\"anzeige\"", "I1"),
        vec![spiegel_aq]
    );
    assert_eq!(
        inputs_of(&out_xml, "Title=\"anzeige\"", "I2"),
        vec![spiegel_q]
    );

    // A consumer no wire pins yet (the twins added before it existed).
    let base3 = first.join("haus-twinned-bare.Loxone");
    std::fs::write(&base3, add_twin(&std::fs::read_to_string(&base).unwrap())).unwrap();
    let fresh = make_project("routing-twins-fresh", "Beschattung", module, Some(&base3));
    let out = lxir(&fresh, &["compile"]);
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success());
    assert!(
        err.contains("several mirrors") && err.contains("3000000a-0000-00a0-ffff504f94112233"),
        "{err}"
    );
}

#[test]
fn cross_page_consumers_stay_direct() {
    let base = base_with_mirrors("routing-base-crosspage");
    let dir = make_project(
        "routing-crosspage",
        "Werkstatt",
        "extern quelle = VirtualIn(iname: \"VI1\")\n\
         extern senke = VirtualIn(iname: \"VI2\")\n\
         \n\
         anzeige = And(\n\
         \tI1: quelle.Q,\n\
         \tI2: quelle.Q,\n\
         )\n\
         \n\
         senke.Qm <- anzeige.Q\n",
        Some(&base),
    );
    let out = lxir(&dir, &["compile"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let xml = std::fs::read_to_string(dir.join("out.Loxone")).unwrap();
    // The mirror lives on Beschattung; the consumer on Werkstatt wires
    // the VirtualIn directly.
    let quelle_q = port_of(&xml, "IName=\"VI1\"", "Q");
    assert_eq!(
        inputs_of(&xml, "Title=\"anzeige\"", "I1"),
        vec![quelle_q.clone()],
        "cross-page wires must stay direct"
    );
    assert_eq!(
        inputs_of(&xml, "Title=\"anzeige\"", "I2"),
        vec![quelle_q],
        "cross-page wires must stay direct"
    );
}

#[test]
fn decompile_folds_consumer_wires_to_the_target() {
    // The inverse view (D34): a consumer wire drawn on a ref is shown
    // against the mirrored object itself; the ref and its plumbing
    // disappear from the full view.
    let base = base_with_mirrors("routing-base-decompile");
    let dir = base.parent().unwrap();
    // Decompile the *output* config — it carries the minted refs.
    let out = lxir(dir, &["decompile", "out.Loxone"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let view = String::from_utf8_lossy(&out.stdout).into_owned();
    // halter read spiegel.Q (I-fed from the VirtualIn's Qm) — the view
    // names the VirtualIn's own port (decompile derives slugs from
    // titles, so match on the port). The actor write through the
    // OutputRef's AI names the actor port its AQ feeds.
    assert!(view.contains("Input: "), "{view}");
    assert!(
        view.contains(".Qm,"),
        "read must name the target port: {view}"
    );
    assert!(view.contains("Qm <- halter.Q"), "{view}");
    assert!(!view.contains("InputRef"), "refs must fold away: {view}");
    assert!(!view.contains("OutputRef"), "refs must fold away: {view}");
}

#[test]
fn explicit_ref_externs_still_wire_literally() {
    let base = base_with_mirrors("routing-base-explicit");
    // The D32 form: name the ref itself, wire its ports — no rerouting,
    // and feed wires into a ref are never treated as consumer wires.
    let dir = make_project(
        "routing-explicit",
        "Beschattung",
        "extern quelle = VirtualIn(iname: \"VI1\")\n\
         extern spiegel = InputRef(mirrors: quelle)\n\
         \n\
         anzeige = And(\n\
         \tI1: spiegel.AQ,\n\
         \tI2: spiegel.Q,\n\
         )\n",
        Some(&base),
    );
    let out = lxir(&dir, &["compile"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let xml = std::fs::read_to_string(dir.join("out.Loxone")).unwrap();
    let spiegel_aq = port_of(&xml, "<C Type=\"InputRef\"", "AQ");
    assert_eq!(inputs_of(&xml, "Title=\"anzeige\"", "I1"), vec![spiegel_aq]);
}
