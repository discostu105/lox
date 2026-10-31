//! The Miniserver's custom changes: what the app, the web UI or the API
//! changed at runtime and Loxone Config has not saved into the program file
//! yet (light moods, timer schedules, autopilot rules, ...). The Miniserver
//! keeps them as one LoxCC file per object in `/sys/custom/<UUID>.LoxCC`. A
//! download of the program file does not contain them, and loading an
//! uploaded `sps_new.zip` deletes them (def.log: `Loading sps_new.zip - Remove
//! old custom changes`). Loxone Config merges them in first ("Load from
//! Miniserver"). lox names them and refuses, except for the ones that belong
//! to the app rather than the program, which [`fold_app_owned`] carries into
//! the upload: the push settings of registered phones.

use loxone_config::LoxoneDoc;
use loxone_config::archive;
use loxone_config::xml::{Element, XmlDocument};
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

/// The `WF=` bit the Miniserver adds to an object the app changed (live,
/// 2026-10-31: a registered phone and its caption, each the program's `WF`
/// plus `0x1000000`; an app-deleted rule carries `0x2000000` instead).
const WF_CHANGED: i64 = 0x0100_0000;

/// The caption that holds the registered phones (`CaptionType="28"`,
/// "Push-Benachrichtigungen: Registrierte Geräte").
const CAPTION_PUSH_DEVICES: &str = "28";

/// Carry the app-owned custom changes into `doc` (the compiled program), so
/// loading the upload keeps them. Returns the UUIDs of the files it took over.
///
/// These are a registered phone's push settings (`PuDe`: `PCfg=`, `pTkn=`).
/// The app writes them, and losing them silently stops the phone's notifications.
///
/// - A `PuDe` change is folded when the program has the phone: all attributes
///   but `WF=` are copied over.
/// - The phones' caption is folded when it differs only by [`WF_CHANGED`] and
///   all its phones are folded.
/// - Anything else, a newly registered phone too, stays a custom change.
pub(crate) fn fold_app_owned(doc: &mut LoxoneDoc, files: &[(String, Vec<u8>)]) -> Vec<String> {
    let fragments: Vec<Element> = files
        .iter()
        .filter_map(|(_, data)| {
            let xml = archive::decompress_loxcc(data).ok()?;
            let root = XmlDocument::parse(&xml).ok()?.root;
            (root.name == "C").then_some(root)
        })
        .collect();
    let mut folded = Vec::new();
    for frag in &fragments {
        let Some(uuid) = frag.attr("U").map(str::to_string) else {
            continue;
        };
        let ok = match frag.attr("Type") {
            Some("PuDe") => fold_device(doc, frag),
            Some("LoxCaption") if frag.attr("CaptionType") == Some(CAPTION_PUSH_DEVICES) => {
                fold_device_caption(doc, frag)
            }
            _ => false,
        };
        if ok {
            folded.push(uuid);
        }
    }
    folded
}

/// The program's element with the UUID of `frag`, if it has the same type.
fn counterpart<'a>(doc: &'a mut LoxoneDoc, frag: &Element) -> Option<&'a mut Element> {
    let uuid = frag.attr("U")?;
    let obj = doc
        .objects()
        .into_iter()
        .find(|o| o.uuid.eq_ignore_ascii_case(uuid))?;
    let el = doc.element_at_mut(&obj.path)?;
    (el.attr("Type") == frag.attr("Type")).then_some(el)
}

/// The app's mark: the fragment's `WF=` is the program's plus [`WF_CHANGED`].
fn marked_changed(program: &Element, frag: &Element) -> bool {
    let wf = |e: &Element| e.attr("WF").map_or(Some(0), |v| v.parse::<i64>().ok());
    matches!((wf(program), wf(frag)), (Some(p), Some(f)) if p & WF_CHANGED == 0 && f == p | WF_CHANGED)
}

fn fold_device(doc: &mut LoxoneDoc, frag: &Element) -> bool {
    if frag.child_elements().next().is_some() {
        return false;
    }
    let Some(el) = counterpart(doc, frag) else {
        return false;
    };
    if el.child_elements().next().is_some() || !marked_changed(el, frag) {
        return false;
    }
    let wf = el.attr("WF").map(str::to_string);
    el.attrs = frag.attrs.clone();
    match wf {
        Some(wf) => el.set_attr_raw("WF", &wf),
        None => {
            el.remove_attr("WF");
        }
    }
    true
}

fn fold_device_caption(doc: &mut LoxoneDoc, frag: &Element) -> bool {
    {
        let Some(el) = counterpart(doc, frag) else {
            return false;
        };
        let rest = |e: &Element| -> Vec<(String, String)> {
            e.attrs
                .iter()
                .filter(|a| a.name != "WF")
                .map(|a| (a.name.clone(), a.value.clone()))
                .collect()
        };
        if rest(el) != rest(frag) || !marked_changed(el, frag) {
            return false;
        }
    }
    // Every phone in it must fold; the caption itself stays as programmed.
    let mut all = true;
    for dev in frag.child_elements() {
        all &= dev.attr("Type") == Some("PuDe") && fold_device(doc, dev);
    }
    all
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
    match loxone_client::ftp::download_custom_changes(&cfg.connection()) {
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
            loxone_client::ftp::CUSTOM_CHANGES_DIR
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

    /// A program with a phone registered for push, inside its caption.
    const WITH_PHONE: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<ControlList>
	<C Type="Document" V="178" U="0f000000-0000-0000-ffff000000000000">
		<C Type="LoxCaption" V="178" U="0f000000-0000-0010-ffff000000000000" Title="Push-Benachrichtigungen: Registrierte Geräte" Cl="0,0,0" WF="16384" CaptionType="28" SubType="28">
			<C Type="PuDe" V="178" U="0f000000-0000-0011-ffff000000000000" Title="Registriertes Gerät" Cl="0,0,0" WF="4210688" pTkn="t1" PCfg="-388" Usr="0f000000-0000-0020-ffff000000000000" DevT="samsung SM-S911B" DevOs="android"/>
			<C Type="PuDe" V="178" U="0f000000-0000-0012-ffff000000000000" Title="Registriertes Gerät" Cl="0,0,0" WF="4210688" pTkn="t2" PCfg="-129" Usr="0f000000-0000-0021-ffff000000000000" DevT="Google Pixel 7 Pro" DevOs="android"/>
		</C>
	</C>
</ControlList>
"#;

    /// The phone's custom change as the Miniserver wrote it (live,
    /// 2026-10-31): new `PCfg=`, `WF=` with the app's mark.
    const PHONE_CHANGED: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<C Type="PuDe" V="178" U="0f000000-0000-0011-ffff000000000000" Title="Registriertes Gerät" Cl="0,0,0" WF="20987904" pTkn="t1" PCfg="-385" Usr="0f000000-0000-0020-ffff000000000000" DevT="samsung SM-S911B" DevOs="android"/>
"#;

    /// The caption's custom change: only the changed phone inside.
    const CAPTION_CHANGED: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<C Type="LoxCaption" V="178" U="0f000000-0000-0010-ffff000000000000" Title="Push-Benachrichtigungen: Registrierte Geräte" Cl="0,0,0" WF="16793600" CaptionType="28" SubType="28">
	<C Type="PuDe" V="178" U="0f000000-0000-0011-ffff000000000000" Title="Registriertes Gerät" Cl="0,0,0" WF="20987904" pTkn="t1" PCfg="-385" Usr="0f000000-0000-0020-ffff000000000000" DevT="samsung SM-S911B" DevOs="android"/>
</C>
"#;

    fn fold(xml: &str, files: &[(String, Vec<u8>)]) -> (Vec<String>, String) {
        let mut doc = LoxoneDoc::parse(xml.as_bytes()).unwrap();
        let folded = fold_app_owned(&mut doc, files);
        (folded, String::from_utf8(doc.to_bytes()).unwrap())
    }

    #[test]
    fn a_phones_push_settings_are_folded_into_the_program() {
        let files = [
            file("0f000000-0000-0011-ffff000000000000", PHONE_CHANGED),
            file("0f000000-0000-0010-ffff000000000000", CAPTION_CHANGED),
        ];
        let (folded, out) = fold(WITH_PHONE, &files);
        assert_eq!(
            folded,
            [
                "0f000000-0000-0011-ffff000000000000",
                "0f000000-0000-0010-ffff000000000000"
            ]
        );
        // Exactly the PCfg changed; WF stays the program's, the caption and
        // the other phone stay as they were.
        assert_eq!(
            out,
            WITH_PHONE.replacen(r#"PCfg="-388""#, r#"PCfg="-385""#, 1)
        );
    }

    #[test]
    fn only_app_owned_changes_known_to_the_program_are_folded() {
        // A newly registered phone: the program has no place for it yet.
        let new_phone = PHONE_CHANGED.replace("0000-0011", "0000-0013");
        // A phone change without the app's mark.
        let unmarked = PHONE_CHANGED.replace(r#"WF="20987904""#, r#"WF="4210688""#);
        // Something the program owns.
        let timer = r#"<C Type="DayTimer" U="0f000000-0000-0002-ffff000000000000" WF="16793600"/>"#;
        for xml in [new_phone.as_str(), unmarked.as_str(), timer] {
            let files = [file("x", xml)];
            let (folded, out) = fold(WITH_PHONE, &files);
            assert!(folded.is_empty(), "{xml}");
            assert_eq!(out, WITH_PHONE);
        }
        // A caption holding a phone the program lacks is not taken over.
        let caption = CAPTION_CHANGED.replace("0000-0011", "0000-0013");
        let (folded, _) = fold(WITH_PHONE, &[file("c", &caption)]);
        assert!(folded.is_empty());
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
