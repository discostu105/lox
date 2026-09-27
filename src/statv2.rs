//! Statistics V2 (Loxone Config 13.1+, Structure File § "StatisticV2").
//!
//! Controls with a `statisticV2` block (meters of the energy flow monitor, …) keep
//! their history in groups instead of the legacy `/stats/{uuid}.{YYYYMM}` files.
//! It is read over plain HTTP:
//!
//! ```text
//! dev/sps/getStatistic/{uuid}/raw/{fromUtc}/{toUtc}/all/{group}[/{output}]
//! ```
//!
//! It returns packed little-endian records: a `u32` unix-UTC timestamp followed
//! by one `f64` per requested output, with no header. (A `diff` variant returns
//! per-hour/day sums of counters.)

use serde_json::Value;

/// One statistic group of a control.
#[derive(Debug, Clone, PartialEq)]
pub struct Group {
    pub id: String,
    /// Recording mode (0 none, 1/7 on change, 8–12 fixed intervals)
    pub mode: u64,
    /// Counter values (kWh); the useful graph is their difference
    pub accumulated: bool,
    /// (output state name, title)
    pub points: Vec<(String, String)>,
}

/// The `statisticV2` groups of a control (empty if it has none).
pub fn groups(ctrl: &Value) -> Vec<Group> {
    let Some(gs) = ctrl
        .pointer("/statisticV2/groups")
        .and_then(|g| g.as_array())
    else {
        return Vec::new();
    };
    gs.iter()
        .filter_map(|g| {
            let id = match g.get("id")? {
                Value::String(s) => s.clone(),
                Value::Number(n) => n.to_string(),
                _ => return None,
            };
            let points = g
                .get("dataPoints")
                .and_then(|d| d.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|p| {
                            let out = p.get("output")?.as_str()?.to_string();
                            let title = p
                                .get("title")
                                .and_then(|t| t.as_str())
                                .unwrap_or(&out)
                                .to_string();
                            Some((out, title))
                        })
                        .collect()
                })
                .unwrap_or_default();
            Some(Group {
                id,
                mode: g.get("mode").and_then(|m| m.as_u64()).unwrap_or(0),
                accumulated: g
                    .get("accumulated")
                    .and_then(|a| a.as_bool())
                    .unwrap_or(false),
                points,
            })
        })
        .filter(|g| g.mode != 0 && !g.points.is_empty())
        .collect()
}

/// The series worth plotting: the first recorded non-accumulated data point
/// (a meter's `actual` power), as (group id, output).
pub fn plot_point(ctrl: &Value) -> Option<(String, String)> {
    groups(ctrl)
        .into_iter()
        .find(|g| !g.accumulated)
        .and_then(|g| g.points.first().map(|(o, _)| (g.id.clone(), o.clone())))
}

/// `raw` request path: the values as recorded.
pub fn raw_path(uuid: &str, from: i64, to: i64, group: &str, output: Option<&str>) -> String {
    let mut p = format!("/dev/sps/getStatistic/{uuid}/raw/{from}/{to}/all/{group}");
    if let Some(o) = output {
        p.push('/');
        p.push_str(o);
    }
    p
}

/// Parse a result: (unix seconds, one value per requested output).
pub fn parse(data: &[u8], outputs: usize) -> Vec<(i64, Vec<f64>)> {
    let n = outputs.max(1);
    data.chunks_exact(4 + 8 * n)
        .map(|rec| {
            let ts = u32::from_le_bytes([rec[0], rec[1], rec[2], rec[3]]) as i64;
            let vals = rec[4..]
                .as_chunks::<8>()
                .0
                .iter()
                .map(|b| f64::from_le_bytes(*b))
                .collect();
            (ts, vals)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn meter() -> Value {
        json!({"statisticV2": {"groups": [
            {"id": "1", "mode": 12, "dataPoints": [
                {"title": "Leistung", "format": "0.000kW", "output": "actual"}]},
            {"id": "2", "mode": 11, "accumulated": true, "dataPoints": [
                {"title": "Zählerstand", "output": "total"},
                {"title": "Export", "output": "totalNeg"}]}
        ]}})
    }

    #[test]
    fn groups_and_plot_point() {
        let g = groups(&meter());
        assert_eq!(g.len(), 2);
        assert_eq!(g[0].id, "1");
        assert!(!g[0].accumulated);
        assert!(g[1].accumulated);
        assert_eq!(g[1].points[1], ("totalNeg".into(), "Export".into()));
        assert_eq!(plot_point(&meter()), Some(("1".into(), "actual".into())));
        assert!(groups(&json!({"statistic": {}})).is_empty());
        // numeric ids, mode 0 (not recorded) is skipped
        let c = json!({"statisticV2": {"groups": [
            {"id": 3, "mode": 0, "dataPoints": [{"output": "a"}]},
            {"id": 4, "mode": 1, "dataPoints": [{"output": "b"}]}]}});
        assert_eq!(plot_point(&c), Some(("4".into(), "b".into())));
    }

    #[test]
    fn paths() {
        assert_eq!(
            raw_path("u", 1, 2, "1", Some("actual")),
            "/dev/sps/getStatistic/u/raw/1/2/all/1/actual"
        );
    }

    #[test]
    fn parse_records() {
        let mut d = Vec::new();
        for (t, v) in [(1_790_000_000u32, 0.158f64), (1_790_000_900, 0.06)] {
            d.extend_from_slice(&t.to_le_bytes());
            d.extend_from_slice(&v.to_le_bytes());
        }
        d.push(0xff); // a trailing partial record is ignored
        let p = parse(&d, 1);
        assert_eq!(p.len(), 2);
        assert_eq!(p[1], (1_790_000_900, vec![0.06]));
        let mut d2 = Vec::new();
        d2.extend_from_slice(&7u32.to_le_bytes());
        d2.extend_from_slice(&1.0f64.to_le_bytes());
        d2.extend_from_slice(&2.0f64.to_le_bytes());
        assert_eq!(parse(&d2, 2), [(7, vec![1.0, 2.0])]);
    }
}
