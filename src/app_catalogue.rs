//! The app catalogue in a config ZIP: `LoxAPP3.json`, which Loxone Config
//! writes next to `sps0.LoxCC` and the Miniserver serves as
//! `/data/LoxApp3.json` (the structure the app and `lox` read). The
//! Miniserver does not rebuild it on start (2026-10-29: a push that added
//! objects left the served catalogue at the `lastModified` of the last save,
//! #155), so `lox config push` carries the changes to the app's controls
//! into it ([`lxir::catalogue`]) and names what it cannot.

use anyhow::{Context, Result};
use lxir::LoxoneDoc;
use serde::Serialize;
use serde_json::Value;

pub(crate) const MEMBER: &str = lxir::catalogue::MEMBER;

/// A control of the app whose entry the push changes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct StaleEntry {
    pub uuid: String,
    /// The control's name in the app (for one the app does not list yet:
    /// the name it gets)
    pub app_name: String,
    /// What the push changes, e.g. `renamed "A" → "B"`
    pub change: String,
}

impl From<lxir::catalogue::Change> for StaleEntry {
    fn from(c: lxir::catalogue::Change) -> Self {
        StaleEntry {
            uuid: c.uuid,
            app_name: c.name,
            change: c.what,
        }
    }
}

/// The catalogue to upload with the compiled config.
pub(crate) struct Update {
    /// The catalogue of the live download (parsed)
    pub live: Value,
    /// The new file, when it differs from the live one
    pub content: Option<Vec<u8>>,
    /// What it carries over
    pub updated: Vec<StaleEntry>,
    /// What stays stale until Loxone Config saves the project
    pub stale: Vec<StaleEntry>,
}

/// Carry the changes of the app's controls from `live` to `compiled` into the
/// catalogue of `live_zip`; `None` when the download has none. A new
/// `lastModified` (now, local time) makes the app reload the structure.
pub(crate) fn update(
    live_zip: &[u8],
    live: &LoxoneDoc,
    compiled: &LoxoneDoc,
) -> Result<Option<Update>> {
    let Some(bytes) = lxir::archive::read_entry(live_zip, MEMBER)? else {
        return Ok(None);
    };
    let parsed = parse(&bytes)?;
    let text = String::from_utf8(bytes).with_context(|| format!("{MEMBER} is not UTF-8"))?;
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let u = lxir::catalogue::update(&text, live, compiled, &now).map_err(anyhow::Error::msg)?;
    Ok(Some(Update {
        live: parsed,
        content: (u.text != text).then(|| u.text.into_bytes()),
        updated: u.applied.into_iter().map(Into::into).collect(),
        stale: u.left.into_iter().map(Into::into).collect(),
    }))
}

/// Parse a catalogue (Loxone writes a UTF-8 BOM).
pub(crate) fn parse(bytes: &[u8]) -> Result<Value> {
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    serde_json::from_slice(bytes).with_context(|| format!("{MEMBER} is not valid JSON"))
}

/// `lastModified`: when the catalogue was written.
pub(crate) fn last_modified(catalogue: &Value) -> Option<&str> {
    catalogue.get("lastModified").and_then(Value::as_str)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn zip_with(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for (name, data) in entries {
            w.start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            w.write_all(data).unwrap();
        }
        w.finish().unwrap().into_inner()
    }

    fn doc(title: &str) -> LoxoneDoc {
        LoxoneDoc::parse(
            format!(
                r#"<?xml version="1.0" encoding="utf-8"?>
<ControlList>
	<C Type="Document" V="178" U="0f000000-0000-0000-ffff000000000000">
		<C Type="Page" U="0f000000-0000-0001-ffff000000000000" Title="Licht">
			<C Type="Switch" U="0f000000-0000-0002-ffff000000000000" Title="{title}">
				<IoData Visu="true"/>
			</C>
		</C>
	</C>
</ControlList>
"#
            )
            .as_bytes(),
        )
        .unwrap()
    }

    const CATALOGUE: &str = "\u{feff}{\"lastModified\":\"2026-10-29 17:15:28\",\"controls\":{\
         \"U:0f000000-0000-0002-ffff000000000000\":{\"name\":\"Licht\",\
         \"uuidAction\":\"0f000000-0000-0002-ffff000000000000\"}}}";

    #[test]
    fn a_rename_is_carried_into_the_catalogue_with_a_new_last_modified() {
        let zip = zip_with(&[("sps0.LoxCC", b"x"), (MEMBER, CATALOGUE.as_bytes())]);
        let u = update(&zip, &doc("Licht"), &doc("Deckenlicht"))
            .unwrap()
            .unwrap();

        assert_eq!(last_modified(&u.live), Some("2026-10-29 17:15:28"));
        assert_eq!(
            u.updated,
            [StaleEntry {
                uuid: "0f000000-0000-0002-ffff000000000000".into(),
                app_name: "Licht".into(),
                change: "renamed \"Licht\" → \"Deckenlicht\"".into(),
            }]
        );
        assert!(u.stale.is_empty());
        let new = parse(u.content.as_deref().unwrap()).unwrap();
        assert_eq!(
            new["controls"]["U:0f000000-0000-0002-ffff000000000000"]["name"],
            "Deckenlicht"
        );
        assert_ne!(last_modified(&new), last_modified(&u.live));

        // Nothing for the app: the file stays as it is.
        let u = update(&zip, &doc("Licht"), &doc("Licht")).unwrap().unwrap();
        assert!(u.content.is_none() && u.updated.is_empty());
    }

    #[test]
    fn a_zip_without_a_catalogue_has_none() {
        let zip = zip_with(&[("sps0.LoxCC", b"x")]);
        assert!(update(&zip, &doc("A"), &doc("B")).unwrap().is_none());
    }
}
