//! Network Periphery plugins (`<C Type="Plugin" gid="…">`): MCP Server,
//! Matter and the other add-ons Loxone Config installs on a Miniserver.
//! A declaration takes over an existing plugin by `uuid:` or creates one
//! by `gid:` ([`crate::logic`]). Its settings are the package's
//! parameters, held in the element's `<SET>` as `<id t="type" v="value"/>`.
//!
//! The table below is each package's manifest (`<gid>.json` inside the
//! `.LxAddOn` Loxone Config downloads) as of the versions named, with
//! `maxCountPerMiniserver` from the add-on catalogue (`addons.json`,
//! 2026-11-12). A plugin a newer package adds is still declared by
//! `uuid:`; its parameters are then the ones its `<SET>` holds.

use crate::logic::Value;
use crate::xml::{Element, XmlDocument};

/// One parameter of a package.
#[derive(Debug)]
pub struct Param {
    /// The parameter id, the element name inside `<SET>`.
    pub id: &'static str,
    /// The manifest's type, written as `t=`: 1 a box, 7 a number, 8 a
    /// choice (its value), 11 text, 12 a choice of keys, 15 a password,
    /// 16 a time of day.
    pub t: u32,
    /// `v=` on a new plugin: the manifest default as a save writes it
    /// (`true` for a ticked box; nothing for an unticked one or empty
    /// text).
    pub default: Option<&'static str>,
    /// Shown in the plugin's properties: neither hidden nor read-only.
    /// Adopt writes only these.
    pub shown: bool,
}

/// One plugin package.
#[derive(Debug)]
pub struct PluginKind {
    /// The package id, written as `gid=`.
    pub gid: &'static str,
    /// The package's name: the title of a new one.
    pub name: &'static str,
    /// How many one Miniserver may hold.
    pub max_count: Option<u32>,
    /// Whether a declaration may create it. Only packages whose plugin
    /// carries nothing but its online status, and whose new element was
    /// checked against a save in Loxone Config.
    pub creatable: bool,
    pub params: &'static [Param],
}

/// The `CaptionType=` of the "Network Periphery" folder plugins live in.
pub const FOLDER_CAPTION_TYPE: &str = "27";

#[rustfmt::skip]
const KINDS: &[PluginKind] = &[
    // Alexa 0.4.5: children ['online']
    PluginKind {
        gid: "Alexa",
        name: "Alexa",
        max_count: Some(1),
        creatable: false,
        params: &[

        ],
    },
    // BMW 0.0.61: children ['online', 'pluginDevice']
    PluginKind {
        gid: "BMW",
        name: "BMW CarData",
        max_count: Some(10),
        creatable: false,
        params: &[
            Param { id: "bmw_client_id", t: 11, default: None, shown: true },
        ],
    },
    // EEBUS 2.1.3: children ['online']
    PluginKind {
        gid: "EEBUS",
        name: "EEBUS Gerät",
        max_count: None,
        creatable: false,
        params: &[
            Param { id: "address", t: 11, default: None, shown: true },
        ],
    },
    // Exo 2.0.2: children ['online']
    PluginKind {
        gid: "Exo",
        name: "Exo",
        max_count: Some(10),
        creatable: false,
        params: &[
            Param { id: "broker_address", t: 11, default: Some("eu.broker.loxonecloud.com"), shown: true },
            Param { id: "broker_port", t: 7, default: Some("8443"), shown: true },
            Param { id: "mqtt_enabled", t: 1, default: Some("true"), shown: false },
            Param { id: "workspace_name", t: 11, default: None, shown: false },
            Param { id: "provisioning_code", t: 15, default: None, shown: true },
        ],
    },
    // FIAS 1.0.10: children ['online', 'pluginDevice', 'pluginDevice']
    PluginKind {
        gid: "FIAS",
        name: "Fias",
        max_count: Some(10),
        creatable: false,
        params: &[
            Param { id: "fias_address", t: 11, default: None, shown: true },
            Param { id: "fias_port", t: 7, default: Some("9000"), shown: true },
            Param { id: "fias_port_keys", t: 7, default: Some("9000"), shown: true },
            Param { id: "fias_checkout_fallback_time", t: 16, default: Some("23:00:00"), shown: true },
            Param { id: "fias_database_refresh", t: 1, default: None, shown: true },
            Param { id: "fias_hide_checkout_warning", t: 1, default: None, shown: true },
            Param { id: "fias_key_option_1", t: 12, default: None, shown: true },
            Param { id: "fias_key_option_2", t: 12, default: None, shown: true },
            Param { id: "fias_key_option_3", t: 12, default: None, shown: true },
            Param { id: "fias_key_option_4", t: 12, default: None, shown: true },
            Param { id: "fias_key_option_5", t: 12, default: None, shown: true },
            Param { id: "fias_key_option_6", t: 12, default: None, shown: true },
            Param { id: "fias_key_option_7", t: 12, default: None, shown: true },
            Param { id: "fias_key_option_8", t: 12, default: None, shown: true },
            Param { id: "fias_key_option_9", t: 12, default: None, shown: true },
            Param { id: "fias_key_option_10", t: 12, default: None, shown: true },
            Param { id: "fias_key_option_11", t: 12, default: None, shown: true },
            Param { id: "fias_key_option_12", t: 12, default: None, shown: true },
            Param { id: "fias_key_option_13", t: 12, default: None, shown: true },
            Param { id: "fias_key_option_14", t: 12, default: None, shown: true },
            Param { id: "fias_key_option_15", t: 12, default: None, shown: true },
            Param { id: "fias_key_option_16", t: 12, default: None, shown: true },
            Param { id: "fias_key_option_17", t: 12, default: None, shown: true },
            Param { id: "fias_key_option_18", t: 12, default: None, shown: true },
            Param { id: "fias_key_option_19", t: 12, default: None, shown: true },
            Param { id: "fias_key_option_20", t: 12, default: None, shown: true },
        ],
    },
    // Fronius 1.11.11: children ['online']
    PluginKind {
        gid: "Fronius",
        name: "Fronius",
        max_count: Some(10),
        creatable: false,
        params: &[

        ],
    },
    // Gardena 1.0.8: children ['online']
    PluginKind {
        gid: "Gardena",
        name: "Gardena",
        max_count: Some(5),
        creatable: false,
        params: &[

        ],
    },
    // Hephaistos 0.1.0: children ['online']
    PluginKind {
        gid: "Hephaistos",
        name: "SmartThings (Alpha)",
        max_count: None,
        creatable: false,
        params: &[

        ],
    },
    // HomeConnect 1.3.18: children ['online']
    PluginKind {
        gid: "HomeConnect",
        name: "Home Connect",
        max_count: None,
        creatable: false,
        params: &[

        ],
    },
    // HomeKit 1.5.1: children ['online']
    PluginKind {
        gid: "HomeKit",
        name: "Apple HomeKit",
        max_count: Some(1),
        creatable: false,
        params: &[

        ],
    },
    // Honeywell 1.0.4: children ['online']
    PluginKind {
        gid: "Honeywell",
        name: "Honeywell",
        max_count: Some(5),
        creatable: false,
        params: &[
            Param { id: "address", t: 11, default: None, shown: true },
            Param { id: "username", t: 11, default: None, shown: true },
            Param { id: "password", t: 15, default: None, shown: true },
            Param { id: "systemstatusOpif", t: 8, default: Some("2"), shown: true },
            Param { id: "honeywellUserData", t: 11, default: None, shown: false },
            Param { id: "honeywellDocData", t: 11, default: None, shown: false },
        ],
    },
    // HunterDouglas 2.0.9: children ['online']
    PluginKind {
        gid: "HunterDouglas",
        name: "Hunter Douglas PowerView Hub",
        max_count: None,
        creatable: false,
        params: &[
            Param { id: "address", t: 11, default: None, shown: true },
        ],
    },
    // Husqvarna 1.1.2: children ['online']
    PluginKind {
        gid: "Husqvarna",
        name: "Husqvarna",
        max_count: Some(1),
        creatable: false,
        params: &[

        ],
    },
    // Matter 0.4.2: children ['online']
    PluginKind {
        gid: "Matter",
        name: "Matter (Alpha)",
        max_count: Some(1),
        creatable: true,
        params: &[
            Param { id: "mx_add_roomnames_to_endpoint_names", t: 1, default: Some("true"), shown: true },
        ],
    },
    // McpServer 1.0.10: children ['online']
    PluginKind {
        gid: "McpServer",
        name: "MCP Server",
        max_count: Some(1),
        creatable: true,
        params: &[
            Param { id: "custom-remote-url", t: 11, default: None, shown: true },
            Param { id: "custom-local-url", t: 11, default: None, shown: true },
        ],
    },
    // Minotaurus 1.0.13: children ['online', 'ai', 'di', 'ai', 'ai', 'ai', 'ai', 'ai', 'ai', 'ai']
    PluginKind {
        gid: "Minotaurus",
        name: "Tesla Powerwall",
        max_count: None,
        creatable: false,
        params: &[
            Param { id: "address", t: 11, default: None, shown: true },
            Param { id: "mail", t: 11, default: None, shown: true },
            Param { id: "password", t: 11, default: None, shown: true },
        ],
    },
    // Mqtt 1.1.1: children ['online', 'TO', 'TI']
    PluginKind {
        gid: "Mqtt",
        name: "Mqtt",
        max_count: Some(10),
        creatable: false,
        params: &[
            Param { id: "mqtt_broker_address", t: 11, default: None, shown: true },
            Param { id: "mqtt_broker_port", t: 7, default: Some("1883"), shown: true },
            Param { id: "mqtt_protocol_version", t: 8, default: Some("50000"), shown: true },
            Param { id: "mqtt_client_id", t: 11, default: None, shown: true },
            Param { id: "mqtt_auth_user", t: 11, default: None, shown: true },
            Param { id: "mqtt_auth_pwd", t: 15, default: None, shown: true },
            Param { id: "mqtt_use_ssl_tls", t: 1, default: None, shown: true },
        ],
    },
    // MySkoda 0.0.7: children ['online', 'pluginDevice']
    PluginKind {
        gid: "MySkoda",
        name: "MyŠkoda",
        max_count: Some(10),
        creatable: false,
        params: &[
            Param { id: "api_key", t: 15, default: None, shown: true },
        ],
    },
    // ocpp 1.4.8: children ['online', 'pluginDevice']
    PluginKind {
        gid: "ocpp",
        name: "OCPP Server Connector",
        max_count: Some(10),
        creatable: false,
        params: &[
            Param { id: "host", t: 11, default: None, shown: true },
            Param { id: "protocol", t: 8, default: Some("0"), shown: true },
            Param { id: "local_first", t: 1, default: None, shown: true },
        ],
    },
    // SegwayNavimow 1.0.3: children ['online']
    PluginKind {
        gid: "SegwayNavimow",
        name: "SegwayNavimow",
        max_count: Some(10),
        creatable: false,
        params: &[
            Param { id: "segwaynavimow_region", t: 7, default: Some("0"), shown: true },
        ],
    },
    // Vesta 1.0.26: children ['online']
    PluginKind {
        gid: "Vesta",
        name: "Miele@home",
        max_count: None,
        creatable: false,
        params: &[

        ],
    },
];

/// The package with this `gid`.
pub fn kind(gid: &str) -> Option<&'static PluginKind> {
    KINDS.iter().find(|k| k.gid == gid)
}

/// The packages a declaration can create, for messages.
pub fn creatable() -> Vec<&'static str> {
    KINDS
        .iter()
        .filter(|k| k.creatable)
        .map(|k| k.gid)
        .collect()
}

/// The argument name of a parameter id in source: `-` is written `_`
/// (`custom-local-url` → `custom_local_url`).
pub fn source_key(id: &str) -> String {
    id.replace('-', "_")
}

impl PluginKind {
    /// The parameter an argument names.
    pub fn param(&self, key: &str) -> Option<&'static Param> {
        self.params.iter().find(|p| source_key(p.id) == key)
    }

    /// The argument names, for messages.
    pub fn keys(&self) -> Vec<String> {
        self.params.iter().map(|p| source_key(p.id)).collect()
    }

    /// A new plugin as a save in Loxone Config 17.3 keeps it: the
    /// parameters at their defaults, room and category unset, and its
    /// online status input. `minter` gives the UUIDs.
    pub fn element(
        &self,
        title: &str,
        version: &str,
        minter: &mut crate::uuid::Minter,
        entity: [u8; 6],
    ) -> Element {
        let esc = crate::xml::escape;
        let mut set = String::new();
        for p in self.params {
            match p.default {
                Some(v) => {
                    set.push_str(&format!("\t\t<{} t=\"{}\" v=\"{}\"/>\n", p.id, p.t, esc(v)))
                }
                None => set.push_str(&format!("\t\t<{} t=\"{}\"/>\n", p.id, p.t)),
            }
        }
        let text = format!(
            "<C Type=\"Plugin\" gid=\"{gid}\" V=\"{version}\" U=\"@U\" Title=\"{t}\" Cl=\"0,0,0\" \
             WF=\"16384\" Addr=\"\" MAC=\"\">\n\t<SET>\n{set}\t</SET>\n\t<IoData></IoData>\n\
             \t<C Type=\"Online\" IName=\"S1\" V=\"{version}\" U=\"@U\" Title=\"Online Status {t}\" \
             Cl=\"0,0,0\" Nio=\"1\" WF=\"16384\" ValOT=\"1\" MinTime=\"10\">\n\
             \t\t<Co K=\"Q\" U=\"@U\"/>\n\t\t<IoData St=\"2\"/>\n\
             \t\t<Display Unit=\"&lt;v&gt;\" StateOnly=\"true\"/>\n\t</C>\n</C>",
            gid = self.gid,
            t = esc(title),
        );
        let doc = XmlDocument::parse(text.as_bytes()).expect("the plugin template parses");
        let mut el = doc.root;
        crate::tree_kinds::fill(&mut el, minter, entity);
        el
    }
}

/// What `<SET>` holds for a value of a parameter of type `t`: `Some` is
/// `v=`, `None` no `v=` (an unticked box, empty text). `Err` says what
/// the parameter takes.
pub fn encode(t: &str, value: &Value) -> Result<Option<String>, &'static str> {
    match (t, value) {
        ("1", Value::Bool(b)) => Ok(b.then(|| "true".to_string())),
        ("1", _) => Err("`on` or `off`"),
        ("7" | "8", Value::Number(n)) => Ok(Some(n.clone())),
        ("7" | "8", _) => Err("a number"),
        ("16", Value::Clock(c)) => Ok(Some(if c.len() == 5 {
            format!("{c}:00")
        } else {
            c.clone()
        })),
        ("16", _) => Err("a time of day, e.g. `23:00`"),
        (_, Value::Str(s)) => Ok((!s.is_empty()).then(|| s.clone())),
        (_, Value::Number(n)) => Ok(Some(n.clone())),
        _ => Err("a quoted string"),
    }
}

/// The source value of a `<SET>` entry, as adopt writes it. `None` for a
/// number or time without a value: what it means is unknown, so it is
/// left to Loxone Config.
pub fn decode(t: &str, v: Option<&str>) -> Option<Value> {
    match t {
        "1" => Some(Value::Bool(v == Some("true"))),
        "7" | "8" => v.map(|n| Value::Number(n.to_string())),
        "16" => v.map(|c| Value::Clock(c.strip_suffix(":00").unwrap_or(c).to_string())),
        _ => Some(Value::Str(v.unwrap_or_default().to_string())),
    }
}

/// The `<SET>` of a plugin element.
pub fn set_of(el: &Element) -> Option<&Element> {
    el.child_elements().find(|c| c.name == "SET")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_knows_mcp_server_and_matter() {
        let mcp = kind("McpServer").unwrap();
        assert!(mcp.creatable);
        assert_eq!(mcp.max_count, Some(1));
        assert_eq!(mcp.keys(), ["custom_remote_url", "custom_local_url"]);
        assert_eq!(
            mcp.param("custom_local_url").unwrap().id,
            "custom-local-url"
        );
        let matter = kind("Matter").unwrap();
        assert_eq!(matter.params[0].default, Some("true"));
        assert!(!kind("Exo").unwrap().creatable);
        assert_eq!(creatable(), ["Matter", "McpServer"]);
    }

    #[test]
    fn values_encode_by_type() {
        assert_eq!(encode("1", &Value::Bool(true)), Ok(Some("true".into())));
        assert_eq!(encode("1", &Value::Bool(false)), Ok(None));
        assert!(encode("1", &Value::Str("x".into())).is_err());
        assert_eq!(encode("11", &Value::Str(String::new())), Ok(None));
        assert_eq!(
            encode("7", &Value::Number("8443".into())),
            Ok(Some("8443".into()))
        );
        assert_eq!(
            encode("16", &Value::Clock("23:00".into())),
            Ok(Some("23:00:00".into()))
        );
        assert_eq!(
            decode("16", Some("23:00:00")),
            Some(Value::Clock("23:00".into()))
        );
        assert_eq!(decode("11", None), Some(Value::Str(String::new())));
        assert_eq!(decode("7", None), None);
    }

    #[test]
    fn a_new_plugin_has_fresh_uuids() {
        let mut m = crate::uuid::Minter::new([1, 2, 3, 4, 5, 6], 1_767_225_600);
        let el = kind("Matter")
            .unwrap()
            .element("Matter & Co", "179", &mut m, [9; 6]);
        assert_eq!(el.attr("gid"), Some("Matter"));
        let online = el.child_elements().find(|c| c.name == "C").unwrap();
        assert_eq!(online.attr("Title"), Some("Online Status Matter &amp; Co"));
        assert_ne!(el.attr("U"), online.attr("U"));
        assert!(!format!("{el:?}").contains("@U"));
    }
}
