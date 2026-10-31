//! Loxone timestamps: seconds since 2009-01-01. Most count from local
//! midnight (statistics, the event table); a few from UTC (a user's
//! `lastEdit`, token validity).

/// The Loxone epoch, 2009-01-01T00:00:00Z, as a Unix timestamp.
pub const LOXONE_EPOCH_SECS: u64 = 1_230_768_000;

/// Loxone epoch: 2009-01-01 00:00:00 local time.
/// Statistics and event timestamps count seconds from it.
pub fn lox_epoch() -> chrono::DateTime<chrono::Local> {
    chrono::NaiveDate::from_ymd_opt(2009, 1, 1)
        .unwrap()
        .and_hms_opt(0, 0, 0)
        .unwrap()
        .and_local_timezone(chrono::Local)
        .unwrap()
}

/// Convert a Loxone timestamp (seconds since 2009-01-01) to a formatted string.
pub fn lox_timestamp_to_string(ts: i64) -> String {
    let dt = lox_epoch() + chrono::Duration::seconds(ts);
    dt.format("%Y-%m-%d %H:%M:%S").to_string()
}

/// Convert a Loxone timestamp counted from 2009-01-01 **UTC** (a user's
/// `lastEdit`) to local time.
pub fn lox_utc_timestamp_to_string(ts: i64) -> String {
    let unix = LOXONE_EPOCH_SECS as i64 + ts;
    match chrono::DateTime::from_timestamp(unix, 0) {
        Some(dt) => dt
            .with_timezone(&chrono::Local)
            .format("%Y-%m-%d %H:%M:%S")
            .to_string(),
        None => ts.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let ts = 1_793_290_920 - LOXONE_EPOCH_SECS as i64;
        let local = chrono::Local.timestamp_opt(1_793_290_920, 0).unwrap();
        assert_eq!(
            lox_utc_timestamp_to_string(ts),
            local.format("%Y-%m-%d %H:%M:%S").to_string()
        );
    }
}
