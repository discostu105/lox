//! Rooms and categories from source (#324, D136): a declared `Place` or
//! `Category` is minted with what Loxone Config 17.3 gives one it creates
//! (its permission group, a room's sort node), objects name it by slug or
//! title, and removing it takes its group and node along.

use loxone_config::catalogue::update;
use loxone_config::logic::{CompileOptions, Module, adopt_one, compile};
use loxone_config::uuid::parse_serial;
use loxone_config::xml::Element;
use loxone_config::{Lockfile, LoxoneDoc};

const ROOM: &str = "00000010-0000-0000-ffff000000000001";
const ROOM_GROUP: &str = "00000011-0000-0000-ffff000000000001";
const CAT: &str = "00000020-0000-0000-ffff000000000001";
const CAT_GROUP: &str = "00000021-0000-0000-ffff000000000001";
const SWITCH: &str = "00000030-0000-0000-ffff000000000001";

/// A project with one room, one category and one virtual input in both,
/// as Loxone Config 17.3 saves them.
fn base_text() -> String {
    format!(
        "<ControlList Version=\"283\" NextObj=\"50\" NextConst=\"1\" NextNote=\"1\" NextMem=\"1\">\r\n\
         \t<C Type=\"Document\" V=\"17031112\" U=\"00000001-0000-0000-ffff000000000001\" Title=\"Haus\" ConfigVersion=\"17031112\">\r\n\
         \t\t<C Type=\"CategoryCaption\" V=\"179\" U=\"00000002-0000-0000-ffff000000000001\" Title=\"Kategorien\" WF=\"16384\">\r\n\
         \t\t\t<C Type=\"Category\" V=\"179\" U=\"{CAT}\" Title=\"Beleuchtung\" WF=\"16384\" Icon=\"00000153-00ff-0000-0000000000000000\" Rating=\"1\" UseFav=\"true\" CatGroup=\"2\" PType=\"2\" RGR=\"{CAT_GROUP}\"/>\r\n\
         \t\t</C>\r\n\
         \t\t<C Type=\"PlaceCaption\" V=\"179\" U=\"00000003-0000-0000-ffff000000000001\" Title=\"Räume\" WF=\"16384\" u=\"00000000-0000-0000-0000000000000000\">\r\n\
         \t\t\t<nd u=\"00000012-0000-0000-ffff000000000001\" t=\"1\" ctrlUuid=\"{ROOM}\"/>\r\n\
         \t\t\t<C Type=\"Place\" V=\"179\" U=\"{ROOM}\" Title=\"Küche\" WF=\"16384\" Icon=\"0000015c-00ff-0000-0000000000000000\" PType=\"2\" RGR=\"{ROOM_GROUP}\"/>\r\n\
         \t\t</C>\r\n\
         \t\t<C Type=\"VirtualInCaption\" V=\"179\" U=\"00000004-0000-0000-ffff000000000001\" Title=\"Virtuelle Eingänge\">\r\n\
         \t\t\t<C Type=\"VirtualIn\" V=\"175\" U=\"{SWITCH}\" Title=\"Schalter\" Nio=\"1\" WF=\"16384\" Analog=\"false\">\r\n\
         \t\t\t\t<Co K=\"Q\" U=\"00000031-0000-0000-ffff000000000001\"/>\r\n\
         \t\t\t\t<IoData Visu=\"true\" Cr=\"{CAT}\" Pr=\"{ROOM}\"/>\r\n\
         \t\t\t</C>\r\n\
         \t\t</C>\r\n\
         \t\t<C Type=\"LoxCaption\" V=\"179\" U=\"00000005-0000-0000-ffff000000000001\" Title=\"Berechtigungsgruppen\" Cl=\"0,0,0\" WF=\"20480\" CaptionType=\"13\" SubType=\"13\">\r\n\
         \t\t\t<C Type=\"RightGroup\" V=\"179\" U=\"{CAT_GROUP}\" Title=\"Beleuchtung\" Cl=\"0,0,0\" WF=\"16384\" GT=\"2\" MG=\"\"/>\r\n\
         \t\t\t<C Type=\"RightGroup\" V=\"179\" U=\"{ROOM_GROUP}\" Title=\"Küche\" Cl=\"0,0,0\" WF=\"16384\" GT=\"1\" MG=\"\"/>\r\n\
         \t\t</C>\r\n\
         \t\t<C Type=\"Page\" V=\"175\" U=\"00000006-0000-0000-ffff000000000001\" Title=\"Regeln\"></C>\r\n\
         \t</C>\r\n\
         </ControlList>\r\n"
    )
}

fn text(doc: &LoxoneDoc) -> String {
    String::from_utf8(doc.xml.to_bytes()).unwrap()
}

fn base() -> LoxoneDoc {
    LoxoneDoc::parse(base_text().as_bytes()).unwrap()
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

const SWITCH_EXTERN: &str =
    "extern schalter = VirtualIn(uuid: \"00000030-0000-0000-ffff000000000001\")\n";

fn find(doc: &LoxoneDoc, pred: impl Fn(&Element) -> bool) -> Option<&Element> {
    fn walk<'a>(el: &'a Element, pred: &dyn Fn(&Element) -> bool) -> Option<&'a Element> {
        if pred(el) {
            return Some(el);
        }
        el.child_elements().find_map(|c| walk(c, pred))
    }
    walk(&doc.xml.root, &pred)
}

fn object<'a>(doc: &'a LoxoneDoc, ty: &str, title: &str) -> &'a Element {
    find(doc, |e| {
        e.name == "C" && e.attr("Type") == Some(ty) && e.attr("Title") == Some(title)
    })
    .unwrap_or_else(|| panic!("no {ty} {title:?}"))
}

fn attrs(el: &Element) -> Vec<(&str, &str)> {
    el.attrs
        .iter()
        .map(|a| (a.name.as_str(), a.value.as_str()))
        .collect()
}

fn io_data(doc: &LoxoneDoc) -> &Element {
    let switch = find(doc, |e| e.attr("U") == Some(SWITCH)).unwrap();
    switch
        .child_elements()
        .find(|c| c.name == "IoData")
        .unwrap()
}

#[test]
fn a_new_room_and_category_are_what_loxone_config_creates() {
    let src = "werkstatt = Place(\"Werkstatt\")\n\
               garten = Category(\"Garten\")\n";
    let mut lock = Lockfile::default();
    let out = compile(&base(), &Module::parse(src).unwrap(), &mut lock, &opts()).unwrap();

    let room = object(&out, "Place", "Werkstatt");
    let room_uuid = room.attr("U").unwrap();
    let room_group = room.attr("RGR").unwrap();
    // Oracle session 95: "Create room" in Loxone Config 17.3.
    assert_eq!(
        attrs(room),
        [
            ("Type", "Place"),
            ("V", "179"),
            ("U", room_uuid),
            ("Title", "Werkstatt"),
            ("WF", "16384"),
            ("Icon", "0000015c-00ff-0000-0000000000000000"),
            ("PType", "2"),
            ("RGR", room_group),
        ]
    );
    let group = find(&out, |e| e.attr("U") == Some(room_group)).unwrap();
    assert_eq!(
        attrs(group),
        [
            ("Type", "RightGroup"),
            ("V", "179"),
            ("U", room_group),
            ("Title", "Werkstatt"),
            ("Cl", "0,0,0"),
            ("WF", "16384"),
            ("GT", "1"),
            ("MG", ""),
        ]
    );
    // The room's node in the folder's sort tree, after the others.
    let folder = find(&out, |e| e.attr("Type") == Some("PlaceCaption")).unwrap();
    let nodes: Vec<&Element> = folder.child_elements().filter(|c| c.name == "nd").collect();
    assert_eq!(nodes.len(), 2);
    assert_eq!(nodes[1].attr("ctrlUuid"), Some(room_uuid));
    assert_eq!(
        nodes[1].attr("ico"),
        Some("0000015c-00ff-0000-0000000000000000")
    );
    assert_eq!(
        folder
            .child_elements()
            .map(|c| c.name.as_str())
            .collect::<Vec<_>>(),
        ["nd", "nd", "C", "C"]
    );

    let cat = object(&out, "Category", "Garten");
    let cat_group = cat.attr("RGR").unwrap();
    assert_eq!(
        attrs(cat)[4..],
        [
            ("WF", "16384"),
            ("Icon", "00000038-00ff-0000-0000000000000000"),
            ("PType", "2"),
            ("RGR", cat_group),
        ]
    );
    let group = find(&out, |e| e.attr("U") == Some(cat_group)).unwrap();
    assert_eq!(group.attr("GT"), Some("2"));
    assert_eq!(group.attr("Title"), Some("Garten"));

    // The lock pins the group and the node: a recompile against the same
    // base is byte-identical.
    let entry = &lock.objects["werkstatt"];
    assert_eq!(entry.companions.len(), 2);
    assert_eq!(entry.companions[0], room_group);
    assert_eq!(lock.objects["garten"].companions, [cat_group.to_string()]);
    let mut again = lock.clone();
    let out2 = compile(&base(), &Module::parse(src).unwrap(), &mut again, &opts()).unwrap();
    assert_eq!(text(&out), text(&out2));
    assert_eq!(lock, again);
}

#[test]
fn a_recompile_against_the_deployed_config_changes_nothing() {
    let src = "werkstatt = Place(\"Werkstatt\", PType: 6, Rating: 3, UseFav: on)\n";
    let mut lock = Lockfile::default();
    let out = compile(&base(), &Module::parse(src).unwrap(), &mut lock, &opts()).unwrap();
    let deployed = LoxoneDoc::parse(&out.xml.to_bytes()).unwrap();
    let again = compile(&deployed, &Module::parse(src).unwrap(), &mut lock, &opts()).unwrap();
    assert_eq!(text(&again), text(&out));
}

#[test]
fn settings_go_where_a_save_writes_them() {
    let src = "werkstatt = Place(\"Werkstatt\", PType: 6, Rating: 3, UseFav: on)\n\
               garten = Category(\"Garten\", Rating: 0, UseFav: off)\n";
    let out = compile(
        &base(),
        &Module::parse(src).unwrap(),
        &mut Lockfile::default(),
        &opts(),
    )
    .unwrap();
    let names = |el: &Element| -> Vec<String> { el.attrs.iter().map(|a| a.name.clone()).collect() };
    let room = object(&out, "Place", "Werkstatt");
    assert_eq!(
        names(room)[4..],
        ["WF", "Icon", "Rating", "UseFav", "PType", "RGR"]
    );
    assert_eq!(room.attr("PType"), Some("6"));
    assert_eq!(room.attr("Rating"), Some("3"));
    assert_eq!(room.attr("UseFav"), Some("true"));
    // No stars and no favourite are no attributes.
    let cat = object(&out, "Category", "Garten");
    assert_eq!(names(cat)[4..], ["WF", "Icon", "PType", "RGR"]);
}

#[test]
fn an_unknown_room_type_is_an_error() {
    let src = "werkstatt = Place(\"Werkstatt\", PType: 9)\n";
    let err = compile(
        &base(),
        &Module::parse(src).unwrap(),
        &mut Lockfile::default(),
        &opts(),
    )
    .unwrap_err()
    .to_string();
    assert!(err.contains("no room type"), "{err}");
    assert!(err.contains("6 (Outdoor area)"), "{err}");
}

#[test]
fn objects_name_a_room_by_slug_or_by_title() {
    let mut lock = Lockfile::default();
    let src = format!(
        "{SWITCH_EXTERN}werkstatt = Place(\"Werkstatt\")\n\
         garten = Category(\"Garten\")\n\
         schalter.room = werkstatt\n\
         schalter.category = garten\n"
    );
    let out = compile(&base(), &Module::parse(&src).unwrap(), &mut lock, &opts()).unwrap();
    let room = object(&out, "Place", "Werkstatt").attr("U").unwrap();
    let cat = object(&out, "Category", "Garten").attr("U").unwrap();
    assert_eq!(io_data(&out).attr("Pr"), Some(room));
    assert_eq!(io_data(&out).attr("Cr"), Some(cat));

    // By title: a minted room is in the compiled document.
    let src = format!(
        "{SWITCH_EXTERN}werkstatt = Place(\"Werkstatt\")\n\
         schalter.room = \"Werkstatt\"\n"
    );
    let out = compile(
        &base(),
        &Module::parse(&src).unwrap(),
        &mut Lockfile::default(),
        &opts(),
    )
    .unwrap();
    let room = object(&out, "Place", "Werkstatt").attr("U").unwrap();
    assert_eq!(io_data(&out).attr("Pr"), Some(room));

    // An extern room by its name.
    let src = format!(
        "{SWITCH_EXTERN}extern kueche = Place(uuid: \"{ROOM}\")\n\
         extern licht = Category(uuid: \"{CAT}\")\n\
         schalter.room = kueche\n\
         schalter.category = licht\n"
    );
    let out = compile(
        &base(),
        &Module::parse(&src).unwrap(),
        &mut Lockfile::default(),
        &opts(),
    )
    .unwrap();
    assert_eq!(io_data(&out).attr("Pr"), Some(ROOM));
}

#[test]
fn a_room_slug_must_name_a_room() {
    let src = format!(
        "{SWITCH_EXTERN}garten = Category(\"Garten\")\n\
         schalter.room = garten\n"
    );
    let err = Module::parse(&src)
        .and_then(|m| m.validate().map(|_| m))
        .and_then(|m| compile(&base(), &m, &mut Lockfile::default(), &opts()))
        .unwrap_err()
        .to_string();
    assert!(err.contains("`garten` is a Category, not a room"), "{err}");
}

#[test]
fn a_new_room_must_not_repeat_a_title() {
    let src = "kueche = Place(\"Küche\")\n";
    let err = compile(
        &base(),
        &Module::parse(src).unwrap(),
        &mut Lockfile::default(),
        &opts(),
    )
    .unwrap_err()
    .to_string();
    assert!(err.contains("already exists"), "{err}");
    assert!(err.contains(ROOM), "{err}");
}

/// The base's room and category, adopted: a label renames the
/// permission group with them, as Loxone Config does.
#[test]
fn renaming_an_adopted_room_renames_its_group() {
    let mut lock = Lockfile::default();
    let mut module = Module::parse("").unwrap();
    let adopted = adopt_one(&base(), ROOM, "kueche", &module, &mut lock).unwrap();
    module.items.extend(adopted.items);
    assert_eq!(module.to_text(), "kueche = Place(\"Küche\", PType: 2)\n");
    assert_eq!(lock.objects["kueche"].companions, Vec::<String>::new());
    let out = compile(&base(), &module, &mut lock.clone(), &opts()).unwrap();
    assert_eq!(text(&out), base_text());
    let src = "kueche = Place(\"Wohnküche\", PType: 2, Rating: 2)\n";
    let out = compile(&base(), &Module::parse(src).unwrap(), &mut lock, &opts()).unwrap();
    let room = object(&out, "Place", "Wohnküche");
    assert_eq!(room.attr("U"), Some(ROOM));
    assert_eq!(
        attrs(room)[4..],
        [
            ("WF", "16384"),
            ("Icon", "0000015c-00ff-0000-0000000000000000"),
            ("Rating", "2"),
            ("PType", "2"),
            ("RGR", ROOM_GROUP),
        ]
    );
    let group = find(&out, |e| e.attr("U") == Some(ROOM_GROUP)).unwrap();
    assert_eq!(group.attr("Title"), Some("Wohnküche"));
}

#[test]
fn a_removed_room_takes_its_group_and_node_along() {
    let src = "werkstatt = Place(\"Werkstatt\")\n";
    let mut lock = Lockfile::default();
    let out = compile(&base(), &Module::parse(src).unwrap(), &mut lock, &opts()).unwrap();
    let deployed = LoxoneDoc::parse(&out.xml.to_bytes()).unwrap();
    let removing = CompileOptions {
        allow_removals: true,
        ..opts()
    };
    let gone = compile(
        &deployed,
        &Module::parse("").unwrap(),
        &mut lock.clone(),
        &removing,
    )
    .unwrap();
    // `NextObj` only counts up.
    assert_eq!(
        text(&gone),
        base_text().replace("NextObj=\"50\"", "NextObj=\"51\"")
    );

    // Not while an object still names it: one assigned to the room in
    // Loxone Config.
    let room = object(&out, "Place", "Werkstatt").attr("U").unwrap();
    let assigned = text(&deployed).replace(&format!("Pr=\"{ROOM}\""), &format!("Pr=\"{room}\""));
    let assigned = LoxoneDoc::parse(assigned.as_bytes()).unwrap();
    let err = compile(&assigned, &Module::parse("").unwrap(), &mut lock, &removing)
        .unwrap_err()
        .to_string();
    assert!(err.contains("still name it"), "{err}");
}

/// The app's catalogue lists rooms and categories under `rooms` and
/// `cats`: a compile that adds, renames or removes one carries it over
/// in the shape Loxone Config 17.3 writes.
#[test]
fn the_catalogue_lists_new_rooms_and_categories() {
    let catalogue = format!(
        "{{\"lastModified\":\"2026-01-01 00:00:00\",\"msInfo\":{{}},\"controls\":{{}},\
         \"rooms\":{{\"{ROOM}\":{{\"uuid\":\"{ROOM}\",\"name\":\"Küche\",\"image\":\"IconsFilled/location-pin.svg\",\"defaultRating\":0,\"isFavorite\":false,\"type\":2,\"color\":\"#8DFF70\"}}}},\
         \"cats\":{{\"{CAT}\":{{\"uuid\":\"{CAT}\",\"name\":\"Beleuchtung\",\"image\":\"IconsFilled/lightbulb.svg\",\"defaultRating\":1,\"isFavorite\":true,\"type\":\"lights\",\"color\":\"#8DFF70\"}}}}}}"
    );
    let src = format!(
        "extern kueche = Place(uuid: \"{ROOM}\")\n\
         werkstatt = Place(\"Werkstatt\", PType: 6, Rating: 2)\n\
         garten = Category(\"Garten\", UseFav: on)\n"
    );
    let mut lock = Lockfile::default();
    let out = compile(&base(), &Module::parse(&src).unwrap(), &mut lock, &opts()).unwrap();
    let up = update(&catalogue, &base(), &out, "2026-01-02 00:00:00").unwrap();
    let v: serde_json::Value = serde_json::from_str(&up.text).unwrap();
    let room = object(&out, "Place", "Werkstatt").attr("U").unwrap();
    let cat = object(&out, "Category", "Garten").attr("U").unwrap();
    assert_eq!(
        v["rooms"][room],
        serde_json::json!({"uuid": room, "name": "Werkstatt", "image": "IconsFilled/location-pin.svg",
            "defaultRating": 2, "isFavorite": false, "type": 6, "color": "#8DFF70"})
    );
    assert_eq!(
        v["cats"][cat],
        serde_json::json!({"uuid": cat, "name": "Garten", "image": "IconsFilled/binders.svg",
            "defaultRating": 0, "isFavorite": true, "type": "undefined", "color": "#8DFF70"})
    );
    // The others stay as they were.
    assert_eq!(v["rooms"][ROOM]["name"], "Küche");
    assert_eq!(v["cats"][CAT]["type"], "lights");
    assert!(up.left.is_empty(), "{:?}", up.left);

    // Renamed and removed.
    let deployed = LoxoneDoc::parse(&out.xml.to_bytes()).unwrap();
    let src = "werkstatt = Place(\"Hobbyraum\", PType: 6, Rating: 2)\n";
    let removing = CompileOptions {
        allow_removals: true,
        ..opts()
    };
    let out2 = compile(
        &deployed,
        &Module::parse(src).unwrap(),
        &mut lock,
        &removing,
    )
    .unwrap();
    let up2 = update(&up.text, &deployed, &out2, "2026-01-03 00:00:00").unwrap();
    let v: serde_json::Value = serde_json::from_str(&up2.text).unwrap();
    assert_eq!(v["rooms"][room]["name"], "Hobbyraum");
    assert!(v["cats"].get(cat).is_none(), "{}", up2.text);
    assert_eq!(v["cats"][CAT]["name"], "Beleuchtung");
}
