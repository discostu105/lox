//! Polled data shapes (diagnostics, devices, log, sites) and their parsers.
//!
//! Live pollers and the demo backend both produce these, so screens don't care
//! where the data came from.

use super::text::clean;

/// `/jdev/sys/*` diagnostics. `None` = not available on this Miniserver (Gen 2
/// returns empty strings for several counters).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Diag {
    pub cpu: Option<f64>,
    pub sps: Option<f64>,
    pub heap_used_kb: Option<f64>,
    pub heap_total_kb: Option<f64>,
    pub tasks: Option<f64>,
    pub ctx_switches: Option<f64>,
    pub ints: Option<f64>,
    pub comints: Option<f64>,
    pub sd: Option<String>,
}

impl Diag {
    pub fn heap_pct(&self) -> Option<f64> {
        match (self.heap_used_kb, self.heap_total_kb) {
            (Some(u), Some(t)) if t > 0.0 => Some(u / t * 100.0),
            _ => None,
        }
    }
    /// SD card test reports an error.
    pub fn sd_error(&self) -> bool {
        self.sd.as_deref().is_some_and(|s| {
            let l = s.to_lowercase();
            l.contains("error") && !l.contains("no error")
        })
    }
}

/// Static-ish Miniserver information (polled once per connection).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MsInfo {
    pub firmware: String,
    pub serial: String,
    pub ms_type: String,
    pub ip: String,
    pub mask: String,
    pub gateway: String,
    pub dns: Vec<String>,
    pub mac: String,
    pub dhcp: Option<bool>,
    pub ntp: Option<bool>,
    pub structure_version: String,
    pub sps_state: Option<i64>,
    /// Miniserver clock "YYYY-MM-DD HH:MM:SS"
    pub ms_time: Option<String>,
}

/// Cumulative CAN/LAN counters, in display order.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BusLan {
    pub counters: Vec<(String, Option<u64>)>,
}

/// Counters where any increase is a problem (errors, overruns, lost buffers).
pub fn is_error_counter(name: &str) -> bool {
    let l = name.to_lowercase();
    [
        "error",
        "overrun",
        "overflow",
        "underrun",
        "collision",
        "exhausted",
        "no buffer",
    ]
    .iter()
    .any(|k| l.contains(k))
}

/// A Tree/Air/network device from `/data/status`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Device {
    pub name: String,
    pub kind: String,
    pub place: Option<String>,
    pub online: bool,
    /// Battery %, `None` = no battery (the Miniserver reports 127 for n/a)
    pub battery: Option<u32>,
    /// Air signal quality 0–4
    pub signal: Option<u8>,
    pub last_seen: Option<String>,
    pub firmware: Option<String>,
}

impl Device {
    /// Problem severity: 3 offline, 2 battery < 20 %, 1 weak signal, 0 fine.
    pub fn problem(&self) -> u8 {
        if !self.online {
            3
        } else if self.battery.is_some_and(|b| b < 20) {
            2
        } else if self.signal.is_some_and(|q| q <= 1) {
            1
        } else {
            0
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LogLevel {
    Info,
    Important,
    Warning,
    Error,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LogLine {
    pub time: String,
    pub level: LogLevel,
    pub text: String,
}

impl LogLine {
    /// Parse `YYYY-MM-DD HH:MM:SS.mmm;message`.
    pub fn parse(line: &str) -> Option<LogLine> {
        let line = line.trim_start_matches('\u{feff}').trim_end();
        if line.is_empty() {
            return None;
        }
        let (time, text) = match line.split_once(';') {
            Some((t, m)) if t.len() >= 19 && t.as_bytes()[4] == b'-' => (t.to_string(), m),
            _ => (String::new(), line),
        };
        let lower = text.to_lowercase();
        let level = if lower.starts_with("error") || lower.contains(" error ") {
            LogLevel::Error
        } else if lower.starts_with("warning") {
            LogLevel::Warning
        } else if lower.starts_with("important") {
            LogLevel::Important
        } else {
            LogLevel::Info
        };
        Some(LogLine {
            time,
            level,
            text: clean(text),
        })
    }
}

/// Parse the tail of `def.log` (bytes: UTF-8 with BOM and occasional garbage).
pub fn parse_log(bytes: &[u8], keep: usize) -> Vec<LogLine> {
    let text = String::from_utf8_lossy(bytes);
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(keep)..]
        .iter()
        .filter_map(|l| LogLine::parse(l))
        .collect()
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SiteStatus {
    pub name: String,
    pub host: String,
    pub online: bool,
    pub firmware: Option<String>,
    pub cpu: Option<f64>,
    pub heap_pct: Option<f64>,
    pub latency_ms: Option<u64>,
    pub error: Option<String>,
}

/// A time series: (unix seconds, value).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Series {
    pub points: Vec<(i64, f64)>,
}

impl Series {
    pub fn values(&self) -> Vec<f64> {
        self.points.iter().map(|p| p.1).collect()
    }
    /// Keep points newer than `since`.
    pub fn since(&self, since: i64) -> Series {
        Series {
            points: self
                .points
                .iter()
                .copied()
                .filter(|p| p.0 >= since)
                .collect(),
        }
    }
}

// ── Parsers for live responses ──────────────────────────────────────────────

/// `CPU:12 SPS:23 Cycles:99` → (cpu, sps). Older firmware returns a bare number.
pub fn parse_lastcpu(v: &str) -> (Option<f64>, Option<f64>) {
    let num = |key: &str| {
        v.split_whitespace()
            .find_map(|p| p.strip_prefix(key))
            .and_then(|n| n.trim_end_matches('%').parse::<f64>().ok())
    };
    let cpu = num("CPU:").or_else(|| v.trim().trim_end_matches('%').parse().ok());
    (cpu, num("SPS:"))
}

/// `359276/1016404kB` → (used, total) in kB.
pub fn parse_heap(v: &str) -> (Option<f64>, Option<f64>) {
    match v.split_once('/') {
        Some((u, t)) => (
            u.trim().parse().ok(),
            t.trim().trim_end_matches("kB").trim().parse().ok(),
        ),
        None => (None, None),
    }
}

/// A plain numeric counter; empty strings (Gen 2) become `None`.
pub fn parse_num(v: &str) -> Option<f64> {
    let v = v.trim();
    if v.is_empty() { None } else { v.parse().ok() }
}

/// Parse `/data/status` into devices (Tree, Air, network, extensions).
pub fn parse_status_xml(xml: &str) -> Vec<Device> {
    use quick_xml::Reader;
    use quick_xml::events::Event;
    fn attr(e: &quick_xml::events::BytesStart, name: &[u8]) -> Option<String> {
        e.attributes()
            .flatten()
            .find(|a| a.key.as_ref() == name)
            .map(|a| clean(&String::from_utf8_lossy(&a.value)))
            .filter(|s| !s.is_empty())
    }
    let mut out = Vec::new();
    let mut reader = Reader::from_str(xml);
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Empty(ref e)) | Ok(Event::Start(ref e)) => {
                let tag = e.name().as_ref().to_vec();
                let kind = match tag.as_slice() {
                    b"Extension" | b"GenericNetworkDevice" | b"AirDevice" => {
                        attr(e, b"Type").unwrap_or_else(|| String::from_utf8_lossy(&tag).into())
                    }
                    b"TreeDevice" => attr(e, b"Type").unwrap_or_else(|| "Tree device".into()),
                    b"TreeBranch" => "Tree branch".into(),
                    _ => {
                        buf.clear();
                        continue;
                    }
                };
                if tag == b"TreeBranch" {
                    buf.clear();
                    continue;
                }
                let online = attr(e, b"Online").is_none_or(|o| o == "true");
                let battery = attr(e, b"Battery")
                    .and_then(|b| b.parse::<u32>().ok())
                    .filter(|b| *b <= 100);
                let signal = attr(e, b"QualityDev")
                    .or_else(|| attr(e, b"QualityExt"))
                    .and_then(|q| q.parse::<u8>().ok());
                out.push(Device {
                    name: attr(e, b"Name").unwrap_or_else(|| kind.clone()),
                    kind,
                    place: attr(e, b"Place").or_else(|| attr(e, b"Room")),
                    online,
                    battery,
                    signal,
                    last_seen: attr(e, b"LastReceived"),
                    firmware: attr(e, b"Version"),
                });
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    out
}

/// Parse a statistics file (`/dev/fsget//stats/<uuid>.<YYYYMM>`) into
/// (unix time, first output value) points.
pub fn parse_stats(data: &[u8], num_outputs: usize, lox_epoch_unix: i64) -> Series {
    let entry = 8 + num_outputs.max(1) * 8;
    let off = crate::stats_data_offset(data, entry).unwrap_or(0);
    let mut points = Vec::new();
    let mut i = off;
    while i + entry <= data.len() {
        let ts = u32::from_le_bytes([data[i + 4], data[i + 5], data[i + 6], data[i + 7]]);
        let v = f64::from_le_bytes(data[i + 8..i + 16].try_into().unwrap_or([0; 8]));
        if v.is_finite() {
            points.push((lox_epoch_unix + ts as i64, v));
        }
        i += entry;
    }
    Series { points }
}

/// Average a series into today's 96 quarter-hours (local time); NaN = no data.
pub fn quarter_hours(points: &[(i64, f64)], tz: i64, now_unix: i64) -> Vec<f64> {
    let day_start = (now_unix + tz).div_euclid(86_400) * 86_400 - tz;
    let mut sum = [0.0f64; 96];
    let mut n = [0u32; 96];
    for (t, v) in points {
        if *t < day_start || *t >= day_start + 86_400 || !v.is_finite() {
            continue;
        }
        let q = ((t - day_start) / 900) as usize;
        sum[q] += v;
        n[q] += 1;
    }
    (0..96)
        .map(|i| {
            if n[i] > 0 {
                sum[i] / n[i] as f64
            } else {
                f64::NAN
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diag_values_real_shapes() {
        assert_eq!(
            parse_lastcpu("CPU:12 SPS:23 Cycles:99"),
            (Some(12.0), Some(23.0))
        );
        assert_eq!(parse_lastcpu("38%"), (Some(38.0), None));
        assert_eq!(
            parse_heap("359276/1016404kB"),
            (Some(359276.0), Some(1016404.0))
        );
        assert_eq!(parse_num(""), None);
        assert_eq!(parse_num("64"), Some(64.0));
    }

    #[test]
    fn status_battery_127_is_na() {
        let xml = r#"<Status><AirDevice Name="Contact" Type="Door &amp; Window Contact Air" Place="Bath" Online="true" Battery="8" QualityDev="3"/>
<AirDevice Name="Plug" Type="Smart Socket Air" Online="true" Battery="127" QualityExt="2"/>
<TreeBranch Name="Left"><TreeDevice Name="Touch" Online="false" LastReceived="2026-09-27 09:12"/></TreeBranch></Status>"#;
        let d = parse_status_xml(xml);
        assert_eq!(d.len(), 3);
        assert_eq!(d[0].battery, Some(8));
        assert_eq!(d[0].problem(), 2);
        assert_eq!(d[1].battery, None);
        assert!(!d[2].online);
        assert_eq!(d[2].problem(), 3);
    }

    #[test]
    fn log_lines_lossy_and_levels() {
        let mut bytes = b"\xef\xbb\xbf2026-09-27 06:00:02.114;Important 1045 Mode\n".to_vec();
        bytes.extend_from_slice(b"2026-09-27 06:00:03.000;Warning 503 bad \xff byte\n");
        bytes.extend_from_slice(b"2026-09-27 06:00:04.000;Error 29 NTP\x1b[31m\n");
        let l = parse_log(&bytes, 100);
        assert_eq!(l.len(), 3);
        assert_eq!(l[0].level, LogLevel::Important);
        assert_eq!(l[0].time, "2026-09-27 06:00:02.114");
        assert_eq!(l[1].level, LogLevel::Warning);
        assert_eq!(l[2].level, LogLevel::Error);
        assert!(!l[2].text.contains('\x1b'));
    }
}
