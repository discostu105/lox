//! Typed reads of the Miniserver's system state: firmware, PLC state, heap,
//! CPU load, the diagnostic counters (`/dev/sys/…`, `/jdev/sys/…`), the
//! network settings (`/jdev/cfg/…`), the CAN bus and LAN counters
//! (`/jdev/bus/…`, `/jdev/lan/…`) and the system log.
//!
//! The parsers are public too, for responses that arrive another way.

use serde::Serialize;

use crate::client::LoxClient;
use crate::error::{Error, Result};

/// The `value` of an `LL` response, from either form the Miniserver sends:
/// XML (`<LL control="…" value="…" Code="200"/>`, the `/dev/` paths) or JSON
/// (`{"LL": {"control": "…", "value": …, "Code": "200"}}`, the `/jdev/`
/// paths). A numeric JSON value comes back as its text. `None` when the
/// response has no value.
pub fn ll_value(text: &str) -> Option<String> {
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(text) {
        return match v.pointer("/LL/value")? {
            serde_json::Value::String(s) => Some(s.clone()),
            other => Some(other.to_string()),
        };
    }
    xml_attr(text, "value").map(str::to_string)
}

/// The `Code` of an `LL` response (`200` on success), from either form.
pub fn ll_code(text: &str) -> Option<u16> {
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(text) {
        return code_of(&v);
    }
    xml_attr(text, "Code")
        .or_else(|| xml_attr(text, "code"))
        .and_then(|c| c.trim().parse().ok())
}

/// The `Code` of a parsed JSON `LL` answer (`{"LL": {"Code": …}}`), sent as
/// a number (Gen 2) or a string (Gen 1); `None` without one.
pub fn code_of(v: &serde_json::Value) -> Option<u16> {
    let c = v.pointer("/LL/Code").or_else(|| v.pointer("/LL/code"))?;
    c.as_u64()
        .and_then(|n| u16::try_from(n).ok())
        .or_else(|| c.as_str()?.trim().parse().ok())
}

/// `name="…"` in a single XML element, matched as a whole attribute name.
fn xml_attr<'a>(xml: &'a str, name: &str) -> Option<&'a str> {
    let key = format!("{name}=\"");
    let mut from = 0;
    while let Some(pos) = xml[from..].find(&key) {
        let at = from + pos;
        if at == 0 || matches!(xml.as_bytes()[at - 1], b' ' | b'<' | b'\t' | b'\n') {
            let start = at + key.len();
            let end = xml[start..].find('"')? + start;
            return Some(&xml[start..end]);
        }
        from = at + 1;
    }
    None
}

/// An error when a JSON `LL` answer (a command's, say) carries a `Code`
/// other than 200; `what` names the request in the message.
pub fn check_ll(what: &str, v: &serde_json::Value) -> Result<()> {
    match code_of(v) {
        None | Some(200) => Ok(()),
        Some(c) => Err(code_error(what, c)),
    }
}

/// `{what}: Code {code}` with what the code usually means.
fn code_error(what: &str, code: u16) -> Error {
    let hint = match code {
        401 => " (unauthorized)",
        403 => " (forbidden: the user lacks the rights for this)",
        404 => " (unknown control or command)",
        500 => " (Miniserver error)",
        _ => "",
    };
    Error::Protocol(format!("{what}: Code {code}{hint}"))
}

/// A yes/no setting as the Miniserver writes it (`1`, `true`, `on`, `yes`,
/// any case); `None` when empty.
pub fn parse_flag(v: &str) -> Option<bool> {
    match v.trim().to_lowercase().as_str() {
        "" => None,
        "1" | "true" | "on" | "yes" => Some(true),
        _ => Some(false),
    }
}

/// The state of the PLC program (`/dev/sps/state`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlcState {
    /// `1`
    Booting,
    /// `3`: loaded, not yet running.
    Started,
    /// `5`
    Running,
    /// `7`
    Error,
    /// `8`: a firmware update is running.
    Updating,
    /// Any other code, as the Miniserver sent it.
    Other(String),
}

impl PlcState {
    pub fn from_code(code: &str) -> PlcState {
        match code.trim() {
            "1" => PlcState::Booting,
            "3" => PlcState::Started,
            "5" => PlcState::Running,
            "7" => PlcState::Error,
            "8" => PlcState::Updating,
            other => PlcState::Other(other.to_string()),
        }
    }

    /// A lowercase name: `running`, `error`, … (`unknown` for other codes).
    pub fn name(&self) -> &'static str {
        match self {
            PlcState::Booting => "booting",
            PlcState::Started => "started",
            PlcState::Running => "running",
            PlcState::Error => "error",
            PlcState::Updating => "updating",
            PlcState::Other(_) => "unknown",
        }
    }

    pub fn is_running(&self) -> bool {
        *self == PlcState::Running
    }
}

/// Heap usage (`/dev/sys/heap`: `359276/1016404kB`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Heap {
    pub used_kb: f64,
    pub total_kb: f64,
}

impl Heap {
    pub fn parse(v: &str) -> Option<Heap> {
        let (used, total) = v.split_once('/')?;
        let used_kb = used.trim().parse().ok()?;
        let total_kb = total.trim().trim_end_matches("kB").trim().parse().ok()?;
        Some(Heap { used_kb, total_kb })
    }

    /// Used share in percent; `None` for a zero total.
    pub fn used_pct(&self) -> Option<f64> {
        (self.total_kb > 0.0).then(|| self.used_kb / self.total_kb * 100.0)
    }
}

/// CPU load (`/jdev/sys/lastcpu`: `CPU:12 SPS:23 Cycles:99`, older firmware
/// a bare `38%`): the whole CPU and the PLC's share, in percent.
pub fn parse_cpu(v: &str) -> (Option<f64>, Option<f64>) {
    let num = |key: &str| {
        v.split_whitespace()
            .find_map(|p| p.strip_prefix(key))
            .and_then(|n| n.trim_end_matches('%').parse::<f64>().ok())
    };
    let cpu = num("CPU:").or_else(|| v.trim().trim_end_matches('%').parse().ok());
    (cpu, num("SPS:"))
}

/// A plain counter; empty (Gen 2 leaves some empty) is `None`.
pub fn parse_counter(v: &str) -> Option<f64> {
    let v = v.trim();
    if v.is_empty() { None } else { v.parse().ok() }
}

/// Firmware, PLC state and heap: what `lox status` shows first.
#[derive(Debug, Clone, PartialEq)]
pub struct SystemStatus {
    /// `/dev/cfg/version`, e.g. `16.1.11.6`.
    pub firmware: String,
    pub plc: PlcState,
    /// `/dev/sys/heap` as sent.
    pub heap_raw: String,
    /// The parsed heap; `None` when the value has another form.
    pub heap: Option<Heap>,
}

/// The diagnostic counters. Each is `None` when the Miniserver leaves it
/// empty or does not answer it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Diagnostics {
    /// Whole CPU load in percent (`lastcpu`).
    pub cpu: Option<f64>,
    /// The PLC's share of the CPU in percent (`lastcpu`, newer firmware).
    pub sps: Option<f64>,
    pub tasks: Option<f64>,
    pub context_switches: Option<f64>,
    /// Context switches caused by interrupts.
    pub context_switches_i: Option<f64>,
    pub interrupts: Option<f64>,
    /// Communication interrupts.
    pub comm_interrupts: Option<f64>,
}

/// The network settings (`/jdev/cfg/…`), as the Miniserver sends them;
/// empty when it leaves one out or does not answer it. [`parse_flag`] reads
/// `dhcp` and `ntp`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Network {
    pub ip: String,
    pub mask: String,
    pub gateway: String,
    pub dns1: String,
    pub dns2: String,
    pub mac: String,
    pub dhcp: String,
    pub ntp: String,
}

/// The CAN bus counters (`/jdev/bus/…`), since the last reboot. Each is
/// `None` when the Miniserver leaves it empty or does not answer it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct BusStats {
    pub packets_sent: Option<u64>,
    pub packets_received: Option<u64>,
    pub receive_errors: Option<u64>,
    pub frame_errors: Option<u64>,
    pub overruns: Option<u64>,
    pub parity_errors: Option<u64>,
}

/// The CAN bus endpoints, in [`BusStats::counters`] order.
const BUS: [(&str, &str); 6] = [
    ("packets_sent", "/jdev/bus/packetssent"),
    ("packets_received", "/jdev/bus/packetsreceived"),
    ("receive_errors", "/jdev/bus/receiveerrors"),
    ("frame_errors", "/jdev/bus/frameerrors"),
    ("overruns", "/jdev/bus/overruns"),
    ("parity_errors", "/jdev/bus/parityerrors"),
];

impl BusStats {
    /// Every counter with its field name, in a fixed order.
    pub fn counters(&self) -> [(&'static str, Option<u64>); 6] {
        let v = [
            self.packets_sent,
            self.packets_received,
            self.receive_errors,
            self.frame_errors,
            self.overruns,
            self.parity_errors,
        ];
        std::array::from_fn(|i| (BUS[i].0, v[i]))
    }
}

/// The LAN counters (`/jdev/lan/…`), since the last reboot. Each is `None`
/// when the Miniserver leaves it empty or does not answer it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct LanStats {
    pub tx_packets: Option<u64>,
    pub tx_errors: Option<u64>,
    pub tx_collisions: Option<u64>,
    pub tx_underruns: Option<u64>,
    pub rx_packets: Option<u64>,
    pub rx_overflow: Option<u64>,
    /// Frames ended early (`eof`).
    pub eof_errors: Option<u64>,
    /// Buffer exhaustion (`exh`).
    pub exhaustion: Option<u64>,
    /// No free buffer for a received frame (`nob`).
    pub no_buffers: Option<u64>,
}

/// The LAN endpoints, in [`LanStats::counters`] order.
const LAN: [(&str, &str); 9] = [
    ("tx_packets", "/jdev/lan/txp"),
    ("tx_errors", "/jdev/lan/txe"),
    ("tx_collisions", "/jdev/lan/txc"),
    ("tx_underruns", "/jdev/lan/txu"),
    ("rx_packets", "/jdev/lan/rxp"),
    ("rx_overflow", "/jdev/lan/rxo"),
    ("eof_errors", "/jdev/lan/eof"),
    ("exhaustion", "/jdev/lan/exh"),
    ("no_buffers", "/jdev/lan/nob"),
];

impl LanStats {
    /// Every counter with its field name, in a fixed order.
    pub fn counters(&self) -> [(&'static str, Option<u64>); 9] {
        let v = [
            self.tx_packets,
            self.tx_errors,
            self.tx_collisions,
            self.tx_underruns,
            self.rx_packets,
            self.rx_overflow,
            self.eof_errors,
            self.exhaustion,
            self.no_buffers,
        ];
        std::array::from_fn(|i| (LAN[i].0, v[i]))
    }
}

impl LoxClient {
    /// The `value` of an `LL` response at `path`; an error when the
    /// Miniserver answers with a `Code` other than 200. Empty when the
    /// response has no value.
    pub fn value(&self, path: &str) -> Result<String> {
        let text = self.get_text(path)?;
        if let Some(code) = ll_code(&text)
            && code != 200
        {
            return Err(code_error(path, code));
        }
        Ok(ll_value(&text).unwrap_or_default())
    }

    /// Firmware, PLC state and heap (three requests).
    pub fn system_status(&self) -> Result<SystemStatus> {
        let firmware = self.value("/dev/cfg/version")?;
        let plc = PlcState::from_code(&self.value("/dev/sps/state")?);
        let heap_raw = self.value("/dev/sys/heap")?;
        let heap = Heap::parse(&heap_raw);
        Ok(SystemStatus {
            firmware,
            plc,
            heap_raw,
            heap,
        })
    }

    /// The heap alone (`/dev/sys/heap`).
    pub fn heap(&self) -> Result<Option<Heap>> {
        Ok(Heap::parse(&self.value("/dev/sys/heap")?))
    }

    /// The diagnostic counters. The CPU load is required; the rest is best
    /// effort, one request each.
    pub fn diagnostics(&self) -> Result<Diagnostics> {
        let (cpu, sps) = parse_cpu(&self.value("/jdev/sys/lastcpu")?);
        let counter = |p: &str| self.value(p).ok().and_then(|v| parse_counter(&v));
        Ok(Diagnostics {
            cpu,
            sps,
            tasks: counter("/jdev/sys/numtasks"),
            context_switches: counter("/jdev/sys/contextswitches"),
            context_switches_i: counter("/jdev/sys/contextswitchesi"),
            interrupts: counter("/jdev/sys/ints"),
            comm_interrupts: counter("/jdev/sys/comints"),
        })
    }

    /// The network settings (eight requests, best effort).
    pub fn network(&self) -> Network {
        let get = |p: &str| self.value(p).unwrap_or_default();
        Network {
            ip: get("/jdev/cfg/ip"),
            mask: get("/jdev/cfg/mask"),
            gateway: get("/jdev/cfg/gateway"),
            dns1: get("/jdev/cfg/dns1"),
            dns2: get("/jdev/cfg/dns2"),
            mac: get("/jdev/cfg/mac"),
            dhcp: get("/jdev/cfg/dhcp"),
            ntp: get("/jdev/cfg/ntp"),
        }
    }

    /// A counter at `path`; `None` when empty or not answered.
    fn counter(&self, path: &str) -> Option<u64> {
        let v = self.value(path).ok()?;
        let v = v.trim();
        v.parse()
            .ok()
            .or_else(|| parse_counter(v).map(|f| f as u64))
    }

    /// The CAN bus counters (six requests, best effort).
    pub fn bus_stats(&self) -> BusStats {
        let [a, b, c, d, e, f] = BUS.map(|(_, p)| self.counter(p));
        BusStats {
            packets_sent: a,
            packets_received: b,
            receive_errors: c,
            frame_errors: d,
            overruns: e,
            parity_errors: f,
        }
    }

    /// The LAN counters (nine requests, best effort).
    pub fn lan_stats(&self) -> LanStats {
        let [a, b, c, d, e, f, g, h, i] = LAN.map(|(_, p)| self.counter(p));
        LanStats {
            tx_packets: a,
            tx_errors: b,
            tx_collisions: c,
            tx_underruns: d,
            rx_packets: e,
            rx_overflow: f,
            eof_errors: g,
            exhaustion: h,
            no_buffers: i,
        }
    }

    /// The system log (`/dev/fsget/log/def.log`), which needs an
    /// administrator. An error when the Miniserver refuses it.
    pub fn log(&self) -> Result<String> {
        let path = "/dev/fsget/log/def.log";
        let text = String::from_utf8_lossy(&self.get_bytes(path)?).into_owned();
        if text.trim_start().starts_with('<') && text.contains("<errorcode>") {
            let code = text
                .split_once("<errorcode>")
                .and_then(|(_, r)| r.split_once("</errorcode>"))
                .map_or("?", |(c, _)| c.trim());
            return Err(Error::Protocol(match code {
                "401" | "403" => "access denied: the system log needs an administrator".into(),
                c => format!("{path}: error code {c}"),
            }));
        }
        Ok(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ll_value_reads_xml_and_json() {
        let xml = r#"<LL control="dev/sys/heap" value="352880/1016404kB" Code="200"/>"#;
        assert_eq!(ll_value(xml).as_deref(), Some("352880/1016404kB"));
        assert_eq!(ll_code(xml), Some(200));
        let json =
            r#"{"LL": { "control": "dev/sys/lastcpu", "value": "CPU:12 SPS:26", "Code": "200"}}"#;
        assert_eq!(ll_value(json).as_deref(), Some("CPU:12 SPS:26"));
        assert_eq!(ll_code(json), Some(200));
        let numeric = r#"{"LL": {"control": "dev/sys/numtasks", "value": 123, "Code": 200}}"#;
        assert_eq!(ll_value(numeric).as_deref(), Some("123"));
        assert_eq!(ll_code(numeric), Some(200));
        assert_eq!(ll_value("<html/>"), None);
    }

    /// Every counter endpoint answers in Gen 1 form (XML or a JSON string)
    /// or Gen 2 form (a JSON number); all of them read as the same counter.
    #[test]
    fn counters_in_both_generations_formats() {
        for (endpoint, num) in [
            ("dev/sys/ints", 987654),
            ("dev/sys/comints", 12345),
            ("dev/sys/contextswitchesi", 98765432),
            ("bus/parityerrors", 0),
            ("lan/txu", 42),
        ] {
            let gen2 = format!(r#"{{"LL":{{"control":"{endpoint}","value":{num},"Code":"200"}}}}"#);
            let gen1 =
                format!(r#"{{"LL":{{"control":"{endpoint}","value":"{num}","Code":"200"}}}}"#);
            let xml = format!(r#"<LL control="{endpoint}" value="{num}" Code="200"/>"#);
            for text in [gen2, gen1, xml] {
                let v = ll_value(&text).unwrap();
                assert_eq!(parse_counter(&v), Some(num as f64), "{text}");
            }
        }
        let cpu = r#"{"LL":{"value":"CPU:12 SPS:26","Code":"200"}}"#;
        assert_eq!(parse_cpu(&ll_value(cpu).unwrap()), (Some(12.0), Some(26.0)));
    }

    #[test]
    fn xml_attr_matches_whole_names() {
        let xml = r#"<LL control="x" myvalue="no" value="yes"/>"#;
        assert_eq!(ll_value(xml).as_deref(), Some("yes"));
    }

    #[test]
    fn plc_codes() {
        assert!(PlcState::from_code("5").is_running());
        assert_eq!(PlcState::from_code("7"), PlcState::Error);
        assert_eq!(PlcState::from_code("9").name(), "unknown");
    }

    #[test]
    fn heap_and_cpu() {
        let h = Heap::parse("359276/1016404kB").unwrap();
        assert_eq!((h.used_kb, h.total_kb), (359276.0, 1016404.0));
        assert!((h.used_pct().unwrap() - 35.35).abs() < 0.01);
        assert_eq!(Heap::parse("?"), None);
        assert_eq!(
            parse_cpu("CPU:12 SPS:23 Cycles:99"),
            (Some(12.0), Some(23.0))
        );
        assert_eq!(parse_cpu("38%"), (Some(38.0), None));
        assert_eq!(parse_counter(""), None);
        assert_eq!(parse_counter(" 42 "), Some(42.0));
    }

    #[test]
    fn code_of_both_generations() {
        assert_eq!(
            code_of(&serde_json::json!({"LL": {"Code": 200}})),
            Some(200)
        );
        assert_eq!(
            code_of(&serde_json::json!({"LL": {"code": " 403"}})),
            Some(403)
        );
        assert_eq!(code_of(&serde_json::json!({"LL": {}})), None);
    }

    #[test]
    fn check_ll_codes() {
        let ok = serde_json::json!({"LL": {"Code": "200"}});
        assert!(check_ll("x", &ok).is_ok());
        let denied = serde_json::json!({"LL": {"Code": 403}});
        let e = check_ll("io/abc/on", &denied).unwrap_err().to_string();
        assert_eq!(
            e,
            "io/abc/on: Code 403 (forbidden: the user lacks the rights for this)"
        );
        assert!(check_ll("x", &serde_json::json!({})).is_ok());
    }

    #[test]
    fn flags_and_counter_order() {
        assert_eq!(parse_flag("Yes"), Some(true));
        assert_eq!(parse_flag("0"), Some(false));
        assert_eq!(parse_flag(" "), None);
        let b = BusStats {
            overruns: Some(3),
            ..Default::default()
        };
        assert_eq!(b.counters()[4], ("overruns", Some(3)));
        let l = LanStats {
            no_buffers: Some(7),
            ..Default::default()
        };
        assert_eq!(l.counters()[8], ("no_buffers", Some(7)));
        assert_eq!(
            serde_json::to_value(&b).unwrap()["overruns"],
            serde_json::json!(3)
        );
    }
}
