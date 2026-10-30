//! The app catalogue in a config ZIP: `LoxAPP3.json`, which Loxone Config
//! writes next to `sps0.LoxCC` and the Miniserver serves as
//! `/data/LoxApp3.json` (the structure the app and `lox` read). `lox config
//! push` only replaces `sps0.LoxCC`, so the catalogue it uploads still
//! describes the program before the push. The Miniserver does not rebuild it
//! on start (2026-10-29: a push that added objects left the served catalogue
//! at the `lastModified` of the last save, #155); the push compares what it
//! serves afterwards.

use anyhow::{Context, Result};
use lxir::LoxoneDoc;
use lxir::diff::DocDiff;
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;

pub(crate) const MEMBER: &str = "LoxAPP3.json";

/// A control in the app whose title, room or category the push changes, or
/// that the push deletes, shows or hides in the app.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct StaleEntry {
    pub uuid: String,
    /// The control's name in the app (for an object the app does not list
    /// yet: its title, or its block type)
    pub app_name: String,
    /// What the push changes, e.g. `renamed "A" → "B"`
    pub change: String,
}

/// The catalogue of a config ZIP, if it has one.
pub(crate) fn from_zip(zip: &[u8]) -> Result<Option<Value>> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(zip))?;
    let mut file = match archive.by_name(MEMBER) {
        Ok(file) => file,
        Err(zip::result::ZipError::FileNotFound) => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    parse(&bytes).map(Some)
}

/// Parse a catalogue (Loxone writes a UTF-8 BOM).
pub(crate) fn parse(bytes: &[u8]) -> Result<Value> {
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    serde_json::from_slice(bytes).with_context(|| format!("{MEMBER} is not valid JSON"))
}

/// `lastModified`: when Loxone Config wrote the catalogue.
pub(crate) fn last_modified(catalogue: &Value) -> Option<&str> {
    catalogue.get("lastModified").and_then(Value::as_str)
}

/// The controls the catalogue lists: lowercase `uuidAction` → name.
fn control_names(catalogue: &Value) -> BTreeMap<String, String> {
    catalogue
        .get("controls")
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(|controls| controls.values())
        .filter_map(|c| {
            let uuid = c.get("uuidAction")?.as_str()?.to_ascii_lowercase();
            let name = c.get("name").and_then(Value::as_str).unwrap_or("");
            Some((uuid, name.to_string()))
        })
        .collect()
}

/// The controls of `diff` (live → compiled) that the app lists, with what
/// the push changes about them, and the objects whose visibility in the app
/// (`<IoData Visu="true">`) the push changes: a new visible object (lxir
/// mints a `VirtualIn` visible, as a save does) is missing from the
/// catalogue, a hidden one stays in it.
pub(crate) fn stale_entries(
    catalogue: &Value,
    diff: &DocDiff,
    live: &LoxoneDoc,
    compiled: &LoxoneDoc,
) -> Vec<StaleEntry> {
    let names = control_names(catalogue);
    let mut out = Vec::new();
    let mut push = |uuid: &str, change: String| {
        if let Some(name) = names.get(&uuid.to_ascii_lowercase()) {
            out.push(StaleEntry {
                uuid: uuid.to_string(),
                app_name: name.clone(),
                change,
            });
        }
    };
    let text = |v: &Option<String>| match v {
        Some(v) => format!("\"{v}\""),
        None => "none".to_string(),
    };
    for r in &diff.renamed {
        push(
            &r.uuid,
            format!("renamed {} → {}", text(&r.from), text(&r.to)),
        );
    }
    for p in &diff.placements {
        let from = p.from_title.clone().or_else(|| p.from.clone());
        let to = p.to_title.clone().or_else(|| p.to.clone());
        push(
            &p.object_uuid,
            format!("{} {} → {}", p.attr, text(&from), text(&to)),
        );
    }
    for o in &diff.removed {
        push(&o.uuid, "deleted".to_string());
    }
    let (was, now) = (visible(live), visible(compiled));
    let existing: BTreeSet<String> = live
        .objects()
        .into_iter()
        .map(|o| o.uuid.to_ascii_lowercase())
        .collect();
    for (uuid, label) in &now {
        if !was.contains_key(uuid) && !names.contains_key(uuid) {
            let change = if existing.contains(uuid) {
                "shown in the app, which does not list it yet"
            } else {
                "new and shown in the app, which does not list it yet"
            };
            out.push(StaleEntry {
                uuid: uuid.clone(),
                app_name: label.clone(),
                change: change.to_string(),
            });
        }
    }
    let compiled_uuids: BTreeSet<String> = compiled
        .objects()
        .into_iter()
        .map(|o| o.uuid.to_ascii_lowercase())
        .collect();
    for uuid in was.keys() {
        if !now.contains_key(uuid)
            && compiled_uuids.contains(uuid)
            && let Some(name) = names.get(uuid)
        {
            out.push(StaleEntry {
                uuid: uuid.clone(),
                app_name: name.clone(),
                change: "hidden from the app, which still lists it".to_string(),
            });
        }
    }
    out
}

/// The objects a document shows in the app (`<IoData Visu="true">`):
/// lowercase UUID → title, or the block type when it has none.
fn visible(doc: &LoxoneDoc) -> BTreeMap<String, String> {
    doc.objects()
        .into_iter()
        .filter(|o| {
            doc.element_at(&o.path).is_some_and(|el| {
                el.child_elements()
                    .any(|c| c.name == "IoData" && c.attr("Visu") == Some("true"))
            })
        })
        .map(|o| {
            let label = o.title.clone().filter(|t| !t.is_empty());
            (o.uuid.to_ascii_lowercase(), label.unwrap_or(o.block_type))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use lxir::LoxoneDoc;

    fn doc(title: &str, room: &str) -> LoxoneDoc {
        LoxoneDoc::parse(
            format!(
                r#"<?xml version="1.0" encoding="utf-8"?>
<ControlList>
	<C Type="Document" V="178" U="0f000000-0000-0000-ffff000000000000">
		<C Type="Place" U="0f000000-0000-00a1-ffff000000000000" Title="Küche"/>
		<C Type="Place" U="0f000000-0000-00a2-ffff000000000000" Title="Bad"/>
		<C Type="Page" U="0f000000-0000-0001-ffff000000000000" Title="Licht">
			<C Type="Switch" U="0f000000-0000-0002-ffff000000000000" Title="{title}">
				<IoData Pr="{room}"/>
			</C>
			<C Type="And" U="0f000000-0000-0003-ffff000000000000" Title="Logik"/>
		</C>
	</C>
</ControlList>
"#
            )
            .as_bytes(),
        )
        .unwrap()
    }

    const KUECHE: &str = "0f000000-0000-00a1-ffff000000000000";
    const BAD: &str = "0f000000-0000-00a2-ffff000000000000";

    fn catalogue() -> Value {
        parse(
            "\u{feff}{\"lastModified\":\"2026-10-29 17:15:28\",\"controls\":{\
             \"0f000000-0000-0002-ffff000000000000\":{\"name\":\"Licht Küche\",\
             \"uuidAction\":\"0f000000-0000-0002-ffff000000000000\"}}}"
                .as_bytes(),
        )
        .unwrap()
    }

    #[test]
    fn renames_and_moves_of_app_controls_are_stale_in_the_catalogue() {
        let live = doc("Licht", KUECHE);
        let compiled = doc("Deckenlicht", BAD);
        let d = lxir::diff::diff(&live, &compiled);

        let stale = stale_entries(&catalogue(), &d, &live, &compiled);

        let changes: Vec<&str> = stale.iter().map(|s| s.change.as_str()).collect();
        assert_eq!(
            changes,
            [
                "renamed \"Licht\" → \"Deckenlicht\"",
                "room \"Küche\" → \"Bad\""
            ]
        );
        assert!(stale.iter().all(|s| s.app_name == "Licht Küche"));
        assert_eq!(last_modified(&catalogue()), Some("2026-10-29 17:15:28"));
    }

    #[test]
    fn objects_the_app_does_not_list_are_not_reported() {
        let live = doc("Licht", KUECHE);
        let mut compiled = doc("Licht", KUECHE);
        compiled.remove_by_uuid("0f000000-0000-0003-ffff000000000000");
        let d = lxir::diff::diff(&live, &compiled);
        assert_eq!(d.removed.len(), 1);

        assert!(stale_entries(&catalogue(), &d, &live, &compiled).is_empty());
    }

    /// `doc` with the Switch shown in the app or not, plus an extra object.
    fn doc_with(switch_visu: bool, extra: &str) -> LoxoneDoc {
        let xml = String::from_utf8(doc("Licht", KUECHE).to_bytes()).unwrap();
        let io = if switch_visu {
            format!(r#"<IoData Visu="true" Pr="{KUECHE}"/>"#)
        } else {
            format!(r#"<IoData Pr="{KUECHE}"/>"#)
        };
        let xml = xml
            .replace(&format!(r#"<IoData Pr="{KUECHE}"/>"#), &io)
            .replace(
                r#"<C Type="And" U="0f000000-0000-0003-ffff000000000000" Title="Logik"/>"#,
                &format!(
                    r#"<C Type="And" U="0f000000-0000-0003-ffff000000000000" Title="Logik"/>{extra}"#
                ),
            );
        LoxoneDoc::parse(xml.as_bytes()).unwrap()
    }

    const NEW_INPUT: &str = r#"<C Type="VirtualIn" U="0f000000-0000-0004-ffff000000000000" Title="Taster"><IoData Visu="true" St="2"/></C>"#;
    const OLD_VISIBLE: &str = r#"<C Type="Memory" U="0f000000-0000-0005-ffff000000000000" Title="Merker"><IoData Visu="true"/></C>"#;

    #[test]
    fn a_new_object_shown_in_the_app_is_missing_from_the_catalogue() {
        let live = doc_with(true, OLD_VISIBLE);
        let compiled = doc_with(true, &format!("{OLD_VISIBLE}{NEW_INPUT}"));
        let d = lxir::diff::diff(&live, &compiled);

        let stale = stale_entries(&catalogue(), &d, &live, &compiled);

        // The Memory was visible and unlisted before the push: not the
        // push's doing, so it is not reported.
        assert_eq!(
            stale,
            [StaleEntry {
                uuid: "0f000000-0000-0004-ffff000000000000".into(),
                app_name: "Taster".into(),
                change: "new and shown in the app, which does not list it yet".into(),
            }]
        );
    }

    #[test]
    fn showing_and_hiding_existing_objects_is_stale_in_the_catalogue() {
        let hidden = r#"<C Type="Memory" U="0f000000-0000-0005-ffff000000000000" Title="Merker"><IoData/></C>"#;
        let live = doc_with(true, hidden);
        let compiled = doc_with(false, OLD_VISIBLE);
        let d = lxir::diff::diff(&live, &compiled);

        let stale = stale_entries(&catalogue(), &d, &live, &compiled);

        let changes: Vec<(&str, &str)> = stale
            .iter()
            .map(|s| (s.app_name.as_str(), s.change.as_str()))
            .collect();
        assert_eq!(
            changes,
            [
                ("Merker", "shown in the app, which does not list it yet"),
                ("Licht Küche", "hidden from the app, which still lists it"),
            ]
        );
    }

    #[test]
    fn a_zip_without_a_catalogue_has_none() {
        use std::io::Write;
        let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        w.start_file("sps0.LoxCC", zip::write::SimpleFileOptions::default())
            .unwrap();
        w.write_all(b"x").unwrap();
        let zip = w.finish().unwrap().into_inner();
        assert!(from_zip(&zip).unwrap().is_none());
    }
}
