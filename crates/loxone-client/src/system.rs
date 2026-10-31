//! Typed reads of the Miniserver's system state: firmware, PLC state, heap,
//! CPU load and the diagnostic counters (`/dev/sys/…`, `/jdev/sys/…`).
//!
//! The parsers are public too, for responses that arrive another way.

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
        let c = v.pointer("/LL/Code").or_else(|| v.pointer("/LL/code"))?;
        return c
            .as_u64()
            .and_then(|n| u16::try_from(n).ok())
            .or_else(|| c.as_str()?.parse().ok());
    }
    xml_attr(text, "Code")
        .or_else(|| xml_attr(text, "code"))
        .and_then(|c| c.parse().ok())
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

impl LoxClient {
    /// The `value` of an `LL` response at `path`; an error when the
    /// Miniserver answers with a `Code` other than 200. Empty when the
    /// response has no value.
    pub fn value(&self, path: &str) -> Result<String> {
        let text = self.get_text(path)?;
        if let Some(code) = ll_code(&text)
            && code != 200
        {
            return Err(Error::Protocol(format!("{path}: Code {code}")));
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
}
