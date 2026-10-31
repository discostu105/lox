//! A page as a picture: SVG drawing and layout metrics of one
//! [`PageGeometry`] — eyes on a layout without opening Loxone Config.
//!
//! Wires are drawn orthogonally (out of the source port, across at the
//! midpoint, into the sink port; a wire running right to left loops out
//! and back). The metrics count what makes a page hard to read: overlaps,
//! backward wires, long wires, crossings.

use crate::layout::{GeoWire, Layout, PageGeometry, ROW, Shape, Side};
use std::collections::BTreeSet;
use std::fmt::Write as _;

/// A wire longer than this (Manhattan, canvas units) counts as long —
/// about the width of two function blocks with their refs.
pub const LONG_WIRE: i64 = 8000;
/// How far a backward wire runs out of its source before turning.
const LOOP: i64 = 240;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PageMetrics {
    pub objects: usize,
    /// Wires drawn between objects on the page.
    pub wires: usize,
    /// Connections to objects elsewhere, drawn as labels.
    pub labels: usize,
    /// Pairs of objects whose rectangles overlap (titles).
    pub overlaps: Vec<(String, String)>,
    /// Wires whose sink lies left of their source, as
    /// (`source.port`, `sink.port`).
    pub backward: Vec<(String, String)>,
    /// Wires longer than [`LONG_WIRE`].
    pub long: usize,
    /// Pairs of wires crossing each other.
    pub crossings: usize,
    /// Sum of all wire lengths.
    pub wire_length: i64,
    /// The page's extent.
    pub extent: Option<Layout>,
    /// The drawable part of the paper sheet, if its format is known.
    pub sheet: Option<Layout>,
    /// Objects drawn past the sheet's edge (titles).
    pub off_sheet: Vec<String>,
}

impl std::fmt::Display for PageMetrics {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(
            f,
            "{} objects, {} wires, {} labels",
            self.objects, self.wires, self.labels
        )?;
        writeln!(
            f,
            "overlaps {}, backward wires {}, long wires {}, crossings {}, total wire length {}",
            self.overlaps.len(),
            self.backward.len(),
            self.long,
            self.crossings,
            self.wire_length
        )?;
        if let Some(e) = self.extent {
            writeln!(f, "extent {}..{} × {}..{}", e.px, e.px2, e.py, e.py2)?;
        }
        if let Some(s) = self.sheet {
            writeln!(
                f,
                "sheet {}..{} × {}..{}, {} objects off it",
                s.px,
                s.px2,
                s.py,
                s.py2,
                self.off_sheet.len()
            )?;
        }
        for (a, b) in &self.overlaps {
            writeln!(f, "  overlap: {a} / {b}")?;
        }
        for (a, b) in &self.backward {
            writeln!(f, "  backward: {a} -> {b}")?;
        }
        for t in &self.off_sheet {
            writeln!(f, "  off sheet: {t}")?;
        }
        Ok(())
    }
}

/// The orthogonal route of a wire: a polyline of points.
fn route(geo: &PageGeometry, w: &GeoWire) -> Vec<(i64, i64)> {
    let (x1, y1) = geo.objects[w.from.0].port_point(w.from.1);
    let (x2, y2) = geo.objects[w.to.0].port_point(w.to.1);
    let leaves_right = geo.objects[w.from.0].ports[w.from.1].side == Side::Right;
    let enters_left = geo.objects[w.to.0].ports[w.to.1].side == Side::Left;
    let out = if leaves_right { x1 + LOOP } else { x1 - LOOP };
    let into = if enters_left { x2 - LOOP } else { x2 + LOOP };
    if leaves_right && enters_left && x2 - x1 >= 2 * LOOP {
        let xm = (x1 + x2) / 2;
        vec![(x1, y1), (xm, y1), (xm, y2), (x2, y2)]
    } else {
        // Loop: out, across between the two objects, back in.
        let a = &geo.objects[w.from.0].rect;
        let b = &geo.objects[w.to.0].rect;
        let ym = if a.py2 <= b.py || b.py2 <= a.py {
            (a.py2.min(b.py2) + a.py.max(b.py)) / 2
        } else {
            a.py2.max(b.py2) + ROW
        };
        vec![
            (x1, y1),
            (out, y1),
            (out, ym),
            (into, ym),
            (into, y2),
            (x2, y2),
        ]
    }
}

fn is_backward(geo: &PageGeometry, w: &GeoWire) -> bool {
    let (x1, _) = geo.objects[w.from.0].port_point(w.from.1);
    let (x2, _) = geo.objects[w.to.0].port_point(w.to.1);
    x2 < x1
}

fn end_name(geo: &PageGeometry, (o, p): (usize, usize)) -> String {
    let o = &geo.objects[o];
    format!("{}.{}", o.title, o.ports[p].key)
}

fn length(points: &[(i64, i64)]) -> i64 {
    points
        .windows(2)
        .map(|s| (s[1].0 - s[0].0).abs() + (s[1].1 - s[0].1).abs())
        .sum()
}

/// Segments as (horizontal?, fixed coordinate, from, to).
fn segments(points: &[(i64, i64)]) -> Vec<(bool, i64, i64, i64)> {
    points
        .windows(2)
        .filter_map(|s| {
            let ((ax, ay), (bx, by)) = (s[0], s[1]);
            if ay == by && ax != bx {
                Some((true, ay, ax.min(bx), ax.max(bx)))
            } else if ax == bx && ay != by {
                Some((false, ax, ay.min(by), ay.max(by)))
            } else {
                None
            }
        })
        .collect()
}

pub fn metrics(geo: &PageGeometry) -> PageMetrics {
    let mut m = PageMetrics {
        objects: geo.objects.len(),
        wires: geo.wires.len(),
        labels: geo.labels.len(),
        extent: geo.objects.iter().map(|o| o.rect).reduce(|a, b| Layout {
            px: a.px.min(b.px),
            py: a.py.min(b.py),
            px2: a.px2.max(b.px2),
            py2: a.py2.max(b.py2),
        }),
        sheet: geo.sheet,
        off_sheet: geo
            .off_sheet()
            .into_iter()
            .map(|i| geo.objects[i].title.clone())
            .collect(),
        ..PageMetrics::default()
    };
    for (i, a) in geo.objects.iter().enumerate() {
        for b in &geo.objects[i + 1..] {
            if a.rect.intersects(&b.rect) {
                m.overlaps.push((a.title.clone(), b.title.clone()));
            }
        }
    }
    let routes: Vec<Vec<(i64, i64)>> = geo.wires.iter().map(|w| route(geo, w)).collect();
    for (w, r) in geo.wires.iter().zip(&routes) {
        let len = length(r);
        m.wire_length += len;
        m.long += (len > LONG_WIRE) as usize;
        if is_backward(geo, w) {
            m.backward
                .push((end_name(geo, w.from), end_name(geo, w.to)));
        }
    }
    // Crossings between wires that share no port (fan-out runs together).
    let segs: Vec<Vec<(bool, i64, i64, i64)>> = routes.iter().map(|r| segments(r)).collect();
    for i in 0..geo.wires.len() {
        for j in i + 1..geo.wires.len() {
            let (a, b) = (&geo.wires[i], &geo.wires[j]);
            let ends: BTreeSet<(usize, usize)> = [a.from, a.to].into_iter().collect();
            if ends.contains(&b.from) || ends.contains(&b.to) {
                continue;
            }
            let crosses = segs[i].iter().any(|s| {
                segs[j].iter().any(|t| {
                    let (h, v) = match (s.0, t.0) {
                        (true, false) => (s, t),
                        (false, true) => (t, s),
                        _ => return false,
                    };
                    // h: y = h.1, x in h.2..h.3; v: x = v.1, y in v.2..v.3
                    h.2 < v.1 && v.1 < h.3 && v.2 < h.1 && h.1 < v.3
                })
            });
            m.crossings += crosses as usize;
        }
    }
    m
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// The page as a standalone SVG document. Objects whose UUID is in
/// `managed` are drawn in the compiler's color.
pub fn svg(geo: &PageGeometry, managed: &BTreeSet<String>) -> String {
    let m = metrics(geo);
    let e = m.extent.unwrap_or(Layout::at(0, 0, 9600, 9600));
    let e = m.sheet.map_or(e, |s| Layout {
        px: e.px.min(s.px),
        py: e.py.min(s.py),
        px2: e.px2.max(s.px2),
        py2: e.py2.max(s.py2),
    });
    let pad = 2400;
    let (x0, y0) = (e.px - pad, e.py - pad - 1200);
    let (w, h) = (e.w() + 2 * pad, e.h() + 2 * pad + 1200);
    let mut s = String::new();
    let _ = writeln!(
        s,
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="{x0} {y0} {w} {h}" width="{}" height="{}" font-family="sans-serif">"##,
        w / 12,
        h / 12
    );
    let _ = writeln!(
        s,
        r##"<rect x="{x0}" y="{y0}" width="{w}" height="{h}" fill="#ffffff"/>"##
    );
    if let Some(sh) = m.sheet {
        let _ = writeln!(
            s,
            r##"<rect x="{}" y="{}" width="{}" height="{}" fill="none" stroke="#94a3b8" stroke-width="48" stroke-dasharray="240 120"/>"##,
            sh.px,
            sh.py,
            sh.w(),
            sh.h()
        );
    }
    let _ = writeln!(
        s,
        r##"<text x="{}" y="{}" font-size="360" fill="#0f172a">{}</text>"##,
        e.px,
        e.py - pad,
        esc(&geo.title)
    );
    let _ = writeln!(
        s,
        r##"<text x="{}" y="{}" font-size="200" fill="#475569">{}</text>"##,
        e.px,
        e.py - pad + 360,
        esc(&format!(
            "overlaps {} · backward {} · long {} · crossings {} · length {}",
            m.overlaps.len(),
            m.backward.len(),
            m.long,
            m.crossings,
            m.wire_length
        ))
    );
    for wire in &geo.wires {
        let pts = route(geo, wire)
            .iter()
            .map(|(x, y)| format!("{x},{y}"))
            .collect::<Vec<_>>()
            .join(" ");
        let color = if is_backward(geo, wire) {
            "#dc2626"
        } else {
            "#475569"
        };
        let _ = writeln!(
            s,
            r##"<polyline points="{pts}" fill="none" stroke="{color}" stroke-width="20"/>"##
        );
    }
    for o in &geo.objects {
        let r = o.rect;
        let ours = managed.contains(&o.uuid);
        let (fill, stroke) = match (o.shape, ours) {
            (Shape::Tag, true) => ("#bfdbfe", "#2563eb"),
            (Shape::Tag, false) => ("#e2e8f0", "#64748b"),
            (_, true) => ("#dbeafe", "#2563eb"),
            (_, false) => ("#f8fafc", "#64748b"),
        };
        let _ = writeln!(
            s,
            r##"<rect x="{}" y="{}" width="{}" height="{}" rx="48" fill="{fill}" stroke="{stroke}" stroke-width="24"><title>{}</title></rect>"##,
            r.px,
            r.py,
            r.w(),
            r.h(),
            esc(&format!("{} ({})", o.title, o.block_type))
        );
        let (tx, ty, size) = match o.shape {
            Shape::Tag => (r.px + 96, r.py + 140, 120),
            _ => (r.px + 96, r.py + 180, 140),
        };
        let _ = writeln!(
            s,
            r##"<text x="{tx}" y="{ty}" font-size="{size}" fill="#0f172a">{}</text>"##,
            esc(&o.title)
        );
        if o.shape != Shape::Tag {
            for (p, port) in o.ports.iter().enumerate() {
                let (x, y) = o.port_point(p);
                let (lx, anchor) = match port.side {
                    Side::Left => (x + 72, "start"),
                    Side::Right => (x - 72, "end"),
                };
                let _ = writeln!(
                    s,
                    r##"<circle cx="{x}" cy="{y}" r="36" fill="{stroke}"/><text x="{lx}" y="{}" font-size="100" text-anchor="{anchor}" fill="#334155">{}</text>"##,
                    y + 34,
                    esc(&port.key)
                );
            }
        }
    }
    for l in &geo.labels {
        let (o, p) = l.at;
        let (x, y) = geo.objects[o].port_point(p);
        let (lx, anchor) = match geo.objects[o].ports[p].side {
            Side::Left => (x - 72, "end"),
            Side::Right => (x + 72, "start"),
        };
        let _ = writeln!(
            s,
            r##"<text x="{lx}" y="{}" font-size="100" font-style="italic" text-anchor="{anchor}" fill="#7c3aed">{}</text>"##,
            y + 34,
            esc(&l.text)
        );
    }
    s.push_str("</svg>\n");
    s
}
