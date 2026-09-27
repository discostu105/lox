//! ² Rooms — the control surface (§5.3).

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Modifier;

use super::common::{self, RowOpts};
use super::inspector;
use crate::tui::app::{App, FacetList, GKey, GroupBy, Hit, RoomSort};
use crate::tui::keymap::{Cmd, Ctx};
use crate::tui::lists::{self, CRow};
use crate::tui::text::{fit, rfit};
use crate::tui::theme::Grad;
use crate::tui::update::{WIDE_ROOMS, list_id};
use crate::tui::vm;
use crate::tui::widgets::notchbox::{Hint, Notch, NotchBox};
use crate::tui::widgets::{cell, put};

pub fn render(app: &App, area: Rect, buf: &mut Buffer) {
    let pane = app.rooms.pane;
    if area.width < 90 {
        // Narrow: one pane at a time, breadcrumb in the title
        match pane {
            0 => groups(app, area, buf, true),
            _ => ctrls(app, area, buf, true, true),
        }
        return;
    }
    let gw = if area.width >= 160 { 32 } else { 28 };
    let g = Rect::new(area.x, area.y, gw, area.height);
    groups(app, g, buf, pane == 0);
    if area.width >= WIDE_ROOMS {
        let iw = if area.width >= 180 { 52 } else { 42 };
        let mid = Rect::new(area.x + gw, area.y, area.width - gw - iw, area.height);
        ctrls(app, mid, buf, pane == 1, false);
        let ir = Rect::new(mid.right(), area.y, iw, area.height);
        match lists::selected_ctrl(app) {
            Some(cid) => inspector::render(app, ir, buf, cid, pane == 2, 0),
            None => {
                let inner = NotchBox::new()
                    .title(Notch::new("inspector"))
                    .render(ir, buf, &app.th);
                common::empty(app, buf, inner, &["nothing selected"]);
            }
        }
    } else {
        let mid = Rect::new(area.x + gw, area.y, area.width - gw, area.height);
        ctrls(app, mid, buf, pane >= 1, false);
    }
}

fn group_notches(app: &App) -> Vec<Notch> {
    let by = match app.rooms.group {
        GroupBy::Room => "by room",
        GroupBy::Category => "by category",
        GroupBy::Type => "by type",
    };
    let mut v = vec![Notch::new("²rooms"), Notch::hot(by, 'b')];
    if app.rooms.group == GroupBy::Room {
        let s = match app.rooms.sort {
            RoomSort::Name => "a–z",
            RoomSort::Activity => "activity",
            RoomSort::Temp => "temp",
        };
        v.push(Notch::new(format!("o {}", s)).active(app.rooms.sort != RoomSort::Name));
    }
    if !app.rooms.facets.is_empty() {
        v.push(Notch::hot(format!("f {} facets", app.rooms.facets.len()), 'f').active(true));
    }
    v
}

fn groups(app: &App, area: Rect, buf: &mut Buffer, focus: bool) {
    let th = &app.th;
    let gs = lists::groups(app);
    let cur = lists::current_group(app);
    let sel = gs.iter().position(|g| g.key == cur).unwrap_or(0);
    let mut nb = NotchBox::new()
        .focus(focus)
        .position(format!("{}/{}", sel + 1, gs.len()));
    for n in group_notches(app) {
        nb = nb.title(n);
    }
    if !app.rooms.filter_groups.is_empty() {
        nb = nb.meta(Notch::new(format!("/{}", app.rooms.filter_groups)).active(true));
    }
    if focus {
        let mut hints = vec![Hint::new("⏎", "open")];
        if matches!(cur, GKey::Room(_)) && !app.opts.read_only {
            hints.push(Hint::new("<>", "lights"));
            hints.push(Hint::new("m", "mood"));
        }
        hints.extend(common::ctx_hints(
            Ctx::Rooms,
            &[Cmd::GroupBy, Cmd::Sort, Cmd::Facets],
        ));
        nb = nb.hints(hints);
    }
    let inner = nb.render(area, buf, th);
    common::hit(app, inner, Hit::Pane(list_id::ROOM_GROUPS));
    let hgt = inner.height as usize;
    if focus {
        common::set_page(app, hgt);
    }
    let off = common::offset(app, list_id::ROOM_GROUPS, sel, gs.len(), hgt);
    let w = inner.width as usize;
    for (i, g) in gs.iter().enumerate().skip(off).take(hgt) {
        let y = inner.y + (i - off) as u16;
        let row = Rect::new(inner.x, y, inner.width, 1);
        common::hit(app, row, Hit::Row(list_id::ROOM_GROUPS, i));
        let selected = i == sel;
        if selected {
            crate::tui::widgets::fill(
                buf,
                row,
                if focus {
                    th.s_selected()
                } else {
                    ratatui::style::Style::default()
                },
            );
            cell(
                buf,
                inner,
                inner.x,
                y,
                if focus { "▌" } else { "▸" },
                th.s_accent(),
            );
        }
        let nst = if selected {
            th.s_text().add_modifier(Modifier::BOLD)
        } else {
            th.s_text()
        };
        match g.key {
            GKey::Room(r) => {
                let s = lists::room_sum(app, r);
                let name_w = w.saturating_sub(13);
                common::put_name(
                    buf,
                    inner,
                    inner.x + 1,
                    y,
                    &g.label,
                    name_w,
                    nst,
                    &app.rooms.filter_groups,
                    th,
                );
                let mut x = inner.x + 1 + name_w as u16;
                match s.temp {
                    Some(t) => put(
                        buf,
                        inner,
                        x,
                        y,
                        &rfit(&format!("{:.1}°", t), 6),
                        th.grad_style(Grad::Temp, vm::temp_frac(t)),
                    ),
                    None => put(buf, inner, x, y, &rfit("—", 6), th.s_faint()),
                };
                x += 7;
                if s.lights_on > 0 {
                    put(
                        buf,
                        inner,
                        x,
                        y,
                        &format!("●{}", s.lights_on.min(9)),
                        th.s_on(),
                    );
                }
                x += 2;
                if s.blind_pos.is_some_and(|p| p > 0.05) || s.blinds_moving {
                    put(
                        buf,
                        inner,
                        x + 1,
                        y,
                        "▾",
                        if s.blinds_moving {
                            th.s_accent()
                        } else {
                            th.s_info()
                        },
                    );
                }
                if s.attention {
                    put(buf, inner, x + 2, y, "⚠", th.s_warn());
                }
            }
            _ => {
                let st = if matches!(g.key, GKey::Favorites | GKey::All) {
                    th.s_dim()
                } else {
                    nst
                };
                let name_w = w.saturating_sub(7);
                common::put_name(
                    buf,
                    inner,
                    inner.x + 1,
                    y,
                    &g.label,
                    name_w,
                    st,
                    &app.rooms.filter_groups,
                    th,
                );
                put(
                    buf,
                    inner,
                    inner.x + 1 + name_w as u16,
                    y,
                    &rfit(&g.count.to_string(), 5),
                    th.s_faint(),
                );
            }
        }
    }
}

fn group_title(app: &App, key: &GKey) -> String {
    let h = &app.house;
    match key {
        GKey::Favorites => "★ Favorites".into(),
        GKey::All => "All controls".into(),
        GKey::Room(r) => h.rooms[*r].name.clone(),
        GKey::Cat(i) => h.cats[*i].name.clone(),
        GKey::Type(t) => t.clone(),
        GKey::Unassigned => "No room".into(),
    }
}

fn ctrls(app: &App, area: Rect, buf: &mut Buffer, focus: bool, narrow: bool) {
    let th = &app.th;
    let key = lists::current_group(app);
    let rows = lists::ctrl_rows(app, &key);
    let cids = lists::row_cids(&rows);
    let sel_cid = lists::selected_ctrl(app);
    let sel_idx = rows
        .iter()
        .position(|r| matches!(r, CRow::Ctrl { cid, .. } if Some(*cid) == sel_cid))
        .unwrap_or(0);
    let pos = sel_cid
        .and_then(|c| cids.iter().position(|x| *x == c))
        .map_or(0, |i| i + 1);
    let title = if narrow {
        format!("²rooms › {}", group_title(app, &key))
    } else {
        group_title(app, &key)
    };
    let mut nb = NotchBox::new()
        .focus(focus)
        .title(Notch::new(title))
        .meta(Notch::new(if app.rooms.facets.is_empty() {
            format!("{} controls", cids.len())
        } else {
            format!(
                "{} of {} controls",
                cids.len(),
                lists::group_base(app, &key).len()
            )
        }))
        .position(format!("{}/{}", pos, cids.len()));
    for pill in lists::facet_pills(app, FacetList::Controls) {
        nb = nb.title(Notch::new(pill).active(true));
    }
    if !app.rooms.filter_ctrls.is_empty() {
        nb = nb.title(Notch::new(format!("/{}", app.rooms.filter_ctrls)).active(true));
    } else if focus {
        nb = nb.title(Notch::hot("/ filter", '/'));
    }
    if !app.rooms.marks.is_empty() {
        nb = nb.bottom_right(Notch::new(format!("{} marked", app.rooms.marks.len())).active(true));
    }
    if focus && let Some(c) = sel_cid {
        nb = nb.hints(common::item_hints(app, c));
    }
    let inner = nb.render(area, buf, th);
    common::hit(app, inner, Hit::Pane(list_id::ROOM_CTRLS));
    if rows.is_empty() {
        let msg: &[&str] = if !app.rooms.filter_ctrls.is_empty() {
            &["no match", "Esc clears the filter"]
        } else if !app.rooms.facets.is_empty() {
            &["no controls match the facets", "f edits · Esc clears"]
        } else if key == GKey::Favorites {
            &["no favorites yet", "press * on a control to pin it"]
        } else {
            &["no controls here"]
        };
        common::empty(app, buf, inner, msg);
        return;
    }
    let hgt = inner.height as usize;
    if focus {
        common::set_page(app, hgt);
    }
    let off = common::offset(app, list_id::ROOM_CTRLS, sel_idx, rows.len(), hgt);
    let name_w = ((inner.width as usize).saturating_sub(40)).clamp(12, 28);
    let show_room = matches!(
        key,
        GKey::Favorites | GKey::All | GKey::Cat(_) | GKey::Type(_)
    );
    let name_w = if show_room {
        (name_w + 8).min(inner.width as usize / 2)
    } else {
        name_w
    };
    for (i, r) in rows.iter().enumerate().skip(off).take(hgt) {
        let y = inner.y + (i - off) as u16;
        common::hit(
            app,
            Rect::new(inner.x, y, inner.width, 1),
            Hit::Row(list_id::ROOM_CTRLS, i),
        );
        match r {
            CRow::Header(h) => {
                common::header_row(app, buf, inner, y, &fit(h, inner.width as usize - 2))
            }
            CRow::Ctrl { cid, depth } => common::ctrl_row(
                app,
                buf,
                inner,
                y,
                *cid,
                &RowOpts {
                    selected: Some(*cid) == sel_cid,
                    marked: app.rooms.marks.contains(cid),
                    depth: *depth,
                    name_w,
                    show_room: show_room && *depth == 0,
                    filter: &app.rooms.filter_ctrls,
                    focus,
                },
            ),
        }
    }
}
