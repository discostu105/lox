//! The Miniserver's device search (`jdev/sps/livelearn/…`), the API the
//! Loxone App's device search uses: list paired devices per technology,
//! search a Tree branch or an Air base for unpaired devices, and make a
//! device identify itself. A search and an identify always end with their
//! stop command: on the normal end, on an error and on Ctrl-C.
//!
//! Needs the user rights Device management and Expert mode.

use anyhow::{Context, Result, bail};
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::time::Duration;

/// The technologies `lox hw ls` lists, as the Miniserver names them.
pub const TECHS: [&str; 3] = ["tree", "air", "link"];

/// The App sends a keepalive every 10 s; without it the search ends.
pub const KEEPALIVE: Duration = Duration::from_secs(10);

/// How often a running search asks for new results.
pub const POLL: Duration = Duration::from_secs(2);

/// Names this client in `liveSearch.deviceId`.
pub const DEVICE_ID: &str = "lox-cli";

pub const STOP: &str = "jdev/sps/livelearn/stop";
pub const KEEPALIVE_CMD: &str = "jdev/sps/livelearn/keepalive";
pub const RESULTS: &str = "jdev/sps/livelearn/results";
pub const IDENTIFY_STOP: &str = "jdev/sps/livelearn/identifystop";

pub fn search_cmd(base: &str) -> String {
    format!("jdev/sps/livelearn/search/{base}/{DEVICE_ID}")
}

pub fn identify_cmd(base: &str, serial: &str) -> String {
    format!("jdev/sps/livelearn/identify/{base}/{serial}")
}

pub fn listtree_cmd(ms: &str, tech: &str, devs: bool) -> String {
    let suffix = if devs { "/devs" } else { "" };
    format!("jdev/sps/livelearn/listtree/techs/{ms}/{tech}{suffix}")
}

pub const LIST_MINISERVERS: &str = "jdev/sps/livelearn/listtree/techs/-1";

/// One answer: the `Code` of the `LL` envelope (or the HTTP status) and its
/// `value`, as text.
#[derive(Debug, Clone, PartialEq)]
pub struct Reply {
    pub code: u16,
    pub value: String,
}

/// The HTTP side: a single GET, no retry (a retried `search` would start a
/// second search). The Miniserver in production, a script in tests.
pub trait Http {
    fn get(&mut self, path: &str) -> Result<Reply>;
}

/// Time and cancel: `sleep` returns false when the wait was cancelled
/// (Ctrl-C). Real time in production, virtual time in tests.
pub trait Clock {
    fn elapsed(&self) -> Duration;
    fn sleep(&mut self, d: Duration) -> bool;
}

/// Read an `{"LL": {"Code": …, "value": …}}` answer. A body without the
/// envelope (the `listtree` answers are bare JSON) is the value itself.
pub fn parse_reply(http_status: u16, body: &str) -> Reply {
    let parsed: Option<Value> = serde_json::from_str(body).ok();
    let ll = parsed.as_ref().and_then(|v| v.get("LL"));
    let Some(ll) = ll else {
        return Reply {
            code: http_status,
            value: body.to_string(),
        };
    };
    let code = ll
        .get("Code")
        .or_else(|| ll.get("code"))
        .and_then(|c| match c {
            Value::String(s) => s.trim().parse().ok(),
            Value::Number(n) => n.as_u64().and_then(|n| u16::try_from(n).ok()),
            _ => None,
        })
        .unwrap_or(http_status);
    let value = match ll.get("value") {
        Some(Value::String(s)) => s.clone(),
        Some(v) => v.to_string(),
        None => String::new(),
    };
    Reply { code, value }
}

/// A paired device, as `listtree/techs/<ms>/<tech>/devs` lists it.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct PairedDevice {
    /// `tree`, `air` or `link`
    pub tech: String,
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
    /// As the Miniserver writes it: 8 hex digits, or a MAC for Air devices
    pub serial: String,
    /// The serial of the Tree branch, Air base or extension it is paired with;
    /// `lox hw identify` and `lox hw search` address it
    pub base: String,
    pub ms_serial: String,
    pub uuid: String,
    /// The room's UUID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub room: Option<String>,
    /// The room's name, from the structure file
    #[serde(skip_serializing_if = "Option::is_none")]
    pub room_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub place: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub switchboard: Option<String>,
    /// `deviceState`; `None` for devices without one (Air bases on the Tree bus)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub online: Option<bool>,
}

/// A search base: a Tree branch's extension or an Air base.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct Base {
    pub tech: String,
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub serial: String,
    pub ms_serial: String,
}

fn text(v: &Value, key: &str) -> Option<String> {
    v.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
}

/// The serial of the extension a node hangs on: an object
/// `{msSerial, serialNr}` in `listtree`, maybe a plain serial elsewhere.
fn extension_serial(v: &Value) -> Option<String> {
    match v.get("extension")? {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        Value::Object(_) => text(&v["extension"], "serialNr"),
        _ => None,
    }
}

fn children(v: &Value) -> impl Iterator<Item = &Value> {
    ["captions", "devices"]
        .into_iter()
        .filter_map(move |k| v.get(k).and_then(Value::as_array))
        .flatten()
}

/// The top-level nodes of `tech` under each Miniserver: the search bases.
fn tech_roots<'a>(answer: &'a Value, tech: &'a str) -> impl Iterator<Item = (&'a Value, String)> {
    answer.as_array().into_iter().flatten().flat_map(move |ms| {
        let ms_serial = text(ms, "serialNr").unwrap_or_default();
        ms.get("techtags")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter(move |t| t.get("name").and_then(Value::as_str) == Some(tech))
            .flat_map(children)
            .map(move |n| (n, ms_serial.clone()))
    })
}

/// The search bases of `listtree/techs/<ms>/<tech>` (or its `/devs` form).
pub fn parse_bases(answer: &Value, tech: &str) -> Vec<Base> {
    tech_roots(answer, tech)
        .filter_map(|(n, ms)| {
            Some(Base {
                tech: tech.to_string(),
                name: text(n, "name").unwrap_or_default(),
                kind: text(n, "type").unwrap_or_default(),
                serial: text(n, "serialNr")?,
                ms_serial: text(n, "msSerial").unwrap_or(ms),
            })
        })
        .collect()
}

/// The paired devices of `listtree/techs/<ms>/<tech>/devs`: every node below
/// a base that has a serial. Branches (`LoxCaption.*`) only group devices.
pub fn parse_devices(answer: &Value, tech: &str) -> Vec<PairedDevice> {
    fn walk(n: &Value, tech: &str, parent_base: &str, ms: &str, out: &mut Vec<PairedDevice>) {
        let kind = text(n, "type").unwrap_or_default();
        if let (false, Some(serial)) = (kind.starts_with("LoxCaption"), text(n, "serialNr")) {
            out.push(PairedDevice {
                tech: tech.to_string(),
                name: text(n, "name").unwrap_or_default(),
                kind,
                base: extension_serial(n)
                    .filter(|e| *e != serial)
                    .unwrap_or_else(|| parent_base.to_string()),
                serial,
                ms_serial: text(n, "msSerial").unwrap_or_else(|| ms.to_string()),
                uuid: text(n, "uuid").unwrap_or_default(),
                room: text(n, "room"),
                room_name: None,
                place: text(n, "place"),
                switchboard: text(n, "switchboard"),
                online: n.get("deviceState").and_then(Value::as_bool),
            });
        }
        for c in children(n) {
            walk(c, tech, parent_base, ms, out);
        }
    }
    let mut out = Vec::new();
    for (base, ms) in tech_roots(answer, tech) {
        let serial = text(base, "serialNr").unwrap_or_default();
        for c in children(base) {
            walk(c, tech, &serial, &ms, &mut out);
        }
    }
    out
}

/// The Miniservers and the technologies each offers (`listtree/techs/-1`).
pub fn parse_miniservers(answer: &Value) -> Vec<(String, Vec<String>)> {
    answer
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|ms| {
            let techs = ms
                .get("techtags")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|t| text(t, "name"))
                .collect();
            Some((text(ms, "serialNr")?, techs))
        })
        .collect()
}

/// Hex digits only, upper case, so `50:4F:94:…` and `504f94…` match.
pub fn normalize_serial(s: &str) -> String {
    s.chars()
        .filter(char::is_ascii_hexdigit)
        .map(|c| c.to_ascii_uppercase())
        .collect()
}

/// A serial lox puts into a path: letters and digits, `:` for MACs.
pub fn check_serial(s: &str) -> Result<()> {
    if s.is_empty() || !s.chars().all(|c| c.is_ascii_alphanumeric() || c == ':') {
        bail!("'{s}' is not a serial. Use the serial `lox hw ls` shows, e.g. 13000001.");
    }
    Ok(())
}

/// A device a search found, from `livelearn/results`.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct Found {
    pub serial: String,
    #[serde(rename = "type")]
    pub kind: String,
    /// The extension or base that sees it
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ms_serial: Option<String>,
    /// What the device can be given when paired (bit mask)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<u64>,
}

/// The JSON array of `livelearn/results` (also the value of a Tree `search`).
pub fn parse_results(value: &str) -> Result<Vec<Found>> {
    let value = value.trim();
    if value.is_empty() {
        return Ok(Vec::new());
    }
    let v: Value = serde_json::from_str(value)
        .with_context(|| format!("unexpected search results from the Miniserver: {value}"))?;
    Ok(v.as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| {
            Some(Found {
                serial: text(r, "serialNr")?,
                kind: text(r, "type").unwrap_or_default(),
                base: extension_serial(r),
                ms_serial: text(r, "msSerial"),
                capabilities: r.get("capabilities").and_then(Value::as_u64),
            })
        })
        .collect())
}

fn refused(what: &str, code: u16) -> anyhow::Error {
    let hint = match code {
        401 | 403 => {
            " The user lox connects with needs the rights Device management and Expert mode."
        }
        _ => "",
    };
    anyhow::anyhow!("the Miniserver refused {what} (code {code}).{hint}")
}

/// One command that must succeed with code 200.
fn call_ok<H: Http + ?Sized>(http: &mut H, path: &str, what: &str) -> Result<Reply> {
    let r = http.get(path).with_context(|| format!("{what} failed"))?;
    if r.code != 200 {
        return Err(refused(what, r.code));
    }
    Ok(r)
}

/// End something; a problem doing so becomes a warning.
fn end<H: Http + ?Sized>(http: &mut H, path: &str, what: &str) -> Option<String> {
    let warn = |e: String| Some(format!("could not {what}: {e}"));
    match http.get(path) {
        Ok(r) if r.code == 200 => None,
        Ok(r) => warn(format!("the Miniserver answered code {}", r.code)),
        Err(e) => warn(format!("{e:#}")),
    }
}

/// How a search ended.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Searched {
    pub base: String,
    pub found: Vec<Found>,
    /// Ended by Ctrl-C before its time was up
    pub cancelled: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

/// Search `base` for `duration`: start, keepalive every 10 s, poll the
/// results, and always send `stop`. `on_found` sees each device once, as
/// it appears.
pub fn search<H, C, F>(
    http: &mut H,
    clock: &mut C,
    base: &str,
    duration: Duration,
    mut on_found: F,
) -> Result<Searched>
where
    H: Http + ?Sized,
    C: Clock + ?Sized,
    F: FnMut(&Found),
{
    let mut found: Vec<Found> = Vec::new();
    let mut cancelled = false;
    let mut take = |list: Vec<Found>, found: &mut Vec<Found>| {
        for f in list {
            let key = normalize_serial(&f.serial);
            if !found.iter().any(|g| normalize_serial(&g.serial) == key) {
                on_found(&f);
                found.push(f);
            }
        }
    };
    let outcome = (|| -> Result<()> {
        let start = clock.elapsed();
        // a Tree search answers with what it found already
        let r = call_ok(http, &search_cmd(base), "starting the search")?;
        take(parse_results(&r.value)?, &mut found);
        let mut next_keepalive = start + KEEPALIVE;
        loop {
            let now = clock.elapsed();
            let left = (start + duration).saturating_sub(now);
            if left.is_zero() {
                return Ok(());
            }
            if !clock.sleep(POLL.min(left)) {
                cancelled = true;
                return Ok(());
            }
            if clock.elapsed() >= next_keepalive {
                call_ok(http, KEEPALIVE_CMD, "keeping the search alive")?;
                next_keepalive += KEEPALIVE;
            }
            let r = call_ok(http, RESULTS, "reading the search results")?;
            take(parse_results(&r.value)?, &mut found);
        }
    })();
    let stop_warning = end(http, STOP, "end the search");
    match outcome {
        Ok(()) => Ok(Searched {
            base: base.to_string(),
            found,
            cancelled,
            warnings: stop_warning.into_iter().collect(),
        }),
        Err(e) => Err(match stop_warning {
            Some(w) => e.context(w),
            None => e,
        }),
    }
}

/// How an identify ended.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Identified {
    pub serial: String,
    pub base: String,
    pub cancelled: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

/// Let `serial` on `base` identify itself (it flashes) for `duration`, then
/// always end it: `identify/<base>/0` and `identifystop`.
pub fn identify<H, C>(
    http: &mut H,
    clock: &mut C,
    base: &str,
    serial: &str,
    duration: Duration,
) -> Result<Identified>
where
    H: Http + ?Sized,
    C: Clock + ?Sized,
{
    let outcome = call_ok(http, &identify_cmd(base, serial), "starting identify")
        .map(|_| !clock.sleep(duration));
    let warnings: Vec<String> = [
        end(http, &identify_cmd(base, "0"), "end identify"),
        end(http, IDENTIFY_STOP, "end identify"),
    ]
    .into_iter()
    .flatten()
    .collect();
    match outcome {
        Ok(cancelled) => Ok(Identified {
            serial: serial.to_string(),
            base: base.to_string(),
            cancelled,
            warnings,
        }),
        Err(e) => Err(match warnings.first() {
            Some(w) => e.context(w.clone()),
            None => e,
        }),
    }
}

/// Room UUID → name, from the structure file's `rooms`.
pub fn room_names(structure: &Value) -> BTreeMap<String, String> {
    structure
        .get("rooms")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .filter_map(|(uuid, r)| Some((uuid.clone(), text(r, "name")?)))
        .collect()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::collections::VecDeque;

    // Trimmed answers of a Miniserver Gen 2 (firmware 17.3), serials and
    // UUIDs replaced.
    pub const MINISERVERS: &str = r#"[{"name":"Loxone Miniserver","type":"LoxLIVE.2","serialNr":"504F94000001","msSerial":"504F94000001","extension":{"msSerial":"504F94000001","serialNr":"504F94000001"},"uuid":"10000000-0000-0001-ffff000000000001","techtags":[{"name":"tree","globalSearch":true,"title":"Tree"},{"name":"air","globalSearch":false,"title":"Air"},{"name":"network"},{"name":"audiodevices"},{"name":"link"}]}]"#;

    pub const TREE_BASES: &str = r#"[{"name":"Loxone Miniserver","type":"LoxLIVE.2","serialNr":"504F94000001","msSerial":"504F94000001","techtags":[{"name":"tree","devices":[{"name":"Loxone Miniserver - Tree","type":"TreeBase","serialNr":"13000001","msSerial":"504F94000001","extension":{"msSerial":"504F94000001","serialNr":"13000001"},"uuid":"10000000-0000-0002-ffff000000000001","maxDevices":50,"captions":[{"name":"Tree - Branch","type":"LoxCaption.3","serialNr":"","msSerial":"504F94000001","uuid":"10000000-0000-0003-ffff000000000001","devices":[{"name":"Air Base A","type":"LoxAIR.32797","serialNr":"0C000A01","msSerial":"504F94000001","extension":{"msSerial":"504F94000001","serialNr":"13000001"},"uuid":"10000000-0000-0004-ffff000000000001","maxDevices":128,"room":"20000000-0000-0001-ffff000000000001"}]}]},{"name":"Intercom - Tree","type":"TreeBase","serialNr":"13000003","msSerial":"504F94000001","extension":{"msSerial":"504F94000001","serialNr":"13000003"},"uuid":"10000000-0000-0005-ffff000000000001","maxDevices":50,"captions":[{"name":"Tree - Tree","type":"LoxCaption.3","serialNr":"","msSerial":"504F94000001","uuid":"10000000-0000-0006-ffff000000000001"}]}]}]}]"#;

    pub const TREE_DEVS: &str = r#"[{"name":"Loxone Miniserver","type":"LoxLIVE.2","serialNr":"504F94000001","msSerial":"504F94000001","techtags":[{"name":"tree","devices":[{"name":"Loxone Miniserver - Tree","type":"TreeBase","serialNr":"13000001","msSerial":"504F94000001","extension":{"msSerial":"504F94000001","serialNr":"13000001"},"uuid":"10000000-0000-0002-ffff000000000001","captions":[{"name":"Tree - Branch","type":"LoxCaption.3","serialNr":"","msSerial":"504F94000001","uuid":"10000000-0000-0003-ffff000000000001","devices":[{"name":"Room Sensor","type":"RoomsensorTree","serialNr":"B0000001","msSerial":"504F94000001","extension":{"msSerial":"504F94000001","serialNr":"13000001"},"uuid":"10000000-0000-0010-ffff000000000001","deviceState":true,"room":"20000000-0000-0001-ffff000000000001"},{"name":"Air Base A","type":"LoxAIR.32797","serialNr":"0C000A01","msSerial":"504F94000001","extension":{"msSerial":"504F94000001","serialNr":"13000001"},"uuid":"10000000-0000-0004-ffff000000000001","maxDevices":128,"room":"20000000-0000-0001-ffff000000000001"},{"name":"Touch","type":"TouchFlexTree_V2","serialNr":"B0000002","msSerial":"504F94000001","extension":{"msSerial":"504F94000001","serialNr":"13000001"},"uuid":"10000000-0000-0011-ffff000000000001","place":"Door","deviceState":false,"room":"20000000-0000-0002-ffff000000000001"}]}]},{"name":"Intercom - Tree","type":"TreeBase","serialNr":"13000003","msSerial":"504F94000001","extension":{"msSerial":"504F94000001","serialNr":"13000003"},"uuid":"10000000-0000-0005-ffff000000000001","captions":[{"name":"Tree - Tree","type":"LoxCaption.3","serialNr":"","msSerial":"504F94000001","uuid":"10000000-0000-0006-ffff000000000001","devices":[{"name":"Code Touch","type":"NFCCodeTouchTree","serialNr":"B0000003","msSerial":"504F94000001","extension":{"msSerial":"504F94000001","serialNr":"13000003"},"uuid":"10000000-0000-0012-ffff000000000001","deviceState":true,"room":"20000000-0000-0003-ffff000000000001"}]}]}]}]}]"#;

    // Air devices hang directly on their base, under "captions".
    pub const AIR_DEVS: &str = r#"[{"name":"Loxone Miniserver","type":"LoxLIVE.2","serialNr":"504F94000001","msSerial":"504F94000001","techtags":[{"name":"air","devices":[{"name":"Air Base A","type":"LoxAIR.32797","serialNr":"0C000A01","msSerial":"504F94000001","extension":{"msSerial":"504F94000001","serialNr":"13000001"},"uuid":"10000000-0000-0004-ffff000000000001","maxDevices":128,"room":"20000000-0000-0001-ffff000000000001","captions":[{"name":"Nano IO","type":"NanoIO_V2","serialNr":"50:4F:94:FF:FE:00:00:01","msSerial":"504F94000001","extension":{"msSerial":"504F94000001","serialNr":"0C000A01"},"uuid":"10000000-0000-0020-ffff000000000001","place":"Cabinet","deviceState":true,"room":"20000000-0000-0001-ffff000000000001"}]},{"name":"Air Base B","type":"LoxAIR.32797","serialNr":"0C000A02","msSerial":"504F94000001","extension":{"msSerial":"504F94000001","serialNr":"13000001"},"uuid":"10000000-0000-0007-ffff000000000001","maxDevices":128}]}]}]"#;

    // Extensions sit under a "link" caption and are their own extension.
    pub const LINK_DEVS: &str = r#"[{"name":"Loxone Miniserver","type":"LoxLIVE.2","serialNr":"504F94000001","msSerial":"504F94000001","techtags":[{"name":"link","devices":[{"name":"Loxone Miniserver - Link","type":"LoxCaption.24","serialNr":"link","msSerial":"504F94000001","uuid":"10000000-0000-0008-ffff000000000001","maxDevices":30,"devices":[{"name":"Relay Extension","type":"RelayExtension_V2","serialNr":"0B000001","msSerial":"504F94000001","extension":{"msSerial":"504F94000001","serialNr":"0B000001"},"uuid":"10000000-0000-0030-ffff000000000001","switchboard":"Cabinet R01","deviceState":true,"room":"20000000-0000-0001-ffff000000000001"}]}]}]}]"#;

    fn json(s: &str) -> Value {
        serde_json::from_str(s).unwrap()
    }

    #[test]
    fn replies_unwrap_the_ll_envelope() {
        let r = parse_reply(
            200,
            r#"{"LL": { "control": "dev/sps/livelearn/results", "value": "[]", "Code": "200"}}"#,
        );
        assert_eq!(
            r,
            Reply {
                code: 200,
                value: "[]".into()
            }
        );
        let r = parse_reply(200, r#"{"LL":{"control":"x","code":"403","value":""}}"#);
        assert_eq!(r.code, 403);
        // listtree answers are bare JSON
        let r = parse_reply(200, "[1]");
        assert_eq!((r.code, r.value.as_str()), (200, "[1]"));
    }

    #[test]
    fn miniservers_list_their_technologies() {
        let ms = parse_miniservers(&json(MINISERVERS));
        assert_eq!(ms.len(), 1);
        assert_eq!(ms[0].0, "504F94000001");
        assert_eq!(
            ms[0].1,
            ["tree", "air", "network", "audiodevices", "link"].map(String::from)
        );
    }

    #[test]
    fn tree_bases_are_the_top_level_nodes() {
        let bases = parse_bases(&json(TREE_BASES), "tree");
        let serials: Vec<_> = bases.iter().map(|b| b.serial.as_str()).collect();
        assert_eq!(serials, ["13000001", "13000003"]);
        assert_eq!(bases[0].kind, "TreeBase");
        let air = parse_bases(&json(AIR_DEVS), "air");
        let serials: Vec<_> = air.iter().map(|b| b.serial.as_str()).collect();
        assert_eq!(serials, ["0C000A01", "0C000A02"]);
        // another technology's answer has no bases for this one
        assert!(parse_bases(&json(AIR_DEVS), "tree").is_empty());
    }

    #[test]
    fn tree_devices_skip_branches_and_keep_their_base() {
        let devs = parse_devices(&json(TREE_DEVS), "tree");
        let rows: Vec<_> = devs
            .iter()
            .map(|d| (d.serial.as_str(), d.base.as_str(), d.online))
            .collect();
        assert_eq!(
            rows,
            [
                ("B0000001", "13000001", Some(true)),
                ("0C000A01", "13000001", None),
                ("B0000002", "13000001", Some(false)),
                ("B0000003", "13000003", Some(true)),
            ]
        );
        assert_eq!(devs[2].place.as_deref(), Some("Door"));
        assert_eq!(
            devs[0].room.as_deref(),
            Some("20000000-0000-0001-ffff000000000001")
        );
    }

    #[test]
    fn air_devices_hang_on_their_base() {
        let devs = parse_devices(&json(AIR_DEVS), "air");
        assert_eq!(devs.len(), 1);
        assert_eq!(devs[0].serial, "50:4F:94:FF:FE:00:00:01");
        assert_eq!(devs[0].base, "0C000A01");
        assert_eq!(devs[0].kind, "NanoIO_V2");
        assert_eq!(devs[0].tech, "air");
    }

    #[test]
    fn extensions_are_listed_under_link() {
        let devs = parse_devices(&json(LINK_DEVS), "link");
        assert_eq!(devs.len(), 1);
        assert_eq!(devs[0].serial, "0B000001");
        // an extension is its own extension: the base is the link branch
        assert_eq!(devs[0].base, "link");
        assert_eq!(devs[0].switchboard.as_deref(), Some("Cabinet R01"));
    }

    #[test]
    fn results_parse_with_either_extension_form() {
        let found = parse_results(
            r#"[{"serialNr":"B0000009","type":"TouchTree","extension":"13000001","msSerial":"504F94000001","capabilities":17},
                {"serialNr":"50:4F:94:FF:FE:00:00:09","type":"NanoIO_V2","extension":{"serialNr":"0C000A01"}}]"#,
        )
        .unwrap();
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].base.as_deref(), Some("13000001"));
        assert_eq!(found[0].capabilities, Some(17));
        assert_eq!(found[1].base.as_deref(), Some("0C000A01"));
        assert!(parse_results("[]").unwrap().is_empty());
        assert!(parse_results("").unwrap().is_empty());
        assert!(parse_results("oops").is_err());
    }

    #[test]
    fn serials_are_checked_before_they_go_into_a_path() {
        assert!(check_serial("13000001").is_ok());
        assert!(check_serial("50:4F:94:FF:FE:00:00:01").is_ok());
        assert!(check_serial("../x").is_err());
        assert!(check_serial("").is_err());
        assert_eq!(normalize_serial("50:4f:94"), "504F94");
    }

    /// The Miniserver's side: answers by command prefix, records what was
    /// sent. `results` hands out its queue one answer per call.
    #[derive(Default)]
    pub struct FakeHttp {
        pub sent: Vec<String>,
        pub results: VecDeque<String>,
        /// Commands (prefix) answered with this code instead of 200
        pub codes: Vec<(String, u16)>,
        /// Commands (prefix) that fail like a dropped connection
        pub fail: Vec<String>,
        pub search_value: String,
    }

    impl Http for FakeHttp {
        fn get(&mut self, path: &str) -> Result<Reply> {
            self.sent.push(path.to_string());
            if self.fail.iter().any(|p| path.starts_with(p.as_str())) {
                bail!("connection reset");
            }
            let code = self
                .codes
                .iter()
                .find(|(p, _)| path.starts_with(p.as_str()))
                .map_or(200, |(_, c)| *c);
            let value = if path == RESULTS {
                self.results.pop_front().unwrap_or_else(|| "[]".into())
            } else if path.starts_with("jdev/sps/livelearn/search/") {
                self.search_value.clone()
            } else {
                String::new()
            };
            Ok(Reply { code, value })
        }
    }

    /// Virtual time; `cancel_at` is when Ctrl-C comes.
    pub struct FakeClock {
        pub now: Duration,
        pub cancel_at: Option<Duration>,
    }

    impl FakeClock {
        pub fn new(cancel_at: Option<Duration>) -> Self {
            Self {
                now: Duration::ZERO,
                cancel_at,
            }
        }
    }

    impl Clock for FakeClock {
        fn elapsed(&self) -> Duration {
            self.now
        }
        fn sleep(&mut self, d: Duration) -> bool {
            match self.cancel_at {
                Some(c) if self.now + d >= c => {
                    self.now = c;
                    false
                }
                _ => {
                    self.now += d;
                    true
                }
            }
        }
    }

    const NEW: &str = r#"[{"serialNr":"B0000009","type":"TouchTree","extension":"13000001"}]"#;

    #[test]
    fn a_search_keeps_alive_reports_each_device_once_and_stops() {
        let mut http = FakeHttp {
            results: ["[]", NEW, NEW].map(String::from).into(),
            ..Default::default()
        };
        let mut clock = FakeClock::new(None);
        let mut seen = Vec::new();
        let s = search(
            &mut http,
            &mut clock,
            "13000001",
            Duration::from_secs(25),
            |f| seen.push(f.serial.clone()),
        )
        .unwrap();
        assert_eq!(seen, ["B0000009"]);
        assert_eq!(s.found.len(), 1);
        assert!(!s.cancelled && s.warnings.is_empty());
        assert_eq!(http.sent[0], "jdev/sps/livelearn/search/13000001/lox-cli");
        assert_eq!(http.sent.last().unwrap(), STOP);
        let keepalives = http.sent.iter().filter(|c| *c == KEEPALIVE_CMD).count();
        assert_eq!(keepalives, 2, "at 10 s and 20 s: {:?}", http.sent);
        assert_eq!(clock.now, Duration::from_secs(25));
    }

    #[test]
    fn a_tree_search_takes_the_devices_of_its_start_answer() {
        let mut http = FakeHttp {
            search_value: NEW.into(),
            ..Default::default()
        };
        let s = search(
            &mut http,
            &mut FakeClock::new(None),
            "13000001",
            Duration::from_secs(3),
            |_| {},
        )
        .unwrap();
        assert_eq!(s.found[0].serial, "B0000009");
    }

    #[test]
    fn ctrl_c_ends_the_search_and_still_stops() {
        let mut http = FakeHttp::default();
        let mut clock = FakeClock::new(Some(Duration::from_secs(5)));
        let s = search(
            &mut http,
            &mut clock,
            "0C000A01",
            Duration::from_secs(60),
            |_| {},
        )
        .unwrap();
        assert!(s.cancelled);
        assert_eq!(clock.now, Duration::from_secs(5));
        assert_eq!(http.sent.last().unwrap(), STOP);
    }

    #[test]
    fn a_refused_start_names_the_rights_and_still_stops() {
        let mut http = FakeHttp {
            codes: vec![("jdev/sps/livelearn/search/".into(), 403)],
            ..Default::default()
        };
        let e = search(
            &mut http,
            &mut FakeClock::new(None),
            "13000001",
            Duration::from_secs(60),
            |_| {},
        )
        .unwrap_err();
        assert!(format!("{e:#}").contains("Expert mode"), "{e:#}");
        assert_eq!(http.sent, [search_cmd("13000001"), STOP.to_string()]);
    }

    #[test]
    fn an_error_while_searching_still_stops_and_a_failed_stop_is_reported() {
        let mut http = FakeHttp {
            fail: vec![RESULTS.into(), STOP.into()],
            ..Default::default()
        };
        let e = search(
            &mut http,
            &mut FakeClock::new(None),
            "13000001",
            Duration::from_secs(60),
            |_| {},
        )
        .unwrap_err();
        let msg = format!("{e:#}");
        assert!(msg.contains("could not end the search"), "{msg}");
        assert!(msg.contains("reading the search results"), "{msg}");
        assert_eq!(http.sent.last().unwrap(), STOP);
    }

    #[test]
    fn a_stop_with_another_code_is_a_warning() {
        let mut http = FakeHttp {
            codes: vec![(STOP.into(), 500)],
            ..Default::default()
        };
        let s = search(
            &mut http,
            &mut FakeClock::new(None),
            "13000001",
            Duration::from_secs(1),
            |_| {},
        )
        .unwrap();
        assert_eq!(s.warnings.len(), 1);
        assert!(s.warnings[0].contains("code 500"));
    }

    #[test]
    fn identify_always_ends_with_both_stop_commands() {
        let ends = [identify_cmd("13000001", "0"), IDENTIFY_STOP.to_string()];

        let mut http = FakeHttp::default();
        let mut clock = FakeClock::new(None);
        let i = identify(
            &mut http,
            &mut clock,
            "13000001",
            "B0000001",
            Duration::from_secs(10),
        )
        .unwrap();
        assert!(!i.cancelled);
        assert_eq!(clock.now, Duration::from_secs(10));
        assert_eq!(
            http.sent[0],
            "jdev/sps/livelearn/identify/13000001/B0000001"
        );
        assert_eq!(http.sent[1..], ends);

        // Ctrl-C
        let mut http = FakeHttp::default();
        let i = identify(
            &mut http,
            &mut FakeClock::new(Some(Duration::from_secs(2))),
            "13000001",
            "B0000001",
            Duration::from_secs(10),
        )
        .unwrap();
        assert!(i.cancelled);
        assert_eq!(http.sent[1..], ends);

        // refused start
        let mut http = FakeHttp {
            codes: vec![("jdev/sps/livelearn/identify/13000001/B".into(), 403)],
            ..Default::default()
        };
        let e = identify(
            &mut http,
            &mut FakeClock::new(None),
            "13000001",
            "B0000001",
            Duration::from_secs(10),
        )
        .unwrap_err();
        assert!(format!("{e:#}").contains("refused starting identify"));
        assert_eq!(http.sent[1..], ends);
    }

    #[test]
    fn room_names_come_from_the_structure() {
        let st = json(r#"{"rooms":{"r1":{"name":"Kitchen"},"r2":{}}}"#);
        let m = room_names(&st);
        assert_eq!(m.get("r1").map(String::as_str), Some("Kitchen"));
        assert_eq!(m.len(), 1);
    }
}
