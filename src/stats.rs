//! The Miniserver's statistics files (`/stats/<uuid>.<period>`, the
//! binary format of `/binstatisticdata`) and its weather file
//! (`/data/weatheru.bin`).

use byteorder::{LittleEndian, ReadBytesExt};
use std::io::Cursor;

use crate::util::lox_timestamp_to_string;

/// A single parsed statistics entry.
pub(crate) struct StatsEntry {
    pub(crate) timestamp: String,
    pub(crate) values: Vec<f64>,
}

/// Parse binary statistics data (after header has been skipped).
/// `data` is the raw bytes starting from the first entry.
/// `num_outputs` is the number of f64 values per entry.
pub(crate) fn parse_stats_entries(data: &[u8], num_outputs: usize) -> Vec<StatsEntry> {
    let entry_size = 4 + 4 + num_outputs * 8; // UUID(4) + ts(4) + values
    let mut cursor = Cursor::new(data);
    let mut entries = Vec::new();
    while cursor.position() as usize + entry_size <= data.len() {
        let _uuid_part = cursor.read_u32::<LittleEndian>().unwrap_or(0);
        let ts = cursor.read_u32::<LittleEndian>().unwrap_or(0);
        let mut values = Vec::new();
        for _ in 0..num_outputs {
            values.push(cursor.read_f64::<LittleEndian>().unwrap_or(0.0));
        }
        entries.push(StatsEntry {
            timestamp: lox_timestamp_to_string(ts as i64),
            values,
        });
    }
    entries
}

/// Skip the binary stats file header and return the offset where entries begin.
/// Header format: u32 valueCount | u32 controlType | u32 nameLength | name bytes
/// Returns None if data is too short.
pub(crate) fn stats_data_offset(data: &[u8], entry_size: usize) -> Option<usize> {
    if data.len() <= 12 {
        return None;
    }
    let mut cursor = Cursor::new(data);
    let _value_count = cursor.read_u32::<LittleEndian>().ok()?;
    let _control_type = cursor.read_u32::<LittleEndian>().ok()?;
    let name_length = cursor.read_u32::<LittleEndian>().ok()?;
    let header_end = 12 + name_length as usize;
    Some(header_end.div_ceil(entry_size) * entry_size)
}

/// Determine the stats period string from day/month flags.
pub(crate) fn stats_period(day: Option<&str>, month: Option<&str>) -> String {
    if let Some(d) = day {
        d.replace('-', "")
    } else if let Some(m) = month {
        m.replace('-', "")
    } else {
        chrono::Local::now().format("%Y%m").to_string()
    }
}

/// Build the bare fsget path for a stats file.
pub(crate) fn stats_file_path(uuid: &str, period: &str) -> String {
    format!("/dev/fsget//stats/{}.{}", uuid, period)
}

/// Parse an fslist output to find matching stats files for a UUID and period.
pub(crate) fn find_stats_files<'a>(listing: &'a str, uuid: &str, period: &str) -> Vec<&'a str> {
    let suffix = format!(".{}", period);
    let mut files: Vec<&str> = listing
        .lines()
        .filter_map(|line| line.rsplit_once(' ').map(|(_, name)| name))
        .filter(|name| name.starts_with(uuid) && name.ends_with(suffix.as_str()))
        .collect();
    files.sort();
    files
}

pub(crate) fn parse_weather_entry(
    cursor: &mut Cursor<&[u8]>,
    lox_epoch: &chrono::DateTime<chrono::Local>,
) -> Option<serde_json::Value> {
    // Weather entry: 108 bytes total (mixed i32 + f64 fields)
    // Offset  0: u32 timestamp (seconds since Loxone epoch 2009-01-01)
    let ts = cursor.read_u32::<LittleEndian>().ok()?;
    // Offset  4: i32 weather type (picto-code)
    let _weather_type = cursor.read_i32::<LittleEndian>().ok()?;
    // Offset  8: i32 wind direction (degrees)
    let wind_dir = cursor.read_i32::<LittleEndian>().ok()?;
    // Offset 12: i32 solar radiation (UV index)
    let _radiation = cursor.read_i32::<LittleEndian>().ok()?;
    // Offset 16: i32 relative humidity (%)
    let humidity = cursor.read_i32::<LittleEndian>().ok()?;
    // Offset 20: f64 temperature (°C)
    let temperature = cursor.read_f64::<LittleEndian>().ok()?;
    // Offset 28: f64 felt temperature (°C)
    let _felt_temp = cursor.read_f64::<LittleEndian>().ok()?;
    // Offset 36: f64 dewpoint (°C)
    let _dewpoint = cursor.read_f64::<LittleEndian>().ok()?;
    // Offset 44: f64 precipitation (mm)
    let rain = cursor.read_f64::<LittleEndian>().ok()?;
    // Offset 52: f64 wind speed (m/s)
    let wind_speed = cursor.read_f64::<LittleEndian>().ok()?;
    // Offset 60: f64 barometric pressure (hPa)
    let pressure = cursor.read_f64::<LittleEndian>().ok()?;
    // Offset 68: i32 low clouds (%)
    let low_clouds = cursor.read_i32::<LittleEndian>().ok()?;
    // Offset 72: i32 medium clouds (%)
    let med_clouds = cursor.read_i32::<LittleEndian>().ok()?;
    // Offset 76: i32 high clouds (%)
    let high_clouds = cursor.read_i32::<LittleEndian>().ok()?;
    // Offset 80: i32 precipitation probability
    let _precip_prob = cursor.read_i32::<LittleEndian>().ok()?;
    // Offset 84: f64 absolute radiation
    let _abs_radiation = cursor.read_f64::<LittleEndian>().ok()?;
    // Offset 92: f64 snow fraction
    let _snow_fraction = cursor.read_f64::<LittleEndian>().ok()?;
    // Offset 100: f64 CAPE
    let _cape = cursor.read_f64::<LittleEndian>().ok()?;

    let clouds = ((low_clouds + med_clouds + high_clouds) as f64 / 3.0).round();
    let dt = *lox_epoch + chrono::Duration::seconds(ts as i64);
    Some(serde_json::json!({
        "timestamp": dt.format("%Y-%m-%d %H:%M").to_string(),
        "temperature": temperature,
        "humidity": humidity as f64,
        "wind_speed": wind_speed,
        "wind_direction": wind_dir as f64,
        "rain": rain,
        "pressure": pressure,
        "clouds": clouds,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::lox_epoch;

    #[test]
    fn test_parse_weather_entry() {
        use byteorder::{LittleEndian, WriteBytesExt};
        let epoch = lox_epoch();
        let mut data = Vec::new();
        // 108-byte weather entry: u32 ts + 4×i32 + 6×f64 + 4×i32 + 3×f64
        data.write_u32::<LittleEndian>(3600).unwrap(); // timestamp
        data.write_i32::<LittleEndian>(1).unwrap(); // weather type
        data.write_i32::<LittleEndian>(180).unwrap(); // wind direction
        data.write_i32::<LittleEndian>(5).unwrap(); // radiation
        data.write_i32::<LittleEndian>(65).unwrap(); // humidity
        data.write_f64::<LittleEndian>(21.5).unwrap(); // temperature
        data.write_f64::<LittleEndian>(19.0).unwrap(); // felt temp
        data.write_f64::<LittleEndian>(15.0).unwrap(); // dewpoint
        data.write_f64::<LittleEndian>(0.5).unwrap(); // precipitation
        data.write_f64::<LittleEndian>(10.0).unwrap(); // wind speed
        data.write_f64::<LittleEndian>(1013.0).unwrap(); // pressure
        data.write_i32::<LittleEndian>(30).unwrap(); // low clouds
        data.write_i32::<LittleEndian>(60).unwrap(); // med clouds
        data.write_i32::<LittleEndian>(90).unwrap(); // high clouds
        data.write_i32::<LittleEndian>(40).unwrap(); // precip probability
        data.write_f64::<LittleEndian>(500.0).unwrap(); // abs radiation
        data.write_f64::<LittleEndian>(0.0).unwrap(); // snow fraction
        data.write_f64::<LittleEndian>(100.0).unwrap(); // CAPE
        assert_eq!(data.len(), 108);
        let mut cursor = Cursor::new(data.as_slice());
        let entry = parse_weather_entry(&mut cursor, &epoch).unwrap();
        assert_eq!(entry["temperature"].as_f64().unwrap(), 21.5);
        assert_eq!(entry["humidity"].as_f64().unwrap(), 65.0);
        assert_eq!(entry["wind_speed"].as_f64().unwrap(), 10.0);
        assert_eq!(entry["wind_direction"].as_f64().unwrap(), 180.0);
        assert_eq!(entry["rain"].as_f64().unwrap(), 0.5);
        assert_eq!(entry["pressure"].as_f64().unwrap(), 1013.0);
        assert_eq!(entry["clouds"].as_f64().unwrap(), 60.0); // avg(30,60,90)
    }

    /// Full round-trip: build a binary stats file with header + aligned
    /// entries and verify we parse the correct timestamps and values
    /// using the extracted helper functions.
    #[test]
    fn test_stats_file_parse_with_header() {
        use byteorder::{LittleEndian, WriteBytesExt};
        let mut data = Vec::new();
        // Header: valueCount=1 (with version flag), controlType=0, nameLength=3
        data.write_u32::<LittleEndian>(0x8000_0001).unwrap();
        data.write_u32::<LittleEndian>(0).unwrap();
        data.write_u32::<LittleEndian>(3).unwrap(); // "CO2"
        data.extend_from_slice(b"CO2");
        // Pad to 16 bytes (entry_size alignment)
        data.push(0); // null terminator / padding to reach 16
        assert_eq!(data.len(), 16);
        // Entry 1: uuid_prefix(4) + ts(4) + value(8) = 16 bytes
        data.write_u32::<LittleEndian>(0x03b498a5).unwrap(); // uuid prefix
        data.write_u32::<LittleEndian>(541_293_056).unwrap(); // ~2026-03-01
        data.write_f64::<LittleEndian>(985.2).unwrap();
        // Entry 2
        data.write_u32::<LittleEndian>(0x03b498a5).unwrap();
        data.write_u32::<LittleEndian>(541_296_656).unwrap(); // +1 hour
        data.write_f64::<LittleEndian>(1215.1).unwrap();

        let num_outputs = 1;
        let entry_size = 4 + 4 + num_outputs * 8;
        let offset = stats_data_offset(&data, entry_size).unwrap();
        assert_eq!(offset, 16);
        let entries = parse_stats_entries(&data[offset..], num_outputs);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].values[0], 985.2);
        assert_eq!(entries[1].values[0], 1215.1);
    }

    #[test]
    fn test_stats_period_day() {
        assert_eq!(stats_period(Some("2026-03-15"), None), "20260315");
    }

    #[test]
    fn test_stats_period_month() {
        assert_eq!(stats_period(None, Some("2026-03")), "202603");
    }

    #[test]
    fn test_stats_period_default() {
        // Default returns current month in YYYYMM format
        let p = stats_period(None, None);
        assert_eq!(p.len(), 6);
        assert!(p.chars().all(|c| c.is_ascii_digit()));
    }

    #[test]
    fn test_stats_file_path() {
        assert_eq!(
            stats_file_path("abc-123", "202603"),
            "/dev/fsget//stats/abc-123.202603"
        );
    }

    #[test]
    fn test_find_stats_files_basic() {
        let listing = "123456 abc-123.202603\n789012 abc-123_1.202603\n345678 other.202603";
        let files = find_stats_files(listing, "abc-123", "202603");
        assert_eq!(files, vec!["abc-123.202603", "abc-123_1.202603"]);
    }

    #[test]
    fn test_find_stats_files_no_match() {
        let listing = "123456 other.202603";
        let files = find_stats_files(listing, "abc-123", "202603");
        assert!(files.is_empty());
    }

    #[test]
    fn test_stats_data_offset_short_name() {
        use byteorder::{LittleEndian, WriteBytesExt};
        let mut data = Vec::new();
        data.write_u32::<LittleEndian>(0x8000_0001).unwrap();
        data.write_u32::<LittleEndian>(0).unwrap();
        data.write_u32::<LittleEndian>(3).unwrap(); // name_length=3
        data.extend_from_slice(b"CO2\0"); // pad to 16
        let entry_size = 4 + 4 + 8;
        assert_eq!(stats_data_offset(&data, entry_size), Some(16));
    }

    #[test]
    fn test_stats_data_offset_too_short() {
        let data = vec![0u8; 10]; // Less than 12 bytes
        assert_eq!(stats_data_offset(&data, 16), None);
    }

    #[test]
    fn test_parse_stats_entries_empty() {
        let entries = parse_stats_entries(&[], 1);
        assert!(entries.is_empty());
    }

    #[test]
    fn test_parse_stats_entries_single_output() {
        use byteorder::{LittleEndian, WriteBytesExt};
        let mut data = Vec::new();
        data.write_u32::<LittleEndian>(0xAABBCCDD).unwrap(); // uuid prefix
        data.write_u32::<LittleEndian>(1000).unwrap();
        data.write_f64::<LittleEndian>(23.5).unwrap();
        let entries = parse_stats_entries(&data, 1);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].values, vec![23.5]);
    }

    #[test]
    fn test_parse_stats_entries_multi_output() {
        use byteorder::{LittleEndian, WriteBytesExt};
        let mut data = Vec::new();
        data.write_u32::<LittleEndian>(0xAABBCCDD).unwrap();
        data.write_u32::<LittleEndian>(5000).unwrap();
        data.write_f64::<LittleEndian>(100.0).unwrap();
        data.write_f64::<LittleEndian>(200.0).unwrap();
        data.write_f64::<LittleEndian>(300.0).unwrap();
        let entries = parse_stats_entries(&data, 3);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].values, vec![100.0, 200.0, 300.0]);
    }
}
