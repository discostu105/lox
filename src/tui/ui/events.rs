//! ³ Events — the live feed, newest at the bottom like `tail -f` (§5.4).

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Modifier;

use super::common;
use crate::tui::app::{App, Chip, Hit};
use crate::tui::keymap::{Cmd, Ctx};
use crate::tui::lists;
use crate::tui::store::{Event, Val, fmt_val};
use crate::tui::text::{fit, rfit};
use crate::tui::theme::Grad;
use crate::tui::update::{list_id, sel_event};
use crate::tui::widgets::braille::{GraphOpts, graph};
use crate::tui::widgets::notchbox::{Hint, Notch, NotchBox};
use crate::tui::widgets::{cell, fill, put};

pub fn change_text(e: &Event) -> String {
    let arrow = |o: &Option<Val>, n: &Val| match (o, n) {
        (Some(Val::Num(a)), Val::Num(b))
            if (*a == 0.0 || *a == 1.0) && (*b == 0.0 || *b == 1.0) =>
        {
            format!(
                "{} → {}",
                if *a == 1.0 { "●" } else { "○" },
                if *b == 1.0 { "●" } else { "○" }
            )
        }
        _ => format!("{} → {}", fmt_val(o), fmt_val(&Some(n.clone()))),
    };
    arrow(&e.old, &e.new)
}

pub fn render(app: &App, area: Rect, buf: &mut Buffer) {
    let th = &app.th;
    let h = &app.house;
    let rows = lists::event_rows(app);
    let sel = sel_event(app);
    let sel_i = sel.and_then(|s| rows.iter().position(|i| app.store.events[*i].seq == s));
    let focus = app.overlays.is_empty();

    let detail_h = if area.height >= 30 { 6 } else { 5 };
    let list_area = Rect::new(area.x, area.y, area.width, area.height - detail_h);
    let mut nb = NotchBox::new()
        .focus(focus && app.events.pane == 0)
        .title(Notch::new("³events"));
    nb = nb.title(if app.paused {
        Notch::new("⏸ paused").styled(th.s_warn())
    } else if app.events.follow {
        Notch::hot("● following", 'F').active(true)
    } else {
        Notch::hot("○ scrolled — F follow", 'F')
    });
    nb = nb.meta(Notch::new(format!(
        "{} buffered · {}/min",
        app.store.events.len(),
        app.store.per_minute()
    )));
    if !app.events.filter.is_empty() {
        nb = nb.title(Notch::new(format!("/{}", app.events.filter)).active(true));
    }
    for c in &app.events.chips {
        let label = match c {
            Chip::Room(r) => format!("room:{}", h.rooms[*r].name),
            Chip::Ctrl(c) => format!("ctrl:{}", h.display_name(*c)),
        };
        nb = nb.title(Notch::new(label).active(true));
    }
    if app.events.show_muted {
        nb = nb.title(Notch::new("+muted").active(true));
    }
    let muted = app.store.mutes.len();
    // "↓ N new" while scrolled back or paused
    let newer = if app.events.follow && !app.paused {
        0
    } else {
        app.store
            .events
            .iter()
            .rev()
            .take_while(|e| e.seq >= app.events.seen.max(1))
            .count()
            + app.paused_buf.len()
    };
    if newer > 0 {
        nb = nb.bottom_right(Notch::new(format!("↓ {} new · G", newer)).styled(th.s_accent()));
    } else {
        nb = nb.position(format!(
            "{}/{}",
            sel_i.map_or(rows.len(), |i| i + 1),
            rows.len()
        ));
    }
    if focus && app.events.pane == 0 {
        let mut hints = vec![
            Hint::new("⏎", "open"),
            Hint::new("e", "only this"),
            Hint::new("f", "room"),
        ];
        hints.extend(common::ctx_hints(
            Ctx::Events,
            &[Cmd::Mute, Cmd::ShowMuted, Cmd::Pause],
        ));
        hints.push(Hint::new("w", "wiring"));
        nb = nb.hints(hints);
    }
    let inner = nb.render(list_area, buf, th);
    common::hit(app, inner, Hit::Pane(list_id::EVENTS));

    // rate graph (events per 10 s)
    let gh = if inner.height >= 16 { 3 } else { 0 };
    if gh > 0 {
        let n = inner.width as usize * 2;
        let data = app.store.rate_series(10, n);
        let max = data.iter().copied().fold(1.0, f64::max);
        graph(
            buf,
            Rect::new(inner.x, inner.y, inner.width, gh),
            &data,
            max,
            Grad::Acc,
            &GraphOpts {
                floor: true,
                ..Default::default()
            },
            th,
        );
        put(
            buf,
            inner,
            inner.x,
            inner.y,
            &format!("{:.0}/10s", max),
            th.s_faint(),
        );
        let lbl = if muted > 0 {
            format!("{} muted · {} hidden", muted, app.store.muted_count)
        } else {
            String::new()
        };
        put(
            buf,
            inner,
            inner.right().saturating_sub(lbl.len() as u16 + 1),
            inner.y,
            &lbl,
            th.s_faint(),
        );
    }
    let hy = inner.y + gh;
    let (tw, rw) = (10u16, 14usize);
    let nw = ((inner.width as usize).saturating_sub(tw as usize + rw + 50)).clamp(12, 26);
    let sw = 14usize;
    let head = format!(
        "{:<tw$}{:<rw$} {:<nw$} {:<sw$} CHANGE",
        "TIME",
        "ROOM",
        "CONTROL",
        "STATE",
        tw = tw as usize,
        rw = rw,
        nw = nw,
        sw = sw
    );
    put(
        buf,
        inner,
        inner.x + 1,
        hy,
        &fit(&head, inner.width as usize - 2),
        th.s_dim().add_modifier(Modifier::BOLD),
    );
    let body = Rect::new(
        inner.x,
        hy + 1,
        inner.width,
        inner.height.saturating_sub(gh + 1),
    );
    if rows.is_empty() {
        let msg: &[&str] = if !app.events.filter.is_empty() || !app.events.chips.is_empty() {
            &["no events match", "Esc clears filter and chips"]
        } else {
            &[
                "waiting for events…",
                "changes from the Miniserver appear here as they happen",
            ]
        };
        common::empty(app, buf, body, msg);
    } else {
        let hgt = body.height as usize;
        if focus {
            common::set_page(app, hgt);
        }
        // newest at the bottom: the window ends at the selection (or the newest row)
        let target = sel_i.unwrap_or(rows.len() - 1);
        let off = if app.events.follow {
            rows.len().saturating_sub(hgt)
        } else {
            common::offset(app, list_id::EVENTS, target, rows.len(), hgt)
        };
        if app.events.follow {
            app.ui.borrow_mut().offsets.insert(list_id::EVENTS, off);
        }
        let corr: Vec<u64> = sel
            .filter(|_| !app.events.follow)
            .and_then(|s| app.store.index_of(s))
            .map(|i| {
                let e = &app.store.events[i];
                app.store
                    .around(e.t, 2.0, e.seq)
                    .iter()
                    .map(|x| x.seq)
                    .collect()
            })
            .unwrap_or_default();
        for (k, &i) in rows.iter().enumerate().skip(off).take(hgt) {
            let e = &app.store.events[i];
            let y = body.y + (k - off) as u16;
            let row = Rect::new(body.x, y, body.width, 1);
            common::hit(app, row, Hit::Row(list_id::EVENTS, k));
            let is_sel = Some(e.seq) == sel && !app.events.follow;
            if is_sel {
                fill(
                    buf,
                    row,
                    if focus {
                        th.s_selected()
                    } else {
                        Default::default()
                    },
                );
                cell(buf, body, body.x, y, "▌", th.s_accent());
            } else if corr.contains(&e.seq) {
                cell(buf, body, body.x, y, "•", th.s_info());
            }
            let age = (app.now - e.t).max(0.0);
            let tst = if age < 1.5 {
                th.s_accent()
            } else {
                th.grad_style(Grad::Fade, (age / 600.0).clamp(0.0, 0.8))
            };
            let mut x = body.x + 1;
            put(buf, body, x, y, &app.hhmmss(e.t), tst);
            x += tw;
            let room = e.cid.and_then(|c| h.room_name(c)).unwrap_or("—");
            put(buf, body, x, y, &fit(room, rw), th.s_dim());
            x += rw as u16 + 1;
            let name = e
                .cid
                .map(|c| h.display_name(c))
                .unwrap_or_else(|| "—".into());
            common::put_name(
                buf,
                body,
                x,
                y,
                &name,
                nw,
                th.s_text(),
                &app.events.filter,
                th,
            );
            x += nw as u16 + 1;
            put(buf, body, x, y, &fit(&e.state, sw), th.s_dim());
            x += sw as u16 + 1;
            let mut rest = body.right().saturating_sub(x + 1) as usize;
            if e.count > 1 {
                let c = format!("×{}", e.count);
                put(
                    buf,
                    body,
                    body.right().saturating_sub(c.len() as u16 + 1),
                    y,
                    &c,
                    th.s_faint(),
                );
                rest = rest.saturating_sub(c.len() + 1);
            }
            put(buf, body, x, y, &fit(&change_text(e), rest), th.s_text());
        }
    }
    detail(
        app,
        Rect::new(area.x, list_area.bottom(), area.width, detail_h),
        buf,
    );
}

fn detail(app: &App, area: Rect, buf: &mut Buffer) {
    let th = &app.th;
    let h = &app.house;
    let focus = app.overlays.is_empty() && app.events.pane == 1;
    let nb = NotchBox::new().focus(focus).title(Notch::new("detail"));
    let inner = nb.render(area, buf, th);
    let Some(i) = sel_event(app).and_then(|s| app.store.index_of(s)) else {
        common::empty(app, buf, inner, &["no event selected"]);
        return;
    };
    let e = &app.store.events[i];
    let x = inner.x + 1;
    let w = inner.width.saturating_sub(2) as usize;
    let ms = ((e.t.fract()) * 1000.0) as u32;
    let who = match e.cid {
        Some(c) => format!("{} · {}", h.room_name(c).unwrap_or("—"), h.display_name(c)),
        None => "(unknown state)".into(),
    };
    let head = format!(
        "{} · {} {} at {}.{:03}",
        who,
        e.state,
        change_text(e),
        app.hhmmss(e.t),
        ms
    );
    put(
        buf,
        inner,
        x,
        inner.y,
        &fit(&head, w),
        th.s_text().add_modifier(Modifier::BOLD),
    );
    if inner.height < 2 {
        return;
    }
    // correlation: same room first
    let room = e.cid.and_then(|c| h.ctrls[c].room);
    let mut near: Vec<&Event> = app.store.around(e.t, 2.0, e.seq);
    near.sort_by_key(|x| {
        (
            x.cid.and_then(|c| h.ctrls[c].room) != room,
            (x.t - e.t).abs() as i64,
        )
    });
    let parts: Vec<String> = near
        .iter()
        .take(4)
        .map(|x| {
            let n = x
                .cid
                .map(|c| h.display_name(c))
                .unwrap_or_else(|| x.state.clone());
            format!("{:+.1}s {} {}", x.t - e.t, n, change_text(x))
        })
        .collect();
    let corr = if parts.is_empty() {
        "nothing else changed within ±2 s".to_string()
    } else {
        parts.join("  ·  ")
    };
    put(buf, inner, x, inner.y + 1, "around ±2 s ", th.s_dim());
    put(
        buf,
        inner,
        x + 12,
        inner.y + 1,
        &fit(&corr, w.saturating_sub(12)),
        th.s_info(),
    );
    if inner.height >= 3 {
        let cli = match e.cid {
            Some(c) => {
                let top = h.top(c);
                let mut s = format!(
                    "lox stream -c {}",
                    crate::actions::shell_quote(&h.ctrls[top].name)
                );
                if let Some(r) = h.room_name(top) {
                    s.push_str(&format!(" -r {}", crate::actions::shell_quote(r)));
                }
                s
            }
            None => format!("lox stream  # uuid {}", e.uuid),
        };
        put(
            buf,
            inner,
            x,
            inner.bottom() - 1,
            "y",
            th.s_accent().add_modifier(Modifier::BOLD),
        );
        put(
            buf,
            inner,
            x + 2,
            inner.bottom() - 1,
            &fit(&cli, w.saturating_sub(2)),
            th.s_dim(),
        );
        let ago = crate::tui::text::fmt_age((app.now - e.t).max(0.0) as u64);
        put(
            buf,
            inner,
            inner.right().saturating_sub(ago.len() as u16 + 6),
            inner.bottom() - 1,
            &rfit(&format!("{} ago", ago), ago.len() + 4),
            th.s_faint(),
        );
    }
}
