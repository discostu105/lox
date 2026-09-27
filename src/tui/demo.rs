//! `lox tui --demo`: a built-in fake Miniserver.
//!
//! A synthetic, English-language house (10 rooms, ~50 controls, an energy manager
//! with PV + battery + wallbox) and a small simulator that reacts to commands and
//! produces a believable event stream. Used for tests, screenshots and trying the
//! TUI without a Miniserver. Nothing here is real installation data.

use std::collections::HashMap;

use serde_json::{Map, Value, json};

use super::data::{BusLan, Device, Diag, LogLine, MsInfo, Series, SiteStatus};
use crate::stream::StateEvent;

// ── Structure fixture ────────────────────────────────────────────────────────

struct Builder {
    n: u32,
    rooms: Map<String, Value>,
    cats: Map<String, Value>,
    controls: Map<String, Value>,
    room_ids: HashMap<&'static str, String>,
    cat_ids: HashMap<&'static str, String>,
}

fn uuid(kind: u16, n: u32) -> String {
    format!(
        "1d{:06x}-{:04x}-4b2a-ffff{:012x}",
        n,
        kind,
        0x5eed0000u64 + n as u64
    )
}

impl Builder {
    fn next(&mut self, kind: u16) -> String {
        self.n += 1;
        uuid(kind, self.n)
    }

    fn room(&mut self, name: &'static str) {
        let u = self.next(1);
        self.rooms
            .insert(u.clone(), json!({"uuid": u, "name": name, "type": 0}));
        self.room_ids.insert(name, u);
    }

    fn cat(&mut self, name: &'static str) {
        let u = self.next(2);
        self.cats.insert(
            u.clone(),
            json!({"uuid": u, "name": name, "type": "undefined"}),
        );
        self.cat_ids.insert(name, u);
    }

    /// Build one control JSON (not inserted).
    fn ctrl(
        &mut self,
        typ: &str,
        name: &str,
        room: Option<&'static str>,
        cat: Option<&'static str>,
        states: &[&str],
        details: Value,
    ) -> (String, Value) {
        let u = self.next(3);
        let mut st = Map::new();
        for s in states {
            st.insert(s.to_string(), Value::String(self.next(4)));
        }
        let mut c = json!({
            "name": name, "type": typ, "uuidAction": u,
            "defaultRating": 0, "isFavorite": false, "isSecured": false,
            "states": st, "details": details,
        });
        if let Some(r) = room {
            c["room"] = json!(self.room_ids[r]);
        }
        if let Some(k) = cat {
            c["cat"] = json!(self.cat_ids[k]);
        }
        (u, c)
    }

    #[allow(clippy::too_many_arguments)]
    fn add(
        &mut self,
        typ: &str,
        name: &str,
        room: &'static str,
        cat: &'static str,
        states: &[&str],
        details: Value,
    ) -> String {
        let (u, c) = self.ctrl(typ, name, Some(room), Some(cat), states, details);
        self.controls.insert(u.clone(), c);
        u
    }

    fn set(&mut self, u: &str, key: &str, v: Value) {
        self.controls.get_mut(u).unwrap()[key] = v;
    }

    fn sub(
        &mut self,
        parent: &str,
        typ: &str,
        name: &str,
        states: &[&str],
        details: Value,
    ) -> String {
        let (u, c) = self.ctrl(typ, name, None, None, states, details);
        let p = self.controls.get_mut(parent).unwrap();
        if p.get("subControls").is_none() {
            p["subControls"] = json!({});
        }
        p["subControls"][&u] = c;
        u
    }

    fn state(&self, ctrl: &str, s: &str) -> String {
        self.controls[ctrl]["states"][s]
            .as_str()
            .unwrap()
            .to_string()
    }
}

const JALOUSIE: &[&str] = &[
    "up",
    "down",
    "position",
    "shadePosition",
    "safetyActive",
    "autoAllowed",
    "autoActive",
    "locked",
    "targetPosition",
];
const LIGHTCTL: &[&str] = &[
    "activeMoods",
    "moodList",
    "favoriteMoods",
    "additionalMoods",
];
const DIMMER: &[&str] = &["position", "min", "max", "step"];
const CLIMATE: &[&str] = &[
    "tempActual",
    "tempTarget",
    "comfortTemperature",
    "operatingMode",
    "prepareState",
];
const METER: &[&str] = &[
    "actual",
    "total",
    "totalDay",
    "totalWeek",
    "totalMonth",
    "totalYear",
];
const METER_BI: &[&str] = &[
    "actual",
    "total",
    "totalDay",
    "totalWeek",
    "totalMonth",
    "totalYear",
    "totalNeg",
    "totalNegDay",
    "totalNegWeek",
    "totalNegMonth",
    "totalNegYear",
];

fn analog(fmt: &str) -> Value {
    json!({"format": fmt})
}

fn digital(on: &str, off: &str, warn_on: bool) -> Value {
    let (con, coff) = if warn_on {
        ("#F0A33A", "#69C350")
    } else {
        ("#69C350", "#7D8896")
    };
    json!({"text": {"on": on, "off": off}, "color": {"on": con, "off": coff}})
}

fn stats(outputs: &[&str]) -> Value {
    let outs: Vec<Value> = outputs
        .iter()
        .enumerate()
        .map(|(i, n)| json!({"id": i, "name": n, "format": "%.1f", "uuid": uuid(9, i as u32), "visuType": 0}))
        .collect();
    json!({"frequency": 3, "outputs": outs})
}

fn meter(kind: &str) -> Value {
    json!({"type": kind, "actualFormat": "%.3fkW", "totalFormat": "%.1fkWh"})
}

/// The demo structure file (`LoxApp3.json` shape).
pub fn structure() -> Value {
    let mut b = Builder {
        n: 0,
        rooms: Map::new(),
        cats: Map::new(),
        controls: Map::new(),
        room_ids: HashMap::new(),
        cat_ids: HashMap::new(),
    };
    for r in [
        "Living room",
        "Kitchen",
        "Office",
        "Bedroom",
        "Bath",
        "Kids room",
        "Hallway",
        "Garage",
        "Garden",
        "Utility",
    ] {
        b.room(r);
    }
    for c in [
        "Lighting", "Shading", "Climate", "Sensors", "Security", "Energy", "Access",
    ] {
        b.cat(c);
    }

    // Living room
    let lc = b.add(
        "LightControllerV2",
        "Lighting",
        "Living room",
        "Lighting",
        LIGHTCTL,
        json!({"movementScene": 1}),
    );
    b.set(&lc, "isFavorite", json!(true));
    b.sub(&lc, "Dimmer", "Ceiling light", DIMMER, json!({}));
    b.sub(&lc, "Dimmer", "Reading lamp", DIMMER, json!({}));
    b.sub(
        &lc,
        "ColorPickerV2",
        "Floor LEDs",
        &["color", "sequence", "sequenceColorIdx"],
        json!({"pickerType": "Rgb"}),
    );
    let bs = b.add(
        "Jalousie",
        "Blind South",
        "Living room",
        "Shading",
        JALOUSIE,
        json!({"animation": 0, "isAutomatic": true}),
    );
    b.set(&bs, "isFavorite", json!(true));
    b.add(
        "Jalousie",
        "Blind West",
        "Living room",
        "Shading",
        JALOUSIE,
        json!({"animation": 0, "isAutomatic": true}),
    );
    let rc = b.add(
        "IRoomControllerV2",
        "Room climate",
        "Living room",
        "Climate",
        CLIMATE,
        json!({"format": "%.1f°"}),
    );
    b.set(&rc, "statistic", stats(&["tempActual", "tempTarget"]));
    b.add(
        "InfoOnlyAnalog",
        "Humidity",
        "Living room",
        "Sensors",
        &["value"],
        analog("%.0f%%"),
    );
    b.add(
        "InfoOnlyDigital",
        "Window left",
        "Living room",
        "Sensors",
        &["active"],
        digital("open", "closed", true),
    );
    b.add(
        "InfoOnlyDigital",
        "Window right",
        "Living room",
        "Sensors",
        &["active"],
        digital("open", "closed", true),
    );
    b.add(
        "PresenceDetector",
        "Presence",
        "Living room",
        "Sensors",
        &["active", "locked", "events", "infoText"],
        json!({}),
    );

    // Kitchen
    b.add(
        "Dimmer",
        "Ceiling",
        "Kitchen",
        "Lighting",
        DIMMER,
        json!({}),
    );
    b.add(
        "Switch",
        "Under-cabinet light",
        "Kitchen",
        "Lighting",
        &["active"],
        json!({}),
    );
    b.add(
        "Jalousie",
        "Blind",
        "Kitchen",
        "Shading",
        JALOUSIE,
        json!({"animation": 0}),
    );
    let kt = b.add(
        "InfoOnlyAnalog",
        "Temperature",
        "Kitchen",
        "Climate",
        &["value"],
        analog("%.1f°C"),
    );
    b.set(&kt, "statistic", stats(&["value"]));
    b.add(
        "Pushbutton",
        "Extractor fan",
        "Kitchen",
        "Climate",
        &["active"],
        json!({}),
    );

    // Office
    b.add(
        "Switch",
        "Ceiling",
        "Office",
        "Lighting",
        &["active"],
        json!({}),
    );
    b.add(
        "Dimmer",
        "Desk lamp",
        "Office",
        "Lighting",
        DIMMER,
        json!({}),
    );
    b.add(
        "Jalousie",
        "Blind",
        "Office",
        "Shading",
        JALOUSIE,
        json!({"animation": 0}),
    );
    let ot = b.add(
        "InfoOnlyAnalog",
        "Temperature",
        "Office",
        "Climate",
        &["value"],
        analog("%.1f°C"),
    );
    b.set(&ot, "statistic", stats(&["value"]));
    b.add(
        "InfoOnlyAnalog",
        "CO2",
        "Office",
        "Sensors",
        &["value"],
        analog("%.0fppm"),
    );

    // Bedroom
    let blc = b.add(
        "LightControllerV2",
        "Lighting",
        "Bedroom",
        "Lighting",
        LIGHTCTL,
        json!({}),
    );
    b.sub(&blc, "Dimmer", "Bedside left", DIMMER, json!({}));
    b.sub(&blc, "Dimmer", "Bedside right", DIMMER, json!({}));
    b.add(
        "Jalousie",
        "Blind",
        "Bedroom",
        "Shading",
        JALOUSIE,
        json!({"animation": 0}),
    );
    let brc = b.add(
        "IRoomControllerV2",
        "Room climate",
        "Bedroom",
        "Climate",
        CLIMATE,
        json!({"format": "%.1f°"}),
    );
    b.set(&brc, "statistic", stats(&["tempActual", "tempTarget"]));

    // Bath
    b.add(
        "Switch",
        "Mirror light",
        "Bath",
        "Lighting",
        &["active"],
        json!({}),
    );
    b.add(
        "Dimmer",
        "Ceiling light",
        "Bath",
        "Lighting",
        DIMMER,
        json!({}),
    );
    b.add(
        "InfoOnlyDigital",
        "Window",
        "Bath",
        "Sensors",
        &["active"],
        digital("open", "closed", true),
    );
    b.add(
        "IRoomControllerV2",
        "Floor heating",
        "Bath",
        "Climate",
        CLIMATE,
        json!({"format": "%.1f°"}),
    );
    b.add(
        "InfoOnlyAnalog",
        "Humidity",
        "Bath",
        "Sensors",
        &["value"],
        analog("%.0f%%"),
    );

    // Kids room
    let klc = b.add(
        "LightControllerV2",
        "Lighting",
        "Kids room",
        "Lighting",
        LIGHTCTL,
        json!({}),
    );
    b.sub(&klc, "Dimmer", "Ceiling light", DIMMER, json!({}));
    b.add(
        "Jalousie",
        "Blind",
        "Kids room",
        "Shading",
        JALOUSIE,
        json!({"animation": 0}),
    );
    b.add(
        "InfoOnlyAnalog",
        "Temperature",
        "Kids room",
        "Climate",
        &["value"],
        analog("%.1f°C"),
    );

    // Hallway
    let hlc = b.add(
        "LightControllerV2",
        "Hallway light",
        "Hallway",
        "Lighting",
        LIGHTCTL,
        json!({"movementScene": 2}),
    );
    b.sub(&hlc, "Dimmer", "Spots", DIMMER, json!({}));
    b.add(
        "PresenceDetector",
        "Motion",
        "Hallway",
        "Sensors",
        &["active", "locked", "events", "infoText"],
        json!({}),
    );
    b.add(
        "InfoOnlyDigital",
        "Door contact",
        "Hallway",
        "Access",
        &["active"],
        digital("open", "closed", false),
    );
    let fd = b.add(
        "DoorLock",
        "Front door",
        "Hallway",
        "Access",
        &["active", "jLocked"],
        json!({}),
    );
    b.set(&fd, "isSecured", json!(true));
    b.add(
        "IntercomV2",
        "Intercom",
        "Hallway",
        "Access",
        &["bell", "lastBellEvents", "address"],
        json!({"deviceType": 1}),
    );

    // Garage
    b.add(
        "Gate",
        "Garage door",
        "Garage",
        "Access",
        &["position", "active", "preventOpen", "preventClose"],
        json!({"animation": 0}),
    );
    b.add(
        "Switch",
        "Garage light",
        "Garage",
        "Lighting",
        &["active"],
        json!({}),
    );
    let wb = b.add(
        "Wallbox2",
        "Wallbox",
        "Garage",
        "Energy",
        &["charging", "power", "energySession", "limit", "connected"],
        json!({"maxPower": 11}),
    );
    b.set(&wb, "isFavorite", json!(true));
    b.add(
        "InfoOnlyAnalog",
        "Temperature",
        "Garage",
        "Climate",
        &["value"],
        analog("%.1f°C"),
    );

    // Garden
    let out = b.add(
        "InfoOnlyAnalog",
        "Outside temperature",
        "Garden",
        "Sensors",
        &["value"],
        analog("%.1f°C"),
    );
    b.set(&out, "statistic", stats(&["value"]));
    b.set(&out, "isFavorite", json!(true));
    b.add(
        "InfoOnlyAnalog",
        "Brightness",
        "Garden",
        "Sensors",
        &["value"],
        analog("%.0flx"),
    );
    b.add(
        "InfoOnlyDigital",
        "Rain",
        "Garden",
        "Sensors",
        &["active"],
        digital("raining", "dry", false),
    );
    b.add(
        "Switch",
        "Terrace light",
        "Garden",
        "Lighting",
        &["active"],
        json!({}),
    );
    b.add(
        "Pushbutton",
        "Irrigation",
        "Garden",
        "Climate",
        &["active"],
        json!({}),
    );

    // Utility: energy + security
    let grid = b.add(
        "Meter",
        "Grid",
        "Utility",
        "Energy",
        METER_BI,
        meter("bidirectional"),
    );
    let pv = b.add(
        "Meter",
        "PV",
        "Utility",
        "Energy",
        METER,
        meter("unidirectional"),
    );
    let mut sm = METER_BI.to_vec();
    sm.push("storage");
    let batt = b.add(
        "Meter",
        "Battery",
        "Utility",
        "Energy",
        &sm,
        json!({"type": "storage", "actualFormat": "%.3fkW", "storageFormat": "%.0f%%"}),
    );
    let hp = b.add(
        "Meter",
        "Heat pump",
        "Utility",
        "Energy",
        METER,
        meter("unidirectional"),
    );
    let wbm = b.add(
        "Meter",
        "Wallbox meter",
        "Garage",
        "Energy",
        METER,
        meter("unidirectional"),
    );
    for m in [&grid, &pv, &batt, &hp, &wbm] {
        b.set(m, "statistic", stats(&["actual", "total"]));
    }
    let efm = b.add(
        "EFM",
        "Energy flow",
        "Utility",
        "Energy",
        &[
            "Gpwr", "Ppwr", "Spwr", "Ssoc", "actual0", "actual1", "actual2", "actual3",
        ],
        json!({}),
    );
    let rest = b.sub(&efm, "Meter", "Rest", METER_BI, meter("bidirectional"));
    let _ = rest;
    let nodes = json!([
        {"uuid": uuid(7, 1), "title": "Grid", "icon": "", "ctrlUuid": grid, "nodeType": "Grid", "actualEfmState": b.state(&efm, "actual0")},
        {"uuid": uuid(7, 2), "title": "PV", "icon": "", "ctrlUuid": pv, "nodeType": "Production", "actualEfmState": b.state(&efm, "actual1")},
        {"uuid": uuid(7, 3), "title": "Battery", "icon": "", "ctrlUuid": batt, "nodeType": "Storage", "actualEfmState": b.state(&efm, "actual2")},
        {"uuid": uuid(7, 4), "title": "House", "icon": "", "nodeType": "Load", "actualEfmState": b.state(&efm, "actual3"), "nodes": [
            {"uuid": uuid(7, 5), "title": "Heat pump", "icon": "", "ctrlUuid": hp, "nodeType": "Load"},
            {"uuid": uuid(7, 6), "title": "Wallbox", "icon": "", "ctrlUuid": wbm, "nodeType": "Load"}
        ]}
    ]);
    b.controls.get_mut(&efm).unwrap()["details"]["nodes"] = nodes;
    let al = b.add(
        "Alarm",
        "House alarm",
        "Utility",
        "Security",
        &[
            "armed",
            "nextLevel",
            "nextLevelDelay",
            "level",
            "disabledMove",
            "startTime",
        ],
        json!({"alert": true, "presenceConnected": true}),
    );
    b.set(&al, "isSecured", json!(true));
    b.add(
        "SmokeAlarm",
        "Smoke detectors",
        "Utility",
        "Security",
        &["level", "nextLevel", "timeServiceMode"],
        json!({}),
    );
    b.add(
        "TextState",
        "House status",
        "Utility",
        "Sensors",
        &["textAndIcon"],
        json!({}),
    );

    let mut gs = Map::new();
    for g in [
        "operatingMode",
        "sunrise",
        "sunset",
        "notifications",
        "miniserverTime",
    ] {
        gs.insert(g.into(), Value::String(b.next(5)));
    }
    json!({
        "lastModified": "2026-09-20 18:00:00",
        "msInfo": {
            "serialNr": "504F94000000", "msName": "Demo House", "projectName": "Demo House",
            "location": "Demo Street 1", "tempUnit": 0, "currency": "€", "squareMeasure": "m²",
            "languageCode": "ENG", "miniserverType": 2, "swVersion": "15.3.4.2",
        },
        "globalStates": gs,
        "operatingModes": {"0": "Holiday", "1": "Day off", "2": "Weekday", "3": "Weekend", "-1": "Absent"},
        "rooms": b.rooms, "cats": b.cats, "controls": b.controls,
    })
}

// ── Simulator ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct Motion {
    target: f64,
    /// fraction per second
    speed: f64,
}

/// A tiny house simulator: holds state values, reacts to commands, and produces
/// periodic changes (energy, temperatures, motion in the hallway).
pub struct Sim {
    /// state uuid → value
    values: HashMap<String, f64>,
    texts: HashMap<String, String>,
    /// control uuid → (type, state name → uuid)
    ctrls: HashMap<String, (String, HashMap<String, String>)>,
    /// (room name, control name) → control uuid
    names: HashMap<(String, String), String>,
    /// control uuid → sub-controls
    subs: HashMap<String, Vec<String>>,
    moving: HashMap<String, Motion>,
    rng: u64,
    t: f64,
    last_energy: f64,
    last_temp: f64,
    next_motion: f64,
    motion_off_at: Option<f64>,
    out: Vec<StateEvent>,
}

impl Sim {
    pub fn new(st: &Value) -> Sim {
        let mut sim = Sim {
            values: HashMap::new(),
            texts: HashMap::new(),
            ctrls: HashMap::new(),
            names: HashMap::new(),
            subs: HashMap::new(),
            moving: HashMap::new(),
            rng: 0x2545F4914F6CDD1D,
            t: 0.0,
            last_energy: 0.0,
            last_temp: 0.0,
            next_motion: 6.0,
            motion_off_at: None,
            out: Vec::new(),
        };
        let rooms: HashMap<String, String> = st["rooms"]
            .as_object()
            .map(|o| {
                o.iter()
                    .map(|(k, v)| (k.clone(), v["name"].as_str().unwrap_or("").to_string()))
                    .collect()
            })
            .unwrap_or_default();
        fn walk(
            sim: &mut Sim,
            rooms: &HashMap<String, String>,
            uuid: &str,
            c: &Value,
            room: Option<String>,
        ) {
            let room = c["room"]
                .as_str()
                .and_then(|r| rooms.get(r).cloned())
                .or(room);
            let states: HashMap<String, String> = c["states"]
                .as_object()
                .map(|o| {
                    o.iter()
                        .filter_map(|(k, v)| v.as_str().map(|u| (k.clone(), u.to_string())))
                        .collect()
                })
                .unwrap_or_default();
            let typ = c["type"].as_str().unwrap_or("").to_string();
            sim.names.insert(
                (
                    room.clone().unwrap_or_default(),
                    c["name"].as_str().unwrap_or("").to_string(),
                ),
                uuid.to_string(),
            );
            sim.ctrls.insert(uuid.to_string(), (typ, states));
            if let Some(subs) = c["subControls"].as_object() {
                for (su, sc) in subs {
                    sim.subs
                        .entry(uuid.to_string())
                        .or_default()
                        .push(su.clone());
                    walk(sim, rooms, su, sc, room.clone());
                }
            }
        }
        if let Some(obj) = st["controls"].as_object() {
            for (u, c) in obj {
                walk(&mut sim, &rooms, u, c, None);
            }
        }
        sim.initial_values(st);
        sim
    }

    fn rand(&mut self) -> f64 {
        // xorshift64*
        self.rng ^= self.rng >> 12;
        self.rng ^= self.rng << 25;
        self.rng ^= self.rng >> 27;
        (self.rng.wrapping_mul(0x2545F4914F6CDD1D) >> 11) as f64 / (1u64 << 53) as f64
    }

    fn su(&self, room: &str, name: &str, state: &str) -> Option<String> {
        let c = self.names.get(&(room.to_string(), name.to_string()))?;
        self.ctrls[c].1.get(state).cloned()
    }

    fn put(&mut self, uuid: &str, v: f64) {
        let old = self.values.insert(uuid.to_string(), v);
        if old != Some(v) {
            self.out.push(StateEvent::ValueState {
                uuid: uuid.to_string(),
                value: v,
            });
        }
    }

    fn put_text(&mut self, uuid: &str, t: &str) {
        let old = self.texts.insert(uuid.to_string(), t.to_string());
        if old.as_deref() != Some(t) {
            self.out.push(StateEvent::TextState {
                uuid: uuid.to_string(),
                icon_uuid: String::new(),
                text: t.to_string(),
            });
        }
    }

    fn set(&mut self, room: &str, name: &str, state: &str, v: f64) {
        if let Some(u) = self.su(room, name, state) {
            self.put(&u, v);
        }
    }

    fn get(&self, room: &str, name: &str, state: &str) -> f64 {
        self.su(room, name, state)
            .and_then(|u| self.values.get(&u).copied())
            .unwrap_or(0.0)
    }

    fn initial_values(&mut self, st: &Value) {
        // every state starts at 0 / empty
        let all: Vec<(String, String)> = self
            .ctrls
            .values()
            .flat_map(|(t, s)| s.values().map(move |u| (t.clone(), u.clone())))
            .collect();
        for (_, u) in &all {
            self.values.insert(u.clone(), 0.0);
        }
        // dimmer ranges
        for (t, s) in self.ctrls.values() {
            if t.contains("Dimmer") {
                for (k, v) in [("min", 0.0), ("max", 100.0), ("step", 1.0)] {
                    if let Some(u) = s.get(k) {
                        self.values.insert(u.clone(), v);
                    }
                }
            }
        }
        let moods = r#"[{"name":"Bright","id":1,"static":false},{"name":"Evening","id":2,"static":false},{"name":"Reading","id":3,"static":false},{"name":"Off","id":778,"static":true}]"#;
        let lcs: Vec<String> = self
            .ctrls
            .iter()
            .filter(|(_, (t, _))| t == "LightControllerV2")
            .map(|(u, _)| u.clone())
            .collect();
        for lc in lcs {
            let s = self.ctrls[&lc].1.clone();
            self.texts.insert(s["moodList"].clone(), moods.into());
            self.texts.insert(s["activeMoods"].clone(), "[778]".into());
            self.texts
                .insert(s["favoriteMoods"].clone(), "[1,2]".into());
            self.texts.insert(s["additionalMoods"].clone(), "[]".into());
        }
        let mut init = |room: &str, name: &str, state: &str, v: f64| {
            if let Some(u) = self.su(room, name, state) {
                self.values.insert(u, v);
            }
        };
        init("Kitchen", "Ceiling", "position", 100.0);
        init("Kitchen", "Ceiling", "max", 100.0);
        init("Office", "Desk lamp", "position", 40.0);
        init("Office", "Ceiling", "active", 1.0);
        init("Living room", "Blind South", "position", 0.33);
        init("Living room", "Blind South", "shadePosition", 0.45);
        init("Living room", "Blind South", "autoAllowed", 1.0);
        init("Office", "Blind", "position", 0.6);
        init("Bedroom", "Blind", "position", 1.0);
        init("Living room", "Room climate", "tempActual", 22.1);
        init("Living room", "Room climate", "tempTarget", 22.0);
        init("Living room", "Room climate", "comfortTemperature", 22.0);
        init("Bedroom", "Room climate", "tempActual", 19.8);
        init("Bedroom", "Room climate", "tempTarget", 19.0);
        init("Bedroom", "Room climate", "comfortTemperature", 19.0);
        init("Bath", "Floor heating", "tempActual", 23.5);
        init("Bath", "Floor heating", "tempTarget", 24.0);
        init("Bath", "Floor heating", "comfortTemperature", 24.0);
        init("Kitchen", "Temperature", "value", 21.4);
        init("Office", "Temperature", "value", 22.9);
        init("Kids room", "Temperature", "value", 21.0);
        init("Garage", "Temperature", "value", 14.2);
        init("Garden", "Outside temperature", "value", 18.4);
        init("Garden", "Brightness", "value", 22000.0);
        init("Living room", "Humidity", "value", 48.0);
        init("Bath", "Humidity", "value", 61.0);
        init("Office", "CO2", "value", 740.0);
        init("Bath", "Window", "active", 1.0);
        init("Living room", "Presence", "active", 1.0);
        init("Hallway", "Front door", "active", 0.0);
        init("Utility", "Battery", "storage", 74.0);
        init("Utility", "Grid", "total", 12450.2);
        init("Utility", "PV", "total", 8120.7);
        init("Utility", "Heat pump", "total", 3310.4);
        init("Garage", "Wallbox meter", "total", 2204.9);
        // period totals consistent with today's curve so far
        {
            use chrono::{Datelike, Timelike};
            let now = chrono::Local::now();
            let hour = now.hour() as f64 + now.minute() as f64 / 60.0;
            let (pv, usage) = energy_today(hour);
            let pv_day: f64 = pv.iter().take((hour * 4.0) as usize + 1).sum::<f64>() / 4.0;
            let use_day: f64 = usage.iter().sum::<f64>() / 4.0;
            let wd = now.weekday().num_days_from_monday() as f64;
            let md = now.day() as f64 - 1.0;
            for (room, name, day, per_day) in [
                ("Utility", "PV", pv_day, 24.0),
                ("Utility", "Heat pump", use_day * 0.4, 9.0),
                ("Utility", "Grid", use_day * 0.3, 6.5),
                ("Garage", "Wallbox meter", 0.0, 7.0),
            ] {
                let r = |v: f64| (v * 1000.0).round() / 1000.0;
                init(room, name, "totalDay", r(day));
                init(room, name, "totalWeek", r(day + wd * per_day));
                init(room, name, "totalMonth", r(day + md * per_day));
                init(
                    room,
                    name,
                    "totalYear",
                    r(day + (now.ordinal() as f64 - 1.0) * per_day * 0.8),
                );
            }
        }
        init("Garage", "Wallbox", "limit", 20.0);
        init("Garage", "Wallbox", "connected", 1.0);
        let _ = st;
        // living room: evening mood
        if let Some(u) = self.su("Living room", "Lighting", "activeMoods") {
            self.texts.insert(u, "[2]".into());
        }
        if let Some(lc) = self
            .names
            .get(&("Living room".to_string(), "Lighting".to_string()))
            .cloned()
        {
            self.apply_mood(&lc, 2, false);
        }
        if let Some(u) = self.su("Utility", "House status", "textAndIcon") {
            self.texts
                .insert(u, "All windows closed except Bath".into());
        }
        self.energy_step(0.0);
        self.out.clear();
    }

    /// Every known state as one burst (what the Miniserver sends on subscribe).
    pub fn initial_events(&self) -> Vec<StateEvent> {
        let mut out: Vec<StateEvent> = self
            .values
            .iter()
            .map(|(u, v)| StateEvent::ValueState {
                uuid: u.clone(),
                value: *v,
            })
            .collect();
        out.extend(self.texts.iter().map(|(u, t)| StateEvent::TextState {
            uuid: u.clone(),
            icon_uuid: String::new(),
            text: t.clone(),
        }));
        out
    }

    fn apply_mood(&mut self, lc: &str, mood: u32, emit: bool) {
        let subs = self.subs.get(lc).cloned().unwrap_or_default();
        for (i, s) in subs.iter().enumerate() {
            let (typ, states) = self.ctrls[s].clone();
            let level = match mood {
                778 => 0.0,
                1 => 100.0,
                2 => [65.0, 30.0, 40.0][i % 3],
                _ => [20.0, 100.0, 0.0][i % 3],
            };
            if typ == "ColorPickerV2" {
                let t = if level > 0.0 {
                    format!("hsv(30,60,{})", level as i64)
                } else {
                    "hsv(30,60,0)".to_string()
                };
                if emit {
                    self.put_text(&states["color"], &t);
                } else {
                    self.texts.insert(states["color"].clone(), t);
                }
            } else if let Some(u) = states.get("position") {
                if emit {
                    self.put(u, level);
                } else {
                    self.values.insert(u.clone(), level);
                }
            }
        }
    }

    /// Apply a command sent to a control; returns the Miniserver-style JSON reply.
    pub fn command(&mut self, uuid: &str, cmd: &str) -> Result<Value, String> {
        let Some((typ, states)) = self.ctrls.get(uuid).cloned() else {
            return Err(format!("unknown control {}", uuid));
        };
        let st = |s: &str| states.get(s).cloned().unwrap_or_default();
        let num = cmd.parse::<f64>().ok();
        let (head, arg) = cmd.split_once('/').unwrap_or((cmd, ""));
        let argf = arg.split('/').next().and_then(|a| a.parse::<f64>().ok());
        let ok = match typ.as_str() {
            "Switch" | "Pushbutton" => match cmd {
                "on" => {
                    self.put(&st("active"), 1.0);
                    true
                }
                "off" => {
                    self.put(&st("active"), 0.0);
                    true
                }
                "pulse" => {
                    let v = if typ == "Switch" {
                        1.0 - self.values.get(&st("active")).copied().unwrap_or(0.0)
                    } else {
                        1.0
                    };
                    self.put(&st("active"), v);
                    true
                }
                _ => false,
            },
            "Dimmer" => {
                if let Some(v) = num {
                    self.put(&st("position"), v.clamp(0.0, 100.0));
                    true
                } else if cmd == "on" {
                    self.put(&st("position"), 100.0);
                    true
                } else if cmd == "off" {
                    self.put(&st("position"), 0.0);
                    true
                } else {
                    false
                }
            }
            "ColorPickerV2" => {
                if cmd.starts_with("hsv(") || cmd.starts_with("temp(") {
                    self.put_text(&st("color"), cmd);
                    true
                } else if cmd == "off" {
                    self.put_text(&st("color"), "hsv(30,60,0)");
                    true
                } else if cmd == "on" {
                    self.put_text(&st("color"), "hsv(30,60,100)");
                    true
                } else {
                    false
                }
            }
            "LightControllerV2" => {
                let cur: Vec<u32> = self
                    .texts
                    .get(&st("activeMoods"))
                    .and_then(|t| serde_json::from_str(t).ok())
                    .unwrap_or_default();
                let cur = cur.first().copied().unwrap_or(778);
                let next = match (head, argf) {
                    ("on", _) => Some(if cur == 778 { 1 } else { cur }),
                    ("off", _) => Some(778),
                    ("changeTo" | "setMood", Some(m)) => Some(m as u32),
                    ("plus", _) => Some(match cur {
                        778 => 1,
                        3 => 778,
                        n => n + 1,
                    }),
                    ("minus", _) => Some(match cur {
                        778 => 3,
                        1 => 778,
                        n => n - 1,
                    }),
                    _ => None,
                };
                match next {
                    Some(m) => {
                        self.put_text(&st("activeMoods"), &format!("[{}]", m));
                        self.apply_mood(uuid, m, true);
                        true
                    }
                    None => false,
                }
            }
            "Jalousie" => {
                let target = match (head, argf) {
                    ("FullUp", _) | ("up", _) => Some(0.0),
                    ("FullDown", _) | ("down", _) => Some(1.0),
                    ("AutomaticDown", _) => Some(0.6),
                    ("manualPosition", Some(p)) => Some(p / 100.0),
                    ("manualLamella", Some(p)) => {
                        self.put(&st("shadePosition"), p / 100.0);
                        None
                    }
                    ("off", _) | ("stop", _) => {
                        self.moving.remove(uuid);
                        self.put(&st("up"), 0.0);
                        self.put(&st("down"), 0.0);
                        return Ok(ok_reply(uuid, cmd));
                    }
                    _ => return Err(format!("{} is not a Jalousie command", cmd)),
                };
                if let Some(t) = target {
                    let pos = self.values.get(&st("position")).copied().unwrap_or(0.0);
                    self.put(&st("targetPosition"), t);
                    self.put(&st("up"), if t < pos { 1.0 } else { 0.0 });
                    self.put(&st("down"), if t > pos { 1.0 } else { 0.0 });
                    self.moving.insert(
                        uuid.to_string(),
                        Motion {
                            target: t,
                            speed: 0.08,
                        },
                    );
                }
                true
            }
            "Gate" => {
                let t = match cmd {
                    "open" => Some(1.0),
                    "close" => Some(0.0),
                    "stop" => None,
                    _ => return Err(format!("{} is not a Gate command", cmd)),
                };
                match t {
                    Some(t) => {
                        let pos = self.values.get(&st("position")).copied().unwrap_or(0.0);
                        self.put(&st("active"), if t > pos { 1.0 } else { -1.0 });
                        self.moving.insert(
                            uuid.to_string(),
                            Motion {
                                target: t,
                                speed: 0.1,
                            },
                        );
                    }
                    None => {
                        self.moving.remove(uuid);
                        self.put(&st("active"), 0.0);
                    }
                }
                true
            }
            "IRoomControllerV2" => match (head, argf) {
                ("setComfortTemperature", Some(v)) => {
                    self.put(&st("comfortTemperature"), v);
                    self.put(&st("tempTarget"), v);
                    true
                }
                ("setOperatingMode", Some(v)) => {
                    self.put(&st("operatingMode"), v);
                    true
                }
                ("override", Some(v)) => {
                    self.put(&st("tempTarget"), v);
                    true
                }
                _ => false,
            },
            "Alarm" => match head {
                "delayedon" | "on" => {
                    self.put(&st("armed"), 1.0);
                    self.put(
                        &st("disabledMove"),
                        if arg.starts_with('0') { 1.0 } else { 0.0 },
                    );
                    true
                }
                "off" => {
                    self.put(&st("armed"), 0.0);
                    self.put(&st("level"), 0.0);
                    true
                }
                "quit" => {
                    self.put(&st("level"), 0.0);
                    true
                }
                _ => false,
            },
            "DoorLock" => match cmd {
                "on" => {
                    self.put(&st("active"), 1.0);
                    true
                }
                "off" | "open" => {
                    self.put(&st("active"), 0.0);
                    true
                }
                _ => false,
            },
            "IntercomV2" => matches!(cmd, "answer" | "hangup" | "open"),
            "Wallbox2" => match head {
                "start" => {
                    self.put(&st("charging"), 1.0);
                    if let Some(l) = argf {
                        self.put(&st("limit"), l);
                    }
                    true
                }
                "stop" | "pause" => {
                    self.put(&st("charging"), 0.0);
                    self.put(&st("power"), 0.0);
                    true
                }
                _ => false,
            },
            _ => false,
        };
        if ok {
            Ok(ok_reply(uuid, cmd))
        } else {
            Err(format!("demo: '{}' not supported by {}", cmd, typ))
        }
    }

    /// Advance the simulation to `t` (seconds since start) and return new events.
    pub fn tick(&mut self, t: f64, hour: f64) -> Vec<StateEvent> {
        let dt = (t - self.t).clamp(0.0, 5.0);
        self.t = t;
        // motion (blinds, gates)
        let moving: Vec<(String, Motion)> = self
            .moving
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        for (u, m) in moving {
            let (typ, states) = self.ctrls[&u].clone();
            let pu = &states["position"];
            let pos = self.values.get(pu).copied().unwrap_or(0.0);
            let step = m.speed * dt;
            let np = if (m.target - pos).abs() <= step {
                m.target
            } else if m.target > pos {
                pos + step
            } else {
                pos - step
            };
            self.put(pu, (np * 1000.0).round() / 1000.0);
            if (np - m.target).abs() < 1e-9 {
                self.moving.remove(&u);
                if typ == "Gate" {
                    self.put(&states["active"], 0.0);
                } else {
                    self.put(&states["up"], 0.0);
                    self.put(&states["down"], 0.0);
                }
            }
        }
        // hallway motion → hallway light
        if t >= self.next_motion {
            self.set("Hallway", "Motion", "active", 1.0);
            let lc = self.names[&("Hallway".to_string(), "Hallway light".to_string())].clone();
            let am = self.ctrls[&lc].1["activeMoods"].clone();
            self.put_text(&am, "[1]");
            self.apply_mood(&lc, 1, true);
            self.motion_off_at = Some(t + 8.0);
            self.next_motion = t + 20.0 + self.rand() * 25.0;
        }
        if let Some(off) = self.motion_off_at
            && t >= off
        {
            self.motion_off_at = None;
            self.set("Hallway", "Motion", "active", 0.0);
            let lc = self.names[&("Hallway".to_string(), "Hallway light".to_string())].clone();
            let am = self.ctrls[&lc].1["activeMoods"].clone();
            self.put_text(&am, "[778]");
            self.apply_mood(&lc, 778, true);
        }
        // energy every 2 s (real meters report every few seconds)
        if t - self.last_energy >= 2.0 {
            self.last_energy = t;
            self.energy_step(hour);
        }
        // temperatures and sensors every 7 s
        if t - self.last_temp >= 7.0 {
            self.last_temp = t;
            for (room, name, state, amp) in [
                ("Living room", "Room climate", "tempActual", 0.1),
                ("Bedroom", "Room climate", "tempActual", 0.1),
                ("Bath", "Floor heating", "tempActual", 0.1),
                ("Kitchen", "Temperature", "value", 0.1),
                ("Office", "Temperature", "value", 0.1),
                ("Garden", "Outside temperature", "value", 0.2),
                ("Office", "CO2", "value", 15.0),
                ("Living room", "Humidity", "value", 1.0),
            ] {
                let v = self.get(room, name, state);
                let d = (self.rand() - 0.5) * 2.0 * amp;
                let nv = if amp < 1.0 {
                    ((v + d) * 10.0).round() / 10.0
                } else {
                    (v + d).round()
                };
                self.set(room, name, state, nv);
            }
        }
        std::mem::take(&mut self.out)
    }

    fn energy_step(&mut self, hour: f64) {
        let day = ((hour - 6.0) / 14.0 * std::f64::consts::PI).sin().max(0.0);
        let pv = (day * 7.8 * (0.85 + 0.15 * self.rand()) * 1000.0).round() / 1000.0;
        let hp = if self.rand() > 0.3 { 1.9 } else { 0.0 } * (0.9 + self.rand() * 0.2);
        let wb_on = self.get("Garage", "Wallbox", "charging") > 0.5;
        let wb = if wb_on { 11.0 } else { 0.0 };
        self.set("Garage", "Wallbox", "power", wb);
        let base = 0.35 + self.rand() * 0.8;
        let load = base + hp + wb;
        let soc = self.get("Utility", "Battery", "storage");
        let surplus = pv - load;
        let batt = if surplus > 0.0 && soc < 100.0 {
            surplus.min(5.0)
        } else if surplus < 0.0 && soc > 5.0 {
            surplus.max(-5.0)
        } else {
            0.0
        };
        let grid = -(surplus - batt); // + import, - export
        let r3 = |v: f64| (v * 1000.0).round() / 1000.0;
        self.set("Utility", "PV", "actual", r3(pv));
        self.set("Utility", "Heat pump", "actual", r3(hp));
        self.set("Garage", "Wallbox meter", "actual", r3(wb));
        self.set("Utility", "Grid", "actual", r3(grid));
        // Loxone reports storage like a source: + discharging
        self.set("Utility", "Battery", "actual", r3(-batt));
        let nsoc = (soc + batt * 2.0 / 3600.0 * 10.0).clamp(0.0, 100.0);
        self.set(
            "Utility",
            "Battery",
            "storage",
            (nsoc * 100.0).round() / 100.0,
        );
        for (room, n, p) in [
            ("Utility", "PV", pv),
            ("Utility", "Heat pump", hp),
            ("Garage", "Wallbox meter", wb),
        ] {
            for s in ["total", "totalDay", "totalWeek", "totalMonth", "totalYear"] {
                let v = self.get(room, n, s) + p * 2.0 / 3600.0;
                self.set(room, n, s, (v * 1000.0).round() / 1000.0);
            }
        }
        for s in ["total", "totalDay"] {
            let v = self.get("Utility", "Grid", s) + grid.max(0.0) * 2.0 / 3600.0;
            self.set("Utility", "Grid", s, (v * 1000.0).round() / 1000.0);
        }
        self.set("Utility", "Energy flow", "Gpwr", r3(grid));
        self.set("Utility", "Energy flow", "Ppwr", r3(pv));
        self.set("Utility", "Energy flow", "Spwr", r3(-batt));
        self.set("Utility", "Energy flow", "Ssoc", nsoc.round());
        self.set("Utility", "Energy flow", "actual0", r3(grid));
        self.set("Utility", "Energy flow", "actual1", r3(pv));
        self.set("Utility", "Energy flow", "actual2", r3(-batt));
        self.set("Utility", "Energy flow", "actual3", r3(load));
    }
}

fn ok_reply(uuid: &str, cmd: &str) -> Value {
    json!({"LL": {"control": format!("jdev/sps/io/{}/{}", uuid, cmd), "value": "1", "Code": "200"}})
}

// ── Fake polls ───────────────────────────────────────────────────────────────

fn wobble(t: f64, period: f64, lo: f64, hi: f64) -> f64 {
    lo + (hi - lo) * (0.5 + 0.5 * (t / period * std::f64::consts::TAU).sin())
}

pub fn diag(t: f64) -> Diag {
    Diag {
        cpu: Some((wobble(t, 37.0, 9.0, 31.0) + wobble(t, 5.0, 0.0, 6.0)).round()),
        sps: Some(wobble(t, 53.0, 18.0, 26.0).round()),
        heap_used_kb: Some(wobble(t, 300.0, 350_000.0, 372_000.0).round()),
        heap_total_kb: Some(1_016_404.0),
        tasks: Some(64.0),
        ctx_switches: Some(wobble(t, 17.0, 8_400.0, 9_900.0).round()),
        ints: Some(wobble(t, 11.0, 1_100.0, 1_400.0).round()),
        comints: Some(wobble(t, 23.0, 300.0, 420.0).round()),
        sd: Some(
            "SD Performance: Read: 7598kB/s, Write: 3813kB/s, No error (0 0), Usage: 0.00%, \
             Used: 4%, UncorrectableEcc: 0, PowerOnCycles: 23"
                .into(),
        ),
        plc: Some("Running 100/sec".into()),
        clock_drift: Some(0.3),
    }
}

pub fn info() -> MsInfo {
    MsInfo {
        firmware: "15.3.4.2".into(),
        serial: "504F94000000".into(),
        ms_type: "Miniserver Gen 2".into(),
        ip: "192.0.2.20".into(),
        mask: "255.255.255.0".into(),
        gateway: "192.0.2.1".into(),
        dns: vec!["192.0.2.1".into(), "9.9.9.9".into()],
        mac: "50:4F:94:00:00:00".into(),
        dhcp: Some(false),
        ntp: Some(true),
        structure_version: "2026-09-20 18:00:00".into(),
        sps_state: Some(5),
        ms_time: None,
    }
}

pub fn bus_lan(t: f64) -> BusLan {
    let k = (t / 2.0) as u64;
    BusLan {
        counters: vec![
            ("CAN sent".into(), Some(1_204_332 + k * 37)),
            ("CAN received".into(), Some(2_993_110 + k * 81)),
            ("CAN receive errors".into(), Some(0)),
            ("CAN frame errors".into(), Some(0)),
            ("CAN overruns".into(), Some(2)),
            ("CAN parity errors".into(), Some(0)),
            ("LAN tx packets".into(), Some(8_812_004 + k * 140)),
            ("LAN tx errors".into(), Some(0)),
            ("LAN tx collisions".into(), Some(0)),
            ("LAN tx underruns".into(), Some(0)),
            ("LAN rx packets".into(), Some(9_101_552 + k * 162)),
            ("LAN rx overflows".into(), Some(0)),
            ("LAN rx EOF".into(), Some(0)),
            ("LAN exhausted".into(), Some(0)),
            ("LAN no buffer".into(), Some(if t > 60.0 { 1 } else { 0 })),
        ],
    }
}

pub fn devices() -> Vec<Device> {
    let d =
        |name: &str, kind: &str, place: &str, online: bool, battery: Option<u32>, q: Option<u8>| {
            Device {
                name: name.into(),
                kind: kind.into(),
                place: if place.is_empty() {
                    None
                } else {
                    Some(place.into())
                },
                online,
                battery,
                signal: q,
                last_seen: None,
                firmware: Some("15.3.4.2".into()),
            }
        };
    vec![
        d("Tree Extension", "Tree Extension", "", true, None, None),
        d("Air Base Extension", "Air Base", "", true, None, None),
        d("Extension", "Extension", "", true, None, None),
        d(
            "Touch Pure Living",
            "Touch Pure Tree",
            "Living room",
            true,
            None,
            None,
        ),
        d(
            "Presence Sensor Living",
            "Presence Sensor Tree",
            "Living room",
            true,
            None,
            None,
        ),
        d(
            "Window contact Bath",
            "Door & Window Contact Air",
            "Bath",
            true,
            Some(8),
            Some(3),
        ),
        d(
            "Window contact Office",
            "Door & Window Contact Air",
            "Office",
            true,
            Some(71),
            Some(4),
        ),
        d(
            "Smoke detector Hallway",
            "Smoke Detector Air",
            "Hallway",
            true,
            Some(88),
            Some(4),
        ),
        d(
            "Smoke detector Bedroom",
            "Smoke Detector Air",
            "Bedroom",
            true,
            Some(64),
            Some(2),
        ),
        d(
            "Temperature Garage",
            "Temperature & Humidity Sensor Air",
            "Garage",
            false,
            Some(33),
            None,
        ),
        d(
            "Valve Bath",
            "Valve Actuator Air",
            "Bath",
            true,
            Some(55),
            Some(3),
        ),
        d(
            "Nano IO Garden",
            "Nano IO Air",
            "Garden",
            true,
            None,
            Some(2),
        ),
        d(
            "Weather Station",
            "Weather Station Tree",
            "Garden",
            true,
            None,
            None,
        ),
        d(
            "RGBW Dimmer Living",
            "RGBW 24V Dimmer Tree",
            "Living room",
            true,
            None,
            None,
        ),
    ]
}

pub fn log() -> Vec<LogLine> {
    let raw = [
        "2026-09-27 06:00:02.114;Important 1045 Operating mode changed: Weekday",
        "2026-09-27 06:45:13.502;Info Autopilot 'Morning blinds' executed",
        "2026-09-27 08:12:40.003;Warning 503 Air device 'Temperature Garage' offline",
        "2026-09-27 09:30:11.870;Info Config loaded: Demo House.Loxone",
        "2026-09-27 10:02:55.431;Warning 516 Battery low: Window contact Bath (8%)",
        "2026-09-27 11:18:09.228;Info User 'admin' logged in via Web",
        "2026-09-27 12:00:00.001;Info Statistics saved",
        "2026-09-27 12:47:31.994;Error 29 NTP server not reachable, retry in 60s",
        "2026-09-27 12:48:32.101;Info NTP time synchronized",
        "2026-09-27 13:02:44.380;Important 1102 Door 'Front door' unlocked",
        // long enough to be cut in the list (⏎ shows it whole)
        "2026-09-27 13:10:00.000;Important 1016 QUITTED, Intercom Entrance - connection lost \
         (Central), admins, IntercomV2 (Entrance,1f0c2a61-0305-11ee-ffff504f94a0) retry \
         scheduled, last seen 13:09:58 at 192.0.2.40",
    ];
    raw.iter().filter_map(|l| LogLine::parse(l)).collect()
}

pub fn sites(t: f64) -> Vec<SiteStatus> {
    vec![
        SiteStatus {
            name: "demo".into(),
            host: "demo.invalid".into(),
            online: true,
            firmware: Some("15.3.4.2".into()),
            cpu: diag(t).cpu,
            heap_pct: Some(36.0),
            latency_ms: Some(12),
            error: None,
        },
        SiteStatus {
            name: "office".into(),
            host: "office.example.invalid".into(),
            online: true,
            firmware: Some("15.3.4.2".into()),
            cpu: Some(wobble(t, 29.0, 8.0, 16.0).round()),
            heap_pct: Some(55.0),
            latency_ms: Some(48),
            error: None,
        },
        SiteStatus {
            name: "cabin".into(),
            host: "10.8.0.12".into(),
            online: false,
            firmware: None,
            cpu: None,
            heap_pct: None,
            latency_ms: None,
            error: Some("connection timed out".into()),
        },
    ]
}

/// 24 h of hourly history for a control's first statistics output, ending at `now`.
pub fn history(name: &str, now_unix: i64, hour_now: f64) -> Series {
    let base = if name.contains("Outside") {
        14.0
    } else if name.contains("Bedroom") {
        19.5
    } else {
        21.5
    };
    let amp = if name.contains("Outside") { 6.0 } else { 0.8 };
    let pts = (0..=96)
        .map(|i| {
            let back = (96 - i) as f64 * 0.25; // hours ago
            let h = hour_now - back;
            let v = base + amp * ((h - 9.0) / 24.0 * std::f64::consts::TAU).sin();
            (now_unix - (back * 3600.0) as i64, (v * 10.0).round() / 10.0)
        })
        .collect();
    Series { points: pts }
}

/// Demo statistics for any window: a daily swing, a slow drift over the
/// year and a little deterministic noise, sampled every 1/400 of the window.
pub fn history_range(name: &str, from: i64, to: i64) -> Series {
    let base = if name.contains("Outside") {
        12.0
    } else if name.contains("Bedroom") {
        19.5
    } else {
        21.5
    };
    let amp = if name.contains("Outside") { 6.0 } else { 0.8 };
    let season = if name.contains("Outside") { 8.0 } else { 1.0 };
    let step = ((to - from) / 400).max(300);
    let tz = chrono::Local::now().offset().local_minus_utc() as i64;
    let points = (0..)
        .map(|i| from + i * step)
        .take_while(|t| *t <= to)
        .map(|t| {
            let hour = ((t + tz).rem_euclid(86_400)) as f64 / 3600.0;
            let day = (t / 86_400) as f64;
            let noise = ((t / step).wrapping_mul(7919) % 11) as f64 / 11.0 - 0.5;
            let v = base
                + amp * ((hour - 9.0) / 24.0 * std::f64::consts::TAU).sin()
                + season * ((day - 20.0) / 365.0 * std::f64::consts::TAU).sin()
                + noise * amp * 0.08;
            (t, (v * 10.0).round() / 10.0)
        })
        .collect();
    Series { points }
}

/// Demo energy history for today: (pv kW, consumption kW) per quarter hour up to `hour_now`.
pub fn energy_today(hour_now: f64) -> (Vec<f64>, Vec<f64>) {
    let n = (hour_now * 4.0).floor() as usize;
    let mut pv = Vec::new();
    let mut use_ = Vec::new();
    for i in 0..96 {
        let h = i as f64 / 4.0;
        let day = ((h - 6.0) / 14.0 * std::f64::consts::PI).sin().max(0.0);
        if i <= n {
            pv.push(day * 7.2 * (0.85 + 0.15 * ((i * 7919) % 13) as f64 / 13.0));
            let mut u = 0.5 + 0.4 * ((i * 104729) % 7) as f64 / 7.0;
            if (26..34).contains(&i) || (70..84).contains(&i) {
                u += 1.9;
            }
            use_.push(u);
        } else {
            // forecast: PV only, drawn faint
            pv.push(day * 7.0);
        }
    }
    (pv, use_)
}

// ── Synthetic .Loxone for the wiring overlay ─────────────────────────────────

/// A small `.Loxone` document matching [`structure`]: blocks carry the control
/// UUIDs, output connectors carry the state UUIDs, plus a few internal blocks
/// without visualization (memory flags, logic) to show dashed wires.
pub fn loxone_xml(st: &Value) -> String {
    let find = |room: &str, name: &str| -> Option<(String, Value)> {
        let rooms = st["rooms"].as_object()?;
        let ru = rooms
            .iter()
            .find(|(_, r)| r["name"] == room)
            .map(|(u, _)| u.clone())?;
        st["controls"]
            .as_object()?
            .iter()
            .find(|(_, c)| c["name"] == name && c["room"] == ru.as_str())
            .map(|(u, c)| (u.clone(), c.clone()))
    };
    let state = |c: &Value, s: &str| c["states"][s].as_str().unwrap_or("").to_string();
    let sub = |c: &Value, name: &str| -> Option<Value> {
        c["subControls"]
            .as_object()?
            .values()
            .find(|s| s["name"] == name)
            .cloned()
    };
    let mut n = 0u32;
    let mut fresh = || {
        n += 1;
        format!("2e{:06x}-0000-0000-ffff{:012x}", n, 0xabc000u64 + n as u64)
    };
    let mut blocks: Vec<String> = Vec::new();
    let co = |k: &str, u: &str, inputs: &[&str]| -> String {
        if inputs.is_empty() {
            format!("\t\t\t\t<Co K=\"{k}\" U=\"{u}\"/>\r\n")
        } else {
            let ins: String = inputs
                .iter()
                .map(|i| format!("\t\t\t\t\t<In Input=\"{i}\"/>\r\n"))
                .collect();
            format!("\t\t\t\t<Co K=\"{k}\" U=\"{u}\">\r\n{ins}\t\t\t\t</Co>\r\n")
        }
    };
    let block = |typ: &str, u: &str, title: &str, cos: &[String]| -> String {
        format!(
            "\t\t\t<C Type=\"{typ}\" V=\"175\" U=\"{u}\" Title=\"{title}\">\r\n{}\t\t\t</C>\r\n",
            cos.concat()
        )
    };

    // Hallway: Motion + Door contact + Night mode memory + Brightness → Hallway light
    let (mu, mc) = find("Hallway", "Motion").unwrap();
    let (du, dc) = find("Hallway", "Door contact").unwrap();
    let (bu, bc) = find("Garden", "Brightness").unwrap();
    let (hu, hc) = find("Hallway", "Hallway light").unwrap();
    let spots = sub(&hc, "Spots").unwrap();
    let night_q = fresh();
    let night = fresh();
    let leds_q = fresh();
    let stairs_q = fresh();
    blocks.push(block(
        "PresenceDetector",
        &mu,
        "Motion",
        &[co("Q", &state(&mc, "active"), &[])],
    ));
    blocks.push(block(
        "InfoOnlyDigital",
        &du,
        "Door contact",
        &[co("Q", &state(&dc, "active"), &[])],
    ));
    blocks.push(block(
        "InfoOnlyAnalog",
        &bu,
        "Brightness",
        &[co("AQ", &state(&bc, "value"), &[])],
    ));
    blocks.push(block(
        "Memory",
        &night,
        "Night mode",
        &[co("Q", &night_q, &[])],
    ));
    let (hmv, htg, hdis, hbr) = (fresh(), fresh(), fresh(), fresh());
    blocks.push(block(
        "LightController2",
        &hu,
        "Hallway light",
        &[
            co("Mv", &hmv, &[&state(&mc, "active")]),
            co("Tg", &htg, &[&state(&dc, "active")]),
            co("DisP", &hdis, &[&night_q]),
            co("Br", &hbr, &[&state(&bc, "value")]),
            co(
                "AQ1",
                spots["states"]["position"].as_str().unwrap_or(""),
                &[],
            ),
            co("AQ2", &leds_q, &[]),
            co("Qp", &stairs_q, &[]),
        ],
    ));
    let aq = |b: &mut Vec<String>, title: &str, src: &str, u1: String, u2: String| {
        b.push(block("AnalogOutput", &u1, title, &[co("I", &u2, &[src])]));
    };
    aq(
        &mut blocks,
        "AQ Hallway spots",
        spots["states"]["position"].as_str().unwrap_or(""),
        fresh(),
        fresh(),
    );
    aq(&mut blocks, "AQ Hallway LEDs", &leds_q, fresh(), fresh());
    aq(&mut blocks, "Q Stairs light", &stairs_q, fresh(), fresh());

    // Living room lighting: Presence → Lighting → three circuits
    let (pu, pc) = find("Living room", "Presence").unwrap();
    let (lu, lc) = find("Living room", "Lighting").unwrap();
    blocks.push(block(
        "PresenceDetector",
        &pu,
        "Presence",
        &[co("Q", &state(&pc, "active"), &[])],
    ));
    let mut cos = vec![co("Mv", &fresh(), &[&state(&pc, "active")])];
    for (i, name) in ["Ceiling light", "Reading lamp", "Floor LEDs"]
        .iter()
        .enumerate()
    {
        let s = sub(&lc, name).unwrap();
        let out = s["states"]["position"]
            .as_str()
            .or(s["states"]["color"].as_str())
            .unwrap_or("")
            .to_string();
        cos.push(co(&format!("AQ{}", i + 1), &out, &[]));
        blocks.push(block(
            "AnalogOutput",
            &fresh(),
            &format!("AQ {}", name),
            &[co("I", &fresh(), &[&out])],
        ));
    }
    blocks.push(block("LightController2", &lu, "Lighting", &cos));

    // Blind South: shading automatic (internal) + brightness → Jalousie → motor
    let (su, sc) = find("Living room", "Blind South").unwrap();
    let auto = fresh();
    let auto_q = fresh();
    blocks.push(block(
        "AutoJalousie",
        &auto,
        "Shading automatic South",
        &[co("Qs", &auto_q, &[&state(&bc, "value")])],
    ));
    blocks.push(block(
        "Jalousie",
        &su,
        "Blind South",
        &[
            co("As", &fresh(), &[&auto_q]),
            co("OutputPos", &state(&sc, "position"), &[]),
            co("OutputLPos", &state(&sc, "shadePosition"), &[]),
            co("Qu", &state(&sc, "up"), &[]),
            co("Qd", &state(&sc, "down"), &[]),
        ],
    ));
    blocks.push(block(
        "DigitalOutput",
        &fresh(),
        "Motor South up",
        &[co("I", &fresh(), &[&state(&sc, "up")])],
    ));
    blocks.push(block(
        "DigitalOutput",
        &fresh(),
        "Motor South down",
        &[co("I", &fresh(), &[&state(&sc, "down")])],
    ));

    // Climate living room: Humidity + Window → IRC
    let (rcu, rcc) = find("Living room", "Room climate").unwrap();
    let (wu, wc) = find("Living room", "Window left").unwrap();
    blocks.push(block(
        "InfoOnlyDigital",
        &wu,
        "Window left",
        &[co("Q", &state(&wc, "active"), &[])],
    ));
    let valve = fresh();
    blocks.push(block(
        "IRoomControllerV2",
        &rcu,
        "Room climate",
        &[
            co("Wo", &fresh(), &[&state(&wc, "active")]),
            co("AQt", &state(&rcc, "tempActual"), &[]),
            co("AQh", &valve, &[]),
        ],
    ));
    blocks.push(block(
        "AnalogOutput",
        &fresh(),
        "Valve Living room",
        &[co("I", &fresh(), &[&valve])],
    ));

    // Everything else: a block with its state outputs; switchable things get a
    // wall button (Touch / T5) wired to their input, like a typical install.
    let done: std::collections::HashSet<String> = blocks
        .iter()
        .filter_map(|b| {
            b.split("U=\"")
                .nth(1)
                .and_then(|r| r.split('"').next())
                .map(String::from)
        })
        .collect();
    let rooms_by_uuid: HashMap<String, String> = st["rooms"]
        .as_object()
        .map(|o| {
            o.iter()
                .map(|(k, v)| (k.clone(), v["name"].as_str().unwrap_or("").to_string()))
                .collect()
        })
        .unwrap_or_default();
    let mut ctrls: Vec<(String, Value)> = st["controls"]
        .as_object()
        .map(|o| o.iter().map(|(u, c)| (u.clone(), c.clone())).collect())
        .unwrap_or_default();
    ctrls.sort_by(|a, b| a.0.cmp(&b.0));
    for (u, c) in ctrls {
        if done.contains(&u) {
            continue;
        }
        let typ = c["type"].as_str().unwrap_or("");
        let name = c["name"].as_str().unwrap_or("");
        let room = c["room"]
            .as_str()
            .and_then(|r| rooms_by_uuid.get(r))
            .cloned()
            .unwrap_or_default();
        let mut outs: Vec<(String, String)> = c["states"]
            .as_object()
            .map(|o| {
                o.iter()
                    .filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string())))
                    .collect()
            })
            .unwrap_or_default();
        outs.sort();
        let input = match typ {
            "LightControllerV2" | "Dimmer" | "Switch" => Some("Tg"),
            "Jalousie" => Some("Up"),
            "Gate" => Some("Tg"),
            _ => None,
        };
        let mut cos = Vec::new();
        if let Some(k) = input {
            let btn_q = fresh();
            blocks.push(block(
                "Pushbutton",
                &fresh(),
                &format!("Button {}", room),
                &[co("Q", &btn_q, &[])],
            ));
            cos.push(co(k, &fresh(), &[&btn_q]));
        }
        for (k, su) in outs.iter().take(6) {
            cos.push(co(k, su, &[]));
        }
        blocks.push(block(typ, &u, name, &cos));
        // sub-controls (light circuits) hang off the controller's outputs
        if let Some(subs) = c["subControls"].as_object() {
            for (su_u, sc) in subs {
                let so: Vec<String> = sc["states"]
                    .as_object()
                    .map(|o| {
                        o.values()
                            .filter_map(|v| v.as_str().map(|s| co("AQ", s, &[])))
                            .take(2)
                            .collect()
                    })
                    .unwrap_or_default();
                blocks.push(block(
                    sc["type"].as_str().unwrap_or(""),
                    su_u,
                    sc["name"].as_str().unwrap_or(""),
                    &so,
                ));
            }
        }
    }
    let _ = sub;

    format!(
        "\u{feff}<?xml version=\"1.0\" encoding=\"utf-8\"?>\r\n<ControlList Version=\"1\" NextObj=\"900\" NextConst=\"1\" NextNote=\"1\" NextMem=\"1\">\r\n\
\t<C Type=\"Document\" V=\"175\" U=\"2e000000-0000-0000-ffff000000000001\" Title=\"Demo House\" ConfigVersion=\"15030402\">\r\n\
\t\t<C Type=\"Page\" V=\"175\" U=\"2e000000-0000-0000-ffff000000000002\" Title=\"Main\">\r\n{}\t\t</C>\r\n\t</C>\r\n</ControlList>\r\n",
        blocks.concat()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sim_blind_moves_and_stops() {
        let st = structure();
        let mut sim = Sim::new(&st);
        let h = crate::tui::model::House::from_structure(&st);
        let c = h.resolve("Blind South", Some("Living room")).unwrap();
        let u = h.ctrls[c].uuid.clone();
        sim.command(&u, "FullDown").unwrap();
        let mut t = 0.0;
        let pos_u = h.ctrls[c].state("position").unwrap().to_string();
        let mut last = 0.33;
        for _ in 0..200 {
            t += 0.5;
            for e in sim.tick(t, 12.0) {
                if let StateEvent::ValueState { uuid, value } = e
                    && uuid == pos_u
                {
                    assert!(value >= last);
                    last = value;
                }
            }
        }
        assert!((last - 1.0).abs() < 1e-9);
        assert!(sim.command(&u, "bogus").is_err());
    }

    #[test]
    fn sim_moods_drive_dimmers() {
        let st = structure();
        let mut sim = Sim::new(&st);
        let h = crate::tui::model::House::from_structure(&st);
        let c = h.resolve("Lighting", Some("Bedroom")).unwrap();
        sim.command(&h.ctrls[c].uuid, "on").unwrap();
        let ev = sim.tick(0.1, 12.0);
        let _ = ev;
        let sub = h.ctrls[c].subs[0];
        let pu = h.ctrls[sub].state("position").unwrap();
        assert_eq!(sim.values[pu], 100.0);
    }

    #[test]
    fn wiring_fixture_parses() {
        let st = structure();
        let xml = loxone_xml(&st);
        let doc = lxir::LoxoneDoc::parse(xml.as_bytes()).unwrap();
        assert!(doc.objects().len() > 15);
        assert!(doc.wires().len() > 10);
    }
}
