use anyhow::{Context, Result};
use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};
use serde::Serialize;
use std::collections::HashMap;

// ── Users ──

#[derive(Debug, Serialize)]
pub struct LoxUser {
    pub name: String,
    pub nfc: bool,
    pub description: String,
}

// ── Devices ──

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DeviceBus {
    Tree,
    Air,
    Network,
}

#[derive(Debug, Serialize)]
pub struct LoxDevice {
    pub name: String,
    pub bus: DeviceBus,
    pub serial: Option<String>,
    pub sub_type: Option<u32>,
    pub type_label: String,
    pub mac: Option<String>,
    pub address: Option<String>,
}

// ── Diff ──

#[derive(Debug, Serialize)]
pub struct ConfigSummary {
    pub version: String,
    pub date: String,
    pub controls: HashMap<String, ControlEntry>,
    pub rooms: HashMap<String, String>,
    pub categories: HashMap<String, String>,
    pub users: Vec<String>,
}

#[derive(Debug, Serialize, Clone)]
pub struct ControlEntry {
    pub name: String,
    pub control_type: String,
}

// ── Helpers ──

fn attr_value(e: &BytesStart, name: &[u8]) -> Option<String> {
    for attr in e.attributes().flatten() {
        if attr.key.as_ref() == name {
            return String::from_utf8(attr.value.to_vec()).ok();
        }
    }
    None
}

fn attr_value_or(e: &BytesStart, name: &[u8], default: &str) -> String {
    attr_value(e, name).unwrap_or_else(|| default.to_string())
}

pub fn subtype_label(sub_type: u32) -> &'static str {
    match sub_type {
        7 => "Nano IO Air",
        19 => "Smoke detector",
        32 => "Water sensor",
        37 => "Smartaktor",
        48 => "Room climate sensor",
        32780 => "Dimmer",
        32784 => "Room climate sensor",
        32794 => "Presence sensor",
        32796 => "Code Touch keypad",
        32797 => "LoxAIR Bridge",
        32799 => "Flex connector",
        _ => "",
    }
}

fn device_type_label(sub_type: Option<u32>) -> String {
    match sub_type {
        Some(st) => {
            let label = subtype_label(st);
            if label.is_empty() {
                format!("Unknown ({})", st)
            } else {
                label.to_string()
            }
        }
        None => "Unknown".to_string(),
    }
}

/// Check if a `<C>` element is a known control type (function block) for diff purposes.
/// We use a blacklist of infrastructure/structural types.
fn is_control_type(type_attr: &str) -> bool {
    !matches!(
        type_attr,
        "Document"
            | "Page"
            | "Place"
            | "Category"
            | "User"
            | "UserCaption"
            | "LoxCaption"
            | "TreeDevice"
            | "LoxAIRDevice"
            | "NetworkDevice"
            | "TreeAsensor"
            | "TreeDsensor"
            | "TreeAactuator"
            | "TreeDactuator"
            | "LoxAIRAsensor"
            | "LoxAIRDsensor"
            | "LoxAIRAactuator"
            | "LoxAIRDactuator"
            | "InputRef"
            | "OutputRef"
            | "Co"
            | "IoData"
            | "Display"
            | "HP"
            | "IoList"
            | "Const"
            | "Note"
            | "ControlList"
            | "SubGroup"
            | "VirtUserDevice"
            | "CentralDevice"
    )
}

// ── Parsing ──

pub fn parse_users(xml: &[u8]) -> Result<Vec<LoxUser>> {
    let mut reader = Reader::from_reader(xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut users = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e) | Event::Empty(ref e))
                if e.name().as_ref() == b"C"
                    && attr_value(e, b"Type").as_deref() == Some("User") =>
            {
                let name = attr_value_or(e, b"Title", "");
                let nfc_arr = attr_value_or(e, b"NFCArr", "");
                let desc = attr_value_or(e, b"Desc", "").replace('_', " ");
                users.push(LoxUser {
                    name,
                    nfc: !nfc_arr.is_empty(),
                    description: desc,
                });
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(e).context("Failed to parse XML"),
            _ => {}
        }
        buf.clear();
    }
    Ok(users)
}

pub fn parse_devices(xml: &[u8]) -> Result<Vec<LoxDevice>> {
    let mut reader = Reader::from_reader(xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut devices = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e) | Event::Empty(ref e)) if e.name().as_ref() == b"C" => {
                let type_val = attr_value(e, b"Type");
                match type_val.as_deref() {
                    Some("TreeDevice") => {
                        let sub = attr_value(e, b"SubType").and_then(|s| s.parse::<u32>().ok());
                        devices.push(LoxDevice {
                            name: attr_value_or(e, b"Title", ""),
                            bus: DeviceBus::Tree,
                            serial: attr_value(e, b"Serial"),
                            sub_type: sub,
                            type_label: device_type_label(sub),
                            mac: None,
                            address: None,
                        });
                    }
                    Some("LoxAIRDevice") => {
                        let sub = attr_value(e, b"SubType").and_then(|s| s.parse::<u32>().ok());
                        devices.push(LoxDevice {
                            name: attr_value_or(e, b"Title", ""),
                            bus: DeviceBus::Air,
                            serial: None,
                            sub_type: sub,
                            type_label: device_type_label(sub),
                            mac: None,
                            address: None,
                        });
                    }
                    Some("NetworkDevice") => {
                        devices.push(LoxDevice {
                            name: attr_value_or(e, b"Title", ""),
                            bus: DeviceBus::Network,
                            serial: None,
                            sub_type: attr_value(e, b"SubType").and_then(|s| s.parse::<u32>().ok()),
                            type_label: "Network device".to_string(),
                            mac: attr_value(e, b"MAC"),
                            address: attr_value(e, b"Addr"),
                        });
                    }
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(e).context("Failed to parse XML"),
            _ => {}
        }
        buf.clear();
    }
    Ok(devices)
}

pub fn parse_config_summary(xml: &[u8]) -> Result<ConfigSummary> {
    let mut reader = Reader::from_reader(xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();

    let mut version = String::new();
    let mut date = String::new();
    let mut controls: HashMap<String, ControlEntry> = HashMap::new();
    let mut rooms: HashMap<String, String> = HashMap::new();
    let mut categories: HashMap<String, String> = HashMap::new();
    let mut users: Vec<String> = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e) | Event::Empty(ref e)) => {
                let tag = e.name();
                if tag.as_ref() == b"ControlList" {
                    version = attr_value_or(e, b"Version", "");
                } else if tag.as_ref() == b"C" {
                    let type_val = attr_value(e, b"Type");
                    match type_val.as_deref() {
                        Some("Document") => {
                            date = attr_value_or(e, b"Date", "");
                        }
                        Some("Place") => {
                            if let (Some(u), Some(t)) =
                                (attr_value(e, b"U"), attr_value(e, b"Title"))
                            {
                                rooms.insert(u, t);
                            }
                        }
                        Some("Category") => {
                            if let (Some(u), Some(t)) =
                                (attr_value(e, b"U"), attr_value(e, b"Title"))
                            {
                                categories.insert(u, t);
                            }
                        }
                        Some("User") => {
                            if let Some(t) = attr_value(e, b"Title") {
                                users.push(t);
                            }
                        }
                        Some(typ) if is_control_type(typ) => {
                            if let (Some(u), Some(t)) =
                                (attr_value(e, b"U"), attr_value(e, b"Title"))
                            {
                                // Only include elements with a Title (real controls)
                                if !t.is_empty() {
                                    controls.insert(
                                        u,
                                        ControlEntry {
                                            name: t,
                                            control_type: typ.to_string(),
                                        },
                                    );
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(e).context("Failed to parse XML"),
            _ => {}
        }
        buf.clear();
    }

    Ok(ConfigSummary {
        version,
        date,
        controls,
        rooms,
        categories,
        users,
    })
}

// ── Diff output ──

#[derive(Debug, Serialize)]
pub struct ConfigDiff {
    pub version_old: String,
    pub version_new: String,
    pub date_old: String,
    pub date_new: String,
    pub controls_added: Vec<ControlEntry>,
    pub controls_removed: Vec<ControlEntry>,
    pub controls_changed: Vec<ControlChange>,
    pub rooms_added: Vec<String>,
    pub rooms_removed: Vec<String>,
    pub rooms_renamed: Vec<NameChange>,
    pub categories_added: Vec<String>,
    pub categories_removed: Vec<String>,
    pub categories_renamed: Vec<NameChange>,
    pub users_added: Vec<String>,
    pub users_removed: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct ControlChange {
    pub name: String,
    pub field: String,
    pub old_value: String,
    pub new_value: String,
}

#[derive(Debug, Serialize)]
pub struct NameChange {
    pub old: String,
    pub new: String,
}

impl ConfigDiff {
    pub fn has_changes(&self) -> bool {
        !self.controls_added.is_empty()
            || !self.controls_removed.is_empty()
            || !self.controls_changed.is_empty()
            || !self.rooms_added.is_empty()
            || !self.rooms_removed.is_empty()
            || !self.rooms_renamed.is_empty()
            || !self.categories_added.is_empty()
            || !self.categories_removed.is_empty()
            || !self.categories_renamed.is_empty()
            || !self.users_added.is_empty()
            || !self.users_removed.is_empty()
    }
}

pub fn diff_configs(old: &ConfigSummary, new: &ConfigSummary) -> ConfigDiff {
    let mut diff = ConfigDiff {
        version_old: old.version.clone(),
        version_new: new.version.clone(),
        date_old: old.date.clone(),
        date_new: new.date.clone(),
        controls_added: Vec::new(),
        controls_removed: Vec::new(),
        controls_changed: Vec::new(),
        rooms_added: Vec::new(),
        rooms_removed: Vec::new(),
        rooms_renamed: Vec::new(),
        categories_added: Vec::new(),
        categories_removed: Vec::new(),
        categories_renamed: Vec::new(),
        users_added: Vec::new(),
        users_removed: Vec::new(),
    };

    // Controls
    for (uuid, entry) in &new.controls {
        match old.controls.get(uuid) {
            None => diff.controls_added.push(entry.clone()),
            Some(old_entry) => {
                if old_entry.name != entry.name {
                    diff.controls_changed.push(ControlChange {
                        name: entry.name.clone(),
                        field: "name".to_string(),
                        old_value: old_entry.name.clone(),
                        new_value: entry.name.clone(),
                    });
                }
                if old_entry.control_type != entry.control_type {
                    diff.controls_changed.push(ControlChange {
                        name: entry.name.clone(),
                        field: "type".to_string(),
                        old_value: old_entry.control_type.clone(),
                        new_value: entry.control_type.clone(),
                    });
                }
            }
        }
    }
    for (uuid, entry) in &old.controls {
        if !new.controls.contains_key(uuid) {
            diff.controls_removed.push(entry.clone());
        }
    }

    // Rooms
    diff_named_map(
        &old.rooms,
        &new.rooms,
        &mut diff.rooms_added,
        &mut diff.rooms_removed,
        &mut diff.rooms_renamed,
    );

    // Categories
    diff_named_map(
        &old.categories,
        &new.categories,
        &mut diff.categories_added,
        &mut diff.categories_removed,
        &mut diff.categories_renamed,
    );

    // Users (compare by name since they have UUIDs but name is more meaningful)
    let old_users: std::collections::HashSet<&String> = old.users.iter().collect();
    let new_users: std::collections::HashSet<&String> = new.users.iter().collect();
    for u in &new_users {
        if !old_users.contains(u) {
            diff.users_added.push((*u).clone());
        }
    }
    for u in &old_users {
        if !new_users.contains(u) {
            diff.users_removed.push((*u).clone());
        }
    }

    diff
}

fn diff_named_map(
    old: &HashMap<String, String>,
    new: &HashMap<String, String>,
    added: &mut Vec<String>,
    removed: &mut Vec<String>,
    renamed: &mut Vec<NameChange>,
) {
    for (uuid, name) in new {
        match old.get(uuid) {
            None => added.push(name.clone()),
            Some(old_name) if old_name != name => {
                renamed.push(NameChange {
                    old: old_name.clone(),
                    new: name.clone(),
                });
            }
            _ => {}
        }
    }
    for (uuid, name) in old {
        if !new.contains_key(uuid) {
            removed.push(name.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_users() {
        let xml = br#"<?xml version="1.0"?>
        <ControlList Version="1">
          <C Type="Document" Date="2026-01-01">
            <C Type="UserCaption">
              <C Type="User" Title="admin" NFCArr="" Desc=""/>
              <C Type="User" Title="chris" NFCArr="AABB" Desc="Main_user"/>
            </C>
          </C>
        </ControlList>"#;
        let users = parse_users(xml).unwrap();
        assert_eq!(users.len(), 2);
        assert_eq!(users[0].name, "admin");
        assert!(!users[0].nfc);
        assert_eq!(users[0].description, "");
        assert_eq!(users[1].name, "chris");
        assert!(users[1].nfc);
        assert_eq!(users[1].description, "Main user");
    }

    #[test]
    fn test_parse_devices() {
        let xml = br#"<?xml version="1.0"?>
        <ControlList Version="1">
          <C Type="TreeDevice" Title="Raumklima" Serial="B03C7ED6" SubType="32784"/>
          <C Type="LoxAIRDevice" Title="Rauchmelder" SubType="19"/>
          <C Type="NetworkDevice" Title="Intercom" Addr="ice:7091" MAC="504F94E0F182" SubType="2"/>
        </ControlList>"#;
        let devices = parse_devices(xml).unwrap();
        assert_eq!(devices.len(), 3);

        assert_eq!(devices[0].name, "Raumklima");
        assert_eq!(devices[0].bus, DeviceBus::Tree);
        assert_eq!(devices[0].serial.as_deref(), Some("B03C7ED6"));
        assert_eq!(devices[0].type_label, "Room climate sensor");

        assert_eq!(devices[1].name, "Rauchmelder");
        assert_eq!(devices[1].bus, DeviceBus::Air);
        assert_eq!(devices[1].type_label, "Smoke detector");

        assert_eq!(devices[2].name, "Intercom");
        assert_eq!(devices[2].bus, DeviceBus::Network);
        assert_eq!(devices[2].mac.as_deref(), Some("504F94E0F182"));
    }

    #[test]
    fn test_subtype_label_known() {
        assert_eq!(subtype_label(32784), "Room climate sensor");
        assert_eq!(subtype_label(19), "Smoke detector");
    }

    #[test]
    fn test_subtype_label_unknown() {
        assert_eq!(subtype_label(99999), "");
    }

    #[test]
    fn test_device_type_label_unknown() {
        assert_eq!(device_type_label(Some(99999)), "Unknown (99999)");
        assert_eq!(device_type_label(None), "Unknown");
    }

    #[test]
    fn test_parse_config_summary() {
        let xml = br#"<?xml version="1.0"?>
        <ControlList Version="42">
          <C Type="Document" Date="2026-03-01 02:12:26" U="doc-1" Title="Test"/>
          <C Type="Category" U="cat-1" Title="Lighting"/>
          <C Type="Place" U="room-1" Title="Kitchen"/>
          <C Type="User" Title="admin"/>
          <C Type="Switch" U="sw-1" Title="Light Kitchen"/>
        </ControlList>"#;
        let s = parse_config_summary(xml).unwrap();
        assert_eq!(s.version, "42");
        assert_eq!(s.date, "2026-03-01 02:12:26");
        assert_eq!(s.categories.get("cat-1").unwrap(), "Lighting");
        assert_eq!(s.rooms.get("room-1").unwrap(), "Kitchen");
        assert_eq!(s.users, vec!["admin"]);
        assert!(s.controls.contains_key("sw-1"));
        assert_eq!(s.controls["sw-1"].name, "Light Kitchen");
        assert_eq!(s.controls["sw-1"].control_type, "Switch");
    }

    #[test]
    fn test_diff_configs() {
        let old = ConfigSummary {
            version: "1".into(),
            date: "2026-01-01".into(),
            controls: HashMap::from([
                (
                    "a".into(),
                    ControlEntry {
                        name: "Light".into(),
                        control_type: "Switch".into(),
                    },
                ),
                (
                    "b".into(),
                    ControlEntry {
                        name: "Blind".into(),
                        control_type: "Jalousie".into(),
                    },
                ),
            ]),
            rooms: HashMap::from([("r1".into(), "Kitchen".into())]),
            categories: HashMap::new(),
            users: vec!["admin".into(), "chris".into()],
        };
        let new = ConfigSummary {
            version: "2".into(),
            date: "2026-02-01".into(),
            controls: HashMap::from([
                (
                    "a".into(),
                    ControlEntry {
                        name: "Light Renamed".into(),
                        control_type: "Switch".into(),
                    },
                ),
                (
                    "c".into(),
                    ControlEntry {
                        name: "Outlet".into(),
                        control_type: "Switch".into(),
                    },
                ),
            ]),
            rooms: HashMap::from([("r1".into(), "Wohnküche".into())]),
            categories: HashMap::new(),
            users: vec!["admin".into(), "newuser".into()],
        };

        let diff = diff_configs(&old, &new);
        assert_eq!(diff.controls_added.len(), 1);
        assert_eq!(diff.controls_added[0].name, "Outlet");
        assert_eq!(diff.controls_removed.len(), 1);
        assert_eq!(diff.controls_removed[0].name, "Blind");
        assert_eq!(diff.controls_changed.len(), 1);
        assert_eq!(diff.controls_changed[0].old_value, "Light");
        assert_eq!(diff.rooms_renamed.len(), 1);
        assert_eq!(diff.rooms_renamed[0].old, "Kitchen");
        assert_eq!(diff.users_added, vec!["newuser"]);
        assert_eq!(diff.users_removed, vec!["chris"]);
    }
}
