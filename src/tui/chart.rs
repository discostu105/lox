//! History chart math (§5.10): time buckets, summaries, energy integrals.
//! Pure functions over `(unix seconds, value)` points sorted by time.

/// One plot column: average and range of the values in its time slice.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bucket {
    pub avg: f64,
    pub lo: f64,
    pub hi: f64,
}

/// Split [from, to) into `n` columns. Columns without a point hold the last
/// value before them (on/off series, recorded on change) or, with `linear`,
/// interpolate towards the next one (sampled analog values); columns before
/// the first point are `None`.
pub fn buckets(
    points: &[(i64, f64)],
    from: i64,
    to: i64,
    n: usize,
    linear: bool,
) -> Vec<Option<Bucket>> {
    if n == 0 || to <= from {
        return Vec::new();
    }
    let span = (to - from) as f64;
    let mut out = vec![None; n];
    let mut i = points.partition_point(|p| p.0 < from);
    let mut held = i.checked_sub(1).map(|k| points[k].1);
    for (c, slot) in out.iter_mut().enumerate() {
        let end = from + ((c + 1) as f64 * span / n as f64) as i64;
        let (mut sum, mut cnt) = (0.0, 0usize);
        let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
        while i < points.len() && (points[i].0 < end || c + 1 == n && points[i].0 <= to) {
            let v = points[i].1;
            if v.is_finite() {
                sum += v;
                cnt += 1;
                lo = lo.min(v);
                hi = hi.max(v);
                held = Some(v);
            }
            i += 1;
        }
        *slot = if cnt > 0 {
            Some(Bucket {
                avg: sum / cnt as f64,
                lo,
                hi,
            })
        } else {
            let mid = end as f64 - span / n as f64 / 2.0;
            let v = match (i.checked_sub(1).map(|k| points[k]), points.get(i)) {
                (Some(a), Some(b)) if linear && b.0 > a.0 => {
                    Some(a.1 + (b.1 - a.1) * (mid - a.0 as f64) / (b.0 - a.0) as f64)
                }
                _ => held,
            };
            v.map(|v| Bucket {
                avg: v,
                lo: v,
                hi: v,
            })
        };
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Summary {
    pub min: (i64, f64),
    pub max: (i64, f64),
    /// Time-weighted mean (each value holds until the next)
    pub avg: f64,
    pub last: (i64, f64),
}

/// On/off series (only 0 and 1) are drawn as steps, everything else as lines.
pub fn is_binary(points: &[(i64, f64)]) -> bool {
    points.iter().all(|p| p.1 == 0.0 || p.1 == 1.0)
}

/// Min / max / mean / last of the points inside [from, to].
pub fn summary(points: &[(i64, f64)], from: i64, to: i64) -> Option<Summary> {
    let inside: Vec<(i64, f64)> = points
        .iter()
        .copied()
        .filter(|p| p.0 >= from && p.0 <= to && p.1.is_finite())
        .collect();
    let first = *inside.first()?;
    let mut min = first;
    let mut max = first;
    for p in &inside {
        if p.1 < min.1 {
            min = *p;
        }
        if p.1 > max.1 {
            max = *p;
        }
    }
    let last = *inside.last()?;
    let end = to.max(last.0);
    let (mut acc, mut dur) = (0.0, 0.0);
    for (k, p) in inside.iter().enumerate() {
        let next = inside.get(k + 1).map_or(end, |q| q.0);
        let dt = (next - p.0).max(0) as f64;
        acc += p.1 * dt;
        dur += dt;
    }
    let avg = if dur > 0.0 {
        acc / dur
    } else {
        inside.iter().map(|p| p.1).sum::<f64>() / inside.len() as f64
    };
    Some(Summary {
        min,
        max,
        avg,
        last,
    })
}

/// Energy of a power series (kW) over [from, to] in kWh: each value holds
/// until the next point, but never longer than `max_hold` seconds (gaps).
pub fn energy_kwh(points: &[(i64, f64)], from: i64, to: i64, max_hold: i64) -> f64 {
    let mut kwh = 0.0;
    for (k, p) in points.iter().enumerate() {
        let next = points.get(k + 1).map_or(to, |q| q.0);
        let a = p.0.max(from);
        let b = next.min(to).min(p.0 + max_hold);
        if b > a && p.1.is_finite() {
            kwh += p.1 * (b - a) as f64 / 3600.0;
        }
    }
    kwh
}

/// "Nice" axis bounds: the data range with a little headroom, rounded.
pub fn axis(lo: f64, hi: f64) -> (f64, f64) {
    let span = (hi - lo).abs().max(1e-6);
    let step = 10f64.powf((span / 4.0).log10().floor());
    let a = ((lo - span * 0.05) / step).floor() * step;
    // non-negative data (power, percent) keeps its zero line
    let a = if lo >= 0.0 { a.max(0.0) } else { a };
    let b = ((hi + span * 0.05) / step).ceil() * step;
    (a, b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buckets_hold_and_range() {
        let p = [(10, 1.0), (12, 3.0), (35, 5.0)];
        let b = buckets(&p, 0, 40, 4, false);
        assert_eq!(b[0], None, "before the first point");
        assert_eq!(
            b[1],
            Some(Bucket {
                avg: 2.0,
                lo: 1.0,
                hi: 3.0
            })
        );
        // no point in [20, 30): holds 3.0
        assert_eq!(b[2].unwrap().avg, 3.0);
        assert_eq!(b[3].unwrap().avg, 5.0);
        // a point before the window carries into it
        let b = buckets(&[(-5, 7.0)], 0, 10, 2, false);
        assert_eq!(b[0].unwrap().avg, 7.0);
        assert!(buckets(&p, 0, 0, 3, false).is_empty());
        // linear: the empty column [20, 30) interpolates 3.0 → 5.0 at 25
        let b = buckets(&p, 0, 40, 4, true);
        assert!((b[2].unwrap().avg - (3.0 + 2.0 * 13.0 / 23.0)).abs() < 1e-9);
        // after the last point it holds
        let b = buckets(&[(0, 1.0), (10, 2.0)], 0, 40, 4, true);
        assert_eq!(b[3].unwrap().avg, 2.0);
    }

    #[test]
    fn summary_is_time_weighted() {
        // 1.0 for 90 s, 5.0 for 10 s
        let s = summary(&[(0, 1.0), (90, 5.0)], 0, 100).unwrap();
        assert_eq!(s.min, (0, 1.0));
        assert_eq!(s.max, (90, 5.0));
        assert!((s.avg - 1.4).abs() < 1e-9);
        assert_eq!(s.last, (90, 5.0));
        assert!(summary(&[(200, 1.0)], 0, 100).is_none());
    }

    #[test]
    fn energy_integral() {
        // 2 kW for one hour, then 1 kW for half an hour
        let p = [(0, 2.0), (3600, 1.0)];
        assert!((energy_kwh(&p, 0, 5400, 7200) - 2.5).abs() < 1e-9);
        // a gap longer than max_hold counts only max_hold
        assert!((energy_kwh(&p, 0, 5400, 900) - (0.5 + 0.25)).abs() < 1e-9);
    }

    #[test]
    fn axis_rounds_outward() {
        let (a, b) = axis(21.3, 23.6);
        assert!(a <= 21.3 && b >= 23.6);
        assert!(b - a < 4.0);
    }
}
