//! The Miniserver's SD card self-test (`/jdev/sys/sdtest`): parsing its
//! free-text response into fields.

use serde::Serialize;

use crate::client::LoxClient;
use crate::error::Result;

/// Parsed fields from a Miniserver `/jdev/sys/sdtest` response.
///
/// Example raw value:
/// ```text
/// SD Performance: Read: 540kB/s, Write: 370kB/s, No error (0 0),
/// ManufacturerId: 27, Date 2014/2, CardType 2, BlockSize 512, Erase 0,
/// MaxTransferRate 25000000, RwFactor 2, ReadSpeed 22222222Hz,
/// WriteSpeed 22222222Hz, MaxReadCurrentVDDmin 3, MaxReadCurrentVDDmax 5,
/// MaxWriteCurrentVDDmin: 3, MaxWriteCurrentVDDmax: 1, Usage: 2.79%
/// ```
///
/// Serializes like [`to_json`](Self::to_json): the speeds as
/// `read_speed_kbs`/`write_speed_kbs`, missing fields left out.
#[derive(Debug, Clone, Default, Serialize)]
pub struct SdTestResult {
    #[serde(rename = "read_speed_kbs", skip_serializing_if = "Option::is_none")]
    pub read_speed_kb: Option<f64>,
    #[serde(rename = "write_speed_kbs", skip_serializing_if = "Option::is_none")]
    pub write_speed_kb: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_type: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_count: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manufacturer_id: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub card_type: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub block_size: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage_pct: Option<f64>,
    pub raw: String,
}

impl SdTestResult {
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or_default()
    }
}

impl LoxClient {
    /// Run the SD card self-test (`/jdev/sys/sdtest`). It reads and writes
    /// the card: don't call it often.
    pub fn sdtest(&self) -> Result<SdTestResult> {
        Ok(parse_sdtest(&self.value("/jdev/sys/sdtest")?))
    }
}

/// Parse the raw value string from `/jdev/sys/sdtest` into structured fields.
pub fn parse_sdtest(raw: &str) -> SdTestResult {
    let mut result = SdTestResult {
        raw: raw.to_string(),
        ..Default::default()
    };

    // Read: NNNkB/s
    if let Some(pos) = raw.find("Read:") {
        let rest = &raw[pos + 5..];
        let rest = rest.trim_start();
        if let Some(end) = rest.find(|c: char| !c.is_ascii_digit() && c != '.')
            && let Ok(v) = rest[..end].parse::<f64>()
        {
            let unit_area = &rest[end..];
            if unit_area.starts_with("MB/s") {
                result.read_speed_kb = Some(v * 1024.0);
            } else {
                result.read_speed_kb = Some(v);
            }
        }
    }

    // Write: NNNkB/s
    if let Some(pos) = raw.find("Write:") {
        let rest = &raw[pos + 6..];
        let rest = rest.trim_start();
        if let Some(end) = rest.find(|c: char| !c.is_ascii_digit() && c != '.')
            && let Ok(v) = rest[..end].parse::<f64>()
        {
            let unit_area = &rest[end..];
            if unit_area.starts_with("MB/s") {
                result.write_speed_kb = Some(v * 1024.0);
            } else {
                result.write_speed_kb = Some(v);
            }
        }
    }

    // Error status: "No error (0 0)" or "error (1 1048576)"
    if let Some(pos) = raw.find('(') {
        let rest = &raw[pos + 1..];
        if let Some(end) = rest.find(')') {
            let nums: Vec<&str> = rest[..end].split_whitespace().collect();
            if nums.len() == 2 {
                result.error_type = nums[0].parse().ok();
                result.error_count = nums[1].parse().ok();
            }
        }
    }

    // ManufacturerId: NN
    if let Some(pos) = raw.find("ManufacturerId:") {
        let rest = raw[pos + 15..].trim_start();
        let end = rest
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(rest.len());
        result.manufacturer_id = rest[..end].parse().ok();
    }

    // Date YYYY/M
    if let Some(pos) = raw.find("Date") {
        let rest = raw[pos + 4..].trim_start();
        // May start with ':' or not
        let rest = rest.strip_prefix(':').unwrap_or(rest).trim_start();
        let end = rest
            .find(|c: char| !c.is_ascii_digit() && c != '/')
            .unwrap_or(rest.len());
        let date_str = rest[..end].trim();
        if !date_str.is_empty() {
            result.date = Some(date_str.to_string());
        }
    }

    // CardType N
    if let Some(pos) = raw.find("CardType") {
        let rest = raw[pos + 8..].trim_start();
        let rest = rest.strip_prefix(':').unwrap_or(rest).trim_start();
        let end = rest
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(rest.len());
        result.card_type = rest[..end].parse().ok();
    }

    // BlockSize NNN
    if let Some(pos) = raw.find("BlockSize") {
        let rest = raw[pos + 9..].trim_start();
        let rest = rest.strip_prefix(':').unwrap_or(rest).trim_start();
        let end = rest
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(rest.len());
        result.block_size = rest[..end].parse().ok();
    }

    // Usage: N.NN%
    if let Some(pos) = raw.find("Usage:") {
        let rest = raw[pos + 6..].trim_start();
        let end = rest
            .find(|c: char| !c.is_ascii_digit() && c != '.')
            .unwrap_or(rest.len());
        result.usage_pct = rest[..end].parse().ok();
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_sdtest_full() {
        let raw = "SD Performance: Read: 540kB/s, Write: 370kB/s, No error (0 0), \
                   ManufacturerId: 27, Date 2014/2, CardType 2, BlockSize 512, Erase 0, \
                   MaxTransferRate 25000000, RwFactor 2, ReadSpeed 22222222Hz, \
                   WriteSpeed 22222222Hz, MaxReadCurrentVDDmin 3, MaxReadCurrentVDDmax 5, \
                   MaxWriteCurrentVDDmin: 3, MaxWriteCurrentVDDmax: 1, Usage: 2.79%";
        let sd = parse_sdtest(raw);
        assert_eq!(sd.read_speed_kb, Some(540.0));
        assert_eq!(sd.write_speed_kb, Some(370.0));
        assert_eq!(sd.error_type, Some(0));
        assert_eq!(sd.error_count, Some(0));
        assert_eq!(sd.manufacturer_id, Some(27));
        assert_eq!(sd.date.as_deref(), Some("2014/2"));
        assert_eq!(sd.card_type, Some(2));
        assert_eq!(sd.block_size, Some(512));
        assert!((sd.usage_pct.unwrap() - 2.79).abs() < 0.01);
    }

    #[test]
    fn test_parse_sdtest_error_card() {
        let raw = "SD Performance: Read: 87381kB/s, Write: 2818kB/s, error (1 1048576), \
                   ManufacturerId: 3, Date 2019/7, CardType 2, BlockSize 512, Usage: 15.20%";
        let sd = parse_sdtest(raw);
        assert_eq!(sd.read_speed_kb, Some(87381.0));
        assert_eq!(sd.write_speed_kb, Some(2818.0));
        assert_eq!(sd.error_type, Some(1));
        assert_eq!(sd.error_count, Some(1048576));
        assert_eq!(sd.manufacturer_id, Some(3));
        assert_eq!(sd.card_type, Some(2));
        assert!((sd.usage_pct.unwrap() - 15.2).abs() < 0.01);
    }

    #[test]
    fn test_parse_sdtest_mb_per_sec() {
        let raw = "SD Performance: Read: 30MB/s, Write: 15MB/s, No error (0 0), Usage: 5.00%";
        let sd = parse_sdtest(raw);
        assert_eq!(sd.read_speed_kb, Some(30.0 * 1024.0));
        assert_eq!(sd.write_speed_kb, Some(15.0 * 1024.0));
    }

    #[test]
    fn test_parse_sdtest_fallback() {
        let raw = "unknown format";
        let sd = parse_sdtest(raw);
        assert_eq!(sd.raw, "unknown format");
        assert!(sd.read_speed_kb.is_none());
        assert!(sd.write_speed_kb.is_none());
        assert!(sd.error_type.is_none());
        assert!(sd.usage_pct.is_none());
    }

    #[test]
    fn test_parse_sdtest_partial() {
        let raw = "SD Performance: Read: 100kB/s, Write: 50kB/s, No error (0 0)";
        let sd = parse_sdtest(raw);
        assert_eq!(sd.read_speed_kb, Some(100.0));
        assert_eq!(sd.write_speed_kb, Some(50.0));
        assert_eq!(sd.error_type, Some(0));
        assert!(sd.manufacturer_id.is_none());
        assert!(sd.usage_pct.is_none());
    }

    #[test]
    fn test_parse_sdtest_question_mark() {
        let sd = parse_sdtest("?");
        assert_eq!(sd.raw, "?");
        assert!(sd.read_speed_kb.is_none());
    }

    #[test]
    fn test_sdtest_to_json() {
        let sd = SdTestResult {
            read_speed_kb: Some(540.0),
            write_speed_kb: Some(370.0),
            error_type: Some(0),
            error_count: Some(0),
            usage_pct: Some(2.79),
            raw: "test".to_string(),
            ..Default::default()
        };
        let json = sd.to_json();
        assert_eq!(json["read_speed_kbs"], 540.0);
        assert_eq!(json["write_speed_kbs"], 370.0);
        assert_eq!(json["error_type"], 0);
        assert_eq!(json["error_count"], 0);
        assert_eq!(json["usage_pct"], 2.79);
        assert_eq!(json["raw"], "test");
        assert!(json.get("manufacturer_id").is_none());
    }
}
