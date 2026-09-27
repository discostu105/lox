//! Derived lists shared by `update` (navigation) and the UI (rendering):
//! room groups, control rows, event rows, attention, home cards.

use std::cell::RefCell;

use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config as NConfig, Matcher, Utf32Str};

use super::app::{App, Chip, GKey, GroupBy, RoomSort};
use super::model::{Cid, Kind};
use super::text::natural_key;
use super::vm;

// ── Fuzzy matching (nucleo, as in Helix) ────────────────────────────────────

thread_local! {
    static MATCHER: RefCell<Matcher> = RefCell::new(Matcher::new(NConfig::DEFAULT));
}

/// Score `text` against `pattern` (empty pattern matches everything with 0).
pub fn fuzzy(pattern: &str, text: &str) -> Option<u32> {
    if pattern.trim().is_empty() {
        return Some(0);
    }
    let pat = Pattern::parse(pattern, CaseMatching::Ignore, Normalization::Smart);
    MATCHER.with(|m| {
        let mut buf = Vec::new();
        pat.score(Utf32Str::new(text, &mut buf), &mut m.borrow_mut())
    })
}

/// Char indices of the match (for highlighting).
///
/// nucleo's indices aren't char indices: it matches ASCII text by byte, and
/// other text by grapheme — unless every grapheme *starts* with an ASCII char
/// (`u` + combining `̈`), then again by byte of the original string. Every char
/// of a matched grapheme is returned.
pub fn fuzzy_indices(pattern: &str, text: &str) -> Vec<usize> {
    if pattern.trim().is_empty() {
        return Vec::new();
    }
    let pat = Pattern::parse(pattern, CaseMatching::Ignore, Normalization::Smart);
    let mut idx = MATCHER.with(|m| {
        let mut buf = Vec::new();
        let mut idx = Vec::new();
        pat.indices(Utf32Str::new(text, &mut buf), &mut m.borrow_mut(), &mut idx);
        idx
    });
    idx.sort_unstable();
    idx.dedup();
    if text.is_ascii() {
        return idx.into_iter().map(|i| i as usize).collect();
    }
    let graphemes = || unicode_segmentation::UnicodeSegmentation::graphemes(text, true);
    if graphemes().all(|g| g.starts_with(|c: char| c.is_ascii())) {
        // byte offsets
        return text
            .char_indices()
            .enumerate()
            .filter(|(_, (b, _))| idx.binary_search(&(*b as u32)).is_ok())
            .map(|(ci, _)| ci)
            .collect();
    }
    let mut out = Vec::new();
    let mut ci = 0;
    for (gi, g) in graphemes().enumerate() {
        let n = g.chars().count();
        if idx.binary_search(&(gi as u32)).is_ok() {
            out.extend(ci..ci + n);
        }
        ci += n;
    }
    out
}

// ── Room summaries ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct RoomSum {
    pub temp: Option<f64>,
    pub target: Option<f64>,
    pub lights_on: usize,
    pub lights: usize,
    /// Average level of lights that are on (0–100)
    pub light_level: Option<f64>,
    pub blinds: usize,
    /// Average closed fraction of blinds (0–1)
    pub blind_pos: Option<f64>,
    pub blinds_moving: bool,
    pub windows_open: usize,
    pub attention: bool,
    pub last_event: Option<f64>,
    pub gate_open: bool,
}

pub fn room_sum(app: &App, r: usize) -> RoomSum {
    let h = &app.house;
    let s = &app.store;
    let room = &h.rooms[r];
    let mut sum = RoomSum {
        temp: room.temp.as_ref().and_then(|(_, u)| s.num(u)),
        target: room.target.as_ref().and_then(|(_, u)| s.num(u)),
        ..Default::default()
    };
    let mut levels = Vec::new();
    let mut pos = Vec::new();
    for &c in &room.ctrls {
        let k = h.ctrls[c].kind;
        if let Some(t) = s.last_event.get(&c) {
            sum.last_event = Some(sum.last_event.map_or(*t, |x: f64| x.max(*t)));
        }
        for &sc in &h.ctrls[c].subs {
            if let Some(t) = s.last_event.get(&sc) {
                sum.last_event = Some(sum.last_event.map_or(*t, |x: f64| x.max(*t)));
            }
        }
        if vm::is_light(h, c) && k != Kind::CentralLight {
            // a light controller counts its sub-lights
            let lights: Vec<Cid> = if k == Kind::LightCtl && !h.ctrls[c].subs.is_empty() {
                h.ctrls[c].subs.clone()
            } else {
                vec![c]
            };
            for l in lights {
                sum.lights += 1;
                if vm::is_light_on(s, h, l) {
                    sum.lights_on += 1;
                    if let Some(v) = vm::light_level(s, h, l) {
                        levels.push(v);
                    }
                }
            }
        }
        match k {
            Kind::Blind => {
                sum.blinds += 1;
                if let Some(p) = s.st(h, c, "position") {
                    pos.push(p);
                }
                if vm::motion(s, h, c).is_some() {
                    sum.blinds_moving = true;
                }
            }
            Kind::Gate if s.st(h, c, "position").unwrap_or(0.0) > 0.01 => {
                sum.gate_open = true;
            }
            _ => {}
        }
        if is_window(app, c) && s.st(h, c, "active").unwrap_or(0.0) > 0.0 {
            sum.windows_open += 1;
        }
        if vm::view(s, h, c).attention {
            sum.attention = true;
        }
    }
    if !levels.is_empty() {
        sum.light_level = Some(levels.iter().sum::<f64>() / levels.len() as f64);
    }
    if !pos.is_empty() {
        sum.blind_pos = Some(pos.iter().sum::<f64>() / pos.len() as f64);
    }
    if sum.windows_open > 0 {
        sum.attention = true;
    }
    sum
}

pub fn is_window(app: &App, c: Cid) -> bool {
    let ctrl = &app.house.ctrls[c];
    let n = ctrl.name.to_lowercase();
    ctrl.kind == Kind::Digital
        && (n.contains("window") || n.contains("fenster") || n.contains("door contact"))
}

/// Activity score for sorting rooms (lights on, recent events).
pub fn activity(app: &App, sum: &RoomSum) -> f64 {
    let recent = sum
        .last_event
        .map(|t| (600.0 - (app.now - t)).max(0.0) / 60.0)
        .unwrap_or(0.0);
    sum.lights_on as f64 * 10.0 + recent + if sum.attention { 50.0 } else { 0.0 }
}

// ── Rooms screen: groups ────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct GroupRow {
    pub key: GKey,
    pub label: String,
    pub count: usize,
}

/// Controls of a group (top-level; LC sub-controls follow in `ctrl_rows`).
pub fn group_ctrls(app: &App, key: &GKey) -> Vec<Cid> {
    let h = &app.house;
    let fav_only = app.rooms.fav_only;
    let keep = |c: Cid| !fav_only || h.ctrls[c].is_favorite || app.is_pinned(c);
    let mut v: Vec<Cid> = match key {
        GKey::Favorites => h
            .top_level()
            .filter(|c| h.ctrls[*c].is_favorite || app.is_pinned(*c))
            .collect(),
        GKey::All => h.top_level().filter(|c| keep(*c)).collect(),
        GKey::Room(r) => h.rooms.get(*r).map(|r| r.ctrls.clone()).unwrap_or_default(),
        GKey::Cat(i) => h
            .top_level()
            .filter(|c| h.ctrls[*c].cat == Some(*i))
            .collect(),
        GKey::Type(t) => h
            .top_level()
            .filter(|c| type_label(app, *c) == *t)
            .collect(),
        GKey::Unassigned => h.unassigned.clone(),
    };
    v.retain(|c| keep(*c));
    v
}

pub fn type_label(app: &App, c: Cid) -> String {
    let t = &app.house.ctrls[c].typ;
    // Fold versions: LightControllerV2 → LightController, IRoomControllerV2 → IRoomController
    t.trim_end_matches(char::is_numeric)
        .trim_end_matches('V')
        .to_string()
}

pub fn groups(app: &App) -> Vec<GroupRow> {
    let h = &app.house;
    let mut out = Vec::new();
    let favs = group_ctrls(app, &GKey::Favorites).len();
    out.push(GroupRow {
        key: GKey::Favorites,
        label: "★ Favorites".into(),
        count: favs,
    });
    out.push(GroupRow {
        key: GKey::All,
        label: "All controls".into(),
        count: group_ctrls(app, &GKey::All).len(),
    });
    let mut body: Vec<GroupRow> = match app.rooms.group {
        GroupBy::Room => (0..h.rooms.len())
            .map(|r| GroupRow {
                key: GKey::Room(r),
                label: h.rooms[r].name.clone(),
                count: group_ctrls(app, &GKey::Room(r)).len(),
            })
            .collect(),
        GroupBy::Category => (0..h.cats.len())
            .map(|i| GroupRow {
                key: GKey::Cat(i),
                label: h.cats[i].name.clone(),
                count: group_ctrls(app, &GKey::Cat(i)).len(),
            })
            .filter(|g| g.count > 0)
            .collect(),
        GroupBy::Type => {
            let mut types: Vec<String> = h.top_level().map(|c| type_label(app, c)).collect();
            types.sort_by_key(|t| natural_key(t));
            types.dedup();
            types
                .into_iter()
                .map(|t| GroupRow {
                    count: group_ctrls(app, &GKey::Type(t.clone())).len(),
                    label: t.clone(),
                    key: GKey::Type(t),
                })
                .collect()
        }
    };
    if app.rooms.fav_only {
        body.retain(|g| g.count > 0);
    }
    if app.rooms.group == GroupBy::Room && app.rooms.sort != RoomSort::Name {
        match &app.rooms.frozen {
            Some(order) => {
                body.sort_by_key(|g| order.iter().position(|k| *k == g.key).unwrap_or(usize::MAX))
            }
            None => sort_rooms(app, &mut body),
        }
    }
    if !app.rooms.filter_groups.is_empty() {
        let mut scored: Vec<(u32, GroupRow)> = body
            .into_iter()
            .filter_map(|g| fuzzy(&app.rooms.filter_groups, &g.label).map(|s| (s, g)))
            .collect();
        scored.sort_by_key(|x| std::cmp::Reverse(x.0));
        body = scored.into_iter().map(|(_, g)| g).collect();
    }
    out.extend(body);
    if app.rooms.group == GroupBy::Room && !h.unassigned.is_empty() {
        let n = group_ctrls(app, &GKey::Unassigned).len();
        if n > 0 {
            out.push(GroupRow {
                key: GKey::Unassigned,
                label: "No room".into(),
                count: n,
            });
        }
    }
    out
}

pub fn sort_rooms(app: &App, rows: &mut [GroupRow]) {
    match app.rooms.sort {
        RoomSort::Name => {}
        RoomSort::Activity => rows.sort_by(|a, b| {
            let score = |g: &GroupRow| match g.key {
                GKey::Room(r) => activity(app, &room_sum(app, r)),
                _ => 0.0,
            };
            score(b).total_cmp(&score(a))
        }),
        RoomSort::Temp => rows.sort_by(|a, b| {
            let t = |g: &GroupRow| match g.key {
                GKey::Room(r) => room_sum(app, r).temp.unwrap_or(f64::NEG_INFINITY),
                _ => f64::NEG_INFINITY,
            };
            t(b).total_cmp(&t(a))
        }),
    }
}

// ── Rooms screen: control rows ──────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum CRow {
    Header(String),
    Ctrl { cid: Cid, depth: u8 },
}

/// Section title for a control (category, uppercased).
fn section(app: &App, c: Cid, by_room: bool) -> String {
    let h = &app.house;
    if by_room {
        h.room_name(c).unwrap_or("NO ROOM").to_uppercase()
    } else {
        h.cat_name(c).unwrap_or("OTHER").to_uppercase()
    }
}

pub fn ctrl_rows(app: &App, key: &GKey) -> Vec<CRow> {
    let h = &app.house;
    let filter = app.rooms.filter_ctrls.trim();
    let mut ctrls = group_ctrls(app, key);
    // Category groups section by room; everything else by category
    let by_room = matches!(key, GKey::Cat(_));
    let mut out = Vec::new();
    if !filter.is_empty() {
        // Filtered: flat, best match first, sub-controls included
        let mut all: Vec<Cid> = Vec::new();
        for c in ctrls {
            all.push(c);
            all.extend(h.ctrls[c].subs.iter().copied());
        }
        let mut scored: Vec<(u32, Cid)> = all
            .into_iter()
            .filter_map(|c| {
                let hay = format!("{} {}", h.display_name(c), h.room_name(c).unwrap_or(""));
                fuzzy(filter, &hay).map(|s| (s, c))
            })
            .collect();
        scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        return scored
            .into_iter()
            .map(|(_, cid)| CRow::Ctrl { cid, depth: 0 })
            .collect();
    }
    ctrls.sort_by_cached_key(|c| {
        (
            natural_key(&section(app, *c, by_room)),
            natural_key(&h.ctrls[*c].name),
        )
    });
    let mut last: Option<String> = None;
    for c in ctrls {
        let sec = section(app, c, by_room);
        if last.as_ref() != Some(&sec) {
            out.push(CRow::Header(sec.clone()));
            last = Some(sec);
        }
        out.push(CRow::Ctrl { cid: c, depth: 0 });
        if matches!(h.ctrls[c].kind, Kind::LightCtl | Kind::CentralLight) {
            for &s in &h.ctrls[c].subs {
                out.push(CRow::Ctrl { cid: s, depth: 1 });
            }
        }
    }
    out
}

pub fn row_cids(rows: &[CRow]) -> Vec<Cid> {
    rows.iter()
        .filter_map(|r| match r {
            CRow::Ctrl { cid, .. } => Some(*cid),
            _ => None,
        })
        .collect()
}

/// The group currently shown in the middle pane (selection or the first real group).
pub fn current_group(app: &App) -> GKey {
    let gs = groups(app);
    match &app.rooms.sel_group {
        Some(k) if gs.iter().any(|g| g.key == *k) => k.clone(),
        _ => gs
            .iter()
            .find(|g| g.count > 0 && !matches!(g.key, GKey::Favorites | GKey::All))
            .or(gs.first())
            .map(|g| g.key.clone())
            .unwrap_or(GKey::All),
    }
}

/// The selected control in the Rooms middle pane.
pub fn selected_ctrl(app: &App) -> Option<Cid> {
    let key = current_group(app);
    let cids = row_cids(&ctrl_rows(app, &key));
    let uuid = app.rooms.sel_ctrl.get(&key);
    uuid.and_then(|u| app.house.by_uuid.get(u).copied())
        .filter(|c| cids.contains(c))
        .or_else(|| cids.first().copied())
}

// ── Events ──────────────────────────────────────────────────────────────────

/// Indices into `store.events` that pass the chips / filter / mutes (oldest first).
pub fn event_rows(app: &App) -> Vec<usize> {
    let h = &app.house;
    let st = &app.store;
    let f = app.events.filter.trim();
    st.events
        .iter()
        .enumerate()
        .filter(|(_, e)| {
            let top = e.cid.map(|c| h.top(c));
            if !app.events.show_muted && top.is_some_and(|c| st.is_muted(h, c)) {
                return false;
            }
            for chip in &app.events.chips {
                let ok = match chip {
                    Chip::Room(r) => e.cid.is_some_and(|c| h.ctrls[c].room == Some(*r)),
                    Chip::Ctrl(c) => top == Some(*c) || e.cid == Some(*c),
                };
                if !ok {
                    return false;
                }
            }
            if !f.is_empty() {
                let hay = match e.cid {
                    Some(c) => format!(
                        "{} {} {}",
                        h.display_name(c),
                        h.room_name(c).unwrap_or(""),
                        e.state
                    ),
                    None => e.state.clone(),
                };
                return fuzzy(f, &hay).is_some();
            }
            true
        })
        .map(|(i, _)| i)
        .collect()
}

// ── Attention (Home) ────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub struct Att {
    /// 3 crit · 2 warn · 1 info
    pub level: u8,
    pub text: String,
    pub cid: Option<Cid>,
}

pub fn attention(app: &App) -> Vec<Att> {
    let h = &app.house;
    let s = &app.store;
    let mut out = Vec::new();
    let raining = h.ctrls.iter().enumerate().any(|(c, ctrl)| {
        ctrl.name.to_lowercase().contains("rain")
            && ctrl.kind == Kind::Digital
            && s.st(h, c, "active").unwrap_or(0.0) > 0.0
    });
    for c in h.top_level() {
        let ctrl = &h.ctrls[c];
        let room = h
            .room_name(c)
            .map(|r| format!("{} ", r))
            .unwrap_or_default();
        match ctrl.kind {
            Kind::Alarm => {
                if s.st(h, c, "level").unwrap_or(0.0) > 0.0 {
                    out.push(Att {
                        level: 3,
                        text: format!("{} triggered", ctrl.name),
                        cid: Some(c),
                    });
                }
            }
            Kind::Smoke => {
                if s.st(h, c, "level").unwrap_or(0.0) > 0.0 {
                    out.push(Att {
                        level: 3,
                        text: format!("{} — alarm", ctrl.name),
                        cid: Some(c),
                    });
                }
            }
            Kind::DoorLock => {
                // unlocked for more than 30 min
                if let Some(u) = ctrl.state("active")
                    && s.num(u) == Some(0.0)
                {
                    let since = s.since.get(u).copied().unwrap_or(app.started);
                    if app.now - since > 1800.0 {
                        out.push(Att {
                            level: 2,
                            text: format!("{} unlocked since {}", ctrl.name, app.hhmm(since)),
                            cid: Some(c),
                        });
                    }
                }
            }
            Kind::Gate if s.st(h, c, "position").unwrap_or(0.0) > 0.01 => {
                out.push(Att {
                    level: 1,
                    text: format!("{} open", ctrl.name),
                    cid: Some(c),
                });
            }
            _ => {}
        }
        if is_window(app, c) && s.st(h, c, "active").unwrap_or(0.0) > 0.0 {
            let rain = if raining { " · raining" } else { "" };
            out.push(Att {
                level: if raining { 3 } else { 1 },
                text: format!("{}{} open{}", room, ctrl.name.to_lowercase(), rain),
                cid: Some(c),
            });
        }
    }
    if let Some((_, devs)) = &app.devices {
        for d in devs {
            let what = match d.problem() {
                3 => "offline".to_string(),
                2 => format!("battery {} %", d.battery.unwrap_or(0)),
                1 => "weak signal".into(),
                _ => continue,
            };
            out.push(Att {
                level: if d.problem() == 3 { 2 } else { 1 } + u8::from(d.problem() == 2),
                text: format!("{} · {}", d.name, what),
                cid: None,
            });
        }
    }
    if let Some((_, d)) = &app.diag {
        if d.cpu.unwrap_or(0.0) > 80.0 {
            out.push(Att {
                level: 2,
                text: format!("Miniserver CPU {:.0} %", d.cpu.unwrap_or(0.0)),
                cid: None,
            });
        }
        if d.heap_pct().unwrap_or(0.0) > 90.0 {
            out.push(Att {
                level: 2,
                text: format!("Miniserver heap {:.0} %", d.heap_pct().unwrap_or(0.0)),
                cid: None,
            });
        }
        if d.sd_error() {
            out.push(Att {
                level: 3,
                text: "SD card errors".into(),
                cid: None,
            });
        }
    }
    let bus_errs: Vec<&String> = app
        .buslan_flash
        .iter()
        .filter(|(_, t)| app.now - **t < 600.0)
        .map(|(n, _)| n)
        .collect();
    if !bus_errs.is_empty() {
        out.push(Att {
            level: 2,
            text: format!("bus errors: {}", bus_errs.len()),
            cid: None,
        });
    }
    match &app.conn {
        super::app::Conn::Offline(e) => out.push(Att {
            level: 3,
            text: format!("offline — {}", e),
            cid: None,
        }),
        super::app::Conn::OutOfService => out.push(Att {
            level: 2,
            text: "Miniserver out of service".into(),
            cid: None,
        }),
        _ => {}
    }
    out.sort_by(|a, b| b.level.cmp(&a.level).then(a.text.cmp(&b.text)));
    out
}

// ── Home cards ──────────────────────────────────────────────────────────────

/// Room order for Home cards: activity, then name. Rooms without controls are skipped.
pub fn home_rooms(app: &App) -> Vec<usize> {
    let h = &app.house;
    let mut rs: Vec<(f64, usize)> = (0..h.rooms.len())
        .filter(|r| !h.rooms[*r].ctrls.is_empty())
        .map(|r| (activity(app, &room_sum(app, r)), r))
        .collect();
    rs.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
    rs.into_iter().map(|(_, r)| r).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fuzzy_matches_like_helix() {
        assert!(fuzzy("blsou", "Blind South").is_some());
        assert!(fuzzy("xyz", "Blind South").is_none());
        assert_eq!(fuzzy_indices("bs", "Blind South"), [0, 6]);
        // decomposed ü (u + U+0308) is one grapheme but two chars
        assert_eq!(fuzzy_indices("kc", "Ku\u{308}che"), [0, 3]);
        assert_eq!(fuzzy_indices("ku", "Ku\u{308}che"), [0, 1]);
        // mixed: a precomposed ü makes nucleo match by grapheme
        assert_eq!(fuzzy_indices("kc", "Ku\u{308}che Tür"), [0, 3]);
        assert_eq!(fuzzy_indices("kc", "Küche"), [0, 2]);
        assert!(fuzzy("", "anything").is_some());
    }
}
