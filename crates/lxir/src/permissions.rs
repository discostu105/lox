//! `permissions.bin`: which principals may see and use which object.
//!
//! Loxone Config writes the file into the program ZIP next to
//! `sps0.LoxCC` and `LoxAPP3.json`. It is a flat list of 16-byte records,
//! no header: Loxone UUIDs in binary form and markers, sixteen times the
//! same byte. Each object has one group:
//!
//! ```text
//! object, FE, state…, FC, principal…, [FB, principal…]…, [FD], FF
//! ```
//!
//! The states are the object's own state UUIDs, the principals the users,
//! user groups and room/category right groups the object is assigned to
//! directly. [`generate`] rebuilds the file from the program and
//! `LoxAPP3.json` of the same ZIP; the rules and their evidence are in
//! docs/loxone-format.md (*permissions.bin*). The control order and the
//! state lists come from `LoxAPP3.json`, which the program alone does not
//! determine.

use crate::doc::LoxoneDoc;
use crate::error::{Error, Result};
use crate::uuid::LoxUuid;
use crate::xml::Element;
use serde::de::{Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// The size of one record.
pub const RECORD: usize = 16;

const STATES: u8 = 0xFE;
const PRINCIPALS: u8 = 0xFC;
const REC: u8 = 0xFB;
const FD: u8 = 0xFD;
const END: u8 = 0xFF;

/// One object's group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    pub object: LoxUuid,
    pub states: Vec<LoxUuid>,
    /// The principals of the object; then, behind an `FB` marker each,
    /// those of the object its `Rec=` names, and so on.
    pub principals: Vec<Vec<LoxUuid>>,
    /// An `FD` marker before the end; it never has entries.
    pub fd: bool,
}

fn uuid_bytes(u: &LoxUuid) -> [u8; RECORD] {
    let mut out = [0u8; RECORD];
    out[..4].copy_from_slice(&u.time.to_le_bytes());
    out[4..6].copy_from_slice(&u.mid.to_le_bytes());
    out[6..8].copy_from_slice(&u.seq.to_le_bytes());
    out[8..].copy_from_slice(&u.tail);
    out
}

fn uuid_of(r: &[u8]) -> LoxUuid {
    LoxUuid {
        time: u32::from_le_bytes(r[..4].try_into().unwrap()),
        mid: u16::from_le_bytes(r[4..6].try_into().unwrap()),
        seq: u16::from_le_bytes(r[6..8].try_into().unwrap()),
        tail: r[8..].try_into().unwrap(),
    }
}

fn marker(r: &[u8]) -> Option<u8> {
    let b = r[0];
    (matches!(b, STATES | PRINCIPALS | REC | FD | END) && r.iter().all(|&x| x == b)).then_some(b)
}

/// The groups of a `permissions.bin`.
pub fn parse(bytes: &[u8]) -> Result<Vec<Group>> {
    let bad = |i: usize, msg: &str| {
        Error::Structure(format!(
            "permissions.bin, record {i} (byte {}): {msg}",
            i * RECORD
        ))
    };
    if !bytes.len().is_multiple_of(RECORD) {
        return Err(Error::Structure(format!(
            "permissions.bin: {} bytes is no whole number of {RECORD}-byte records",
            bytes.len()
        )));
    }
    let records: Vec<&[u8]> = bytes.chunks(RECORD).collect();
    let mut groups = Vec::new();
    let mut i = 0;
    while i < records.len() {
        if marker(records[i]).is_some() {
            return Err(bad(i, "expected an object, found a marker"));
        }
        let object = uuid_of(records[i]);
        i += 1;
        if records.get(i).and_then(|r| marker(r)) != Some(STATES) {
            return Err(bad(i, "expected the FE marker after the object"));
        }
        i += 1;
        let mut group = Group {
            object,
            states: Vec::new(),
            principals: Vec::new(),
            fd: false,
        };
        // States until FC, then principal sections until FD or FF.
        loop {
            let Some(r) = records.get(i) else {
                return Err(bad(i, "the file ends inside a group"));
            };
            i += 1;
            match marker(r) {
                None => group.states.push(uuid_of(r)),
                Some(PRINCIPALS) => break,
                Some(_) => return Err(bad(i - 1, "expected a state or the FC marker")),
            }
        }
        group.principals.push(Vec::new());
        loop {
            let Some(r) = records.get(i) else {
                return Err(bad(i, "the file ends inside a group"));
            };
            i += 1;
            match marker(r) {
                None => group.principals.last_mut().unwrap().push(uuid_of(r)),
                Some(REC) if !group.fd => group.principals.push(Vec::new()),
                Some(FD) if !group.fd => group.fd = true,
                Some(END) => break,
                Some(_) => return Err(bad(i - 1, "unexpected marker")),
            }
            if group.fd && marker(records.get(i).copied().unwrap_or(&[0; RECORD])) != Some(END) {
                return Err(bad(i, "expected the FF marker after FD"));
            }
        }
        groups.push(group);
    }
    Ok(groups)
}

/// The file for `groups`.
pub fn encode(groups: &[Group]) -> Vec<u8> {
    let mut out = Vec::new();
    let mark = |out: &mut Vec<u8>, b: u8| out.extend_from_slice(&[b; RECORD]);
    for g in groups {
        out.extend_from_slice(&uuid_bytes(&g.object));
        mark(&mut out, STATES);
        for s in &g.states {
            out.extend_from_slice(&uuid_bytes(s));
        }
        mark(&mut out, PRINCIPALS);
        for (n, section) in g.principals.iter().enumerate() {
            if n > 0 {
                mark(&mut out, REC);
            }
            for p in section {
                out.extend_from_slice(&uuid_bytes(p));
            }
        }
        if g.fd {
            mark(&mut out, FD);
        }
        mark(&mut out, END);
    }
    out
}

// ── LoxAPP3.json, read in its own key order ─────────────────────────────────

/// A JSON value that keeps object keys in file order: the control order of
/// `permissions.bin` is the key order of `LoxAPP3.json`.
#[derive(Debug, Clone, PartialEq)]
enum Json {
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
    /// Numbers, booleans, null: never needed here.
    Other,
}

impl Json {
    fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Obj(entries) => entries.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    fn entries(&self) -> &[(String, Json)] {
        match self {
            Json::Obj(entries) => entries,
            _ => &[],
        }
    }

    fn as_str(&self) -> Option<&str> {
        match self {
            Json::Str(s) => Some(s),
            _ => None,
        }
    }
}

impl<'de> Deserialize<'de> for Json {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Json;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("JSON")
            }
            fn visit_bool<E>(self, _: bool) -> std::result::Result<Json, E> {
                Ok(Json::Other)
            }
            fn visit_i64<E>(self, _: i64) -> std::result::Result<Json, E> {
                Ok(Json::Other)
            }
            fn visit_u64<E>(self, _: u64) -> std::result::Result<Json, E> {
                Ok(Json::Other)
            }
            fn visit_f64<E>(self, _: f64) -> std::result::Result<Json, E> {
                Ok(Json::Other)
            }
            fn visit_unit<E>(self) -> std::result::Result<Json, E> {
                Ok(Json::Other)
            }
            fn visit_str<E>(self, s: &str) -> std::result::Result<Json, E> {
                Ok(Json::Str(s.to_string()))
            }
            fn visit_string<E>(self, s: String) -> std::result::Result<Json, E> {
                Ok(Json::Str(s))
            }
            fn visit_seq<A: SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> std::result::Result<Json, A::Error> {
                let mut out = Vec::new();
                while let Some(v) = seq.next_element()? {
                    out.push(v);
                }
                Ok(Json::Arr(out))
            }
            fn visit_map<A: MapAccess<'de>>(
                self,
                mut map: A,
            ) -> std::result::Result<Json, A::Error> {
                let mut out = Vec::new();
                while let Some(entry) = map.next_entry::<String, Json>()? {
                    out.push(entry);
                }
                Ok(Json::Obj(out))
            }
        }
        d.deserialize_any(V)
    }
}

// ── The generator ───────────────────────────────────────────────────────────

/// Block types whose state list carries connectors `LoxAPP3.json` does not
/// list, after the control's own states.
const EXTRA_CONNECTORS: &[(&str, &[&str])] = &[
    (
        "Alarm",
        &[
            "Q1", "Q2", "Q3", "Q4", "Q5", "Q6", "AQ", "QT", "I1", "I2", "I3", "I4", "I5",
        ],
    ),
    (
        "SmokeAlarm",
        &[
            "OutAlarm1",
            "OutAlarm2",
            "OutSilent",
            "OutNumAlarms",
            "InputAlarm",
            "InputAlarmW",
            "InputAlarmS",
            "InputTemp",
            "InputAirDigitalS",
            "InputAirDigitalW",
            "InputAirAnalog",
        ],
    ),
];

/// The `globalStates` of the document's own group, in file order.
const DOCUMENT_STATES: &[&str] = &[
    "notifications",
    "modifications",
    "favColorSequences",
    "favColors",
    "miniserverTime",
    "liveSearch",
    "userSettings",
    "userSettingsTs",
    "cloudservice",
    "trustVersion",
    "propsVersion",
];

/// The user right (`UsR=` bit) that puts a user on every automatic rule.
const AUTOPILOT_RIGHT: i64 = 0x40;

fn uuid(s: &str, what: &str) -> Result<LoxUuid> {
    LoxUuid::parse(s).map_err(|e| Error::Structure(format!("LoxAPP3.json, {what}: {e}")))
}

/// Every UUID under `states`, in key order; arrays flattened.
fn states(v: Option<&Json>, what: &str) -> Result<Vec<LoxUuid>> {
    let mut out = Vec::new();
    for (k, s) in v.map(Json::entries).unwrap_or_default() {
        let one = |s: &Json| match s.as_str() {
            Some(s) => uuid(s, &format!("{what}, state {k}")),
            None => Err(Error::Structure(format!(
                "LoxAPP3.json, {what}: state {k} is no UUID"
            ))),
        };
        match s {
            Json::Arr(items) => {
                for item in items {
                    out.push(one(item)?);
                }
            }
            s => out.push(one(s)?),
        }
    }
    Ok(out)
}

/// `"U:<uuid>"` keys name the same object as `"<uuid>"`.
fn bare(key: &str) -> &str {
    key.split_once(':').map_or(key, |(_, rest)| rest)
}

fn required<'a>(v: Option<&'a Json>, what: &str) -> Result<&'a str> {
    v.and_then(Json::as_str)
        .ok_or_else(|| Error::Structure(format!("LoxAPP3.json has no {what}")))
}

/// The program's objects as the generator needs them.
struct Program<'a> {
    /// The first object with each UUID.
    by_uuid: BTreeMap<&'a str, &'a Element>,
    /// Users, user groups and right groups in document order: type, UUID,
    /// element.
    principals: Vec<(&'a str, &'a str, &'a Element)>,
    /// The `REF=` of every `<PRM>` of a user or group.
    refs: BTreeMap<&'a str, BTreeSet<&'a str>>,
}

impl<'a> Program<'a> {
    fn read(doc: &'a LoxoneDoc) -> Program<'a> {
        let mut by_uuid = BTreeMap::new();
        let mut principals = Vec::new();
        let mut refs: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
        for o in doc.objects() {
            let el = doc.element_at(&o.path).expect("path from objects()");
            let (Some(u), Some(ty)) = (el.attr("U"), el.attr("Type")) else {
                continue;
            };
            by_uuid.entry(u).or_insert(el);
            if matches!(ty, "User" | "UserGroup" | "RightGroup") {
                principals.push((ty, u, el));
                let set = refs.entry(u).or_default();
                for ph in el.child_elements().filter(|c| c.name == "PH") {
                    set.extend(
                        ph.child_elements()
                            .filter(|c| c.name == "PRM")
                            .filter_map(|p| p.attr("REF")),
                    );
                }
            }
        }
        Program {
            by_uuid,
            principals,
            refs,
        }
    }

    /// The right groups of the room and category the object is in.
    fn right_groups(&self, uuid: &str) -> BTreeSet<&'a str> {
        let Some(io) = self
            .by_uuid
            .get(uuid)
            .and_then(|el| el.child_elements().find(|c| c.name == "IoData"))
        else {
            return BTreeSet::new();
        };
        ["Pr", "Cr"]
            .iter()
            .filter_map(|k| io.attr(k))
            .filter_map(|t| self.by_uuid.get(t)?.attr("RGR"))
            .collect()
    }

    /// The principals assigned to the object directly: users and groups
    /// with a right on it, the right groups of its room and category.
    fn principals_of(&self, uuid: &str) -> Vec<&'a str> {
        let rgs = self.right_groups(uuid);
        self.principals
            .iter()
            .filter(|(ty, u, _)| {
                if *ty == "RightGroup" {
                    rgs.contains(u)
                } else {
                    self.refs[u].contains(uuid)
                }
            })
            .map(|(_, u, _)| *u)
            .collect()
    }

    /// The object's principals, then those of the `Rec=` chain.
    fn sections(&self, uuid: &str) -> Vec<Vec<&'a str>> {
        let mut out = vec![self.principals_of(uuid)];
        let mut seen = BTreeSet::from([uuid]);
        let mut cur = self.by_uuid.get(uuid);
        while let Some(rec) = cur.and_then(|el| el.attr("Rec")) {
            if !seen.insert(rec) {
                break;
            }
            out.push(self.principals_of(rec));
            cur = self.by_uuid.get(rec);
        }
        out
    }

    fn sections_parsed(&self, uuid: &str) -> Result<Vec<Vec<LoxUuid>>> {
        self.sections(uuid)
            .into_iter()
            .map(|s| s.into_iter().map(uuid_from_program).collect())
            .collect()
    }
}

/// The principals of every object that has any, as [`generate`] lists them
/// (direct, then along `Rec=`), without `LoxAPP3.json`. Two programs with
/// the same assignments get the same `permissions.bin` from the same
/// `LoxAPP3.json`. It covers objects the app does not list too, so a
/// difference here may not reach the file.
pub fn assignments(doc: &LoxoneDoc) -> BTreeMap<String, Vec<Vec<String>>> {
    let program = Program::read(doc);
    program
        .by_uuid
        .keys()
        .filter_map(|u| {
            let sections = program.sections(u);
            sections.iter().any(|s| !s.is_empty()).then(|| {
                let owned = sections
                    .into_iter()
                    .map(|s| s.into_iter().map(str::to_string).collect())
                    .collect();
                (u.to_string(), owned)
            })
        })
        .collect()
}

fn uuid_from_program(s: &str) -> Result<LoxUuid> {
    LoxUuid::parse(s).map_err(|e| Error::Structure(format!("program UUID: {e}")))
}

/// The groups Loxone Config writes for `doc`, given the `LoxAPP3.json` of
/// the same program ZIP (a UTF-8 BOM in front is fine).
pub fn generate(doc: &LoxoneDoc, loxapp3: &str) -> Result<Vec<Group>> {
    let app: Json = serde_json::from_str(loxapp3.trim_start_matches('\u{feff}'))?;
    let program = Program::read(doc);
    let mut out = Vec::new();
    let group = |object: LoxUuid, states: Vec<LoxUuid>, principals, fd| Group {
        object,
        states,
        principals,
        fd,
    };
    let nobody = || vec![Vec::new()];

    // 1. Global objects, fixed order.
    let gs = app.get("globalStates");
    let global = |key: &str| -> Result<LoxUuid> {
        uuid(
            required(gs.and_then(|g| g.get(key)), &format!("globalStates.{key}"))?,
            &format!("globalStates.{key}"),
        )
    };
    for key in ["operatingMode", "sunrise", "sunset"] {
        out.push(group(global(key)?, Vec::new(), nobody(), true));
    }
    let task = doc
        .objects()
        .into_iter()
        .find(|o| o.block_type == "TaskCaption")
        .ok_or_else(|| Error::Structure("the program has no TaskCaption".into()))?
        .uuid;
    out.push(group(
        uuid_from_program(&task)?,
        vec![global("pastTasks")?, global("plannedTasks")?],
        nobody(),
        true,
    ));
    let document_states = DOCUMENT_STATES
        .iter()
        .map(|k| global(k))
        .collect::<Result<Vec<_>>>()?;
    out.push(group(
        global("modifications")?,
        document_states,
        nobody(),
        true,
    ));
    out.push(group(
        global("hasInternet")?,
        vec![global("hasInternet")?],
        nobody(),
        true,
    ));

    // 2. Operating modes.
    for (key, _) in app.get("modes").map(Json::entries).unwrap_or_default() {
        let u = bare(key);
        out.push(group(
            uuid(u, "modes")?,
            Vec::new(),
            program.sections_parsed(u)?,
            false,
        ));
    }

    // 3. Controls, in file order.
    for (key, control) in app.get("controls").map(Json::entries).unwrap_or_default() {
        let u = bare(key);
        let what = format!("control {u}");
        let el = program.by_uuid.get(u).copied();
        let mut st = states(control.get("states"), &what)?;
        let ty = control.get("type").and_then(Json::as_str).unwrap_or("");
        if let Some(el) = el {
            let connectors = || el.child_elements().filter(|c| c.name == "Co");
            if matches!(ty, "InfoOnlyAnalog" | "InfoOnlyDigital")
                && el.attr("StatsType").is_some()
                && let Some(first) = connectors().next().and_then(|c| c.attr("U"))
            {
                st.push(uuid_from_program(first)?);
            }
            let xml_type = el.attr("Type").unwrap_or("");
            if let Some((_, keys)) = EXTRA_CONNECTORS.iter().find(|(t, _)| *t == xml_type) {
                for k in *keys {
                    if let Some(c) = connectors().find(|c| c.attr("K") == Some(k))
                        && let Some(cu) = c.attr("U")
                    {
                        st.push(uuid_from_program(cu)?);
                    }
                }
            }
        }
        for (sk, sub) in control
            .get("subControls")
            .map(Json::entries)
            .unwrap_or_default()
        {
            if sk.starts_with("U:") {
                st.push(uuid(
                    required(
                        sub.get("uuidAction"),
                        &format!("{what}: subControl uuidAction"),
                    )?,
                    &what,
                )?);
            }
            st.extend(states(sub.get("states"), &what)?);
        }
        let mut seen = BTreeSet::new();
        st.retain(|s| seen.insert(*s));
        let fd = control.get("room").is_none() && control.get("cat").is_none();
        out.push(group(uuid(u, &what)?, st, program.sections_parsed(u)?, fd));
    }

    // 4. Weather server, automatic rules, message center.
    if let Some(ws) = app.get("weatherServer").filter(|w| !w.entries().is_empty()) {
        let st = states(ws.get("states"), "weatherServer")?;
        if let Some(&first) = st.first() {
            out.push(group(first, st, nobody(), true));
        }
    }
    let mut autopilot_users = Vec::new();
    for (ty, u, el) in &program.principals {
        let rights = el
            .attr("UsR")
            .and_then(|r| r.parse::<i64>().ok())
            .unwrap_or(0);
        if *ty == "User" && rights & AUTOPILOT_RIGHT != 0 {
            autopilot_users.push(uuid_from_program(u)?);
        }
    }
    for (section, principals, fd) in [
        ("autopilot", vec![autopilot_users], false),
        ("messageCenter", nobody(), true),
    ] {
        for (_, entry) in app.get(section).map(Json::entries).unwrap_or_default() {
            let action = uuid(
                required(entry.get("uuidAction"), &format!("{section} uuidAction"))?,
                section,
            )?;
            let mut st = vec![action];
            st.extend(states(entry.get("states"), section)?);
            out.push(group(action, st, principals.clone(), fd));
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u(s: &str) -> LoxUuid {
        LoxUuid::parse(s).unwrap()
    }

    #[test]
    fn records_roundtrip() {
        let groups = vec![
            Group {
                object: u("1a2b3c4d-0001-0002-ffff504f94112233"),
                states: vec![u("1a2b3c4d-0001-0003-ffff504f94112233")],
                principals: vec![vec![u("8f000000-0000-0001-ffff504f94112233")], vec![]],
                fd: false,
            },
            Group {
                object: u("80000000-0000-0001-1500000000000000"),
                states: vec![],
                principals: vec![vec![]],
                fd: true,
            },
        ];
        let bytes = encode(&groups);
        assert_eq!(bytes.len(), (4 + 5 + 3) * RECORD);
        // d1 little-endian, d4 raw.
        assert_eq!(
            &bytes[..8],
            &[0x4d, 0x3c, 0x2b, 0x1a, 0x01, 0x00, 0x02, 0x00]
        );
        assert_eq!(
            &bytes[8..16],
            &[0xff, 0xff, 0x50, 0x4f, 0x94, 0x11, 0x22, 0x33]
        );
        assert_eq!(parse(&bytes).unwrap(), groups);
    }

    #[test]
    fn broken_files_say_where() {
        assert!(parse(&[0; 15]).is_err());
        let mut bytes = encode(&[Group {
            object: u("1a2b3c4d-0001-0002-ffff504f94112233"),
            states: vec![],
            principals: vec![vec![]],
            fd: false,
        }]);
        bytes.truncate(bytes.len() - RECORD);
        let err = parse(&bytes).unwrap_err().to_string();
        assert!(err.contains("ends inside a group"), "{err}");
    }

    #[test]
    fn json_keeps_key_order() {
        let j: Json = serde_json::from_str(r#"{"b": 1, "a": {"y": "x", "x": [true]}}"#).unwrap();
        let keys: Vec<&str> = j.entries().iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, ["b", "a"]);
        let inner: Vec<&str> = j
            .get("a")
            .unwrap()
            .entries()
            .iter()
            .map(|(k, _)| k.as_str())
            .collect();
        assert_eq!(inner, ["y", "x"]);
    }
}
