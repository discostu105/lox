//! A block minted from source in the app (D66): it gets what a Loxone
//! Config save would give it — the special states its entry names — under
//! UUIDs the lock pins, and the catalogue lists it with a room and a
//! category.

use loxone_config::catalogue::update;
use loxone_config::logic::{CompileOptions, Module, compile};
use loxone_config::uuid::parse_serial;
use loxone_config::{Lockfile, LoxoneDoc};

const POOL: &str = "30000000-0000-0003-ffff504f94112233";
const HEIZUNG: &str = "30000000-0000-0007-ffff504f94112233";

fn base() -> LoxoneDoc {
    LoxoneDoc::parse(&std::fs::read("examples/configs/pool.Loxone").unwrap()).unwrap()
}

fn opts() -> CompileOptions {
    CompileOptions {
        machine: parse_serial("504F94112233").unwrap(),
        mint_time_unix: 1_767_225_600,
        page_title: Some("Pool".into()),
        allow_removals: false,
        accept_version: None,
        relayout: Vec::new(),
    }
}

const SOURCE: &str = "page \"Pool\"\n\
                      rollo = AutoJalousie(\"Rollo Pool\")\n\
                      rollo.room = \"Pool\"\n\
                      rollo.category = \"Heizung\"\n";

fn element<'a>(doc: &'a LoxoneDoc, uuid: &str) -> &'a loxone_config::xml::Element {
    let o = doc.objects().into_iter().find(|o| o.uuid == uuid).unwrap();
    doc.element_at(&o.path).unwrap()
}

#[test]
fn a_minted_jalousie_gets_its_states_and_an_app_entry() {
    let module = Module::parse(SOURCE).unwrap();
    let mut lock = Lockfile::default();
    let out = compile(&base(), &module, &mut lock, &opts()).unwrap();
    let locked = &lock.objects["rollo"];
    assert_eq!(locked.states.len(), 7);
    let el = element(&out, &locked.uuid);
    assert_eq!(el.attr("SpStates"), Some(locked.states.join(",").as_str()));
    let children: Vec<&str> = el
        .child_elements()
        .filter(|c| c.name != "Co")
        .map(|c| c.name.as_str())
        .collect();
    assert_eq!(children, ["IoData", "COHist"]);
    let io = el.child_elements().find(|c| c.name == "IoData").unwrap();
    assert_eq!(io.attr("Visu"), Some("true"));
    assert_eq!(io.attr("Pr"), Some(POOL));

    // The same UUIDs against the old base, and a rebuild of its own output
    // is byte-identical.
    let mut again = lock.clone();
    let out2 = compile(&base(), &module, &mut again, &opts()).unwrap();
    assert!(out2.to_bytes() == out.to_bytes());
    let out3 = compile(&out, &module, &mut again, &opts()).unwrap();
    assert!(out3.to_bytes() == out.to_bytes());
    assert_eq!(again.objects["rollo"].states, locked.states);

    // The catalogue lists it.
    let catalogue = format!(
        "{{\"lastModified\":\"2026-01-01 00:00:00\",\"controls\":{{}},\
         \"rooms\":{{\"{POOL}\":{{\"name\":\"Pool\"}}}},\
         \"cats\":{{\"{HEIZUNG}\":{{\"name\":\"Heizung\"}}}}}}"
    );
    let u = update(&catalogue, &base(), &out, "2026-01-02 00:00:00").unwrap();
    assert!(u.left.is_empty(), "{:?}", u.left);
    assert_eq!(u.applied.len(), 1);
    let v: serde_json::Value = serde_json::from_str(&u.text).unwrap();
    let entry = &v["controls"][format!("U:{}", locked.uuid)];
    assert_eq!(entry["type"], "Jalousie");
    assert_eq!(entry["name"], "Rollo Pool");
    assert_eq!(entry["room"], POOL);
    assert_eq!(entry["details"]["hasHistory"], 20);
    assert_eq!(entry["states"]["infoText"], locked.states[0].as_str());
    assert_eq!(
        entry["states"]["adjustingEndPos"],
        locked.states[6].as_str()
    );
    assert_eq!(entry["states"]["up"], locked.ports["OutputUp"].as_str());
    assert!(entry.get("defaultIcon").is_none());
}

#[test]
fn a_jalousie_without_a_room_is_named_not_listed() {
    let module = Module::parse("page \"Pool\"\nrollo = AutoJalousie(\"Rollo Pool\")\n").unwrap();
    let out = compile(&base(), &module, &mut Lockfile::default(), &opts()).unwrap();
    let catalogue = format!(
        "{{\"lastModified\":\"2026-01-01 00:00:00\",\"controls\":{{}},\
         \"rooms\":{{\"{POOL}\":{{\"name\":\"Pool\"}}}},\
         \"cats\":{{\"{HEIZUNG}\":{{\"name\":\"Heizung\"}}}}}}"
    );
    let u = update(&catalogue, &base(), &out, "2026-01-02 00:00:00").unwrap();
    assert_eq!(u.text, catalogue);
    assert_eq!(u.left.len(), 1);
    assert!(u.left[0].what.contains("needs a room and a category"));
}
