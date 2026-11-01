//! Tree devices the source creates: added to their extension's branch
//! once, unpaired, kept under the UUID the lockfile records, and deleted
//! only by `removed`.

use loxone_config::logic::{CompileOptions, Module, compile, unpaired_created};
use loxone_config::uuid::parse_serial;
use loxone_config::{Lockfile, LoxoneDoc};

const TREE: &str = "00000003-0000-0000-ffff000000000001";
const LEFT: &str = "00000006-0000-0000-ffff000000000001";
const RIGHT: &str = "00000007-0000-0000-ffff000000000001";
const LIGHT: &str = "00000008-0000-0000-ffff000000000001";

/// A Tree extension with both branches and a page with a virtual output.
fn base() -> LoxoneDoc {
    LoxoneDoc::parse(
        format!(
            "<ControlList Version=\"1\" NextObj=\"50\">\r\n\
             \t<C Type=\"Document\" U=\"00000001-0000-0000-ffff000000000001\" ConfigVersion=\"17020828\">\r\n\
             \t\t<C Type=\"LoxTree\" V=\"178\" U=\"{TREE}\" Title=\"Tree\" WF=\"16384\" Serial=\"13000003\" Installation=\"\">\r\n\
             \t\t\t<C Type=\"LoxCaption\" V=\"178\" U=\"{LEFT}\" Title=\"Tree links\" CaptionType=\"2\"/>\r\n\
             \t\t\t<C Type=\"LoxCaption\" V=\"178\" U=\"{RIGHT}\" Title=\"Tree rechts\" CaptionType=\"3\"/>\r\n\
             \t\t</C>\r\n\
             \t\t<C Type=\"Page\" U=\"00000005-0000-0000-ffff000000000001\" Title=\"Regeln\">\r\n\
             \t\t\t<C Type=\"VirtualOut\" U=\"{LIGHT}\" Title=\"Licht\">\r\n\
             \t\t\t\t<Co K=\"I\" U=\"00000009-0000-0000-ffff000000000001\"/>\r\n\
             \t\t\t</C>\r\n\
             \t\t</C>\r\n\
             \t</C>\r\n\
             </ControlList>\r\n"
        )
        .as_bytes(),
    )
    .unwrap()
}

fn opts() -> CompileOptions {
    CompileOptions {
        machine: parse_serial("504F94112233").unwrap(),
        mint_time_unix: 1_767_225_600,
        page_title: Some("Regeln".into()),
        allow_removals: false,
        accept_version: None,
        relayout: Vec::new(),
    }
}

const DECLS: &str = r#"
tree = LoxTree("Tree", serial: "13000003")
melder = TreeDevice("Melder Flur", kind: "PresenceSensorTree", tree: tree, branch: "left")
bewegung = TreeSensor("Bewegung Flur", device: melder, iname: "I1", MinTime: 20)
"#;

/// The element with `uuid`, and the uuid of its parent.
fn element(doc: &LoxoneDoc, uuid: &str) -> Option<(loxone_config::xml::Element, String)> {
    let objects = doc.objects();
    let o = objects.iter().find(|o| o.uuid == uuid)?;
    let parent = objects
        .iter()
        .find(|p| p.path.len() + 1 == o.path.len() && o.path.starts_with(&p.path))
        .map(|p| p.uuid.clone())
        .unwrap_or_default();
    Some((doc.element_at(&o.path)?.clone(), parent))
}

#[test]
fn a_created_device_is_added_once_and_converges() {
    let module = Module::parse(DECLS).unwrap();
    let mut lock = Lockfile::default();
    let out = compile(&base(), &module, &mut lock, &opts()).unwrap();

    let uuid = lock.hardware["melder"].clone();
    let (el, parent) = element(&out, &uuid).unwrap();
    assert_eq!(parent, LEFT);
    assert_eq!(el.attr("TId"), Some("PresenceSensorTree"));
    assert_eq!(el.attr("Title"), Some("Melder Flur"));
    assert_eq!(el.attr("Serial"), Some("00000000"));
    assert_eq!(el.attr("V"), Some("178"));
    assert_eq!(unpaired_created(&out, &lock), ["melder"]);
    // The declared input below it is matched by its designation.
    let motion = &lock.externals["bewegung"];
    assert_eq!(motion.matched_by, "iname");
    let (m, mparent) = element(&out, &motion.uuid).unwrap();
    assert_eq!(mparent, uuid);
    assert_eq!(m.attr("Title"), Some("Bewegung Flur"));
    assert_eq!(m.attr("MinTime"), Some("20"));

    // Against its own output: nothing changes.
    let (hardware, mint) = (lock.hardware.clone(), lock.counters.next_mint);
    let again = compile(&out, &module, &mut lock, &opts()).unwrap();
    assert_eq!(out.to_bytes(), again.to_bytes());
    assert_eq!(
        (lock.hardware.clone(), lock.counters.next_mint),
        (hardware, mint)
    );

    // Against a base without it: it comes back under the same UUID.
    let older = compile(&base(), &module, &mut lock, &opts()).unwrap();
    assert_eq!(lock.hardware["melder"], uuid);
    assert_eq!(element(&older, &uuid).unwrap().1, LEFT);
}

#[test]
fn a_created_device_is_deleted_only_by_removed() {
    let module = Module::parse(DECLS).unwrap();
    let mut lock = Lockfile::default();
    let out = compile(&base(), &module, &mut lock, &opts()).unwrap();
    let uuid = lock.hardware["melder"].clone();

    let tree_only = r#"tree = LoxTree("Tree", serial: "13000003")"#;
    let err = compile(
        &out,
        &Module::parse(tree_only).unwrap(),
        &mut lock.clone(),
        &opts(),
    )
    .unwrap_err()
    .to_string();
    assert!(err.contains("add `removed melder`"), "{err}");

    let removing = Module::parse(&format!("{tree_only}\nremoved melder")).unwrap();
    let gone = compile(&out, &removing, &mut lock, &opts()).unwrap();
    assert!(element(&gone, &uuid).is_none());
    assert!(lock.hardware.is_empty());
    // An older base still carrying it loses it again.
    let again = compile(&out, &removing, &mut lock, &opts()).unwrap();
    assert!(element(&again, &uuid).is_none());
    // Once a base without it was seen, the removal is done.
    compile(&gone, &removing, &mut lock, &opts()).unwrap();
    let err = compile(&gone, &removing, &mut lock, &opts())
        .unwrap_err()
        .to_string();
    assert!(err.contains("delete the `removed` line"), "{err}");
}

#[test]
fn a_device_feeding_a_wire_of_loxone_config_is_not_removed() {
    let licht = format!("extern licht = VirtualOut(uuid: \"{LIGHT}\")\n");
    let wired = format!("{DECLS}{licht}licht.I <- bewegung.Q\n");
    let mut lock = Lockfile::default();
    let out = compile(&base(), &Module::parse(&wired).unwrap(), &mut lock, &opts()).unwrap();
    let removing =
        format!("tree = LoxTree(\"Tree\", serial: \"13000003\")\nremoved melder\n{licht}");
    // The source's own wire leaves with the device.
    let gone = compile(
        &out,
        &Module::parse(&removing).unwrap(),
        &mut lock.clone(),
        &opts(),
    )
    .unwrap();
    assert!(gone.wires().is_empty());
    // The same wire, drawn in Loxone Config, holds the device.
    lock.extern_wires.clear();
    let err = compile(&out, &Module::parse(&removing).unwrap(), &mut lock, &opts())
        .unwrap_err()
        .to_string();
    assert!(err.contains("drives Licht"), "{err}");
}

#[test]
fn a_device_taken_over_by_uuid_stays() {
    let module = Module::parse(DECLS).unwrap();
    let mut lock = Lockfile::default();
    let out = compile(&base(), &module, &mut lock, &opts()).unwrap();
    let uuid = lock.hardware["melder"].clone();
    let by_uuid = format!(
        "tree = LoxTree(\"Tree\", serial: \"13000003\")\nmelder = TreeDevice(\"Melder Flur\", \
         uuid: \"{uuid}\")\n"
    );
    let kept = compile(&out, &Module::parse(&by_uuid).unwrap(), &mut lock, &opts()).unwrap();
    assert!(element(&kept, &uuid).is_some());
    assert!(lock.hardware.is_empty());
    assert!(unpaired_created(&kept, &lock).is_empty());
}

#[test]
fn refusals_name_the_fix() {
    let tree = "tree = LoxTree(\"Tree\", serial: \"13000003\")\n";
    let fails = |src: &str| {
        compile(
            &base(),
            &Module::parse(src).unwrap(),
            &mut Lockfile::default(),
            &opts(),
        )
        .unwrap_err()
        .to_string()
    };
    let e = fails(&format!(
        "{tree}x = TreeDevice(\"X\", kind: \"RoomComfortSensorTree\", tree: tree)"
    ));
    assert!(e.contains("cannot create a `RoomComfortSensorTree`"), "{e}");
    assert!(e.contains("PresenceSensorTree"), "{e}");
    let e = fails("x = TreeDevice(\"X\", kind: \"TouchTree\", tree: nope)");
    assert!(e.contains("`tree: nope` names no Tree extension"), "{e}");
    let e = fails(&format!(
        "{tree}x = TreeDevice(\"X\", kind: \"TouchTree\", tree: tree)"
    ));
    assert!(e.contains("has two branches"), "{e}");
    let e = fails(&format!(
        "{tree}x = TreeDevice(\"X\", kind: \"TouchTree\", tree: tree, branch: \"up\")"
    ));
    assert!(e.contains("`branch:` is \"left\" or \"right\""), "{e}");
    let e = fails(&format!(
        "{tree}x = TreeDevice(\"X\", kind: \"TouchTree\", serial: \"B0A1B2C3\", tree: tree)"
    ));
    assert!(e.contains("create one with `kind:"), "{e}");
}
