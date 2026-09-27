//! Live state store: state UUID → value, the in-memory event ring, session
//! history for sparklines, and the event rate.
//!
//! Nothing here is persisted (§5.4): events live only as long as the TUI runs.

use std::collections::{HashMap, HashSet, VecDeque};

use super::model::{Cid, House, Kind};
use crate::stream::StateEvent;

pub const EVENT_CAP: usize = 10_000;
/// Session history: one point per this many seconds per state
const HIST_STEP: f64 = 30.0;
const HIST_CAP: usize = 480; // 4 h
/// Event rate: per-second buckets
const RATE_CAP: usize = 3600;
/// Consecutive changes of the same analog state within this window collapse into one row
const COLLAPSE_WINDOW: f64 = 10.0;
const COLLAPSE_LOOKBACK: usize = 8;

#[derive(Debug, Clone, PartialEq)]
pub enum Val {
    Num(f64),
    Text(String),
}

#[derive(Debug, Clone)]
pub struct Event {
    /// Monotonic identity (selection follows this, never the row index)
    pub seq: u64,
    /// Unix time (seconds, fractional)
    pub t: f64,
    pub uuid: String,
    /// Owning control (top-level or sub-control)
    pub cid: Option<Cid>,
    pub state: String,
    pub old: Option<Val>,
    pub new: Val,
    /// Number of changes collapsed into this row
    pub count: u32,
}

#[derive(Debug, Default)]
pub struct Store {
    pub values: HashMap<String, f64>,
    pub texts: HashMap<String, String>,
    /// Last change time per state UUID
    pub since: HashMap<String, f64>,
    pub events: VecDeque<Event>,
    pub next_seq: u64,
    /// state UUID → (time, value) points, at most one per `HIST_STEP`
    pub hist: HashMap<String, VecDeque<(f64, f64)>>,
    /// events per second, newest last; `rate_sec` is the second of the last bucket
    pub rate: VecDeque<u32>,
    pub rate_sec: i64,
    /// Muted control UUIDs (top-level): not recorded, not shown
    pub mutes: HashSet<String>,
    /// Record auto-muted states too (`X` in Events)
    pub record_noisy: bool,
    /// Changes before this time (initial state burst after (re)connect) are applied silently
    pub quiet_until: f64,
    /// Counts of changes that were not recorded because they were muted
    pub muted_count: u64,
    /// Last event time per control (for "activity" sorting)
    pub last_event: HashMap<Cid, f64>,
}

impl Store {
    pub fn new() -> Store {
        Store {
            next_seq: 1,
            ..Default::default()
        }
    }

    pub fn num(&self, uuid: &str) -> Option<f64> {
        self.values.get(uuid).copied()
    }

    pub fn text(&self, uuid: &str) -> Option<&str> {
        self.texts.get(uuid).map(|s| s.as_str())
    }

    /// Numeric state of a control by state name.
    pub fn st(&self, house: &House, cid: Cid, state: &str) -> Option<f64> {
        house.ctrls[cid].state(state).and_then(|u| self.num(u))
    }

    pub fn st_text<'a>(&'a self, house: &House, cid: Cid, state: &str) -> Option<&'a str> {
        house.ctrls[cid].state(state).and_then(|u| self.text(u))
    }

    /// High-frequency states that are muted by default: meter totals (≈ 90 % of
    /// a real installation's traffic).
    pub fn is_noisy(house: &House, cid: Cid, state: &str) -> bool {
        matches!(house.ctrls[cid].kind, Kind::Meter | Kind::Efm)
            && (state.starts_with("total") || state == "selfConsumption")
    }

    pub fn is_muted(&self, house: &House, cid: Cid) -> bool {
        let top = house.top(cid);
        self.mutes.contains(&house.ctrls[top].uuid)
    }

    /// Apply a batch from the stream. Returns the number of new feed rows.
    pub fn apply(&mut self, house: &House, batch: &[StateEvent], now: f64) -> usize {
        let mut added = 0;
        let quiet = now < self.quiet_until;
        for ev in batch {
            let (uuid, new) = match ev {
                StateEvent::ValueState { uuid, value } => (uuid, Val::Num(*value)),
                StateEvent::TextState { uuid, text, .. } => (uuid, Val::Text(text.clone())),
                _ => continue,
            };
            let old = match &new {
                Val::Num(v) => self.values.insert(uuid.clone(), *v).map(Val::Num),
                Val::Text(t) => self.texts.insert(uuid.clone(), t.clone()).map(Val::Text),
            };
            if old.as_ref() == Some(&new) {
                continue;
            }
            let owner = house.state_owner.get(uuid.as_str());
            if let (Val::Num(v), Some((cid, sname))) = (&new, owner)
                && !sname.starts_with("total")
            {
                self.push_hist(uuid, now, *v);
                let _ = cid;
            }
            // First value or initial burst: silent
            if old.is_none() || quiet {
                continue;
            }
            // Jitter below display precision (real meters: 1.0561 → 1.0564)
            if matches!(new, Val::Num(_)) && fmt_val(&old) == fmt_val(&Some(new.clone())) {
                continue;
            }
            self.since.insert(uuid.clone(), now);
            self.bump_rate(now);
            let (cid, sname) = match owner {
                Some((c, s)) => (Some(*c), s.clone()),
                None => (None, house_global_name(house, uuid)),
            };
            if let Some(c) = cid {
                if self.is_muted(house, c)
                    || (!self.record_noisy && Self::is_noisy(house, c, &sname))
                {
                    self.muted_count += 1;
                    continue;
                }
                self.last_event.insert(house.top(c), now);
            }
            if self.collapse(uuid, now, &old, &new) {
                continue;
            }
            let seq = self.next_seq;
            self.next_seq += 1;
            self.events.push_back(Event {
                seq,
                t: now,
                uuid: uuid.clone(),
                cid,
                state: sname,
                old,
                new,
                count: 1,
            });
            if self.events.len() > EVENT_CAP {
                self.events.pop_front();
            }
            added += 1;
        }
        added
    }

    /// Fold a repeated analog change into the recent row for the same state.
    fn collapse(&mut self, uuid: &str, now: f64, old: &Option<Val>, new: &Val) -> bool {
        let binary = |v: &Val| matches!(v, Val::Num(x) if *x == 0.0 || *x == 1.0);
        if !matches!(new, Val::Num(_)) || binary(new) && old.as_ref().is_some_and(binary) {
            return false;
        }
        let n = self.events.len();
        for i in (n.saturating_sub(COLLAPSE_LOOKBACK)..n).rev() {
            let e = &mut self.events[i];
            if e.uuid == uuid {
                if now - e.t <= COLLAPSE_WINDOW && !binary(&e.new) {
                    // the row keeps its first time so the feed stays in order
                    e.new = new.clone();
                    e.count += 1;
                    return true;
                }
                return false;
            }
        }
        false
    }

    fn push_hist(&mut self, uuid: &str, now: f64, v: f64) {
        let h = self.hist.entry(uuid.to_string()).or_default();
        match h.back_mut() {
            Some(last) if now - last.0 < HIST_STEP => last.1 = v,
            _ => {
                h.push_back((now, v));
                if h.len() > HIST_CAP {
                    h.pop_front();
                }
            }
        }
    }

    /// Session history values for a state (oldest first).
    pub fn series(&self, uuid: &str) -> Vec<f64> {
        self.hist
            .get(uuid)
            .map(|h| h.iter().map(|p| p.1).collect())
            .unwrap_or_default()
    }

    fn bump_rate(&mut self, now: f64) {
        let sec = now.floor() as i64;
        self.advance_rate(sec);
        if let Some(b) = self.rate.back_mut() {
            *b += 1;
        }
    }

    /// Move the rate window forward to `sec` (fills gaps with zeros).
    pub fn advance_rate(&mut self, sec: i64) {
        if self.rate.is_empty() {
            self.rate.push_back(0);
            self.rate_sec = sec;
            return;
        }
        let gap = (sec - self.rate_sec).clamp(0, RATE_CAP as i64);
        for _ in 0..gap {
            self.rate.push_back(0);
            if self.rate.len() > RATE_CAP {
                self.rate.pop_front();
            }
        }
        if sec > self.rate_sec {
            self.rate_sec = sec;
        }
    }

    /// Events per minute over the last minute.
    pub fn per_minute(&self) -> u32 {
        self.rate.iter().rev().take(60).sum()
    }

    /// Rate series: events per bucket of `secs` seconds, `n` buckets, oldest first.
    pub fn rate_series(&self, secs: usize, n: usize) -> Vec<f64> {
        let v: Vec<u32> = self.rate.iter().copied().collect();
        let mut out = Vec::with_capacity(n);
        for b in (0..n).rev() {
            let end = v.len().saturating_sub(b * secs);
            let start = end.saturating_sub(secs);
            out.push(v[start..end].iter().sum::<u32>() as f64);
        }
        out
    }

    /// Index of the event with this seq (binary search: seqs are increasing).
    pub fn index_of(&self, seq: u64) -> Option<usize> {
        let (a, b) = self.events.as_slices();
        match a.binary_search_by_key(&seq, |e| e.seq) {
            Ok(i) => Some(i),
            Err(_) => b
                .binary_search_by_key(&seq, |e| e.seq)
                .ok()
                .map(|i| i + a.len()),
        }
    }

    /// Events within ±`window` seconds of `t` (excluding `seq`).
    pub fn around(&self, t: f64, window: f64, seq: u64) -> Vec<&Event> {
        self.events
            .iter()
            .filter(|e| e.seq != seq && (e.t - t).abs() <= window)
            .collect()
    }

    /// The last event before `t` whose state belongs to one of `uuids`.
    pub fn last_before(&self, uuids: &HashSet<String>, t: f64) -> Option<&Event> {
        self.events
            .iter()
            .rev()
            .find(|e| e.t <= t && uuids.contains(&e.uuid))
    }
}

fn house_global_name(house: &House, uuid: &str) -> String {
    house
        .globals
        .iter()
        .find(|(_, u)| u.as_str() == uuid)
        .map(|(n, _)| n.clone())
        .unwrap_or_else(|| "state".into())
}

/// Format a value for the feed: `0.33`, `on`, text in quotes.
pub fn fmt_val(v: &Option<Val>) -> String {
    match v {
        None => "—".into(),
        Some(Val::Num(x)) => {
            if x.fract() == 0.0 && x.abs() < 1e12 {
                format!("{}", *x as i64)
            } else {
                let s = format!("{:.3}", x);
                s.trim_end_matches('0').trim_end_matches('.').to_string()
            }
        }
        Some(Val::Text(t)) => super::text::trunc(&super::text::clean(t), 40),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::demo;

    fn setup() -> (House, Store) {
        let st = demo::structure();
        let h = House::from_structure(&st);
        let mut s = Store::new();
        let sim = demo::Sim::new(&st);
        s.apply(&h, &sim.initial_events(), 0.0);
        (h, s)
    }

    fn val(uuid: &str, v: f64) -> StateEvent {
        StateEvent::ValueState {
            uuid: uuid.into(),
            value: v,
        }
    }

    #[test]
    fn initial_burst_is_silent() {
        let (_, s) = setup();
        assert!(s.events.is_empty());
        assert!(!s.values.is_empty());
    }

    #[test]
    fn changes_become_events_and_collapse() {
        let (h, mut s) = setup();
        let c = h.resolve("Blind South", None).unwrap();
        let pos = h.ctrls[c].state("position").unwrap().to_string();
        assert_eq!(s.apply(&h, &[val(&pos, 0.4)], 10.0), 1);
        assert_eq!(s.apply(&h, &[val(&pos, 0.5)], 11.0), 0);
        assert_eq!(s.events.len(), 1);
        assert_eq!(s.events[0].count, 2);
        assert_eq!(s.events[0].new, Val::Num(0.5));
        // unchanged value → nothing
        assert_eq!(s.apply(&h, &[val(&pos, 0.5)], 12.0), 0);
        // after the window: a new row
        assert_eq!(s.apply(&h, &[val(&pos, 0.6)], 30.0), 1);
    }

    #[test]
    fn binary_changes_never_collapse() {
        let (h, mut s) = setup();
        let c = h.resolve("Motion", None).unwrap();
        let u = h.ctrls[c].state("active").unwrap().to_string();
        s.apply(&h, &[val(&u, 1.0)], 1.0);
        s.apply(&h, &[val(&u, 0.0)], 2.0);
        s.apply(&h, &[val(&u, 1.0)], 3.0);
        assert_eq!(s.events.len(), 3);
    }

    #[test]
    fn meter_totals_auto_muted_and_manual_mute() {
        let (h, mut s) = setup();
        let c = h.resolve("PV", None).unwrap();
        let total = h.ctrls[c].state("total").unwrap().to_string();
        let actual = h.ctrls[c].state("actual").unwrap().to_string();
        s.apply(&h, &[val(&total, 9999.0)], 5.0);
        assert!(s.events.is_empty());
        assert_eq!(s.muted_count, 1);
        s.apply(&h, &[val(&actual, 3.3)], 5.0);
        assert_eq!(s.events.len(), 1);
        s.mutes.insert(h.ctrls[c].uuid.clone());
        s.apply(&h, &[val(&actual, 3.9)], 50.0);
        assert_eq!(s.events.len(), 1);
    }

    #[test]
    fn quiet_period_after_reconnect() {
        let (h, mut s) = setup();
        let c = h.resolve("Motion", None).unwrap();
        let u = h.ctrls[c].state("active").unwrap().to_string();
        s.quiet_until = 100.0;
        s.apply(&h, &[val(&u, 1.0)], 99.0);
        assert!(s.events.is_empty());
        assert_eq!(s.num(&u), Some(1.0));
    }

    #[test]
    fn ring_is_bounded_and_seq_lookup_works() {
        let (h, mut s) = setup();
        let c = h.resolve("Motion", None).unwrap();
        let u = h.ctrls[c].state("active").unwrap().to_string();
        for i in 0..(EVENT_CAP + 50) {
            s.apply(&h, &[val(&u, (i % 2) as f64)], 100.0 + i as f64);
        }
        assert_eq!(s.events.len(), EVENT_CAP);
        let mid = s.events[5000].seq;
        assert_eq!(s.index_of(mid), Some(5000));
        assert_eq!(s.index_of(1), None);
    }

    #[test]
    fn rate_buckets() {
        let (h, mut s) = setup();
        let c = h.resolve("Motion", None).unwrap();
        let u = h.ctrls[c].state("active").unwrap().to_string();
        for i in 0..10 {
            s.apply(
                &h,
                &[val(&u, ((i + 1) % 2) as f64)],
                1000.0 + i as f64 * 0.5,
            );
        }
        assert_eq!(s.per_minute(), 10);
        let r = s.rate_series(5, 3);
        assert_eq!(r.iter().sum::<f64>(), 10.0);
    }
}
