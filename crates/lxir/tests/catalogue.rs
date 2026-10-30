//! `lxir::catalogue` against genuine Loxone Config saves (opt-in): each
//! directory below `LXIR_CATALOGUE_CORPUS` holds a `prog.Loxone` and the
//! `LoxAPP3.json` Loxone Config wrote with it.
//!
//! ```sh
//! LXIR_CATALOGUE_CORPUS=~/loxone-saves cargo test -p lxir --test catalogue
//! ```
//!
//! For each save: the catalogue of an unchanged program stays as it is, and
//! hiding the controls lxir writes entries for, then showing them again,
//! gives back Loxone Config's entries byte for byte.

use lxir::LoxoneDoc;
use lxir::catalogue::update;

#[test]
fn hidden_and_shown_again_gives_loxone_configs_entries() {
    let Ok(dir) = std::env::var("LXIR_CATALOGUE_CORPUS") else {
        eprintln!("LXIR_CATALOGUE_CORPUS not set — skipping");
        return;
    };
    let mut saves = 0;
    for d in std::fs::read_dir(&dir).unwrap() {
        let d = d.unwrap().path();
        let (Ok(xml), Ok(cat)) = (
            std::fs::read(d.join("prog.Loxone")),
            std::fs::read_to_string(d.join("LoxAPP3.json")),
        ) else {
            continue;
        };
        saves += 1;
        let doc = LoxoneDoc::parse(&xml).unwrap();
        let same = update(&cat, &doc, &doc, "2000-01-01 00:00:00").unwrap();
        assert_eq!(same.text, cat, "{}", d.display());
        assert!(same.applied.is_empty() && same.left.is_empty());

        let mut hidden = doc.clone();
        let mut n = 0;
        for o in doc.objects() {
            if !["PButtonT", "PushButton", "VirtualIn"].contains(&o.block_type.as_str()) {
                continue;
            }
            let el = hidden.element_at_mut(&o.path).unwrap();
            for io in el.child_elements_mut().filter(|c| c.name == "IoData") {
                if io.attr("Visu") == Some("true") {
                    io.set_attr("Visu", "false");
                    n += 1;
                }
            }
        }
        let stripped = update(&cat, &doc, &hidden, "2000-01-01 00:00:00").unwrap();
        assert!(stripped.left.is_empty(), "{:?}", stripped.left);
        let back = update(&stripped.text, &hidden, &doc, "2000-01-01 00:00:00").unwrap();
        let entries = |t: &str| -> Vec<String> {
            let v: serde_json::Value =
                serde_json::from_str(t.trim_start_matches('\u{feff}')).unwrap();
            let mut out: Vec<String> = v["controls"]
                .as_object()
                .unwrap()
                .iter()
                .map(|(k, e)| format!("{k}={e}"))
                .collect();
            out.sort();
            out
        };
        let left: Vec<_> = back
            .left
            .iter()
            .filter(|c| !c.what.contains("icon"))
            .collect();
        assert!(left.is_empty(), "{}: {left:?}", d.display());
        let with_icons = back.left.len();
        let (want, got) = (entries(&cat), entries(&back.text));
        if with_icons == 0 {
            assert_eq!(got, want, "{}", d.display());
        }
        eprintln!(
            "{}: {n} hidden, {} removed, {} added back, {with_icons} with an icon left",
            d.display(),
            stripped.applied.len(),
            back.applied.len()
        );
        // Byte for byte: every entry added back is the one Loxone Config
        // wrote.
        for c in &back.applied {
            let key = format!("\"U:{}\":", c.uuid);
            assert_eq!(
                entry_text(&cat, &key),
                entry_text(&back.text, &key),
                "{}",
                c.name
            );
        }
    }
    eprintln!("{saves} saves");
}

/// The text of the entry behind `key`.
fn entry_text<'t>(text: &'t str, key: &str) -> &'t str {
    let rest = &text[text.find(key).unwrap() + key.len()..];
    let mut values = serde_json::Deserializer::from_str(rest).into_iter::<serde_json::Value>();
    values.next().unwrap().unwrap();
    &rest[..values.byte_offset()]
}
