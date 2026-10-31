//! Users, access rights, devices and a summary of a program file
//! (`.Loxone` XML), shaped the way `lox` lists and checks them. The parsing
//! is loxone-config's ([`loxone_config::LoxoneDoc`], [`loxone_config::access`]); this module only maps
//! its model onto these views.

use anyhow::{Context, Result};
use loxone_config::LoxoneDoc;
use serde::Serialize;
use std::collections::HashMap;

// ── Users ──

/// A user of the program file, as `lox config users` lists it.
#[derive(Debug, Serialize)]
pub struct LoxUser {
    pub name: String,
    pub uuid: String,
    pub nfc: bool,
    /// Number of NFC tags
    pub nfc_tags: usize,
    pub nfc_names: Vec<String>,
    /// Number of access codes
    pub access_codes: usize,
    /// Names of the user's groups
    pub groups: Vec<String>,
    pub description: String,
    /// Loxone time (seconds since 2009-01-01) of the last change
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_edit: Option<i64>,
    /// The user's own access rights (rights through groups not included)
    pub access: Vec<AccessGrant>,
}

/// An access right by name: the block (e.g. an NFC Code Touch) and the
/// schedule that limits it.
#[derive(Debug, Serialize)]
pub struct AccessGrant {
    pub block: String,
    pub schedule: String,
}

// ── Access rights ──
//
// Users (`<C Type="User">`) and groups (`<C Type="UserGroup">`) carry their
// rights as `<PH><PRM REF="block" …/></PH>`. A PRM with `ACT` is a right in the
// user interface (ACT="3": operate); a PRM with `TIM` is an access right, e.g.
// to open an NFC Code Touch, limited by the schedule TIM points to (a
// `SwitchingTimer`; the built-in ones have FIX="1" always, FIX="2" never).
// Group members are listed on the group (`User="uuid,uuid"`), not on the user.
// The full-access group has UgType="4", the nobody group UgType="3".
// Seen in configs saved by Loxone Config 16.

pub use loxone_config::access::Permission;

#[derive(Debug, Clone, Serialize)]
pub struct ConfigUser {
    pub uuid: String,
    pub name: String,
    pub description: String,
    /// `LastEdit`, Loxone time; matches `lastEdit` of the live user
    pub last_edit: Option<i64>,
    /// Number of NFC tags (`NFCArr` holds one 64-digit hash per tag)
    pub nfc_tags: usize,
    /// Tag names from `NFCNArr`
    pub nfc_names: Vec<String>,
    /// Number of access codes (`CodeArr`, hashed)
    pub access_codes: usize,
    pub permissions: Vec<Permission>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConfigGroup {
    pub uuid: String,
    pub name: String,
    /// 3 nobody, 4 full access; absent for ordinary groups
    pub ug_type: Option<i64>,
    /// Member user UUIDs
    pub members: Vec<String>,
    pub permissions: Vec<Permission>,
}

/// A schedule an access right refers to.
#[derive(Debug, Clone)]
pub struct Schedule {
    pub title: String,
    /// The built-in "never" schedule (FIX="2")
    pub never: bool,
}

/// Users, groups and the access rights between them and the blocks.
#[derive(Debug, Default)]
pub struct AccessModel {
    pub users: Vec<ConfigUser>,
    pub groups: Vec<ConfigGroup>,
    /// NFC Code Touch blocks: UUID, title
    pub code_touches: Vec<(String, String)>,
    /// Title of every titled object, by UUID
    titles: HashMap<String, String>,
    /// `FIX` of the built-in schedules, by UUID
    fixed: HashMap<String, String>,
}

impl AccessModel {
    pub fn user(&self, uuid: &str) -> Option<&ConfigUser> {
        self.users.iter().find(|u| u.uuid == uuid)
    }

    /// The groups the program file lists the user in.
    pub fn groups_of<'a>(&'a self, user: &'a str) -> impl Iterator<Item = &'a ConfigGroup> {
        self.groups
            .iter()
            .filter(move |g| g.members.iter().any(|m| m == user))
    }

    pub fn title(&self, uuid: &str) -> Option<&str> {
        self.titles.get(uuid).map(String::as_str)
    }

    pub fn schedule(&self, uuid: &str) -> Schedule {
        Schedule {
            title: self.title(uuid).unwrap_or(uuid).to_string(),
            never: self.fixed.get(uuid).is_some_and(|f| f == "2"),
        }
    }
}

impl ConfigUser {
    /// Schedules of the user's access rights on a block.
    pub fn access_schedules<'a>(&'a self, target: &'a str) -> impl Iterator<Item = &'a str> {
        access_schedules(&self.permissions, target)
    }
}

impl ConfigGroup {
    /// Schedules of the group's access rights on a block.
    pub fn access_schedules<'a>(&'a self, target: &'a str) -> impl Iterator<Item = &'a str> {
        access_schedules(&self.permissions, target)
    }
}

fn access_schedules<'a>(
    permissions: &'a [Permission],
    target: &'a str,
) -> impl Iterator<Item = &'a str> {
    permissions
        .iter()
        .filter(move |p| p.target == target)
        .filter_map(|p| p.schedule.as_deref())
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

fn parse_doc(xml: &[u8]) -> Result<LoxoneDoc> {
    LoxoneDoc::parse(xml).context("Failed to parse XML")
}

/// An attribute of the object at `path`, XML escapes resolved.
fn attr_at(doc: &LoxoneDoc, path: &[usize], name: &str) -> Option<String> {
    doc.element_at(path)?
        .attr_decoded(name)
        .map(|v| v.into_owned())
}

// ── Parsing ──

pub fn parse_users(xml: &[u8]) -> Result<Vec<LoxUser>> {
    let model = parse_access_model(xml)?;
    Ok(model
        .users
        .iter()
        .map(|u| LoxUser {
            name: u.name.clone(),
            uuid: u.uuid.clone(),
            nfc: u.nfc_tags > 0,
            nfc_tags: u.nfc_tags,
            nfc_names: u.nfc_names.clone(),
            access_codes: u.access_codes,
            groups: model.groups_of(&u.uuid).map(|g| g.name.clone()).collect(),
            description: u.description.clone(),
            last_edit: u.last_edit,
            access: u
                .permissions
                .iter()
                .filter_map(|p| {
                    let schedule = p.schedule.as_deref()?;
                    Some(AccessGrant {
                        block: model.title(&p.target).unwrap_or(&p.target).to_string(),
                        schedule: model.schedule(schedule).title,
                    })
                })
                .collect(),
        })
        .collect())
}

pub fn parse_access_model(xml: &[u8]) -> Result<AccessModel> {
    let doc = parse_doc(xml)?;
    let access = loxone_config::access::AccessModel::read(&doc);
    let mut model = AccessModel::default();
    for o in doc.objects() {
        if let Some(t) = &o.title {
            model.titles.insert(o.uuid.clone(), t.clone());
        }
        if o.block_type == "NfcCodeTouch" {
            model
                .code_touches
                .push((o.uuid.clone(), o.title.clone().unwrap_or_default()));
        }
        if let Some(fix) = attr_at(&doc, &o.path, "FIX") {
            model.fixed.insert(o.uuid.clone(), fix);
        }
    }
    model.users = access
        .users
        .into_iter()
        .map(|u| ConfigUser {
            description: attr_at(&doc, &u.path, "Desc")
                .unwrap_or_default()
                .replace('_', " "),
            last_edit: u.last_edit.and_then(|v| v.parse().ok()),
            nfc_tags: u.nfc_tags.len(),
            nfc_names: u.nfc_tags.iter().filter_map(|t| t.name.clone()).collect(),
            access_codes: u.codes.len(),
            name: u.title.unwrap_or_default(),
            uuid: u.uuid,
            permissions: u.permissions,
        })
        .collect();
    model.groups = access
        .groups
        .into_iter()
        .map(|g| ConfigGroup {
            name: g.title.unwrap_or_default(),
            ug_type: g.kind.and_then(|v| v.parse().ok()),
            uuid: g.uuid,
            members: g.members,
            permissions: g.permissions,
        })
        .collect();
    Ok(model)
}

pub fn parse_devices(xml: &[u8]) -> Result<Vec<LoxDevice>> {
    let doc = parse_doc(xml)?;
    let mut devices = Vec::new();
    for o in doc.objects() {
        let attr = |name: &str| attr_at(&doc, &o.path, name);
        let sub_type = attr("SubType").and_then(|s| s.parse::<u32>().ok());
        let name = o.title.clone().unwrap_or_default();
        let device = match o.block_type.as_str() {
            "TreeDevice" => LoxDevice {
                name,
                bus: DeviceBus::Tree,
                serial: attr("Serial"),
                sub_type,
                type_label: device_type_label(sub_type),
                mac: None,
                address: None,
            },
            "LoxAIRDevice" => LoxDevice {
                name,
                bus: DeviceBus::Air,
                serial: None,
                sub_type,
                type_label: device_type_label(sub_type),
                mac: None,
                address: None,
            },
            "NetworkDevice" => LoxDevice {
                name,
                bus: DeviceBus::Network,
                serial: None,
                sub_type,
                type_label: "Network device".to_string(),
                mac: attr("MAC"),
                address: attr("Addr"),
            },
            _ => continue,
        };
        devices.push(device);
    }
    Ok(devices)
}

pub fn parse_config_summary(xml: &[u8]) -> Result<ConfigSummary> {
    let doc = parse_doc(xml)?;
    let mut summary = ConfigSummary {
        version: doc
            .xml
            .root
            .attr_decoded("Version")
            .map(|v| v.into_owned())
            .unwrap_or_default(),
        date: doc
            .document()
            .and_then(|d| d.attr_decoded("Date"))
            .map(|v| v.into_owned())
            .unwrap_or_default(),
        controls: HashMap::new(),
        rooms: HashMap::new(),
        categories: HashMap::new(),
        users: Vec::new(),
    };
    for o in doc.objects() {
        let title = o.title.clone();
        match o.block_type.as_str() {
            "Place" => {
                if let Some(t) = title {
                    summary.rooms.insert(o.uuid, t);
                }
            }
            "Category" => {
                if let Some(t) = title {
                    summary.categories.insert(o.uuid, t);
                }
            }
            "User" => summary.users.extend(title),
            // only elements with a title are real controls
            ty if is_control_type(ty) => {
                if let Some(name) = title.filter(|t| !t.is_empty()) {
                    summary.controls.insert(
                        o.uuid.clone(),
                        ControlEntry {
                            name,
                            control_type: o.block_type.clone(),
                        },
                    );
                }
            }
            _ => {}
        }
    }
    Ok(summary)
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
              <C Type="User" U="1a2b3c4d-0001-0001-ffff00000000000a" Title="admin" NFCArr="" Desc=""/>
              <C Type="User" U="1a2b3c4d-0001-0001-ffff00000000000b" Title="chris" NFCArr="AABB" Desc="Main_user"/>
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

    const ALICE: &str = "1a2b3c4d-0001-0001-ffff000000000001";
    const BOB: &str = "1a2b3c4d-0001-0002-ffff000000000002";
    const KIDS: &str = "1a2b3c4d-0002-0001-ffff000000000001";
    const TOUCH: &str = "1a2b3c4d-0003-0001-ffff000000000001";
    const ALWAYS: &str = "1a2b3c4d-0004-0001-ffff000000000001";
    const NEVER: &str = "1a2b3c4d-0004-0002-ffff000000000002";

    fn access_xml() -> String {
        let tag = "0123456789abcdef".repeat(4);
        format!(
            r#"<?xml version="1.0"?>
<ControlList Version="1">
<C Type="Document" Title="Test">
<C Type="UserCaption" Title="Users">
<C Type="User" U="{ALICE}" Title="alice &amp; co" NFCArr="{tag}" NFCNArr="alice phone" CodeArr="" LastEdit="562000000">
<HP H="" Sc="-1" S=""/>
<PH NUM="2"><PRM REF="{ALWAYS}" ACT="3"/><PRM REF="{TOUCH}" TIM="{ALWAYS}"/></PH>
</C>
<C Type="User" U="{BOB}" Title="bob" NFCArr="{tag};{tag}" NFCNArr="bob card;bob watch" CodeArr="{tag}" Desc="the_kid"/>
</C>
<C Type="UserGroupCaption" Title="Groups">
<C Type="UserGroup" U="{KIDS}" Title="Kids" Rights="1281" User="{BOB},{ALICE}" UserGroups="">
<PH NUM="1"><PRM REF="{TOUCH}" TIM="{NEVER}"/></PH>
</C>
<C Type="UserGroup" U="1a2b3c4d-0002-0002-ffff000000000002" Title="Full access" Rights="-1" UgType="4" User=""/>
</C>
<C Type="SwitchingTimer" U="{ALWAYS}" Title="Always" FIX="1"><C Type="Inner" U="1a2b3c4d-0005-0001-ffff000000000001" Title="x"/></C>
<C Type="SwitchingTimer" U="{NEVER}" Title="Never" FIX="2"/>
<C Type="NfcCodeTouch" U="{TOUCH}" Title="Front door"><PH NUM="1"><PRM REF="{ALICE}" ACT="3"/></PH></C>
</C>
</ControlList>"#
        )
    }

    #[test]
    fn parses_users_groups_and_access_rights() {
        let m = parse_access_model(access_xml().as_bytes()).unwrap();
        assert_eq!(m.users.len(), 2);
        let alice = m.user(ALICE).unwrap();
        assert_eq!(alice.name, "alice & co");
        assert_eq!(alice.last_edit, Some(562000000));
        assert_eq!((alice.nfc_tags, alice.access_codes), (1, 0));
        assert_eq!(alice.nfc_names, ["alice phone"]);
        assert_eq!(alice.permissions.len(), 2);
        assert_eq!(alice.access_schedules(TOUCH).collect::<Vec<_>>(), [ALWAYS]);
        let bob = m.user(BOB).unwrap();
        assert_eq!((bob.nfc_tags, bob.access_codes), (2, 1));
        assert_eq!(bob.nfc_names, ["bob card", "bob watch"]);
        assert_eq!(bob.description, "the kid");
        assert!(bob.permissions.is_empty());
        let kids: Vec<_> = m.groups_of(BOB).map(|g| g.name.as_str()).collect();
        assert_eq!(kids, ["Kids"]);
        assert_eq!(m.groups[0].access_schedules(TOUCH).count(), 1);
        assert_eq!(m.groups[1].ug_type, Some(4));
        assert!(m.groups[1].members.is_empty());
        assert!(m.schedule(NEVER).never);
        assert!(!m.schedule(ALWAYS).never);
        assert_eq!(m.schedule(ALWAYS).title, "Always");
        assert_eq!(
            m.code_touches,
            [(TOUCH.to_string(), "Front door".to_string())]
        );
        // a PRM on a block is no user's right
        assert_eq!(
            m.users.iter().map(|u| u.permissions.len()).sum::<usize>(),
            2
        );
    }

    #[test]
    fn config_users_lists_groups_and_named_access_rights() {
        let users = parse_users(access_xml().as_bytes()).unwrap();
        assert_eq!(users[0].groups, ["Kids"]);
        assert_eq!(users[0].access.len(), 1);
        assert_eq!(users[0].access[0].block, "Front door");
        assert_eq!(users[0].access[0].schedule, "Always");
        assert_eq!(users[1].nfc_tags, 2);
    }

    #[test]
    fn test_parse_devices() {
        let xml = br#"<?xml version="1.0"?>
        <ControlList Version="1">
          <C Type="TreeDevice" U="dev-1" Title="Raumklima" Serial="B03C7ED6" SubType="32784"/>
          <C Type="LoxAIRDevice" U="dev-2" Title="Rauchmelder" SubType="19"/>
          <C Type="NetworkDevice" U="dev-3" Title="Intercom" Addr="ice:7091" MAC="504F94E0F182" SubType="2"/>
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
          <C Type="User" U="user-1" Title="admin"/>
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
}
