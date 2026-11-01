//! The simulated wall clock: local time as seconds since 1970-01-01
//! 00:00, no time zone (the Miniserver's clock is local time too).

/// Where every simulation starts unless a `clock` statement says
/// otherwise: 2026-01-01 00:00, a Thursday.
pub const DEFAULT_START: f64 = 1_767_225_600.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Clock {
    /// Seconds since 1970-01-01 00:00 local time.
    pub secs: f64,
}

impl Default for Clock {
    fn default() -> Self {
        Clock {
            secs: DEFAULT_START,
        }
    }
}

impl Clock {
    /// Set the clock from `"HH:MM[:SS]"` (same day) or
    /// `"YYYY-MM-DD HH:MM[:SS]"`.
    pub fn set(&mut self, spec: &str) -> Result<(), String> {
        let (date, time) = match spec.trim().split_once(' ') {
            Some((d, t)) => (Some(d), t.trim()),
            None => (None, spec.trim()),
        };
        let day = match date {
            Some(d) => {
                let parts: Vec<&str> = d.split('-').collect();
                let [y, m, dd] = parts.as_slice() else {
                    return Err(format!("clock {spec}: expected YYYY-MM-DD"));
                };
                let num = |s: &str| {
                    s.parse::<i64>()
                        .map_err(|_| format!("clock {spec}: `{s}` is not a number"))
                };
                let (y, m, dd) = (num(y)?, num(m)?, num(dd)?);
                if !(1..=12).contains(&m) || !(1..=31).contains(&dd) {
                    return Err(format!("clock {spec}: no such date"));
                }
                days_from_civil(y, m, dd)
            }
            None => self.day(),
        };
        let fields: Vec<&str> = time.split(':').collect();
        if !matches!(fields.len(), 2 | 3) {
            return Err(format!("clock {spec}: expected HH:MM or HH:MM:SS"));
        }
        let mut hms = [0i64; 3];
        for (slot, f) in hms.iter_mut().zip(&fields) {
            *slot = f
                .parse()
                .map_err(|_| format!("clock {spec}: `{f}` is not a number"))?;
        }
        if hms[0] > 23 || hms[1] > 59 || hms[2] > 59 {
            return Err(format!("clock {spec}: no such time of day"));
        }
        self.secs = (day * 86_400 + hms[0] * 3600 + hms[1] * 60 + hms[2]) as f64;
        Ok(())
    }

    /// Days since 1970-01-01.
    pub fn day(&self) -> i64 {
        (self.secs / 86_400.0).floor() as i64
    }

    /// Minutes since midnight, `0..1440`.
    pub fn minute_of_day(&self) -> f64 {
        (self.secs - self.day() as f64 * 86_400.0) / 60.0
    }

    /// ISO weekday: Monday 1 … Sunday 7.
    pub fn weekday(&self) -> u8 {
        // 1970-01-01 was a Thursday (4).
        ((self.day() + 3).rem_euclid(7) + 1) as u8
    }
}

/// Days since 1970-01-01 for a proleptic Gregorian date (Howard
/// Hinnant's `days_from_civil`).
pub fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The date `(year, month, day)` of a day since 1970-01-01 (Howard
/// Hinnant's `civil_from_days`).
pub fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_start_is_new_year_2026_a_thursday() {
        let c = Clock::default();
        assert_eq!(c.day(), days_from_civil(2026, 1, 1));
        assert_eq!(c.weekday(), 4);
        assert_eq!(c.minute_of_day(), 0.0);
    }

    #[test]
    fn set_date_and_time() {
        let mut c = Clock::default();
        c.set("2026-07-15 14:30").unwrap();
        assert_eq!(c.weekday(), 3); // a Wednesday
        assert_eq!(c.minute_of_day(), 870.0);
        c.set("06:00:30").unwrap();
        assert_eq!(c.day(), days_from_civil(2026, 7, 15));
        assert_eq!(c.minute_of_day(), 360.5);
        assert!(c.set("25:00").is_err());
        assert!(c.set("2026-13-01 00:00").is_err());
    }

    #[test]
    fn civil_round_trip() {
        for (y, m, d) in [(2026, 1, 1), (2024, 2, 29), (2026, 12, 31), (1970, 1, 1)] {
            assert_eq!(civil_from_days(days_from_civil(y, m, d)), (y, m, d));
        }
    }
}
