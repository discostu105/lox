//! The app catalogue in a config ZIP: `LoxAPP3.json`, which Loxone Config
//! writes next to `sps0.LoxCC` and the Miniserver serves as
//! `/data/LoxApp3.json` (the structure the app and `lox` read). `lox config
//! push` only replaces `sps0.LoxCC`, so the catalogue it uploads still
//! describes the program before the push. Whether the Miniserver rebuilds it
//! on start is not known yet; the push compares what it serves afterwards.

use anyhow::{Context, Result};
use lxir::diff::DocDiff;
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::io::Read;

pub(crate) const MEMBER: &str = "LoxAPP3.json";

/// A control in the app whose title, room or category the push changes, or
/// that the push deletes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct StaleEntry {
    pub uuid: String,
    /// The control's name in the app
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

/// The controls of `diff` (live → compiled) that the app lists, with what
/// the push changes about them. Objects the push adds are not in the app:
/// lxir never makes a block visible (visualization stays with Loxone Config).
pub(crate) fn stale_entries(catalogue: &Value, diff: &DocDiff) -> Vec<StaleEntry> {
    let names: BTreeMap<String, String> = catalogue
        .get("controls")
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(|controls| controls.values())
        .filter_map(|c| {
            let uuid = c.get("uuidAction")?.as_str()?.to_ascii_lowercase();
            let name = c.get("name").and_then(Value::as_str).unwrap_or("");
            Some((uuid, name.to_string()))
        })
        .collect();
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
    out
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

        let stale = stale_entries(&catalogue(), &d);

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

        assert!(stale_entries(&catalogue(), &d).is_empty());
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
