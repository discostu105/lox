//! The Miniserver's custom changes: what the app, the web UI or the API
//! changed at runtime and Loxone Config has not saved into the program file
//! yet (light moods, timer schedules, autopilot rules, ...). The Miniserver
//! keeps them as one LoxCC file per object in `/sys/custom/<UUID>.LoxCC`. A
//! download of the program file does not contain them, and loading an
//! uploaded `sps_new.zip` deletes them (def.log: `Loading sps_new.zip - Remove
//! old custom changes`). Loxone Config merges them in first ("Load from
//! Miniserver"); lox can only name them and refuse.

use lxir::LoxoneDoc;
use lxir::archive;
use lxir::xml::XmlDocument;
use serde::Serialize;

/// One pending custom change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct CustomChange {
    pub uuid: String,
    /// The object's type, if known (`DayTimer`, `LightController2`, ...)
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub block_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// The page the object is on in the program file
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page: Option<String>,
    /// The program file has no object with this UUID: created at runtime
    /// (an autopilot rule made in the app, say)
    pub new: bool,
    /// The upload deletes the object too (`removed` in the source): its
    /// custom change goes with it and nothing is lost
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub removed: bool,
}

impl CustomChange {
    /// `"Kitchen timer" (DayTimer, page "Kitchen")`; the UUID when the title
    /// is unknown.
    pub fn label(&self) -> String {
        let mut s = match &self.title {
            Some(t) => format!("\"{t}\""),
            None => self.uuid.clone(),
        };
        let mut what = Vec::new();
        if let Some(ty) = &self.block_type {
            what.push(ty.clone());
        }
        if let Some(page) = &self.page {
            what.push(format!("page \"{page}\""));
        }
        if self.new {
            what.push("not in the program file".into());
        }
        if self.removed {
            what.push("the upload deletes it too".into());
        }
        if !what.is_empty() {
            s.push_str(&format!(" ({})", what.join(", ")));
        }
        s
    }
}

/// Name the files of `/sys/custom` (file name, LoxCC bytes), looking each
/// object up in the program file `live_xml` when there is one. A file that
/// does not decompress or parse still counts: its name carries the UUID.
pub(crate) fn describe(files: &[(String, Vec<u8>)], live_xml: Option<&[u8]>) -> Vec<CustomChange> {
    let live = live_xml.and_then(|xml| LoxoneDoc::parse(xml).ok());
    let objects = live.as_ref().map(|d| d.objects()).unwrap_or_default();
    files
        .iter()
        .map(|(name, data)| {
            let fragment = archive::decompress_loxcc(data)
                .ok()
                .and_then(|xml| XmlDocument::parse(&xml).ok())
                .map(|doc| doc.root)
                .filter(|root| root.name == "C");
            let uuid = fragment
                .as_ref()
                .and_then(|c| c.attr("U"))
                .map(str::to_string)
                .unwrap_or_else(|| uuid_from_name(name));
            match objects.iter().find(|o| o.uuid.eq_ignore_ascii_case(&uuid)) {
                Some(obj) => CustomChange {
                    uuid,
                    block_type: Some(obj.block_type.clone()),
                    title: obj.title.clone(),
                    page: live.as_ref().and_then(|d| page_of(d, &obj.path)),
                    new: false,
                    removed: false,
                },
                None => CustomChange {
                    uuid,
                    block_type: fragment
                        .as_ref()
                        .and_then(|c| c.attr("Type"))
                        .map(str::to_string),
                    title: fragment
                        .as_ref()
                        .and_then(|c| c.attr_decoded("Title"))
                        .map(|t| t.into_owned()),
                    page: None,
                    // Without a program file to look in, nothing is known
                    // to be new.
                    new: live.is_some(),
                    removed: false,
                },
            }
        })
        .collect()
}

/// `/sys/custom/<UUID>.LoxCC` → `<UUID>`.
fn uuid_from_name(name: &str) -> String {
    let base = name.rsplit('/').next().unwrap_or(name);
    match base.rsplit_once('.') {
        Some((stem, ext)) if ext.eq_ignore_ascii_case("loxcc") => stem.to_string(),
        _ => base.to_string(),
    }
}

/// The title of the nearest page around the element at `path`.
fn page_of(doc: &LoxoneDoc, path: &[usize]) -> Option<String> {
    (1..path.len()).rev().find_map(|n| {
        let el = doc.element_at(&path[..n])?;
        (el.name == "C" && el.attr("Type") == Some("Page"))
            .then(|| el.attr_decoded("Title").map(|t| t.into_owned()))
            .flatten()
    })
}

/// Before `lox config upload` / `restore`: list what loading `upload_xml`
/// would delete, on stderr. Best effort — a failed check is reported, not
/// fatal, because these commands already need `--force`.
pub(crate) fn warn_before_upload(cfg: &crate::config::Config, upload_xml: Option<&[u8]>) {
    match crate::ftp::download_custom_changes(cfg) {
        Ok(files) if files.is_empty() => {}
        Ok(files) => {
            let changes = describe(&files, upload_xml);
            eprintln!(
                "⚠  The Miniserver holds {} custom change(s) that are not in any program file \
                 (made in the app, the web UI or through the API). Loading the upload deletes them:",
                changes.len()
            );
            for c in &changes {
                eprintln!("     {}", c.label());
            }
            eprintln!(
                "   To keep them, open the project in Loxone Config, load it from the \
                 Miniserver and save it first."
            );
        }
        Err(e) => eprintln!(
            "⚠  Could not check {} for custom changes an upload would delete: {e:#}",
            crate::ftp::CUSTOM_CHANGES_DIR
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIVE: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<ControlList>
	<C Type="Document" V="178" U="0f000000-0000-0000-ffff000000000000">
		<C Type="Page" U="0f000000-0000-0001-ffff000000000000" Title="Küche">
			<C Type="DayTimer" U="0f000000-0000-0002-ffff000000000000" Title="Timer &amp; Licht"/>
		</C>
	</C>
</ControlList>
"#;

    fn file(uuid: &str, xml: &str) -> (String, Vec<u8>) {
        (
            format!("{uuid}.LoxCC"),
            archive::compress_loxcc(xml.as_bytes()).unwrap(),
        )
    }

    #[test]
    fn a_change_to_a_known_object_is_named_from_the_program_file() {
        let files = [file(
            "0f000000-0000-0002-ffff000000000000",
            r#"<?xml version="1.0" encoding="utf-8"?>
<C Type="DayTimer" V="178" U="0f000000-0000-0002-ffff000000000000" Title="Timer &amp; Licht"/>
"#,
        )];
        let changes = describe(&files, Some(LIVE.as_bytes()));
        assert_eq!(
            changes,
            [CustomChange {
                uuid: "0f000000-0000-0002-ffff000000000000".into(),
                block_type: Some("DayTimer".into()),
                title: Some("Timer & Licht".into()),
                page: Some("Küche".into()),
                new: false,
                removed: false,
            }]
        );
        assert_eq!(
            changes[0].label(),
            "\"Timer & Licht\" (DayTimer, page \"Küche\")"
        );
    }

    #[test]
    fn an_object_the_program_file_lacks_is_new_and_named_from_the_fragment() {
        let files = [file(
            "0f000000-0000-0009-ffff000000000000",
            r#"<C Type="AutopilotRule" U="0f000000-0000-0009-ffff000000000000" Title="Abends"/>"#,
        )];
        let changes = describe(&files, Some(LIVE.as_bytes()));
        assert!(changes[0].new);
        assert_eq!(
            changes[0].label(),
            "\"Abends\" (AutopilotRule, not in the program file)"
        );
    }

    #[test]
    fn a_file_that_does_not_decompress_still_counts_by_its_name() {
        let files = [(
            "/sys/custom/0f000000-0000-0002-ffff000000000000.LoxCC".to_string(),
            b"garbage".to_vec(),
        )];
        let changes = describe(&files, Some(LIVE.as_bytes()));
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].uuid, "0f000000-0000-0002-ffff000000000000");
        assert_eq!(changes[0].title.as_deref(), Some("Timer & Licht"));

        let unknown = describe(&files, None);
        assert_eq!(unknown[0].label(), "0f000000-0000-0002-ffff000000000000");
    }
}
