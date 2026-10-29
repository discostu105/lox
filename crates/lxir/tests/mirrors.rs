//! End-to-end tests for minted mirror blocks (D33): `x = InputRef(mirrors: y)`
//! through the real binary — identity attributes on the element, target
//! resolution against externs and managed blocks, and the refusals that
//! keep a ref from being minted wrong.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn lxir(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_lxir"))
        .current_dir(dir)
        .args(args)
        .output()
        .expect("run lxir")
}

fn make_project(name: &str, module: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    if dir.exists() {
        std::fs::remove_dir_all(&dir).unwrap();
    }
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::copy(
        concat!(env!("CARGO_MANIFEST_DIR"), "/examples/configs/haus.Loxone"),
        dir.join("haus.Loxone"),
    )
    .unwrap();
    std::fs::write(
        dir.join("lox.toml"),
        "base = \"haus.Loxone\"\n\
         module = \"src\"\n\
         lock = \"haus.lock.json\"\n\
         serial = \"504F94112233\"\n\
         page = \"Beschattung\"\n",
    )
    .unwrap();
    std::fs::write(dir.join("src/main.lxir"), module).unwrap();
    dir
}

/// The one full InputRef element of the compiled output.
fn input_ref_element(dir: &Path) -> String {
    let xml = std::fs::read(dir.join("out.Loxone")).unwrap();
    let xml = String::from_utf8_lossy(&xml);
    let start = xml.find("<C Type=\"InputRef\"").expect("minted InputRef");
    let end = xml[start..].find("</C>").expect("closed element") + start;
    xml[start..end].to_string()
}

fn attr(el: &str, name: &str) -> Option<String> {
    let pat = format!(" {name}=\"");
    let i = el.find(&pat)? + pat.len();
    Some(el[i..i + el[i..].find('"')?].to_string())
}

#[test]
fn minted_ref_carries_the_mirror_identity() {
    // Mirror of an extern: `Ref=` must point at the resolved VirtualIn,
    // with the corpus-verified LinkRefType code and no Analog flag.
    let dir = make_project(
        "mirrors-extern",
        "extern quelle = VirtualIn(iname: \"VI1\")\n\
         extern senke = VirtualIn(iname: \"VI3\")\n\
         \n\
         spiegel = InputRef(\n\
         \tmirrors: quelle,\n\
         \tAI: quelle.Q,\n\
         )\n\
         \n\
         anzeige = And(\n\
         \tI1: spiegel.Q,\n\
         \tI2: spiegel.AQ,\n\
         )\n\
         \n\
         senke.Qm <- anzeige.Q\n",
    );
    let out = lxir(&dir, &["compile"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let el = input_ref_element(&dir);
    // The extern resolution pins VI1's uuid; the mirror must carry it.
    let lock = std::fs::read_to_string(dir.join("haus.lock.json")).unwrap();
    assert!(
        lock.contains(&format!("\"{}\"", attr(&el, "Ref").unwrap())),
        "Ref= must be the locked uuid of `quelle`: {el}"
    );
    assert_eq!(attr(&el, "LinkRefType").as_deref(), Some("71"), "{el}");
    assert_eq!(attr(&el, "Nio").as_deref(), Some("4"), "{el}");
    assert_eq!(attr(&el, "Analog"), None, "VirtualIn mirrors are digital");
    // The GUI derives a ref's title from its target (oracle session 11):
    // the compiler emits VI1's title, not the slug.
    assert!(
        attr(&el, "Title").unwrap().contains("entemperatur"),
        "ref title must be the target's: {el}"
    );
    // Deterministic: an unchanged recompile reproduces the output.
    let before = std::fs::read(dir.join("out.Loxone")).unwrap();
    assert!(lxir(&dir, &["compile"]).status.success());
    assert_eq!(before, std::fs::read(dir.join("out.Loxone")).unwrap());
}

#[test]
fn minted_ref_may_mirror_a_managed_block() {
    // A block minted this very compile is a legal target — the corpus
    // shows Memory mirrors (LinkRefType 320).
    let dir = make_project(
        "mirrors-managed",
        "extern quelle = VirtualIn(iname: \"VI1\")\n\
         extern senke = VirtualIn(iname: \"VI3\")\n\
         \n\
         zustand = Memory(\n\
         \tInput: quelle.Q,\n\
         )\n\
         \n\
         spiegel = InputRef(\n\
         \tmirrors: zustand,\n\
         \tAI: zustand.AQ,\n\
         )\n\
         \n\
         senke.Qm <- spiegel.Q\n",
    );
    let out = lxir(&dir, &["compile"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let el = input_ref_element(&dir);
    assert_eq!(attr(&el, "LinkRefType").as_deref(), Some("320"), "{el}");
    // Ref= must equal the minted Memory's own uuid.
    let xml = std::fs::read_to_string(dir.join("out.Loxone")).unwrap();
    let mem_at = xml.find("<C Type=\"Memory\"").expect("minted Memory");
    let mem_uuid = attr(&xml[mem_at..mem_at + 400], "U").unwrap();
    assert_eq!(attr(&el, "Ref").as_deref(), Some(mem_uuid.as_str()), "{el}");
    // Managed target without a label: the derived title is its slug.
    assert_eq!(attr(&el, "Title").as_deref(), Some("zustand"), "{el}");
}

#[test]
fn mirror_refusals() {
    // No `mirrors:` at all — a bare ref block is meaningless.
    let dir = make_project(
        "mirrors-missing",
        "extern senke = VirtualIn(iname: \"VI3\")\n\
         spiegel = InputRef()\n\
         senke.Qm <- spiegel.Q\n",
    );
    let out = lxir(&dir, &["compile"]);
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(err.contains("mirrors: <name>"), "{err}");

    // `mirrors:` on a non-ref type is refused by name.
    let dir = make_project(
        "mirrors-nonref",
        "extern quelle = VirtualIn(iname: \"VI1\")\n\
         extern senke = VirtualIn(iname: \"VI3\")\n\
         g = And(\n\
         \tmirrors: quelle,\n\
         \tI1: quelle.Q,\n\
         )\n\
         senke.Qm <- g.Q\n",
    );
    let out = lxir(&dir, &["compile"]);
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(
        err.contains("applies to InputRef/OutputRef only, not And"),
        "{err}"
    );

    // A literal target is not a name.
    let dir = make_project(
        "mirrors-literal",
        "extern senke = VirtualIn(iname: \"VI3\")\n\
         spiegel = InputRef(mirrors: \"VI1\")\n\
         senke.Qm <- spiegel.Q\n",
    );
    let out = lxir(&dir, &["compile"]);
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(err.contains("not a literal"), "{err}");

    // An explicit label contradicts the GUI-derived title.
    let dir = make_project(
        "mirrors-label",
        "extern quelle = VirtualIn(iname: \"VI1\")\n\
         extern senke = VirtualIn(iname: \"VI3\")\n\
         spiegel = InputRef(\n\
         \t\"Eigener Name\",\n\
         \tmirrors: quelle,\n\
         )\n\
         senke.Qm <- spiegel.Q\n",
    );
    let out = lxir(&dir, &["compile"]);
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(err.contains("derived from its `mirrors:` target"), "{err}");

    // A target type without a verified LinkRefType code refuses the mint.
    let dir = make_project(
        "mirrors-unknown-code",
        "extern quelle = VirtualIn(iname: \"VI1\")\n\
         extern senke = VirtualIn(iname: \"VI3\")\n\
         g = And(\n\
         \tI1: quelle.Q,\n\
         \tI2: quelle.Q,\n\
         )\n\
         spiegel = InputRef(mirrors: g)\n\
         senke.Qm <- spiegel.Q\n",
    );
    let out = lxir(&dir, &["compile"]);
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(err.contains("no verified `LinkRefType=` code"), "{err}");
}
