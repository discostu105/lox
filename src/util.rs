//! Small helpers shared across lox's modules: Loxone value and timestamp
//! formats, path encoding, and text formatting for terminal output.

use serde_json::Value;

pub(crate) fn xml_attr<'a>(xml: &'a str, attr: &str) -> Option<&'a str> {
    // XML format: attr="value"
    let xml_key = format!("{}=\"", attr);
    let mut search_from = 0;
    while let Some(pos) = xml[search_from..].find(&xml_key) {
        let abs_pos = search_from + pos;
        if abs_pos == 0 || matches!(xml.as_bytes()[abs_pos - 1], b' ' | b'<' | b'\t' | b'\n') {
            let start = abs_pos + xml_key.len();
            let end = xml[start..].find('"')? + start;
            return Some(&xml[start..end]);
        }
        search_from = abs_pos + 1;
    }

    // JSON format: "attr": value  (handles /jdev/ responses)
    let json_key = format!("\"{}\": ", attr);
    if let Some(pos) = xml.find(&json_key) {
        let start = pos + json_key.len();
        let rest = &xml[start..];
        if let Some(stripped) = rest.strip_prefix('"') {
            // String value: extract between quotes
            let end = stripped.find('"')?;
            return Some(&stripped[..end]);
        }
        // Numeric/other value: read until delimiter
        let end = rest.find([',', '}', ']']).unwrap_or(rest.len());
        let val = rest[..end].trim();
        if !val.is_empty() {
            return Some(val);
        }
    }

    None
}

/// Extract a JSON value as a display string, handling both string and numeric types.
/// Gen2 Miniservers return numeric values (e.g. `"Code": 200`) instead of strings
/// (`"Code": "200"`), so `.as_str()` alone is insufficient.
pub(crate) fn json_val_str(v: &Value) -> Option<String> {
    v.as_str()
        .map(|s| s.to_string())
        .or_else(|| v.as_i64().map(|n| n.to_string()))
        .or_else(|| v.as_f64().map(|n| n.to_string()))
}

pub(crate) fn bar(v: f64, max: f64) -> String {
    let n = ((v / max * 20.0) as usize).min(20);
    format!(
        "[{}{}] {:.0}%",
        "█".repeat(n),
        "░".repeat(20 - n),
        v / max * 100.0
    )
}

pub(crate) fn kb_fmt(kb: f64) -> String {
    if kb > 1024.0 {
        format!("{:.0} MB", kb / 1024.0)
    } else {
        format!("{:.0} kB", kb)
    }
}

/// The current local time as a ZIP timestamp, for entries staged new
/// (ZIP times are local).
pub(crate) fn zip_now() -> lxir::archive::ZipTime {
    use chrono::{Datelike, Timelike};
    let now = chrono::Local::now().naive_local();
    lxir::archive::ZipTime {
        year: u16::try_from(now.year()).unwrap_or(1980),
        month: now.month() as u8,
        day: now.day() as u8,
        hour: now.hour() as u8,
        minute: now.minute() as u8,
        second: now.second() as u8,
    }
}

/// Loxone epoch: 2009-01-01 00:00:00 local time.
/// All Loxone timestamps are seconds since this epoch.
pub(crate) fn lox_epoch() -> chrono::DateTime<chrono::Local> {
    chrono::NaiveDate::from_ymd_opt(2009, 1, 1)
        .unwrap()
        .and_hms_opt(0, 0, 0)
        .unwrap()
        .and_local_timezone(chrono::Local)
        .unwrap()
}

/// Convert a Loxone timestamp (seconds since 2009-01-01) to a formatted string.
pub(crate) fn lox_timestamp_to_string(ts: i64) -> String {
    let dt = lox_epoch() + chrono::Duration::seconds(ts);
    dt.format("%Y-%m-%d %H:%M:%S").to_string()
}

/// Convert a Loxone timestamp counted from 2009-01-01 **UTC** (a user's
/// `lastEdit`) to local time.
pub(crate) fn lox_utc_timestamp_to_string(ts: i64) -> String {
    let unix = crate::client::LOXONE_EPOCH_SECS as i64 + ts;
    match chrono::DateTime::from_timestamp(unix, 0) {
        Some(dt) => dt
            .with_timezone(&chrono::Local)
            .format("%Y-%m-%d %H:%M:%S")
            .to_string(),
        None => ts.to_string(),
    }
}

/// Normalize a filesystem path for Loxone fsget/fslist API.
/// Ensures the path starts with '/' so the double-slash URL
/// (e.g. /dev/fsget//path) is correct.
pub(crate) fn abs_path(p: &str) -> String {
    if p.starts_with('/') {
        p.to_string()
    } else {
        format!("/{p}")
    }
}

pub(crate) fn now_hms() -> String {
    use chrono::Timelike;
    let now = chrono::Local::now();
    format!("{:02}:{:02}:{:02}", now.hour(), now.minute(), now.second())
}

pub(crate) fn rgb_to_hsv(r: u8, g: u8, b: u8) -> (u16, u16, u16) {
    let rf = r as f64 / 255.0;
    let gf = g as f64 / 255.0;
    let bf = b as f64 / 255.0;
    let max = rf.max(gf).max(bf);
    let min = rf.min(gf).min(bf);
    let delta = max - min;
    let h = if delta == 0.0 {
        0.0
    } else if max == rf {
        60.0 * (((gf - bf) / delta) % 6.0)
    } else if max == gf {
        60.0 * (((bf - rf) / delta) + 2.0)
    } else {
        60.0 * (((rf - gf) / delta) + 4.0)
    };
    let h = if h < 0.0 { h + 360.0 } else { h };
    let s = if max == 0.0 { 0.0 } else { delta / max * 100.0 };
    let v = max * 100.0;
    (h.round() as u16, s.round() as u16, v.round() as u16)
}

/// Percent-encode characters that would corrupt an HTTP path segment.
/// Does NOT encode '/' so that Loxone command separators (e.g. "manualPosition/50") pass through.
pub(crate) fn encode_path_value(s: &str) -> String {
    s.chars()
        .flat_map(|c| match c {
            '%' => vec!['%', '2', '5'],
            ' ' => vec!['%', '2', '0'],
            '#' => vec!['%', '2', '3'],
            '?' => vec!['%', '3', 'F'],
            '+' => vec!['%', '2', 'B'],
            c => vec![c],
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_json_val_str_string() {
        let v = serde_json::json!("200");
        assert_eq!(json_val_str(&v), Some("200".to_string()));
    }

    #[test]
    fn test_json_val_str_integer() {
        let v = serde_json::json!(200);
        assert_eq!(json_val_str(&v), Some("200".to_string()));
    }

    #[test]
    fn test_json_val_str_float() {
        let v = serde_json::json!(21.5);
        assert_eq!(json_val_str(&v), Some("21.5".to_string()));
    }

    #[test]
    fn test_json_val_str_null() {
        let v = serde_json::json!(null);
        assert_eq!(json_val_str(&v), None);
    }

    #[test]
    fn test_encode_path_value_plain() {
        assert_eq!(encode_path_value("on"), "on");
        assert_eq!(encode_path_value("manualPosition/50"), "manualPosition/50");
    }

    #[test]
    fn test_encode_path_value_space() {
        assert_eq!(encode_path_value("hello world"), "hello%20world");
    }

    #[test]
    fn test_encode_path_value_percent() {
        assert_eq!(encode_path_value("50%"), "50%25");
    }

    #[test]
    fn test_encode_path_value_plus() {
        assert_eq!(encode_path_value("a+b"), "a%2Bb");
    }

    #[test]
    fn test_rgb_to_hsv_red() {
        let (h, s, v) = rgb_to_hsv(255, 0, 0);
        assert_eq!(h, 0);
        assert_eq!(s, 100);
        assert_eq!(v, 100);
    }

    #[test]
    fn test_rgb_to_hsv_green() {
        let (h, s, v) = rgb_to_hsv(0, 255, 0);
        assert_eq!(h, 120);
        assert_eq!(s, 100);
        assert_eq!(v, 100);
    }

    #[test]
    fn test_rgb_to_hsv_blue() {
        let (h, s, v) = rgb_to_hsv(0, 0, 255);
        assert_eq!(h, 240);
        assert_eq!(s, 100);
        assert_eq!(v, 100);
    }

    #[test]
    fn test_rgb_to_hsv_white() {
        let (h, s, v) = rgb_to_hsv(255, 255, 255);
        assert_eq!(h, 0);
        assert_eq!(s, 0);
        assert_eq!(v, 100);
    }

    #[test]
    fn test_rgb_to_hsv_black() {
        let (h, s, v) = rgb_to_hsv(0, 0, 0);
        assert_eq!(h, 0);
        assert_eq!(s, 0);
        assert_eq!(v, 0);
    }

    #[test]
    fn test_xml_attr_basic() {
        let xml = r#"<LL control="test" value="42" Code="200"/>"#;
        assert_eq!(xml_attr(xml, "value"), Some("42"));
        assert_eq!(xml_attr(xml, "Code"), Some("200"));
        assert_eq!(xml_attr(xml, "control"), Some("test"));
    }

    #[test]
    fn test_xml_attr_missing() {
        let xml = r#"<LL control="test" value="42"/>"#;
        assert_eq!(xml_attr(xml, "missing"), None);
    }

    #[test]
    fn test_xml_attr_empty_value() {
        let xml = r#"<LL value=""/>"#;
        assert_eq!(xml_attr(xml, "value"), Some(""));
    }

    #[test]
    fn test_xml_attr_spaces_in_value() {
        let xml = r#"<LL value="hello world"/>"#;
        assert_eq!(xml_attr(xml, "value"), Some("hello world"));
    }

    #[test]
    fn test_xml_attr_no_partial_name_match() {
        // "value" must NOT match "value2" — word boundary check
        let xml = r#"<LL value2="wrong" value="right"/>"#;
        assert_eq!(xml_attr(xml, "value"), Some("right"));
    }

    #[test]
    fn test_xml_attr_partial_name_only_match() {
        // When only value2 exists, looking for "value" returns None
        let xml = r#"<LL value2="wrong"/>"#;
        assert_eq!(xml_attr(xml, "value"), None);
    }

    #[test]
    fn test_xml_attr_json_format() {
        // /jdev/ endpoints return JSON instead of XML
        let json =
            r#"{"LL": { "control": "dev/sys/lastcpu", "value": "CPU:12 SPS:26", "Code": "200"}}"#;
        assert_eq!(xml_attr(json, "value"), Some("CPU:12 SPS:26"));
        assert_eq!(xml_attr(json, "Code"), Some("200"));
        assert_eq!(xml_attr(json, "control"), Some("dev/sys/lastcpu"));
    }

    #[test]
    fn test_xml_attr_json_empty_value() {
        let json = r#"{"LL": { "control": "dev/sys/ints", "value": "", "Code": "200"}}"#;
        assert_eq!(xml_attr(json, "value"), Some(""));
    }

    #[test]
    fn test_xml_attr_json_numeric_value() {
        // Gen2 Miniservers return numeric values without quotes for counters
        let json =
            r#"{"LL": { "control": "dev/sys/contextswitches", "value": 123456789, "Code": "200"}}"#;
        assert_eq!(xml_attr(json, "value"), Some("123456789"));
        assert_eq!(xml_attr(json, "Code"), Some("200"));
    }

    #[test]
    fn test_xml_attr_json_numeric_last_field() {
        // Numeric value as the last field before closing brace
        let json = r#"{"LL": { "value": 42}}"#;
        assert_eq!(xml_attr(json, "value"), Some("42"));
    }

    #[test]
    fn test_xml_attr_json_float_value() {
        let json = r#"{"LL": { "value": 3.14, "Code": "200"}}"#;
        assert_eq!(xml_attr(json, "value"), Some("3.14"));
    }

    #[test]
    fn test_xml_attr_json_negative_value() {
        let json = r#"{"LL": { "value": -1, "Code": "200"}}"#;
        assert_eq!(xml_attr(json, "value"), Some("-1"));
    }

    #[test]
    fn test_xml_attr_json_zero_value() {
        let json = r#"{"LL": { "value": 0, "Code": "200"}}"#;
        assert_eq!(xml_attr(json, "value"), Some("0"));
    }

    #[test]
    fn test_xml_attr_json_bool_value() {
        // Unlikely from Miniserver but should not panic
        let json = r#"{"LL": { "value": true, "Code": "200"}}"#;
        assert_eq!(xml_attr(json, "value"), Some("true"));
    }

    #[test]
    fn test_xml_attr_json_null_value() {
        let json = r#"{"LL": { "value": null, "Code": "200"}}"#;
        assert_eq!(xml_attr(json, "value"), Some("null"));
    }

    /// Exercises xml_attr against every response format the Miniserver uses
    /// for the diagnostic endpoints exposed by `lox status --diag/--net/--bus/--lan`.
    /// This is a regression test: Gen2 Miniservers return numeric JSON values
    /// for counters, which the original string-only parser missed.
    #[test]
    fn test_xml_attr_all_diagnostic_response_formats() {
        // Gen1 XML format (most endpoints)
        let xml = r#"<LL control="dev/sys/heap" value="352880/1016404kB" Code="200"/>"#;
        assert_eq!(xml_attr(xml, "value"), Some("352880/1016404kB"));

        // Gen2 JSON with string value (lastcpu, numtasks, sdtest)
        let json = r#"{"LL": { "control": "dev/sys/lastcpu", "value": "CPU:11 SPS:24 Cycles:99", "Code": "200"}}"#;
        assert_eq!(xml_attr(json, "value"), Some("CPU:11 SPS:24 Cycles:99"));

        // Gen2 JSON with numeric value (ints, comints, contextswitches, contextswitchesi)
        for endpoint in [
            "dev/sys/contextswitches",
            "dev/sys/ints",
            "dev/sys/comints",
            "dev/sys/contextswitchesi",
        ] {
            let json = format!(
                r#"{{"LL": {{ "control": "{}", "value": 987654, "Code": "200"}}}}"#,
                endpoint
            );
            assert_eq!(
                xml_attr(&json, "value"),
                Some("987654"),
                "failed for endpoint {}",
                endpoint
            );
        }

        // Gen2 JSON with numeric value for bus/lan counters
        for endpoint in [
            "bus/packetssent",
            "bus/packetsreceived",
            "bus/parityerrors",
            "lan/txp",
            "lan/txu",
            "lan/exh",
            "lan/nob",
        ] {
            let json = format!(
                r#"{{"LL": {{ "control": "{}", "value": 42, "Code": "200"}}}}"#,
                endpoint
            );
            assert_eq!(
                xml_attr(&json, "value"),
                Some("42"),
                "failed for endpoint {}",
                endpoint
            );
        }

        // Gen2 JSON with string value for network config
        let json = r#"{"LL": { "control": "cfg/dns2", "value": "8.8.4.4", "Code": "200"}}"#;
        assert_eq!(xml_attr(json, "value"), Some("8.8.4.4"));
    }

    #[test]
    fn test_bar_zero() {
        let s = bar(0.0, 100.0);
        assert!(s.starts_with("[░"));
        assert!(s.ends_with("0%"));
    }

    #[test]
    fn test_bar_full() {
        let s = bar(100.0, 100.0);
        assert!(s.starts_with("[████████████████████]"));
        assert!(s.ends_with("100%"));
    }

    #[test]
    fn test_bar_half() {
        let s = bar(50.0, 100.0);
        assert!(s.contains("50%"));
    }

    #[test]
    fn test_kb_fmt_kb() {
        assert_eq!(kb_fmt(512.0), "512 kB");
    }

    #[test]
    fn test_kb_fmt_mb() {
        assert_eq!(kb_fmt(2048.0), "2 MB");
    }

    #[test]
    fn test_kb_fmt_boundary() {
        // At exactly 1024, stays in kB
        assert_eq!(kb_fmt(1024.0), "1024 kB");
    }

    #[test]
    fn test_lox_epoch_is_2009() {
        let e = lox_epoch();
        assert_eq!(e.format("%Y-%m-%d").to_string(), "2009-01-01");
    }

    #[test]
    fn test_lox_timestamp_to_string() {
        // 0 seconds = epoch itself
        let s = lox_timestamp_to_string(0);
        assert!(s.starts_with("2009-01-01"));
    }

    #[test]
    fn test_lox_utc_timestamp_is_shown_in_local_time() {
        use chrono::TimeZone;
        // 2026-10-29 16:22:00 UTC, as a user's lastEdit
        let ts = 1_793_290_920 - crate::client::LOXONE_EPOCH_SECS as i64;
        let local = chrono::Local.timestamp_opt(1_793_290_920, 0).unwrap();
        assert_eq!(
            lox_utc_timestamp_to_string(ts),
            local.format("%Y-%m-%d %H:%M:%S").to_string()
        );
    }

    #[test]
    fn test_abs_path_with_leading_slash() {
        assert_eq!(abs_path("/temp/file.img"), "/temp/file.img");
    }

    #[test]
    fn test_abs_path_without_leading_slash() {
        assert_eq!(abs_path("temp/file.img"), "/temp/file.img");
    }

    #[test]
    fn test_abs_path_root() {
        assert_eq!(abs_path("/"), "/");
    }
}
