//! The hardware the Miniserver reports (`/data/status`) against the hardware
//! elements of the program file: what is found but not in the program, in the
//! program but offline or missing, or replaced (another serial).
//!
//! Shared by `lox hw status` and the MCP server; it returns data.

use anyhow::Result;
use loxone_client::client::LoxClient;
use loxone_config::LoxoneDoc;
use schemars::JsonSchema;
use serde::Serialize;
use std::collections::BTreeMap;

/// A device `/data/status` lists, with the identity it carries.
#[derive(Debug, Clone, PartialEq)]
pub struct LiveDevice {
    /// The element: `TreeDevice`, `AirDevice`, `Extension`, `TreeBranch`,
    /// `GenericNetworkDevice`, `OneWireDevice` or `Miniserver`
    pub tag: String,
    pub name: String,
    /// The device type, as `/data/status` names it
    pub kind: String,
    pub place: Option<String>,
    /// The serial (MAC for the Miniserver and network devices), hex digits only
    pub serial: String,
    pub online: bool,
    pub firmware: Option<String>,
}

/// A hardware element of the program file.
#[derive(Debug, Clone, PartialEq)]
pub struct ProgramDevice {
    pub uuid: String,
    /// The element's `Type=` (`TreeDevice`, `LoxAIRDevice`, `LoxLIVE`, …)
    pub block_type: String,
    pub title: String,
    /// Hex digits only; `None` when the element has no serial yet
    pub serial: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum HwStatus {
    /// In the program, and the Miniserver lists a device with another serial
    /// under its name: a replaced device
    SerialChanged,
    /// In the program, but the Miniserver does not list it
    Missing,
    /// Listed by the Miniserver, but not in the program
    NotInProgram,
    /// In the program and listed, but offline
    Offline,
    Ok,
}

impl HwStatus {
    /// The name in JSON.
    pub fn key(self) -> &'static str {
        match self {
            HwStatus::SerialChanged => "serial-changed",
            HwStatus::Missing => "missing",
            HwStatus::NotInProgram => "not-in-program",
            HwStatus::Offline => "offline",
            HwStatus::Ok => "ok",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            HwStatus::SerialChanged => "serial changed",
            HwStatus::Missing => "missing",
            HwStatus::NotInProgram => "not in program",
            HwStatus::Offline => "offline",
            HwStatus::Ok => "ok",
        }
    }
}

/// One device in the comparison.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct HwEntry {
    pub status: HwStatus,
    pub name: String,
    /// The device type the Miniserver reports, else the program element's type
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub place: Option<String>,
    /// The serial the Miniserver reports
    #[serde(skip_serializing_if = "Option::is_none")]
    pub serial: Option<String>,
    /// The serial in the program file
    #[serde(skip_serializing_if = "Option::is_none")]
    pub program_serial: Option<String>,
    /// The program element's UUID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uuid: Option<String>,
    /// `None` when the Miniserver does not list the device
    #[serde(skip_serializing_if = "Option::is_none")]
    pub online: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub firmware: Option<String>,
}

/// How many devices have each status.
pub fn counts(entries: &[HwEntry]) -> BTreeMap<HwStatus, usize> {
    let mut counts = BTreeMap::new();
    for e in entries {
        *counts.entry(e.status).or_insert(0) += 1;
    }
    counts
}

/// Read `/data/status`.
pub fn read_live(lox: &LoxClient) -> Result<Vec<LiveDevice>> {
    Ok(parse_status(&lox.get_text("/data/status")?))
}

/// Hex digits only, upper case: `50:4F:94:…`, `28.9E.A6.…` and `504f94…` all
/// name the same device.
fn normalize(serial: &str) -> String {
    serial
        .chars()
        .filter(char::is_ascii_hexdigit)
        .map(|c| c.to_ascii_uppercase())
        .collect()
}

/// The devices of `/data/status` that carry an identity. Elements without
/// one (plugins, groups) are left out.
pub fn parse_status(xml: &str) -> Vec<LiveDevice> {
    use quick_xml::Reader;
    use quick_xml::events::Event;
    fn attr(e: &quick_xml::events::BytesStart, name: &[u8]) -> Option<String> {
        e.attributes()
            .flatten()
            .find(|a| a.key.as_ref() == name)
            .map(|a| String::from_utf8_lossy(&a.value).trim().to_string())
            .filter(|s| !s.is_empty())
    }
    let mut out = Vec::new();
    let mut reader = Reader::from_str(xml);
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Empty(ref e)) | Ok(Event::Start(ref e)) => {
                let tag = String::from_utf8_lossy(e.name().as_ref()).into_owned();
                // The Miniserver and network devices are known by their MAC.
                let id = match tag.as_str() {
                    "Miniserver" | "GenericNetworkDevice" => b"MAC".as_slice(),
                    "TreeBranch" | "TreeDevice" | "AirDevice" | "Extension" | "OneWireDevice" => {
                        b"Serial".as_slice()
                    }
                    _ => {
                        buf.clear();
                        continue;
                    }
                };
                let serial = attr(e, id).map(|s| normalize(&s)).unwrap_or_default();
                if serial.is_empty() {
                    buf.clear();
                    continue;
                }
                let online = match tag.as_str() {
                    "Miniserver" => attr(e, b"Offline").is_none_or(|o| o != "true"),
                    _ => attr(e, b"Online").is_none_or(|o| o == "true"),
                };
                let kind = attr(e, b"Type")
                    .filter(|t| !t.chars().all(|c| c.is_ascii_digit()))
                    .unwrap_or_else(|| tag.clone());
                out.push(LiveDevice {
                    name: attr(e, b"Name").unwrap_or_else(|| kind.clone()),
                    kind,
                    place: attr(e, b"Place"),
                    serial,
                    online,
                    firmware: attr(e, b"Version").filter(|v| v != "0.0.0.0"),
                    tag,
                });
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    out
}

/// The hardware elements of a program file and their serials: `Serial=` (an
/// Air device's from its `IP=`), a network device's `MAC=`, a 1-Wire device's
/// `ROMID=`.
pub fn program_devices(doc: &LoxoneDoc) -> Vec<ProgramDevice> {
    doc.objects()
        .into_iter()
        .filter_map(|o| {
            let el = doc.element_at(&o.path)?;
            let raw = match o.block_type.as_str() {
                "NetworkDevice" => el.attr("MAC").map(Into::into),
                "Lox1wireDevice" => el.attr("ROMID").map(Into::into),
                _ => loxone_config::doc::element_serial(el),
            };
            let present = el.attr("Serial").is_some()
                || matches!(
                    o.block_type.as_str(),
                    "LoxAIRDevice" | "NetworkDevice" | "Lox1wireDevice"
                );
            if !present {
                return None;
            }
            // A device added offline in Loxone Config has no serial yet.
            let serial = raw
                .map(|s| normalize(&s))
                .filter(|s| !s.is_empty() && !s.bytes().all(|b| b == b'0'));
            Some(ProgramDevice {
                uuid: o.uuid,
                block_type: o.block_type,
                title: o.title.unwrap_or_default(),
                serial,
            })
        })
        .collect()
}

/// The `/data/status` element a program element shows up as.
fn live_tag(block_type: &str) -> &'static str {
    match block_type {
        "LoxLIVE" => "Miniserver",
        "LoxTree" => "TreeBranch",
        "TreeDevice" | "LoxAIR" => "TreeDevice",
        "LoxAIRDevice" => "AirDevice",
        "NetworkDevice" => "GenericNetworkDevice",
        "Lox1wireDevice" => "OneWireDevice",
        _ => "Extension",
    }
}

/// Compare the two, problems first.
///
/// - Same serial: `ok`, or `offline`.
/// - A listed device whose serial the program lacks, with the name of a
///   program element of its kind that the Miniserver does not list:
///   `serial-changed`.
/// - Otherwise `not-in-program` (listed only) or `missing` (program only).
pub fn compare(live: &[LiveDevice], program: &[ProgramDevice]) -> Vec<HwEntry> {
    let mut matched = vec![false; program.len()];
    let mut out = Vec::new();
    let mut unmatched = Vec::new();
    for d in live {
        let hit = program
            .iter()
            .position(|p| p.serial.as_deref() == Some(d.serial.as_str()));
        match hit {
            Some(i) => {
                matched[i] = true;
                let p = &program[i];
                out.push(HwEntry {
                    status: if d.online {
                        HwStatus::Ok
                    } else {
                        HwStatus::Offline
                    },
                    name: d.name.clone(),
                    kind: d.kind.clone(),
                    place: d.place.clone(),
                    serial: Some(d.serial.clone()),
                    program_serial: p.serial.clone(),
                    uuid: Some(p.uuid.clone()),
                    online: Some(d.online),
                    firmware: d.firmware.clone(),
                });
            }
            None => unmatched.push(d),
        }
    }
    for d in unmatched {
        let replaced = program.iter().enumerate().position(|(i, p)| {
            !matched[i] && p.title == d.name && live_tag(&p.block_type) == d.tag
        });
        let (status, p) = match replaced {
            Some(i) => {
                matched[i] = true;
                (HwStatus::SerialChanged, Some(&program[i]))
            }
            None => (HwStatus::NotInProgram, None),
        };
        out.push(HwEntry {
            status,
            name: d.name.clone(),
            kind: d.kind.clone(),
            place: d.place.clone(),
            serial: Some(d.serial.clone()),
            program_serial: p.and_then(|p| p.serial.clone()),
            uuid: p.map(|p| p.uuid.clone()),
            online: Some(d.online),
            firmware: d.firmware.clone(),
        });
    }
    for (p, _) in program.iter().zip(&matched).filter(|(_, m)| !**m) {
        out.push(HwEntry {
            status: HwStatus::Missing,
            name: p.title.clone(),
            kind: p.block_type.clone(),
            place: None,
            serial: None,
            program_serial: p.serial.clone(),
            uuid: Some(p.uuid.clone()),
            online: None,
            firmware: None,
        });
    }
    out.sort_by(|a, b| a.status.cmp(&b.status).then_with(|| a.name.cmp(&b.name)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // Synthetic: the shape of a real `/data/status`, with made-up serials.
    const STATUS: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<Status Modified="2026-11-01 10:02:29">
	<Miniserver Type="2" Name="Loxone Miniserver" MAC="504F94000001" Version="17.2.8.28" Offline="false">
		<NetworkDevices Name="Intercom" Type="NetworkDevices">
			<GenericNetworkDevice Type="Intercom" Name="Intercom Door" Place="Garden" Online="false" MAC="504F94E00001" Version="" Project="IntercomCam"/>
			<Plugin GenericID="Matter" Type="Matter" Name="Matter" Version="" Online="false"/>
		</NetworkDevices>
		<TreeBranch Branch="3" Name="Tree" Serial="B0000001" Version="15.2.9.18" Devices="2" Errors="0">
			<TreeDevice Name="Climate Bedroom" Place="Bedroom" Serial="B0000002" Version="16.2.12.17" Online="true"/>
			<TreeDevice Name="Air Bridge" Place="Office" Serial="B0000003" Version="17.1.5.26" Online="true">
				<AirDevice Type="Nano IO Air" Name="Pool Pump" Place="Pool" Serial="50:4F:94:FF:FE:00:00:01" Version="17.1.5.7" Online="true"/>
				<AirDevice Type="Nano IO Air" Name="Garage" Place="Garage" Serial="50:4F:94:FF:FE:00:00:09" Version="17.1.5.7" Online="true"/>
				<AirDevice Type="Leak Sensor Air" Name="Leak" Place="Basement" Serial="50:4F:94:FF:FE:00:00:03" Version="17.1.5.7" Online="true"/>
			</TreeDevice>
		</TreeBranch>
		<Link Name="Link" Type="Link">
			<Extension Type="1-Wire Extension" Name="1-Wire" Serial="C0000001" Version="12.1.3.22" Online="true">
				<OneWireDevice Name="Water temp" Serial="28.00.00.00.00.00.00.01" Online="true"/>
			</Extension>
		</Link>
	</Miniserver>
</Status>"#;

    // `IP=` of the Air devices: EUI-64, the universal/local bit flipped (50 → 52).
    const PROGRAM: &str = "<?xml version=\"1.0\" encoding=\"utf-8\"?>\r\n\
<ControlList Version=\"1\">\r\n\
\t<C Type=\"Document\" V=\"175\" U=\"00000001-0000-0000-ffff000000000001\" Title=\"T\">\r\n\
\t\t<C Type=\"LoxLIVE\" V=\"175\" U=\"00000002-0000-0000-ffff000000000001\" Title=\"Miniserver\" Serial=\"504F94000001\">\r\n\
\t\t\t<C Type=\"LoxTree\" V=\"175\" U=\"00000003-0000-0000-ffff000000000001\" Title=\"Tree\" Serial=\"B0000001\">\r\n\
\t\t\t\t<C Type=\"TreeDevice\" V=\"175\" U=\"00000004-0000-0000-ffff000000000001\" Title=\"Climate Bedroom\" Serial=\"b0000002\"/>\r\n\
\t\t\t\t<C Type=\"LoxAIR\" V=\"175\" U=\"00000005-0000-0000-ffff000000000001\" Title=\"Air Bridge\" Serial=\"B0000003\">\r\n\
\t\t\t\t\t<C Type=\"LoxAIRDevice\" V=\"175\" U=\"00000006-0000-0000-ffff000000000001\" Title=\"Pool Pump\" IP=\"FE80000000000000524F94FFFE000001\"/>\r\n\
\t\t\t\t\t<C Type=\"LoxAIRDevice\" V=\"175\" U=\"00000007-0000-0000-ffff000000000001\" Title=\"Garage\" IP=\"FE80000000000000524F94FFFE000002\"/>\r\n\
\t\t\t\t\t<C Type=\"LoxAIRDevice\" V=\"175\" U=\"00000008-0000-0000-ffff000000000001\" Title=\"Smoke\" IP=\"FE80000000000000524F94FFFE000004\"/>\r\n\
\t\t\t\t</C>\r\n\
\t\t\t\t<C Type=\"TreeDevice\" V=\"175\" U=\"00000009-0000-0000-ffff000000000001\" Title=\"New Touch\" Serial=\"00000000\"/>\r\n\
\t\t\t</C>\r\n\
\t\t\t<C Type=\"Comm1wire\" V=\"175\" U=\"0000000a-0000-0000-ffff000000000001\" Title=\"1-Wire\" Serial=\"C0000001\">\r\n\
\t\t\t\t<C Type=\"Lox1wireDevice\" V=\"175\" U=\"0000000b-0000-0000-ffff000000000001\" Title=\"Water temp\" ROMID=\"2800000000000001\"/>\r\n\
\t\t\t</C>\r\n\
\t\t\t<C Type=\"NetworkDevice\" V=\"175\" U=\"0000000c-0000-0000-ffff000000000001\" Title=\"Intercom Door\" MAC=\"504F94E00001\"/>\r\n\
\t\t\t<C Type=\"Or\" V=\"175\" U=\"0000000d-0000-0000-ffff000000000001\" Title=\"Logic\"/>\r\n\
\t\t</C>\r\n\
\t</C>\r\n\
</ControlList>\r\n";

    fn entries() -> Vec<HwEntry> {
        let doc = LoxoneDoc::parse(PROGRAM.as_bytes()).unwrap();
        compare(&parse_status(STATUS), &program_devices(&doc))
    }

    fn status_of<'a>(entries: &'a [HwEntry], name: &str) -> &'a HwEntry {
        entries.iter().find(|e| e.name == name).unwrap()
    }

    #[test]
    fn status_lists_the_devices_with_an_identity() {
        let live = parse_status(STATUS);
        let tags: Vec<&str> = live.iter().map(|d| d.tag.as_str()).collect();
        assert_eq!(
            tags,
            [
                "Miniserver",
                "GenericNetworkDevice",
                "TreeBranch",
                "TreeDevice",
                "TreeDevice",
                "AirDevice",
                "AirDevice",
                "AirDevice",
                "Extension",
                "OneWireDevice"
            ]
        );
        assert_eq!(live[0].kind, "Miniserver", "a numeric Type= is no name");
        assert_eq!(live[5].serial, "504F94FFFE000001");
        assert_eq!(live[9].serial, "2800000000000001");
        assert!(!live[1].online);
        assert_eq!(live[1].firmware, None);
    }

    #[test]
    fn program_devices_carry_their_serials() {
        let doc = LoxoneDoc::parse(PROGRAM.as_bytes()).unwrap();
        let devices = program_devices(&doc);
        assert_eq!(devices.len(), 11, "the logic block is no hardware");
        let serial = |t: &str| {
            devices
                .iter()
                .find(|d| d.title == t)
                .unwrap()
                .serial
                .clone()
        };
        assert_eq!(serial("Climate Bedroom").as_deref(), Some("B0000002"));
        assert_eq!(serial("Pool Pump").as_deref(), Some("504F94FFFE000001"));
        assert_eq!(serial("Water temp").as_deref(), Some("2800000000000001"));
        assert_eq!(serial("New Touch"), None, "added offline: no serial yet");
    }

    #[test]
    fn compare_reports_each_kind_of_difference() {
        let e = entries();
        assert_eq!(status_of(&e, "Climate Bedroom").status, HwStatus::Ok);
        assert_eq!(status_of(&e, "Loxone Miniserver").status, HwStatus::Ok);
        assert_eq!(status_of(&e, "Water temp").status, HwStatus::Ok);
        let intercom = status_of(&e, "Intercom Door");
        assert_eq!(intercom.status, HwStatus::Offline);
        assert_eq!(intercom.online, Some(false));
        let garage = status_of(&e, "Garage");
        assert_eq!(garage.status, HwStatus::SerialChanged);
        assert_eq!(garage.serial.as_deref(), Some("504F94FFFE000009"));
        assert_eq!(garage.program_serial.as_deref(), Some("504F94FFFE000002"));
        assert_eq!(status_of(&e, "Leak").status, HwStatus::NotInProgram);
        assert_eq!(status_of(&e, "Smoke").status, HwStatus::Missing);
        assert_eq!(status_of(&e, "New Touch").status, HwStatus::Missing);
        // Problems first.
        let order: Vec<HwStatus> = e.iter().map(|e| e.status).collect();
        let mut sorted = order.clone();
        sorted.sort();
        assert_eq!(order, sorted);
        assert_eq!(e[0].status, HwStatus::SerialChanged);
        assert_eq!(e.len(), 12);
    }

    #[test]
    fn json_names_the_status_in_kebab_case() {
        let e = entries();
        let v = serde_json::to_value(status_of(&e, "Leak")).unwrap();
        assert_eq!(v["status"], "not-in-program");
        assert_eq!(v["status"], HwStatus::NotInProgram.key());
        assert_eq!(v["type"], "Leak Sensor Air");
        assert!(v.get("program_serial").is_none());
    }
}
