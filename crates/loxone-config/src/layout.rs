//! Page geometry: what Loxone Config draws on a page — block
//! footprints, port rows, wires, off-page labels — and the placement of
//! managed blocks that have no position yet.
//!
//! The footprint model is learned from GUI-drawn objects in real
//! configs (docs/loxone-format.md, *Page geometry*). Loxone Config
//! recomputes extents on every save, so the model only has to be close;
//! what placement must get right is room — no overlaps — and wires that
//! run left to right, close to what they connect.
//!
//! Placement is deterministic (integer arithmetic, ordered collections,
//! ties broken by source order) and incremental: it moves only the
//! objects it is handed, everything else on the page is an obstacle.
//! It keeps to the paper sheet the GUI draws the page on.

use crate::connectors::{PortDir, builtin};
use crate::doc::{LoxoneDoc, ports};
pub use crate::lock::Layout;
use crate::xml::Element;
use std::collections::{BTreeMap, BTreeSet};

/// Canvas grid: Loxone Config snaps positions to multiples of 96 on save.
pub const GRID: i64 = 96;
/// One port row.
pub const ROW: i64 = 192;
/// Footprint width of a ref tag (`InputRef`/`OutputRef`) or `Memory`.
pub const TAG_W: i64 = 2112;
/// The GUI's distance between a ref tag and the block it serves.
pub const GAP: i64 = 960;
/// Room reserved beside a port whose connection leaves the page: the GUI
/// draws the far object's name there instead of a wire.
pub const LABEL_W: i64 = TAG_W;
/// Clearance kept around a placed cluster.
const MARGIN: i64 = 384;
/// Vertical gap between blocks stacked in one column.
const STACK_GAP: i64 = 2 * ROW;
/// Placement cost of one wire running right to left — it loops around.
const BACKWARD_COST: i64 = 4 * GAP;
/// A cluster's (column gap, stacking gap), tried in order until it fits
/// on the sheet: the GUI's own spacing first, then tighter.
const SPACINGS: [(i64, i64); 3] = [(GAP, STACK_GAP), (6 * GRID, STACK_GAP), (4 * GRID, ROW)];
/// The drawable part of an A3 landscape sheet (420 × 297 mm in twips,
/// on the grid), above the title block the GUI prints along its bottom.
const A3_LANDSCAPE: Layout = Layout {
    px: 0,
    py: 0,
    px2: 23808,
    py2: 16032,
};

impl Layout {
    pub fn at(x: i64, y: i64, w: i64, h: i64) -> Layout {
        Layout {
            px: x,
            py: y,
            px2: x + w,
            py2: y + h,
        }
    }

    /// The drawing rectangle an element carries (`Px`/`Py`/`Px2`/`Py2`).
    pub fn of_element(el: &Element) -> Option<Layout> {
        let coord = |name: &str| el.attr(name)?.parse().ok();
        Some(Layout {
            px: coord("Px")?,
            py: coord("Py")?,
            px2: coord("Px2")?,
            py2: coord("Py2")?,
        })
    }

    pub fn w(&self) -> i64 {
        self.px2 - self.px
    }

    pub fn h(&self) -> i64 {
        self.py2 - self.py
    }

    /// Positive-area overlap.
    pub fn intersects(&self, o: &Layout) -> bool {
        self.px < o.px2 && o.px < self.px2 && self.py < o.py2 && o.py < self.py2
    }

    /// Lies entirely inside `o`.
    pub fn within(&self, o: &Layout) -> bool {
        o.px <= self.px && self.px2 <= o.px2 && o.py <= self.py && self.py2 <= o.py2
    }

    pub fn shift(&self, dx: i64, dy: i64) -> Layout {
        Layout::at(self.px + dx, self.py + dy, self.w(), self.h())
    }

    /// The same position as far as the GUI is concerned. Loxone Config
    /// snaps `Py` to the grid and recomputes the extents on every save, so
    /// only a move of a grid unit or more is a drag.
    pub fn same_spot(&self, o: &Layout) -> bool {
        (self.px - o.px).abs() < GRID && (self.py - o.py).abs() < GRID
    }

    fn grow(&self, left: i64, right: i64, vertical: i64) -> Layout {
        Layout {
            px: self.px - left,
            py: self.py - vertical,
            px2: self.px2 + right,
            py2: self.py2 + vertical,
        }
    }

    fn union(&self, o: &Layout) -> Layout {
        Layout {
            px: self.px.min(o.px),
            py: self.py.min(o.py),
            px2: self.px2.max(o.px2),
            py2: self.py2.max(o.py2),
        }
    }
}

fn snap(v: i64) -> i64 {
    v.div_euclid(GRID) * GRID
}

/// Where the GUI's paper sheet lets a page be drawn, from the document's
/// `PaperFormat` (one sheet for every page). Only A3 landscape (`2`, the
/// format of nearly every config in the corpus, and the largest the GUI
/// offers) is known; any other format leaves pages unbounded.
pub fn sheet(doc: &LoxoneDoc) -> Option<Layout> {
    match doc.document()?.attr("PaperFormat")? {
        "2" => Some(A3_LANDSCAPE),
        _ => None,
    }
}

/// Less than a grid unit past the edge is on the sheet: the GUI draws a
/// block as tall as its rows, wherever it was dropped.
fn on_sheet(r: &Layout, sheet: &Layout) -> bool {
    r.within(&sheet.grow(GRID - 1, GRID - 1, GRID - 1))
}

/// Of the objects with these UUIDs, those drawn past the edge of the
/// sheet, labels included, by the title of their page — what
/// [`PageGeometry::off_sheet`] says of each page that holds one. Empty
/// when the sheet is unknown.
pub fn off_sheet(doc: &LoxoneDoc, uuids: &BTreeSet<&str>) -> BTreeMap<String, Vec<String>> {
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    if sheet(doc).is_none() {
        return out;
    }
    let pages: BTreeSet<String> = doc
        .objects()
        .into_iter()
        .filter(|o| uuids.contains(o.uuid.as_str()))
        .filter_map(|o| {
            let page = doc.element_at(&o.path[..o.path.len().checked_sub(1)?])?;
            (page.name == "C" && page.attr("Type") == Some("Page"))
                .then(|| page.attr("U").map(str::to_string))?
        })
        .collect();
    for page in &pages {
        let Some(geo) = PageGeometry::of(doc, page) else {
            continue;
        };
        for i in geo.off_sheet() {
            if uuids.contains(geo.objects[i].uuid.as_str()) {
                out.entry(geo.title.clone())
                    .or_default()
                    .push(geo.objects[i].uuid.clone());
            }
        }
    }
    out
}

/// Of the objects with these UUIDs, those `out` places other than `base`
/// does (new ones included): what a compile put where it is. One that
/// keeps its place was put there in Loxone Config.
pub fn placed<'a>(
    base: &LoxoneDoc,
    out: &LoxoneDoc,
    uuids: &BTreeSet<&'a str>,
) -> BTreeSet<&'a str> {
    let at = |doc: &LoxoneDoc| -> BTreeMap<String, (String, String)> {
        doc.objects()
            .into_iter()
            .filter_map(|o| {
                let el = doc.element_at(&o.path)?;
                let xy = (el.attr("Px")?.to_string(), el.attr("Py")?.to_string());
                Some((o.uuid.clone(), xy))
            })
            .collect()
    };
    let (before, after) = (at(base), at(out));
    uuids
        .iter()
        .copied()
        .filter(|u| before.get(*u).is_none_or(|b| after.get(*u) != Some(b)))
        .collect()
}

/// Whether `tag` sits beside `block` the way the GUI sets a ref next to
/// the port it serves: level with the block, at most two gaps off its
/// edge. A ref drawn there was arranged with the block.
pub fn beside(block: &Layout, tag: &Layout) -> bool {
    let apart = (tag.px - block.px2).max(block.px - tag.px2);
    apart <= 2 * GAP && tag.py < block.py2 + ROW && block.py < tag.py2 + ROW
}

/// The three ways the GUI draws an object.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    /// A flat one-row tag: refs and `Memory`.
    Tag,
    /// A 1344-wide logic block: gates, comparators, timers, `Formula`.
    Small,
    /// A 2688-wide function block.
    Large,
}

impl Shape {
    /// The shape of a block type; unknown types draw large.
    pub fn of_type(block_type: &str) -> Shape {
        match block_type {
            "InputRef" | "OutputRef" | "OutputRefLM" | "Memory" => Shape::Tag,
            "And"
            | "Or"
            | "Not"
            | "Equal"
            | "NotEqual"
            | "Greater"
            | "GreaterEqual"
            | "Less"
            | "LessEqual"
            | "Formula"
            | "Monoflop"
            | "PulseGen"
            | "AnalogThresholdTrigger"
            | "Switch2Button" => Shape::Small,
            _ => Shape::Large,
        }
    }

    /// The shape of a drawn object of a type outside the builtin table,
    /// read off the extents the GUI wrote.
    fn of_rect(r: &Layout) -> Shape {
        if r.h() <= ROW {
            Shape::Tag
        } else if r.w() <= Shape::Small.width() {
            Shape::Small
        } else {
            Shape::Large
        }
    }

    pub fn width(self) -> i64 {
        match self {
            Shape::Tag => TAG_W,
            Shape::Small => 1344,
            Shape::Large => 2688,
        }
    }

    /// Offset of the first port row below the top edge.
    fn header(self) -> i64 {
        match self {
            Shape::Tag => 0,
            Shape::Small => 288,
            Shape::Large => 480,
        }
    }

    pub fn height(self, rows: i64) -> i64 {
        match self {
            Shape::Tag => ROW,
            Shape::Small => 312 + ROW * rows.max(1),
            Shape::Large => 504 + ROW * rows.max(1),
        }
    }
}

/// Function blocks whose descriptor is far larger than what the GUI shows:
/// it draws this many rows by default (corpus mode) and otherwise only the
/// connectors in use. Every other builtin type shows its whole descriptor
/// except `Remanence` and parameters, unless those are wired.
fn default_rows(block_type: &str) -> Option<i64> {
    Some(match block_type {
        "LightController2" => 6,
        "AutoJalousie" | "CentralShade" | "CentralLight" | "Code16" => 4,
        "DayTimer" => 3,
        "PButtonT" | "PulseAt" => 2,
        _ => return None,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
}

/// A connector the GUI draws.
#[derive(Debug, Clone)]
pub struct GeoPort {
    pub uuid: String,
    pub key: String,
    pub side: Side,
    /// Row on its side, counted from the top.
    pub row: i64,
}

#[derive(Debug, Clone)]
pub struct GeoObject {
    pub uuid: String,
    pub block_type: String,
    pub title: String,
    pub shape: Shape,
    /// What the GUI draws: for a builtin type its footprint at the
    /// written position (no shorter than written), otherwise the written
    /// extents.
    pub rect: Layout,
    /// The footprint for this type and wiring (width, height).
    pub size: (i64, i64),
    /// Drawn connectors only, in element order.
    pub ports: Vec<GeoPort>,
}

impl GeoObject {
    /// A ref tag: placed beside the port it serves, not in a cluster.
    /// (`Memory` draws as a tag too, but sits in its chain like a block.)
    pub fn is_ref(&self) -> bool {
        matches!(
            self.block_type.as_str(),
            "InputRef" | "OutputRef" | "OutputRefLM"
        )
    }

    /// Port offset from the object's top-left corner.
    fn port_offset(&self, port: usize, width: i64) -> (i64, i64) {
        let p = &self.ports[port];
        let x = match p.side {
            Side::Left => 0,
            Side::Right => width,
        };
        (x, self.shape.header() + ROW * p.row + ROW / 2)
    }

    /// Port position when the object is drawn at `at`.
    pub fn port_at(&self, port: usize, at: &Layout) -> (i64, i64) {
        let (dx, dy) = self.port_offset(port, at.w());
        (at.px + dx, at.py + dy)
    }

    pub fn port_point(&self, port: usize) -> (i64, i64) {
        self.port_at(port, &self.rect)
    }
}

/// A wire between two objects on the page, oriented from a right-side
/// (source) port to a left-side (sink) port wherever the sides allow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GeoWire {
    /// (object index, port index)
    pub from: (usize, usize),
    pub to: (usize, usize),
}

/// A connection to an object that is not on the page: the GUI writes the
/// far object's title at the port instead of drawing a wire.
#[derive(Debug, Clone)]
pub struct GeoLabel {
    pub at: (usize, usize),
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct PageGeometry {
    pub page_uuid: String,
    pub title: String,
    pub objects: Vec<GeoObject>,
    pub wires: Vec<GeoWire>,
    pub labels: Vec<GeoLabel>,
    /// The drawable part of the paper sheet, if its format is known.
    pub sheet: Option<Layout>,
}

impl PageGeometry {
    /// The geometry of the page with this UUID, or `None` if there is none.
    pub fn of(doc: &LoxoneDoc, page_uuid: &str) -> Option<PageGeometry> {
        let all = doc.objects();
        let page = all
            .iter()
            .find(|o| o.block_type == "Page" && o.uuid == page_uuid)?;
        let page_el = doc.element_at(&page.path)?;
        let idx = doc.index();
        let wires = doc.wires();
        let sources: BTreeSet<&str> = wires.iter().map(|w| w.from_port.as_str()).collect();
        let title_of: BTreeMap<&str, String> = all
            .iter()
            .map(|o| (o.uuid.as_str(), o.title.clone().unwrap_or_default()))
            .collect();

        let mut objects = Vec::new();
        let mut port_at: BTreeMap<String, (usize, usize)> = BTreeMap::new();
        for el in page_el.child_elements().filter(|c| c.name == "C") {
            let (Some(written), Some(uuid)) = (Layout::of_element(el), el.attr("U")) else {
                continue;
            };
            let block_type = el.attr("Type").unwrap_or("").to_string();
            let spec = builtin(&block_type);
            let shape = match spec {
                Some(_) => Shape::of_type(&block_type),
                None => Shape::of_rect(&written),
            };
            let subset = default_rows(&block_type);
            let mut geo_ports = Vec::new();
            let mut rows = [0i64; 2];
            for p in ports(el) {
                let fed = !p.inputs.is_empty();
                let wired = fed || sources.contains(p.uuid.as_str());
                let dir = spec.and_then(|s| s.iter().find(|s| s.key == p.key).map(|s| s.dir));
                let side = match dir {
                    Some(PortDir::Input | PortDir::Param) => Side::Left,
                    Some(PortDir::Output | PortDir::Api) => Side::Right,
                    // Unknown type: outputs are named so (`outlbt`,
                    // `OutputAPI` — whose API links sit on the output
                    // side, so "fed" says nothing there); otherwise a fed
                    // connector is an input.
                    None if p.key.to_ascii_lowercase().starts_with("out") => Side::Right,
                    None if fed => Side::Left,
                    None => Side::Right,
                };
                let shown = match (shape, dir, subset) {
                    (Shape::Tag, ..) => true,
                    (_, Some(PortDir::Param), None) => wired,
                    (_, Some(_), None) => wired || p.key != "Remanence",
                    _ => wired,
                };
                if !shown {
                    continue;
                }
                let slot = &mut rows[(side == Side::Right) as usize];
                let row = if shape == Shape::Tag { 0 } else { *slot };
                *slot += 1;
                port_at.insert(p.uuid.clone(), (objects.len(), geo_ports.len()));
                geo_ports.push(GeoPort {
                    uuid: p.uuid,
                    key: p.key,
                    side,
                    row,
                });
            }
            let size = if spec.is_some() {
                let rows = rows[0].max(rows[1]).max(subset.unwrap_or(0));
                (shape.width(), shape.height(rows))
            } else {
                (written.w(), written.h())
            };
            // The GUI draws a builtin at its footprint wherever it was
            // written; a written height above the model (rows the model
            // does not show) still takes room.
            let rect = match spec {
                Some(_) if written.w() == size.0 => {
                    Layout::at(written.px, written.py, size.0, size.1.max(written.h()))
                }
                Some(_) => Layout::at(written.px, written.py, size.0, size.1),
                None => written,
            };
            objects.push(GeoObject {
                uuid: uuid.to_string(),
                title: el
                    .attr_decoded("Title")
                    .map(|t| t.into_owned())
                    .unwrap_or_else(|| block_type.clone()),
                block_type,
                shape,
                rect,
                size,
                ports: geo_ports,
            });
        }

        let mut geo_wires: Vec<GeoWire> = Vec::new();
        let mut seen: BTreeSet<((usize, usize), (usize, usize))> = BTreeSet::new();
        let mut labels: Vec<GeoLabel> = Vec::new();
        let mut labeled: BTreeSet<((usize, usize), String)> = BTreeSet::new();
        let far_title = |port: &str| -> String {
            idx.port_owner
                .get(port)
                .and_then(|(owner, _)| title_of.get(owner.as_str()).cloned())
                .unwrap_or_default()
        };
        for w in &wires {
            match (port_at.get(&w.from_port), port_at.get(&w.to_port)) {
                (Some(&f), Some(&t)) if f.0 != t.0 => {
                    let side = |(o, p): (usize, usize)| objects[o].ports[p].side;
                    let (f, t) = if side(f) == Side::Left && side(t) == Side::Right {
                        (t, f)
                    } else {
                        (f, t)
                    };
                    if seen.insert((f.min(t), f.max(t))) {
                        geo_wires.push(GeoWire { from: f, to: t });
                    }
                }
                (Some(&at), None) | (None, Some(&at)) => {
                    // A ref tag *is* the drawing of its far end.
                    if objects[at.0].shape == Shape::Tag {
                        continue;
                    }
                    let far = if port_at.contains_key(&w.from_port) {
                        &w.to_port
                    } else {
                        &w.from_port
                    };
                    let text = far_title(far);
                    if labeled.insert((at, text.clone())) {
                        labels.push(GeoLabel { at, text });
                    }
                }
                _ => {}
            }
        }
        Some(PageGeometry {
            page_uuid: page_uuid.to_string(),
            title: page.title.clone().unwrap_or_default(),
            objects,
            wires: geo_wires,
            labels,
            sheet: sheet(doc),
        })
    }

    /// The UUIDs of every page in the document with this title.
    pub fn pages_titled(doc: &LoxoneDoc, title: &str) -> Vec<String> {
        doc.objects()
            .into_iter()
            .filter(|o| o.block_type == "Page" && o.title.as_deref() == Some(title))
            .map(|o| o.uuid)
            .collect()
    }

    pub fn index_of(&self, uuid: &str) -> Option<usize> {
        self.objects.iter().position(|o| o.uuid == uuid)
    }

    /// Objects drawn past the edge of the sheet, labels included.
    pub fn off_sheet(&self) -> Vec<usize> {
        let Some(sheet) = self.sheet else {
            return Vec::new();
        };
        (0..self.objects.len())
            .filter(|&i| {
                let r = &self.objects[i].rect;
                !std::iter::once(*r)
                    .chain(self.label_rects(i, r))
                    .all(|r| on_sheet(&r, &sheet))
            })
            .collect()
    }

    /// Room an object takes beyond its rectangle for off-page labels:
    /// (left, right).
    fn label_room(&self) -> Vec<(i64, i64)> {
        let mut room = vec![(0, 0); self.objects.len()];
        for l in &self.labels {
            let (o, p) = l.at;
            match self.objects[o].ports[p].side {
                Side::Left => room[o].0 = LABEL_W,
                Side::Right => room[o].1 = LABEL_W,
            }
        }
        room
    }

    /// Where the labels of object `o` are drawn when it sits at `at`.
    pub fn label_rects(&self, o: usize, at: &Layout) -> Vec<Layout> {
        self.labels
            .iter()
            .filter(|l| l.at.0 == o)
            .map(|l| {
                let (x, y) = self.objects[o].port_at(l.at.1, at);
                let x = match self.objects[o].ports[l.at.1].side {
                    Side::Left => x - LABEL_W,
                    Side::Right => x,
                };
                Layout::at(x, y - ROW / 2, LABEL_W, ROW)
            })
            .collect()
    }
}

/// Positions for the `free` objects of `geo` (object indexes, in source
/// order); every other object stays put and is an obstacle. `prev` holds
/// a free object's previous position; a cluster stays where its first
/// block was when that spot is within one row of the best.
///
/// Free blocks connected to each other are laid out together as a small
/// layered graph — feedback broken in source order, columns by longest
/// path, rows aligned to what feeds them — and the whole cluster goes to
/// the free spot that keeps its wires to the rest of the page shortest
/// and running left to right. Free ref tags go beside the port they
/// serve, stacked like the GUI stacks them.
///
/// Everything stays on the page's sheet: a cluster with no room
/// there at the GUI's spacing is packed tighter, and only one that fits
/// nowhere even then goes past the edge, at the GUI's spacing.
pub fn place(
    geo: &PageGeometry,
    free: &[usize],
    prev: &BTreeMap<usize, Layout>,
) -> BTreeMap<usize, Layout> {
    let is_free: BTreeSet<usize> = free.iter().copied().collect();
    let is_ref = |i: usize| geo.objects[i].is_ref();
    let labels = geo.label_room();

    // Where every object is (fixed ones now, free ones once placed), and
    // what a cluster must keep clear of.
    let mut at: BTreeMap<usize, Layout> = BTreeMap::new();
    let mut hard: Vec<Layout> = Vec::new();
    for (i, o) in geo.objects.iter().enumerate() {
        if !is_free.contains(&i) {
            at.insert(i, o.rect);
            hard.push(o.rect);
            hard.extend(geo.label_rects(i, &o.rect));
        }
    }
    let mut reserved: Vec<Layout> = Vec::new();

    // Room a free block keeps for the free tags it will receive.
    let mut tag_room = vec![(0i64, 0i64); geo.objects.len()];
    for w in &geo.wires {
        for (tag, other) in [(w.from, w.to), (w.to, w.from)] {
            if is_free.contains(&tag.0) && is_ref(tag.0) && !is_ref(other.0) {
                let room = GAP + TAG_W;
                match geo.objects[other.0].ports[other.1].side {
                    Side::Left => tag_room[other.0].0 = room,
                    Side::Right => tag_room[other.0].1 = room,
                }
            }
        }
    }

    // Clusters: free blocks joined by wires, in source order of their
    // first member.
    let blocks: Vec<usize> = free.iter().copied().filter(|&i| !is_ref(i)).collect();
    let mut comp: BTreeMap<usize, usize> = blocks.iter().map(|&i| (i, i)).collect();
    fn root(comp: &mut BTreeMap<usize, usize>, i: usize) -> usize {
        let p = comp[&i];
        if p == i {
            return i;
        }
        let r = root(comp, p);
        comp.insert(i, r);
        r
    }
    for w in &geo.wires {
        if comp.contains_key(&w.from.0) && comp.contains_key(&w.to.0) {
            let (a, b) = (root(&mut comp, w.from.0), root(&mut comp, w.to.0));
            if a != b {
                comp.insert(a.max(b), a.min(b));
            }
        }
    }
    let mut clusters: Vec<Vec<usize>> = Vec::new();
    let mut slot: BTreeMap<usize, usize> = BTreeMap::new();
    for &i in &blocks {
        let r = root(&mut comp, i);
        let k = *slot.entry(r).or_insert_with(|| {
            clusters.push(Vec::new());
            clusters.len() - 1
        });
        clusters[k].push(i);
    }

    let room = |i: usize| {
        (
            labels[i].0.max(tag_room[i].0),
            labels[i].1.max(tag_room[i].1),
        )
    };
    let mut out = BTreeMap::new();
    for nodes in &clusters {
        let settle_at = |(gap, stack): (i64, i64), bound: Option<Layout>| {
            let rel = layer_cluster(geo, nodes, &room, gap, stack);
            settle(geo, nodes, &rel, &room, &at, &hard, &reserved, prev, bound)
        };
        let placed = geo
            .sheet
            .and_then(|sheet| {
                SPACINGS
                    .iter()
                    .find_map(|&spacing| settle_at(spacing, Some(sheet)))
            })
            .or_else(|| settle_at(SPACINGS[0], None))
            .expect("an unbounded page has room below its content");
        for (&i, r) in &placed {
            at.insert(i, *r);
            hard.push(*r);
            hard.extend(geo.label_rects(i, r));
            reserved.push(r.grow(tag_room[i].0, tag_room[i].1, 0));
            out.insert(i, *r);
        }
    }

    // Tags beside their partner's port.
    for &t in free.iter().filter(|&&i| is_ref(i)) {
        let partner = geo.wires.iter().find_map(|w| {
            let other = if w.from.0 == t {
                w.to
            } else if w.to.0 == t {
                w.from
            } else {
                return None;
            };
            (!is_ref(other.0) && at.contains_key(&other.0)).then_some(other)
        });
        let (w, h) = geo.objects[t].size;
        // A tag keeps to the sheet unless its block is past the edge: then
        // it stays with the block.
        let bound = geo
            .sheet
            .filter(|s| partner.is_none_or(|(o, _)| on_sheet(&at[&o], s)));
        let spot = partner.and_then(|(o, p)| {
            let (x, y) = geo.objects[o].port_at(p, &at[&o]);
            let x = match geo.objects[o].ports[p].side {
                Side::Right => x + GAP,
                Side::Left => x - GAP - w,
            };
            let mut r = Layout::at(snap(x), snap(y - ROW / 2), w, h);
            for _ in 0..256 {
                let on_page = bound.map_or(r.px >= 0, |s| r.within(&s));
                if on_page && !hard.iter().any(|o| o.intersects(&r)) {
                    return Some(r);
                }
                r = r.shift(0, ROW);
            }
            None
        });
        let r = spot.unwrap_or_else(|| {
            let rel = BTreeMap::from([(t, Layout::at(0, 0, w, h))]);
            bound
                .and_then(|s| settle(geo, &[t], &rel, &room, &at, &hard, &reserved, prev, Some(s)))
                .or_else(|| settle(geo, &[t], &rel, &room, &at, &hard, &reserved, prev, None))
                .expect("an unbounded page has room below its content")[&t]
        });
        at.insert(t, r);
        hard.push(r);
        out.insert(t, r);
    }
    out
}

/// A cluster's relative layout: columns by longest path over the wires
/// (feedback edges broken by a depth-first walk in source order), order
/// within a column by barycenter sweeps, each block aligned with the
/// ports that feed it. Columns are `gap` apart, blocks in a column
/// `stack_gap`.
fn layer_cluster(
    geo: &PageGeometry,
    nodes: &[usize],
    room: &dyn Fn(usize) -> (i64, i64),
    gap: i64,
    stack_gap: i64,
) -> BTreeMap<usize, Layout> {
    let local: BTreeMap<usize, usize> = nodes.iter().enumerate().map(|(k, &i)| (i, k)).collect();
    let n = nodes.len();
    // (u, u's port, v, v's port), local indexes, deduplicated.
    let mut edges: Vec<(usize, usize, usize, usize)> = geo
        .wires
        .iter()
        .filter_map(|w| {
            Some((
                *local.get(&w.from.0)?,
                w.from.1,
                *local.get(&w.to.0)?,
                w.to.1,
            ))
        })
        .collect();
    edges.sort();
    edges.dedup();

    // Feedback: an edge into a node still on the DFS stack points back.
    let mut succ: Vec<Vec<usize>> = vec![Vec::new(); n];
    for &(u, _, v, _) in &edges {
        succ[u].push(v);
    }
    for s in &mut succ {
        s.sort();
        s.dedup();
    }
    let mut state = vec![0u8; n];
    let mut back: BTreeSet<(usize, usize)> = BTreeSet::new();
    for start in 0..n {
        if state[start] != 0 {
            continue;
        }
        let mut stack = vec![(start, 0usize)];
        state[start] = 1;
        while let Some(top) = stack.len().checked_sub(1) {
            let (u, next) = stack[top];
            if let Some(&v) = succ[u].get(next) {
                stack[top].1 += 1;
                match state[v] {
                    0 => {
                        state[v] = 1;
                        stack.push((v, 0));
                    }
                    1 => {
                        back.insert((u, v));
                    }
                    _ => {}
                }
            } else {
                state[u] = 2;
                stack.pop();
            }
        }
    }
    let dag: BTreeSet<(usize, usize)> = edges
        .iter()
        .filter(|&&(u, _, v, _)| u != v)
        .map(|&(u, _, v, _)| {
            if back.contains(&(u, v)) {
                (v, u)
            } else {
                (u, v)
            }
        })
        .collect();

    // Longest-path columns (Kahn, smallest index first).
    let mut indeg = vec![0usize; n];
    for &(_, v) in &dag {
        indeg[v] += 1;
    }
    let mut ready: BTreeSet<usize> = (0..n).filter(|&k| indeg[k] == 0).collect();
    let mut col = vec![0usize; n];
    while let Some(u) = ready.pop_first() {
        for &(a, v) in dag.iter().filter(|&&(a, _)| a == u) {
            col[v] = col[v].max(col[a] + 1);
            indeg[v] -= 1;
            if indeg[v] == 0 {
                ready.insert(v);
            }
        }
    }
    let ncols = col.iter().max().map_or(0, |m| m + 1);
    let mut columns: Vec<Vec<usize>> = vec![Vec::new(); ncols];
    for k in 0..n {
        columns[col[k]].push(k);
    }

    // Barycenter sweeps: down by predecessors, up by successors.
    let mut pos = vec![0i64; n];
    let reindex = |columns: &Vec<Vec<usize>>, pos: &mut Vec<i64>| {
        for c in columns {
            for (r, &k) in c.iter().enumerate() {
                pos[k] = r as i64;
            }
        }
    };
    reindex(&columns, &mut pos);
    for _ in 0..4 {
        for downward in [true, false] {
            let order: Vec<usize> = if downward {
                (1..ncols).collect()
            } else {
                (0..ncols.saturating_sub(1)).rev().collect()
            };
            for c in order {
                let mut keyed: Vec<(i64, usize)> = columns[c]
                    .iter()
                    .map(|&k| {
                        let nb: Vec<i64> = dag
                            .iter()
                            .filter_map(|&(u, v)| match downward {
                                true if v == k && col[u] < c => Some(pos[u]),
                                false if u == k && col[v] > c => Some(pos[v]),
                                _ => None,
                            })
                            .collect();
                        let key = if nb.is_empty() {
                            pos[k] * 1000
                        } else {
                            nb.iter().sum::<i64>() * 1000 / nb.len() as i64
                        };
                        (key, k)
                    })
                    .collect();
                keyed.sort_by_key(|&(key, _)| key);
                columns[c] = keyed.into_iter().map(|(_, k)| k).collect();
                reindex(&columns, &mut pos);
            }
        }
    }

    // Columns left to right: each as wide as its widest block, with the
    // wire gap plus whatever labels and tags need on either side.
    let size = |k: usize| geo.objects[nodes[k]].size;
    let mut x = vec![0i64; ncols];
    let mut cursor = 0;
    for c in 0..ncols {
        let left = columns[c]
            .iter()
            .map(|&k| room(nodes[k]).0)
            .max()
            .unwrap_or(0);
        x[c] = cursor + left;
        let right = columns[c]
            .iter()
            .map(|&k| room(nodes[k]).1)
            .max()
            .unwrap_or(0);
        let width = columns[c].iter().map(|&k| size(k).0).max().unwrap_or(0);
        cursor = x[c] + width + right + gap;
    }

    // Rows: each block level with the mean of the ports feeding it, never
    // above the block before it in its column.
    let offset = |k: usize, port: usize| geo.objects[nodes[k]].port_offset(port, size(k).0).1;
    let mut y: Vec<Option<i64>> = vec![None; n];
    for column in &columns {
        let mut floor = 0;
        for &k in column {
            let fed: Vec<i64> = edges
                .iter()
                .filter(|&&(u, _, v, _)| v == k && col[u] < col[k])
                .filter_map(|&(u, pu, _, pv)| Some(y[u]? + offset(u, pu) - offset(k, pv)))
                .collect();
            let want = if fed.is_empty() {
                floor
            } else {
                fed.iter().sum::<i64>() / fed.len() as i64
            };
            let top = snap(want.max(floor) + GRID - 1);
            y[k] = Some(top);
            floor = top + size(k).1 + stack_gap;
        }
    }
    (0..n)
        .map(|k| {
            let (w, h) = size(k);
            (
                nodes[k],
                Layout::at(x[col[k]], y[k].expect("every row assigned"), w, h),
            )
        })
        .collect()
}

/// Moves a relatively laid-out cluster to its best free spot on the page,
/// inside `bound` with its labels and tags — `None` if there is none
/// there. Unbounded, the page's right and lower side go on forever: there
/// is always a spot.
#[allow(clippy::too_many_arguments)]
fn settle(
    geo: &PageGeometry,
    nodes: &[usize],
    rel: &BTreeMap<usize, Layout>,
    room: &dyn Fn(usize) -> (i64, i64),
    at: &BTreeMap<usize, Layout>,
    hard: &[Layout],
    reserved: &[Layout],
    prev: &BTreeMap<usize, Layout>,
    bound: Option<Layout>,
) -> Option<BTreeMap<usize, Layout>> {
    let members: BTreeSet<usize> = nodes.iter().copied().collect();
    let bbox = rel
        .iter()
        .map(|(&i, r)| {
            let (l, rt) = room(i);
            r.grow(l, rt, 0)
        })
        .reduce(|a, b| a.union(&b))
        .expect("a cluster has members");

    // Wires to objects already in place: (member, its port, fixed point,
    // member is the source).
    let mut anchors: Vec<(usize, usize, (i64, i64), bool)> = Vec::new();
    for w in &geo.wires {
        for (mine, other, source) in [(w.from, w.to, true), (w.to, w.from, false)] {
            if members.contains(&mine.0)
                && !members.contains(&other.0)
                && let Some(r) = at.get(&other.0)
            {
                anchors.push((
                    mine.0,
                    mine.1,
                    geo.objects[other.0].port_at(other.1, r),
                    source,
                ));
            }
        }
    }
    let port =
        |i: usize, p: usize, dx: i64, dy: i64| geo.objects[i].port_at(p, &rel[&i].shift(dx, dy));

    let content = hard.iter().copied().reduce(|a, b| a.union(&b));
    let home = match content {
        Some(c) => (c.px.max(0), c.py2 + 2 * MARGIN),
        None => (GAP, GAP),
    };
    let mut xs: BTreeSet<i64> = BTreeSet::new();
    let mut ys: BTreeSet<i64> = BTreeSet::new();
    xs.insert(home.0 - bbox.px);
    ys.insert(home.1 - bbox.py);
    for &(i, p, (fx, fy), source) in &anchors {
        let (px, py) = port(i, p, 0, 0);
        xs.insert(if source { fx - GAP - px } else { fx + GAP - px });
        ys.insert(fy - py);
    }
    for o in hard.iter().chain(reserved) {
        xs.insert(o.px2 + MARGIN - bbox.px);
        xs.insert(o.px - MARGIN - bbox.px2);
        ys.insert(o.py2 + MARGIN - bbox.py);
        ys.insert(o.py - MARGIN - bbox.py2);
    }
    if let Some(b) = bound {
        xs.insert(b.px - bbox.px);
        xs.insert(b.px2 - bbox.px2);
        ys.insert(b.py - bbox.py);
        ys.insert(b.py2 - bbox.py2);
    }
    let stay = nodes
        .first()
        .and_then(|i| Some((prev.get(i)?.px - rel[i].px, prev.get(i)?.py - rel[i].py)));
    if let Some((sx, sy)) = stay {
        xs.insert(sx);
        ys.insert(sy);
    }

    // Unwired, a cluster goes below the page's content — a relayout pulls
    // it there too. Where its first block was is worth one row: a spot
    // about as good keeps it.
    let target = (home.0 - bbox.px, home.1 - bbox.py);
    let cost = |dx: i64, dy: i64| -> i64 {
        let mut c = if anchors.is_empty() {
            (target.0 - dx).abs() + (target.1 - dy).abs()
        } else {
            anchors
                .iter()
                .map(|&(i, p, (fx, fy), source)| {
                    let (px, py) = port(i, p, dx, dy);
                    let run = if source { fx - px } else { px - fx };
                    (run - GAP).abs() + (fy - py).abs() + if run < 0 { BACKWARD_COST } else { 0 }
                })
                .sum()
        };
        if stay == Some((dx, dy)) {
            c -= ROW;
        }
        c
    };
    let mut candidates: Vec<(i64, i64, i64)> = Vec::new();
    for &dy in &ys {
        for &dx in &xs {
            let (dx, dy) = (snap(dx), snap(dy));
            let fits = match bound {
                Some(b) => bbox.shift(dx, dy).within(&b),
                None => bbox.px + dx >= 0 && bbox.py + dy >= 0,
            };
            if !fits {
                continue;
            }
            candidates.push((cost(dx, dy), dy, dx));
        }
    }
    candidates.sort();
    candidates.dedup();
    let clear = |dx: i64, dy: i64| {
        let b = bbox.shift(dx, dy).grow(MARGIN, MARGIN, MARGIN);
        !hard.iter().chain(reserved).any(|o| o.intersects(&b))
    };
    let (dx, dy) = match candidates.iter().find(|&&(_, dy, dx)| clear(dx, dy)) {
        Some(&(_, dy, dx)) => (dx, dy),
        None if bound.is_some() => return None,
        None => {
            // Below everything is always clear.
            let floor = content.map_or(0, |c| c.py2) + 2 * MARGIN;
            (snap(home.0 - bbox.px), snap(floor - bbox.py + GRID - 1))
        }
    };
    Some(rel.iter().map(|(&i, r)| (i, r.shift(dx, dy))).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn object(title: &str, shape: Shape, rect: Layout, ports: &[(&str, Side, i64)]) -> GeoObject {
        GeoObject {
            uuid: title.to_string(),
            block_type: String::new(),
            title: title.to_string(),
            shape,
            size: (rect.w(), rect.h()),
            rect,
            ports: ports
                .iter()
                .map(|&(k, side, row)| GeoPort {
                    uuid: format!("{title}.{k}"),
                    key: k.to_string(),
                    side,
                    row,
                })
                .collect(),
        }
    }

    fn small(title: &str) -> GeoObject {
        object(
            title,
            Shape::Small,
            Layout::at(0, 0, 1344, 696),
            &[
                ("I1", Side::Left, 0),
                ("I2", Side::Left, 1),
                ("Q", Side::Right, 0),
            ],
        )
    }

    /// (object, port)
    type End = (usize, usize);

    fn page(objects: Vec<GeoObject>, wires: &[(End, End)]) -> PageGeometry {
        PageGeometry {
            page_uuid: "p".into(),
            title: "P".into(),
            objects,
            wires: wires
                .iter()
                .map(|&(from, to)| GeoWire { from, to })
                .collect(),
            labels: Vec::new(),
            sheet: None,
        }
    }

    #[test]
    fn footprints_match_the_gui() {
        assert_eq!(Shape::Small.height(1), 504); // Not
        assert_eq!(Shape::Small.height(2), 696); // And, Monoflop
        assert_eq!(Shape::Large.height(4), 1272); // PushButton
        assert_eq!(Shape::Large.height(6), 1656); // LightController2
    }

    #[test]
    fn a_chain_runs_left_to_right_beside_its_anchor() {
        // A fixed source tag at the top left feeds a → b → c.
        let fixed = object(
            "src",
            Shape::Tag,
            Layout::at(960, 960, TAG_W, ROW),
            &[("AQ", Side::Right, 0)],
        );
        let geo = page(
            vec![fixed, small("a"), small("b"), small("c")],
            &[((0, 0), (1, 0)), ((1, 2), (2, 0)), ((2, 2), (3, 0))],
        );
        let placed = place(&geo, &[1, 2, 3], &BTreeMap::new());
        let (a, b, c) = (placed[&1], placed[&2], placed[&3]);
        assert!(a.px >= 960 + TAG_W + GAP, "{a:?}");
        assert!(a.px2 < b.px && b.px2 < c.px, "{a:?} {b:?} {c:?}");
        // Straight: each I1 level with the Q feeding it.
        assert_eq!(a.py, b.py);
        assert_eq!(b.py, c.py);
        // a's I1 sits level with the tag's output.
        assert_eq!(geo.objects[1].port_at(0, &a).1, 960 + ROW / 2);
        for r in [a, b, c] {
            assert_eq!(r.px % GRID, 0);
            assert_eq!(r.py % GRID, 0);
        }
    }

    #[test]
    fn placement_avoids_fixed_objects_and_is_deterministic() {
        let wall = object("wall", Shape::Large, Layout::at(0, 0, 9600, 9600), &[]);
        let geo = page(vec![wall, small("a"), small("b")], &[((1, 2), (2, 0))]);
        let first = place(&geo, &[1, 2], &BTreeMap::new());
        for r in first.values() {
            assert!(!r.intersects(&geo.objects[0].rect), "{r:?}");
        }
        assert_eq!(first, place(&geo, &[1, 2], &BTreeMap::new()));
    }

    #[test]
    fn feedback_is_broken_in_source_order() {
        // a → b → a: a comes first in source order, so b → a is the
        // feedback edge and a takes the first column.
        let geo = page(
            vec![small("a"), small("b")],
            &[((0, 2), (1, 0)), ((1, 2), (0, 1))],
        );
        let placed = place(&geo, &[0, 1], &BTreeMap::new());
        assert!(placed[&0].px2 < placed[&1].px);
    }

    #[test]
    fn a_tag_goes_beside_its_port_and_stacks() {
        let block = object(
            "jal",
            Shape::Large,
            Layout::at(960, 960, 2688, 1272),
            &[("OutputAPI", Side::Right, 0)],
        );
        let tag = |t| GeoObject {
            block_type: "OutputRef".into(),
            ..object(
                t,
                Shape::Tag,
                Layout::at(0, 0, TAG_W, ROW),
                &[("AI", Side::Left, 0)],
            )
        };
        let geo = page(
            vec![block, tag("r1"), tag("r2")],
            &[((0, 0), (1, 0)), ((0, 0), (2, 0))],
        );
        let placed = place(&geo, &[1, 2], &BTreeMap::new());
        assert_eq!(
            placed[&1],
            Layout::at(960 + 2688 + GAP, 960 + 480, TAG_W, ROW)
        );
        assert_eq!(placed[&2], placed[&1].shift(0, ROW));
    }

    #[test]
    fn a_memory_flag_sits_in_its_chain() {
        // a → mem → b: the flag is drawn as a tag but is a block of the
        // chain, not a ref placed beside a port.
        let mem = GeoObject {
            block_type: "Memory".into(),
            ..object(
                "mem",
                Shape::Tag,
                Layout::at(0, 0, TAG_W, ROW),
                &[("Input", Side::Left, 0), ("Q", Side::Right, 0)],
            )
        };
        let geo = page(
            vec![small("a"), mem, small("b")],
            &[((0, 2), (1, 0)), ((1, 1), (2, 0))],
        );
        let placed = place(&geo, &[0, 1, 2], &BTreeMap::new());
        let (a, m, b) = (placed[&0], placed[&1], placed[&2]);
        assert!(a.px2 < m.px && m.px2 < b.px, "{a:?} {m:?} {b:?}");
    }

    #[test]
    fn an_object_keeps_a_previous_position_that_is_as_good() {
        // a is fed from two sources a page height apart: any height in
        // between costs the same wire length.
        let mut far = small("src2");
        far.rect = Layout::at(0, 2304, 1344, 696);
        let geo = page(
            vec![small("src1"), far, small("a")],
            &[((0, 2), (2, 0)), ((1, 2), (2, 1))],
        );
        let fresh = place(&geo, &[2], &BTreeMap::new());
        assert_eq!(fresh[&2], Layout::at(1344 + GAP, 0, 1344, 696));
        let prev = BTreeMap::from([(2, Layout::at(1344 + GAP, 960, 1344, 696))]);
        assert_eq!(place(&geo, &[2], &prev)[&2], prev[&2]);
    }

    /// a → b → c, unwired to the rest, beside a wall on a 9600 × 4800
    /// sheet: room for the chain only if its columns close up.
    fn walled_sheet(wall_w: i64) -> PageGeometry {
        let wall = object("wall", Shape::Large, Layout::at(0, 0, wall_w, 4800), &[]);
        PageGeometry {
            sheet: Some(Layout::at(0, 0, 9600, 4800)),
            ..page(
                vec![wall, small("a"), small("b"), small("c")],
                &[((1, 2), (2, 0)), ((2, 2), (3, 0))],
            )
        }
    }

    #[test]
    fn a_cluster_short_of_room_on_the_sheet_closes_up() {
        // 4032..9600 is free: 5568, where the chain at the GUI's spacing
        // takes 3·1344 + 2·960 = 5952.
        let geo = walled_sheet(3648);
        let placed = place(&geo, &[1, 2, 3], &BTreeMap::new());
        let (a, b, c) = (placed[&1], placed[&2], placed[&3]);
        let sheet = geo.sheet.unwrap();
        for r in [a, b, c] {
            assert!(r.within(&sheet), "{r:?}");
            assert!(!r.intersects(&geo.objects[0].rect), "{r:?}");
        }
        assert!(a.px2 < b.px && b.px2 < c.px, "{a:?} {b:?} {c:?}");
        assert!(b.px - a.px2 < GAP, "{a:?} {b:?}");
        // Unbounded, the same chain keeps the GUI's spacing below the wall.
        let open = PageGeometry { sheet: None, ..geo };
        let placed = place(&open, &[1, 2, 3], &BTreeMap::new());
        assert_eq!(placed[&2].px - placed[&1].px2, GAP);
        assert!(placed[&1].py >= 4800, "{:?}", placed[&1]);
    }

    #[test]
    fn a_cluster_that_fits_nowhere_on_the_sheet_goes_past_it() {
        let geo = walled_sheet(9600);
        let placed = place(&geo, &[1, 2, 3], &BTreeMap::new());
        let (a, b) = (placed[&1], placed[&2]);
        for r in placed.values() {
            assert!(!r.intersects(&geo.objects[0].rect), "{r:?}");
        }
        assert!(!a.within(&geo.sheet.unwrap()), "{a:?}");
        // Past the edge at the GUI's spacing, as if there were no sheet.
        assert_eq!(b.px - a.px2, GAP);
        assert_eq!(placed, place(&geo, &[1, 2, 3], &BTreeMap::new()));
    }

    #[test]
    fn a_tag_with_no_room_beside_its_port_stays_on_the_sheet() {
        // The block's right edge is the sheet's: a tag beside its output
        // would hang off it.
        let block = object(
            "jal",
            Shape::Large,
            Layout::at(6912, 960, 2688, 1272),
            &[("OutputAPI", Side::Right, 0)],
        );
        let tag = GeoObject {
            block_type: "OutputRef".into(),
            ..object(
                "r1",
                Shape::Tag,
                Layout::at(0, 0, TAG_W, ROW),
                &[("AI", Side::Left, 0)],
            )
        };
        let geo = PageGeometry {
            sheet: Some(Layout::at(0, 0, 9600, 4800)),
            ..page(vec![block, tag], &[((0, 0), (1, 0))])
        };
        let r = place(&geo, &[1], &BTreeMap::new())[&1];
        assert!(r.within(&geo.sheet.unwrap()), "{r:?}");
        assert!(!r.intersects(&geo.objects[0].rect), "{r:?}");
    }

    #[test]
    fn a_tag_of_a_block_past_the_edge_stays_with_it() {
        let wall = object("wall", Shape::Large, Layout::at(0, 0, 9600, 4800), &[]);
        let tag = GeoObject {
            block_type: "OutputRef".into(),
            ..object(
                "r1",
                Shape::Tag,
                Layout::at(0, 0, TAG_W, ROW),
                &[("AI", Side::Left, 0)],
            )
        };
        let geo = PageGeometry {
            sheet: Some(Layout::at(0, 0, 9600, 4800)),
            ..page(vec![wall, small("a"), tag], &[((1, 2), (2, 0))])
        };
        let placed = place(&geo, &[1, 2], &BTreeMap::new());
        let (a, r) = (placed[&1], placed[&2]);
        assert!(!a.within(&geo.sheet.unwrap()), "{a:?}");
        let (_, y) = geo.objects[1].port_at(2, &a);
        assert_eq!(
            (r.px, r.py),
            (a.px2 + GAP, snap(y - ROW / 2)),
            "{a:?} {r:?}"
        );
    }

    #[test]
    fn off_sheet_counts_labels_and_forgives_less_than_a_grid_unit() {
        let mut near = small("near");
        near.rect = Layout::at(0, 4200, 1344, 696); // past the edge by 96
        let mut fine = small("fine");
        fine.rect = Layout::at(2400, 4128, 1344, 696); // past it by 24
        let mut labeled = small("labeled");
        labeled.rect = Layout::at(8160, 0, 1344, 696); // its output label runs off
        let mut geo = page(vec![near, fine, labeled], &[]);
        geo.sheet = Some(Layout::at(0, 0, 9600, 4800));
        geo.labels.push(GeoLabel {
            at: (2, 2),
            text: "far".into(),
        });
        assert_eq!(geo.off_sheet(), vec![0, 2]);
    }

    #[test]
    fn a_relayout_pulls_an_unwired_block_up_to_the_content() {
        let wall = object("wall", Shape::Large, Layout::at(0, 0, 2688, 1272), &[]);
        let geo = page(vec![wall, small("a")], &[]);
        let prev = BTreeMap::from([(1, Layout::at(9600, 96000, 1344, 696))]);
        let placed = place(&geo, &[1], &prev)[&1];
        assert_eq!(placed.px, 0);
        assert!(
            placed.py > 1272 && placed.py < 1272 + 4 * MARGIN,
            "{placed:?}"
        );
    }
}
