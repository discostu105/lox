//! The house model: `LoxApp3.json` turned into rooms, categories and typed controls.
//!
//! Built once per structure load; everything else refers to controls by index (`Cid`)
//! and to live values by state UUID (see `store.rs`).

use std::collections::{BTreeMap, HashMap};

use serde_json::Value;

use crate::actions::{self, RiskTarget};

use super::text::{clean, natural_key};

/// Index into `House::ctrls`.
pub type Cid = usize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    LightCtl,
    CentralLight,
    Dimmer,
    ColorPicker,
    Switch,
    Pushbutton,
    Blind,
    CentralBlind,
    Gate,
    Climate,
    Alarm,
    Smoke,
    DoorLock,
    Intercom,
    Charger,
    Meter,
    Efm,
    Analog,
    Digital,
    Presence,
    Daytimer,
    Text,
    Slider,
    Other,
}

impl Kind {
    pub fn from_type(t: &str) -> Kind {
        match t {
            "LightControllerV2" | "LightController" => Kind::LightCtl,
            "CentralLightController" => Kind::CentralLight,
            "Dimmer" | "EIBDimmer" => Kind::Dimmer,
            "ColorPickerV2" | "ColorPicker" => Kind::ColorPicker,
            "Switch" => Kind::Switch,
            "Pushbutton" => Kind::Pushbutton,
            "Jalousie" => Kind::Blind,
            "CentralJalousie" => Kind::CentralBlind,
            "Gate" | "CentralGate" => Kind::Gate,
            "IRoomControllerV2" | "IRoomController" => Kind::Climate,
            "Alarm" => Kind::Alarm,
            "SmokeAlarm" => Kind::Smoke,
            "Meter" => Kind::Meter,
            "EFM" | "EnergyManager2" => Kind::Efm,
            "InfoOnlyAnalog" => Kind::Analog,
            "InfoOnlyDigital" => Kind::Digital,
            "PresenceDetector" => Kind::Presence,
            "Daytimer" | "IRCV2Daytimer" => Kind::Daytimer,
            "TextState" | "InfoOnlyText" => Kind::Text,
            "Slider" | "UpDownDigital" | "ValueSelector" | "UpDownAnalog" => Kind::Slider,
            t if t.contains("Intercom") => Kind::Intercom,
            t if t.contains("DoorLock") => Kind::DoorLock,
            t if t.contains("Charger") || t.contains("Wallbox") => Kind::Charger,
            _ => Kind::Other,
        }
    }

    /// Default glyph (§6.2).
    pub fn glyph(self) -> &'static str {
        match self {
            Kind::LightCtl | Kind::CentralLight | Kind::Dimmer | Kind::ColorPicker => "●",
            Kind::Switch => "◆",
            Kind::Pushbutton => "◇",
            Kind::Blind | Kind::CentralBlind => "▾",
            Kind::Gate => "⌂",
            Kind::Climate => "°",
            Kind::Alarm | Kind::Smoke => "⚠",
            Kind::DoorLock => "⚿",
            Kind::Intercom => "☏",
            Kind::Charger => "⏚",
            Kind::Meter | Kind::Efm => "≋",
            Kind::Analog => "°",
            Kind::Digital => "◫",
            Kind::Presence => "◉",
            Kind::Daytimer => "◷",
            Kind::Text => "¶",
            Kind::Slider => "↕",
            Kind::Other => "·",
        }
    }

    /// Nerd Font glyph (opt-in, §6.2).
    pub fn nerd_glyph(self) -> &'static str {
        match self {
            Kind::LightCtl | Kind::CentralLight | Kind::Dimmer | Kind::ColorPicker => "󰌵",
            Kind::Switch | Kind::Pushbutton => "󰔡",
            Kind::Blind | Kind::CentralBlind => "󰷛",
            Kind::Gate => "󰠚",
            Kind::Climate | Kind::Analog => "󰔏",
            Kind::Alarm | Kind::Smoke => "󰀦",
            Kind::DoorLock => "󰌾",
            Kind::Intercom => "󰋑",
            Kind::Charger => "󰄌",
            Kind::Meter | Kind::Efm => "󰚥",
            Kind::Digital => "󰖶",
            Kind::Presence => "󰋑",
            Kind::Daytimer => "󰥔",
            Kind::Text => "󰦨",
            Kind::Slider => "󰘮",
            Kind::Other => "·",
        }
    }

    /// Controls that act on something (vs. pure sensors/meters).
    pub fn is_actuator(self) -> bool {
        !matches!(
            self,
            Kind::Meter
                | Kind::Efm
                | Kind::Analog
                | Kind::Digital
                | Kind::Presence
                | Kind::Text
                | Kind::Other
                | Kind::Smoke
        )
    }
}

#[derive(Debug, Clone)]
pub struct Ctrl {
    pub uuid: String,
    pub name: String,
    pub typ: String,
    pub kind: Kind,
    pub room: Option<usize>,
    pub cat: Option<usize>,
    /// state name → state UUID
    pub states: BTreeMap<String, String>,
    pub details: Value,
    pub is_favorite: bool,
    pub is_secured: bool,
    pub parent: Option<Cid>,
    pub subs: Vec<Cid>,
    /// Miniserver statistics (`statistic` or `statisticV2`) are recorded for this control
    pub has_stats: bool,
    /// Statistics V2 series to plot: (group id, output)
    pub stat_v2: Option<(String, String)>,
    /// Number of statistics outputs (values per record in the stats file)
    pub stat_outputs: usize,
    /// The first statistic output is power (kW), not an energy counter
    pub stat_power: bool,
    /// Number format from details (`format` or `actualFormat`)
    pub format: Option<String>,
    /// `defaultIcon`, e.g. `IconsFilled/login-key.svg`
    pub icon: Option<String>,
    /// On the user's `confirm:` list (set by [`House::apply_confirm_list`])
    pub listed: bool,
}

impl Ctrl {
    pub fn state(&self, name: &str) -> Option<&str> {
        self.states.get(name).map(|s| s.as_str())
    }
    /// A °-temperature sensor (InfoOnlyAnalog with a degree format).
    pub fn is_temperature(&self) -> bool {
        self.kind == Kind::Analog && self.format.as_deref().is_some_and(|f| f.contains('°'))
    }
    pub fn meter_type(&self) -> &str {
        self.details
            .get("type")
            .and_then(|t| t.as_str())
            .unwrap_or("")
    }
}

#[derive(Debug, Clone)]
pub struct Room {
    pub name: String,
    /// Top-level controls in this room
    pub ctrls: Vec<Cid>,
    /// Where the room temperature comes from: (control, state uuid)
    pub temp: Option<(Cid, String)>,
    /// Target temperature (from a room controller), if any
    pub target: Option<(Cid, String)>,
}

#[derive(Debug, Clone)]
pub struct Cat {
    pub name: String,
    /// `image`, e.g. `IconsFilled/door-open.svg` (names are localized, icons are not)
    pub image: Option<String>,
}

/// Energy roles of an EFM node (from `details.nodes[].nodeType`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Grid,
    Production,
    Storage,
    Load,
}

#[derive(Debug, Clone)]
pub struct EnergyNode {
    pub role: Role,
    /// The Meter behind this node
    pub ctrl: Option<Cid>,
    /// The EFM state carrying this node's power (kW)
    pub power_state: Option<String>,
}

impl EnergyNode {
    /// State UUID for the node's power: the EFM state, else the meter's `actual`.
    pub fn power_uuid<'a>(&'a self, house: &'a House) -> Option<&'a str> {
        self.power_state
            .as_deref()
            .or_else(|| self.ctrl.and_then(|c| house.ctrls[c].state("actual")))
    }
}

#[derive(Debug, Clone)]
pub struct Energy {
    /// The EFM control, if the installation has one
    pub efm: Option<Cid>,
    pub nodes: Vec<EnergyNode>,
    /// All top-level Meter controls (for the table)
    pub meters: Vec<Cid>,
}

impl Energy {
    pub fn by_role(&self, role: Role) -> impl Iterator<Item = &EnergyNode> {
        self.nodes.iter().filter(move |n| n.role == role)
    }
}

#[derive(Debug, Clone)]
pub struct House {
    pub ctrls: Vec<Ctrl>,
    pub by_uuid: HashMap<String, Cid>,
    pub rooms: Vec<Room>,
    pub cats: Vec<Cat>,
    /// state UUID → (owner control, state name)
    pub state_owner: HashMap<String, (Cid, String)>,
    /// global state name → UUID
    pub globals: BTreeMap<String, String>,
    /// operating mode id → name
    pub op_modes: BTreeMap<i64, String>,
    pub energy: Energy,
    /// Controls not assigned to any room
    pub unassigned: Vec<Cid>,
    pub last_modified: String,
    /// `msInfo.miniserverType` as text ("Miniserver Gen 2")
    pub ms_type: String,
    pub serial: String,
}

fn s(v: &Value, k: &str) -> String {
    v.get(k)
        .and_then(|x| x.as_str())
        .map(clean)
        .unwrap_or_default()
}

impl House {
    pub fn from_structure(st: &Value) -> House {
        let mut rooms: Vec<Room> = Vec::new();
        let mut room_idx: HashMap<String, usize> = HashMap::new();
        if let Some(obj) = st.get("rooms").and_then(|r| r.as_object()) {
            let mut list: Vec<(&String, &Value)> = obj.iter().collect();
            list.sort_by_key(|(_, r)| natural_key(&s(r, "name")));
            for (uuid, r) in list {
                room_idx.insert(uuid.clone(), rooms.len());
                rooms.push(Room {
                    name: s(r, "name"),
                    ctrls: Vec::new(),
                    temp: None,
                    target: None,
                });
            }
        }
        let mut cats: Vec<Cat> = Vec::new();
        let mut cat_idx: HashMap<String, usize> = HashMap::new();
        if let Some(obj) = st.get("cats").and_then(|r| r.as_object()) {
            let mut list: Vec<(&String, &Value)> = obj.iter().collect();
            list.sort_by_key(|(_, r)| natural_key(&s(r, "name")));
            for (uuid, c) in list {
                cat_idx.insert(uuid.clone(), cats.len());
                cats.push(Cat {
                    name: s(c, "name"),
                    image: c.get("image").and_then(|i| i.as_str()).map(String::from),
                });
            }
        }

        let mut house = House {
            ctrls: Vec::new(),
            by_uuid: HashMap::new(),
            rooms,
            cats,
            state_owner: HashMap::new(),
            globals: BTreeMap::new(),
            op_modes: BTreeMap::new(),
            energy: Energy {
                efm: None,
                nodes: Vec::new(),
                meters: Vec::new(),
            },
            unassigned: Vec::new(),
            last_modified: s(st, "lastModified"),
            ms_type: match st
                .pointer("/msInfo/miniserverType")
                .and_then(|v| v.as_i64())
            {
                Some(0) => "Miniserver Gen 1".into(),
                Some(1) => "Miniserver Go".into(),
                Some(2) => "Miniserver Gen 2".into(),
                Some(3) => "Miniserver Go Gen 2".into(),
                Some(4) => "Miniserver Compact".into(),
                Some(n) => format!("Miniserver (type {})", n),
                None => String::new(),
            },
            serial: st
                .pointer("/msInfo/serialNr")
                .and_then(|v| v.as_str())
                .map(clean)
                .unwrap_or_default(),
        };

        if let Some(obj) = st.get("controls").and_then(|c| c.as_object()) {
            let mut list: Vec<(&String, &Value)> = obj.iter().collect();
            list.sort_by_key(|(_, c)| natural_key(&s(c, "name")));
            for (uuid, c) in list {
                house.add_ctrl(uuid, c, None, &room_idx, &cat_idx);
            }
        }
        // room membership (top-level only), sorted by category then name
        for cid in 0..house.ctrls.len() {
            if house.ctrls[cid].parent.is_some() {
                continue;
            }
            match house.ctrls[cid].room {
                Some(r) => house.rooms[r].ctrls.push(cid),
                None => house.unassigned.push(cid),
            }
        }
        let cat_name = |h: &House, c: Cid| {
            h.ctrls[c]
                .cat
                .map(|i| natural_key(&h.cats[i].name))
                .unwrap_or_default()
        };
        for r in 0..house.rooms.len() {
            let mut list = std::mem::take(&mut house.rooms[r].ctrls);
            list.sort_by_cached_key(|c| (cat_name(&house, *c), natural_key(&house.ctrls[*c].name)));
            house.rooms[r].ctrls = list;
        }
        // room temperature: room controller first, then a ° sensor (prefer one with statistics)
        for r in 0..house.rooms.len() {
            let ctrls = house.rooms[r].ctrls.clone();
            let climate = ctrls
                .iter()
                .find(|c| house.ctrls[**c].kind == Kind::Climate)
                .copied();
            if let Some(c) = climate {
                if let Some(u) = house.ctrls[c].state("tempActual") {
                    house.rooms[r].temp = Some((c, u.to_string()));
                }
                if let Some(u) = house.ctrls[c].state("tempTarget") {
                    house.rooms[r].target = Some((c, u.to_string()));
                }
            }
            if house.rooms[r].temp.is_none() {
                let mut sensors: Vec<Cid> = ctrls
                    .iter()
                    .copied()
                    .filter(|c| house.ctrls[*c].is_temperature())
                    .collect();
                sensors.sort_by_key(|c| (!house.ctrls[*c].has_stats, house.ctrls[*c].name.len()));
                if let Some(c) = sensors.first()
                    && let Some(u) = house.ctrls[*c].state("value")
                {
                    house.rooms[r].temp = Some((*c, u.to_string()));
                }
            }
        }
        // globals
        if let Some(obj) = st.get("globalStates").and_then(|g| g.as_object()) {
            for (k, v) in obj {
                if let Some(u) = v.as_str() {
                    house.globals.insert(k.clone(), u.to_string());
                }
            }
        }
        if let Some(obj) = st.get("operatingModes").and_then(|g| g.as_object()) {
            for (k, v) in obj {
                if let (Ok(id), Some(n)) = (k.parse::<i64>(), v.as_str()) {
                    house.op_modes.insert(id, clean(n));
                }
            }
        }
        // energy
        house.energy.meters = (0..house.ctrls.len())
            .filter(|c| house.ctrls[*c].kind == Kind::Meter && house.ctrls[*c].parent.is_none())
            .collect();
        house.energy.efm = (0..house.ctrls.len())
            .find(|c| house.ctrls[*c].kind == Kind::Efm && house.ctrls[*c].parent.is_none());
        if let Some(e) = house.energy.efm
            && let Some(nodes) = house.ctrls[e]
                .details
                .get("nodes")
                .and_then(|n| n.as_array())
        {
            let nodes = nodes.clone();
            house.energy.nodes = nodes.iter().filter_map(|n| house.energy_node(n)).collect();
        }
        if house.energy.nodes.is_empty() {
            house.energy.nodes = house.heuristic_energy_nodes();
        }
        house
    }

    fn add_ctrl(
        &mut self,
        uuid: &str,
        c: &Value,
        parent: Option<Cid>,
        room_idx: &HashMap<String, usize>,
        cat_idx: &HashMap<String, usize>,
    ) -> Cid {
        let typ = s(c, "type");
        let kind = Kind::from_type(&typ);
        let mut states = BTreeMap::new();
        if let Some(obj) = c.get("states").and_then(|s| s.as_object()) {
            for (k, v) in obj {
                if let Some(u) = v.as_str() {
                    states.insert(k.clone(), u.to_string());
                }
            }
        }
        let details = c.get("details").cloned().unwrap_or(Value::Null);
        let format = details
            .get("format")
            .or_else(|| details.get("actualFormat"))
            .and_then(|f| f.as_str())
            .map(|f| f.to_string());
        let room = c
            .get("room")
            .and_then(|r| r.as_str())
            .and_then(|r| room_idx.get(r).copied())
            .or_else(|| parent.and_then(|p| self.ctrls[p].room));
        let cat = c
            .get("cat")
            .and_then(|r| r.as_str())
            .and_then(|r| cat_idx.get(r).copied())
            .or_else(|| parent.and_then(|p| self.ctrls[p].cat));
        let mut name = s(c, "name");
        if name.is_empty() {
            name = typ.clone();
        }
        let cid = self.ctrls.len();
        let stat_v2 = crate::statv2::plot_point(c);
        for (sname, su) in &states {
            self.state_owner.insert(su.clone(), (cid, sname.clone()));
        }
        self.ctrls.push(Ctrl {
            uuid: uuid.to_string(),
            name,
            typ,
            kind,
            room,
            cat,
            states,
            details,
            is_favorite: c
                .get("isFavorite")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            is_secured: c
                .get("isSecured")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            parent,
            subs: Vec::new(),
            has_stats: c.get("statistic").is_some() || stat_v2.is_some(),
            stat_v2: stat_v2.clone(),
            stat_outputs: c
                .pointer("/statistic/outputs")
                .map(|o| {
                    o.as_array()
                        .map(|a| a.len())
                        .or_else(|| o.as_object().map(|m| m.len()))
                        .unwrap_or(1)
                })
                .unwrap_or(0),
            stat_power: c
                .pointer("/statistic/outputs/0/name")
                .and_then(|n| n.as_str())
                .is_some_and(|n| {
                    let n = n.to_lowercase();
                    n.contains("actual")
                        || n.contains("pwr")
                        || n.contains("power")
                        || n.contains("leistung")
                })
                // a meter's `actual` output is its power
                || stat_v2.as_ref().is_some_and(|(_, o)| o == "actual"),
            format,
            icon: c
                .get("defaultIcon")
                .and_then(|i| i.as_str())
                .map(String::from),
            listed: false,
        });
        self.by_uuid.insert(uuid.to_string(), cid);
        if let Some(subs) = c.get("subControls").and_then(|s| s.as_object()) {
            let mut list: Vec<(&String, &Value)> = subs.iter().collect();
            list.sort_by_key(|(_, c)| natural_key(&s(c, "name")));
            for (su, sc) in list {
                let sid = self.add_ctrl(su, sc, Some(cid), room_idx, cat_idx);
                self.ctrls[cid].subs.push(sid);
            }
        }
        cid
    }

    fn energy_node(&self, n: &Value) -> Option<EnergyNode> {
        let role = match n.get("nodeType").and_then(|t| t.as_str())? {
            "Grid" => Role::Grid,
            "Production" => Role::Production,
            "Storage" => Role::Storage,
            "Load" => Role::Load,
            _ => return None,
        };
        let ctrl = n
            .get("ctrlUuid")
            .and_then(|u| u.as_str())
            .and_then(|u| self.by_uuid.get(u).copied());
        Some(EnergyNode {
            role,
            ctrl,
            power_state: n
                .get("actualEfmState")
                .and_then(|u| u.as_str())
                .map(|u| u.to_string()),
        })
    }

    /// Without an EFM: guess roles from meter types and names (overridable in tui.yaml).
    fn heuristic_energy_nodes(&self) -> Vec<EnergyNode> {
        let mut out = Vec::new();
        for &m in &self.energy.meters {
            let c = &self.ctrls[m];
            let lname = c.name.to_lowercase();
            let role = if c.meter_type() == "storage" {
                Role::Storage
            } else if lname.contains("pv") || lname.contains("solar") || lname.contains("photovolt")
            {
                Role::Production
            } else if c.meter_type() == "bidirectional"
                && (lname.contains("grid") || lname.contains("netz") || lname.contains("bezug"))
            {
                Role::Grid
            } else {
                continue;
            };
            out.push(EnergyNode {
                role,
                ctrl: Some(m),
                power_state: None,
            });
        }
        out
    }

    /// Mark the controls on the `confirm:` list of the config; a listed control's
    /// sub-controls (e.g. the circuits of a lighting controller) count as listed.
    pub fn apply_confirm_list(&mut self, confirm: &[String], aliases: &HashMap<String, String>) {
        for cid in 0..self.ctrls.len() {
            let c = &self.ctrls[cid];
            let room = c.room.map(|r| self.rooms[r].name.as_str());
            let listed = actions::on_confirm_list(confirm, aliases, &c.uuid, &c.name, room)
                || c.parent.is_some_and(|p| self.ctrls[p].listed);
            self.ctrls[cid].listed = listed;
        }
    }

    /// The risk rules' view of a control (icons, type, `confirm:` list).
    pub fn risk_target(&self, cid: Cid) -> RiskTarget<'_> {
        let c = &self.ctrls[cid];
        RiskTarget {
            typ: &c.typ,
            icon: c.icon.as_deref(),
            cat_icon: c.cat.and_then(|k| self.cats[k].image.as_deref()),
            is_secured: c.is_secured,
            listed: c.listed,
        }
    }

    /// Apply role overrides from `tui.yaml`: meter name/UUID → role.
    pub fn apply_role_overrides(&mut self, overrides: &BTreeMap<String, String>) {
        for (key, role) in overrides {
            let role = match role.to_lowercase().as_str() {
                "grid" => Role::Grid,
                "pv" | "production" | "solar" => Role::Production,
                "battery" | "storage" => Role::Storage,
                "load" | "home" | "consumption" => Role::Load,
                _ => continue,
            };
            let Some(cid) = self.by_uuid.get(key).copied().or_else(|| {
                self.energy
                    .meters
                    .iter()
                    .copied()
                    .find(|c| self.ctrls[*c].name.eq_ignore_ascii_case(key))
            }) else {
                continue;
            };
            self.energy.nodes.retain(|n| n.ctrl != Some(cid));
            self.energy.nodes.push(EnergyNode {
                role,
                ctrl: Some(cid),
                power_state: None,
            });
        }
    }

    pub fn room_name(&self, c: Cid) -> Option<&str> {
        self.ctrls[c].room.map(|r| self.rooms[r].name.as_str())
    }

    pub fn cat_name(&self, c: Cid) -> Option<&str> {
        self.ctrls[c].cat.map(|r| self.cats[r].name.as_str())
    }

    /// Top-level ancestor of a (sub-)control.
    pub fn top(&self, mut c: Cid) -> Cid {
        while let Some(p) = self.ctrls[c].parent {
            c = p;
        }
        c
    }

    /// Display name: "Parent / Sub" for sub-controls.
    pub fn display_name(&self, c: Cid) -> String {
        match self.ctrls[c].parent {
            Some(p) => format!("{} / {}", self.ctrls[p].name, self.ctrls[c].name),
            None => self.ctrls[c].name.clone(),
        }
    }

    /// Top-level controls (what lists show).
    pub fn top_level(&self) -> impl Iterator<Item = Cid> + '_ {
        (0..self.ctrls.len()).filter(|c| self.ctrls[*c].parent.is_none())
    }

    /// Resolve a control the way the CLI does: exact UUID → "Name [Room]" → room
    /// filter → exact name → substring. Ambiguity is an error.
    pub fn resolve(&self, name: &str, room: Option<&str>) -> Result<Cid, String> {
        if let Some(c) = self.by_uuid.get(name) {
            return Ok(*c);
        }
        let (name, room) = match (name.rfind(" ["), name.ends_with(']')) {
            (Some(i), true) if room.is_none() => (&name[..i], Some(&name[i + 2..name.len() - 1])),
            _ => (name, room),
        };
        let lname = name.to_lowercase();
        let room_ok = |c: Cid| match room {
            None => true,
            Some(r) => self
                .room_name(c)
                .is_some_and(|n| n.to_lowercase().contains(&r.to_lowercase())),
        };
        let cands: Vec<Cid> = self.top_level().filter(|c| room_ok(*c)).collect();
        let exact: Vec<Cid> = cands
            .iter()
            .copied()
            .filter(|c| self.ctrls[*c].name.to_lowercase() == lname)
            .collect();
        let found = if !exact.is_empty() {
            exact
        } else {
            cands
                .into_iter()
                .filter(|c| self.ctrls[*c].name.to_lowercase().contains(&lname))
                .collect()
        };
        match found.len() {
            0 => Err(format!("No control matching '{}'", name)),
            1 => Ok(found[0]),
            n => Err(format!(
                "'{}' is ambiguous ({} matches) — add -r <room>: {}",
                name,
                n,
                found
                    .iter()
                    .take(4)
                    .map(|c| format!(
                        "{} [{}]",
                        self.ctrls[*c].name,
                        self.room_name(*c).unwrap_or("—")
                    ))
                    .collect::<Vec<_>>()
                    .join(", ")
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::demo;

    #[test]
    fn demo_house_builds() {
        let h = House::from_structure(&demo::structure());
        assert!(h.rooms.len() >= 8);
        assert!(h.ctrls.len() >= 40);
        // every state maps back to its control
        for (cid, c) in h.ctrls.iter().enumerate() {
            for u in c.states.values() {
                assert_eq!(h.state_owner[u].0, cid);
            }
        }
        // EFM roles come from the nodes
        assert!(h.energy.efm.is_some());
        assert_eq!(h.energy.by_role(Role::Grid).count(), 1);
        assert_eq!(h.energy.by_role(Role::Production).count(), 1);
        assert_eq!(h.energy.by_role(Role::Storage).count(), 1);
        // room temperatures resolved from controller or sensor
        let living = h.rooms.iter().find(|r| r.name == "Living room").unwrap();
        assert!(living.temp.is_some());
        assert!(living.target.is_some());
        let office = h.rooms.iter().find(|r| r.name == "Office").unwrap();
        assert!(office.temp.is_some());
    }

    #[test]
    fn resolve_like_cli() {
        let h = House::from_structure(&demo::structure());
        let c = h.resolve("Blind South", Some("Living")).unwrap();
        assert_eq!(h.ctrls[c].name, "Blind South");
        assert!(h.resolve("Ceiling", None).is_err()); // ambiguous
        let c2 = h.resolve("Ceiling [Kitchen]", None).unwrap();
        assert_eq!(h.room_name(c2), Some("Kitchen"));
        assert!(h.resolve("nope", None).is_err());
        let uuid = h.ctrls[c].uuid.clone();
        assert_eq!(h.resolve(&uuid, None).unwrap(), c);
    }

    #[test]
    fn floors_sort_naturally() {
        let st = serde_json::json!({
            "rooms": {
                "a": {"name": "OG Bath"}, "b": {"name": "EG Office"},
                "c": {"name": "Room 10"}, "d": {"name": "Room 2"}
            },
            "controls": {}
        });
        let h = House::from_structure(&st);
        let names: Vec<_> = h.rooms.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["EG Office", "OG Bath", "Room 2", "Room 10"]);
    }

    /// Meters of the energy flow monitor only have `statisticV2` (real data).
    #[test]
    fn statistic_v2_meter_has_power_history() {
        let st = serde_json::json!({
            "rooms": {},
            "controls": {
                "m1": {"name": "PV-Strom", "type": "Meter",
                    "states": {"actual": "s1", "totalDay": "s2"},
                    "statisticV2": {"groups": [
                        {"id": "1", "mode": 12, "dataPoints": [{"title": "Leistung", "output": "actual"}]},
                        {"id": "2", "mode": 11, "accumulated": true,
                         "dataPoints": [{"title": "Zählerstand", "output": "total"}]}]}},
                "m2": {"name": "Plain", "type": "Meter", "states": {"actual": "s3"}}
            }
        });
        let h = House::from_structure(&st);
        let pv = &h.ctrls[h.by_uuid["m1"]];
        assert!(pv.has_stats && pv.stat_power);
        assert_eq!(pv.stat_v2, Some(("1".into(), "actual".into())));
        let plain = &h.ctrls[h.by_uuid["m2"]];
        assert!(!plain.has_stats && !plain.stat_power && plain.stat_v2.is_none());
    }
}

#[cfg(test)]
mod real_structure {
    /// Smoke test against a real `LoxApp3.json` (never committed):
    /// `LOX_TUI_STRUCTURE=~/.lox/cache/structure.json cargo test real_structure -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn real_structure_smoke() {
        let Ok(p) = std::env::var("LOX_TUI_STRUCTURE") else {
            return;
        };
        let st: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap();
        let h = super::House::from_structure(&st);
        let with_temp = h.rooms.iter().filter(|r| r.temp.is_some()).count();
        println!(
            "controls {} rooms {} rooms-with-temp {}",
            h.ctrls.len(),
            h.rooms.len(),
            with_temp
        );
        println!(
            "energy nodes {} meters {} efm {:?}",
            h.energy.nodes.len(),
            h.energy.meters.len(),
            h.energy.efm.is_some()
        );
        for n in &h.energy.nodes {
            println!(
                "  {:?} ctrl={:?} power={:?} stat_power={:?}",
                n.role,
                n.ctrl.map(|c| &h.ctrls[c].name),
                n.power_uuid(&h).is_some(),
                n.ctrl.map(|c| h.ctrls[c].stat_power)
            );
        }
    }
}
