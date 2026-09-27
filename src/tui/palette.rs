//! The `:` palette (§4.6): fuzzy "go to" plus `lox` command lines parsed with
//! the CLI's own clap definitions.

use clap::Parser;

use super::app::{App, PalItem, Screen};
use super::lists::fuzzy;
use super::model::Cid;
use super::vm::{self, Plan};
use crate::actions::{self, Action};
use crate::{Cli, Cmd, InputCmd, LightCmd};

/// Control verbs the palette runs in-process. Everything else prints output
/// and belongs to a screen.
const VERBS: &[&str] = &[
    "on",
    "off",
    "set",
    "pulse",
    "input",
    "blind",
    "light",
    "mood",
    "dimmer",
    "color",
    "gate",
    "thermostat",
    "alarm",
    "door",
    "doorlock",
    "intercom",
    "charger",
    "lock",
    "unlock",
    "run",
    "send",
];

/// Minimal shell-like split: whitespace, "double" and 'single' quotes.
pub fn split(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut any = false;
    for c in s.chars() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => cur.push(c),
            None if c == '"' || c == '\'' => {
                quote = Some(c);
                any = true;
            }
            None if c.is_whitespace() => {
                if !cur.is_empty() || any {
                    out.push(std::mem::take(&mut cur));
                    any = false;
                }
            }
            None => cur.push(c),
        }
    }
    if !cur.is_empty() || any {
        out.push(cur);
    }
    out
}

/// Tokens without a leading `lox`.
fn tokens(input: &str) -> Vec<String> {
    let mut t = split(input);
    if t.first().is_some_and(|w| w == "lox") {
        t.remove(0);
    }
    t
}

pub fn is_command(input: &str) -> bool {
    tokens(input)
        .first()
        .is_some_and(|w| VERBS.contains(&w.as_str()) || is_other_subcommand(w))
}

fn is_other_subcommand(w: &str) -> bool {
    use clap::CommandFactory;
    Cli::command().get_subcommands().any(|c| c.get_name() == w) && !VERBS.contains(&w)
}

/// Palette results for the current input, in section order.
pub fn items(app: &App, input: &str) -> Vec<(&'static str, PalItem)> {
    let q = input.trim();
    if is_command(q) {
        return command_items(app, q);
    }
    let h = &app.house;
    let mut out: Vec<(&'static str, PalItem)> = Vec::new();
    if q.is_empty() {
        for s in Screen::ALL {
            out.push(("SCREENS", PalItem::Screen(s)));
        }
        for s in &app.scenes {
            out.push(("SCENES", PalItem::Scene(s.clone())));
        }
        return out;
    }
    let mut ctrls: Vec<(u32, Cid)> = (0..h.ctrls.len())
        .filter_map(|c| {
            let hay = format!("{} {}", h.display_name(c), h.room_name(c).unwrap_or(""));
            fuzzy(q, &hay).map(|s| (s, c))
        })
        .collect();
    ctrls.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    out.extend(
        ctrls
            .into_iter()
            .take(30)
            .map(|(_, c)| ("CONTROLS", PalItem::Ctrl(c))),
    );
    let mut rooms: Vec<(u32, usize)> = (0..h.rooms.len())
        .filter_map(|r| fuzzy(q, &h.rooms[r].name).map(|s| (s, r)))
        .collect();
    rooms.sort_by_key(|x| std::cmp::Reverse(x.0));
    out.extend(
        rooms
            .into_iter()
            .take(10)
            .map(|(_, r)| ("ROOMS", PalItem::Room(r))),
    );
    for s in &app.scenes {
        if fuzzy(q, s).is_some() {
            out.push(("SCENES", PalItem::Scene(s.clone())));
        }
    }
    for s in Screen::ALL {
        if fuzzy(q, s.title()).is_some() {
            out.push(("SCREENS", PalItem::Screen(s)));
        }
    }
    for c in &app.contexts {
        if fuzzy(q, c).is_some() {
            out.push(("SITES", PalItem::Site(c.clone())));
        }
    }
    out
}

fn command_items(app: &App, q: &str) -> Vec<(&'static str, PalItem)> {
    let toks = tokens(q);
    let verb = toks[0].as_str();
    if !VERBS.contains(&verb) {
        let where_ = match verb {
            "status" | "log" | "health" | "extensions" | "update" | "reboot" | "time" => {
                "the System screen (5)"
            }
            "energy" => "the Energy screen (4)",
            "stream" | "watch" => "the Events screen (3)",
            "ctx" => "the context switcher (C) or Sites (6)",
            _ => "the Rooms screen (2)",
        };
        return vec![(
            "COMMANDS",
            PalItem::Note(format!("`lox {}` prints output — use {}", verb, where_)),
        )];
    }
    let mut argv = vec!["lox".to_string()];
    argv.extend(toks.iter().cloned());
    match Cli::try_parse_from(&argv) {
        Ok(cli) => match plan_cmd(app, cli.cmd) {
            Ok(Parsed::Plans(plans)) => {
                let label = plans
                    .iter()
                    .map(|p| format!("{} → {}", app.house.display_name(p.cid), p.label))
                    .collect::<Vec<_>>()
                    .join(" · ");
                vec![("COMMANDS", PalItem::Run { label, plans })]
            }
            Ok(Parsed::Scene(s)) => vec![("COMMANDS", PalItem::RunScene(s))],
            Err(e) => vec![("COMMANDS", PalItem::Note(e))],
        },
        Err(e) => {
            // Usage hint + name completion candidates for the second token
            let msg = e
                .to_string()
                .lines()
                .find(|l| l.starts_with("error:"))
                .unwrap_or("incomplete command")
                .trim_start_matches("error: ")
                .to_string();
            let mut out = vec![("COMMANDS", PalItem::Note(msg))];
            if let Some(name) = toks.get(1) {
                let h = &app.house;
                let mut ctrls: Vec<(u32, Cid)> = h
                    .top_level()
                    .filter_map(|c| fuzzy(name, &h.ctrls[c].name).map(|s| (s, c)))
                    .collect();
                ctrls.sort_by_key(|x| std::cmp::Reverse(x.0));
                out.extend(
                    ctrls
                        .into_iter()
                        .take(12)
                        .map(|(_, c)| ("CONTROLS", PalItem::Ctrl(c))),
                );
            } else if verb == "run" {
                out.extend(
                    app.scenes
                        .iter()
                        .map(|s| ("SCENES", PalItem::Scene(s.clone()))),
                );
            }
            out
        }
    }
}

pub enum Parsed {
    Plans(Vec<Plan>),
    Scene(String),
}

fn resolve(app: &App, name: &str, room: Option<&str>) -> Result<Cid, String> {
    app.house.resolve(name, room)
}

fn one(
    app: &App,
    name: &str,
    room: Option<&str>,
    action: anyhow::Result<Action>,
) -> Result<Parsed, String> {
    let action = action.map_err(|e| e.to_string())?;
    let cid = resolve(app, name, room)?;
    let c = &app.house.ctrls[cid];
    action
        .check_type(&c.name, &c.typ)
        .map_err(|e| e.to_string())?;
    Ok(Parsed::Plans(vec![Plan {
        cid,
        label: action.describe(),
        action,
    }]))
}

/// Map a parsed CLI command to plans (§4.6). Mirrors `commands/control.rs`.
pub fn plan_cmd(app: &App, cmd: Cmd) -> Result<Parsed, String> {
    match cmd {
        Cmd::On {
            name_or_uuid: None,
            all_in_room: Some(r),
            ..
        } => all_in_room(app, &r, true),
        Cmd::Off {
            name_or_uuid: None,
            all_in_room: Some(r),
            ..
        } => all_in_room(app, &r, false),
        Cmd::On {
            name_or_uuid, room, ..
        } => one(
            app,
            &name_or_uuid.unwrap_or_default(),
            room.as_deref(),
            Ok(Action::On),
        ),
        Cmd::Off {
            name_or_uuid, room, ..
        } => one(
            app,
            &name_or_uuid.unwrap_or_default(),
            room.as_deref(),
            Ok(Action::Off),
        ),
        Cmd::Set {
            name_or_uuid,
            value,
            room,
        }
        | Cmd::Input {
            action:
                InputCmd::Set {
                    name_or_uuid,
                    value,
                    room,
                },
        } => one(
            app,
            &name_or_uuid,
            room.as_deref(),
            Ok(Action::Value(value)),
        ),
        Cmd::Pulse { name_or_uuid, room }
        | Cmd::Input {
            action: InputCmd::Pulse { name_or_uuid, room },
        } => one(app, &name_or_uuid, room.as_deref(), Ok(Action::Pulse)),
        Cmd::Blind {
            name_or_uuid,
            action,
            pos,
            room,
        } => one(
            app,
            &name_or_uuid,
            room.as_deref(),
            actions::parse_blind(&action, pos),
        ),
        Cmd::Light {
            action:
                LightCmd::Mood {
                    name_or_uuid,
                    action,
                    room,
                },
        }
        | Cmd::Mood {
            name_or_uuid,
            action,
            room,
        } => {
            // mood names from the live mood list are accepted too
            let cid = resolve(app, &name_or_uuid, room.as_deref())?;
            let named = vm::mood_list(&app.store, &app.house, cid)
                .into_iter()
                .find(|m| m.name.eq_ignore_ascii_case(&action))
                .map(|m| Ok(Action::Mood(actions::MoodCmd::Set(m.id as u32))));
            one(
                app,
                &name_or_uuid,
                room.as_deref(),
                named.unwrap_or_else(|| actions::parse_mood(&action)),
            )
        }
        Cmd::Light {
            action:
                LightCmd::Dim {
                    name_or_uuid,
                    level,
                    room,
                },
        }
        | Cmd::Dimmer {
            name_or_uuid,
            level,
            room,
        } => one(
            app,
            &name_or_uuid,
            room.as_deref(),
            actions::parse_dim(level),
        ),
        Cmd::Light {
            action:
                LightCmd::Color {
                    name_or_uuid,
                    value,
                    room,
                },
        }
        | Cmd::Color {
            name_or_uuid,
            value,
            room,
        } => one(
            app,
            &name_or_uuid,
            room.as_deref(),
            actions::parse_color(&value),
        ),
        Cmd::Light {
            action: LightCmd::Moods { .. },
        } => Err("select the light and press m for its moods".into()),
        Cmd::Gate {
            name_or_uuid,
            action,
            room,
        } => one(
            app,
            &name_or_uuid,
            room.as_deref(),
            actions::parse_gate(&action),
        ),
        Cmd::Thermostat {
            name_or_uuid,
            action: Some(action),
            value,
            duration,
            room,
        } => one(
            app,
            &name_or_uuid,
            room.as_deref(),
            actions::parse_thermostat(&action, value.as_deref(), duration),
        ),
        Cmd::Thermostat { .. } => Err("thermostat needs an action: temp | mode | override".into()),
        Cmd::Alarm {
            name_or_uuid,
            action,
            no_motion,
            code,
            room,
        } => one(
            app,
            &name_or_uuid,
            room.as_deref(),
            actions::parse_alarm(&action, no_motion, code),
        ),
        Cmd::Door {
            name_or_uuid,
            action,
            room,
        } => one(
            app,
            &name_or_uuid,
            room.as_deref(),
            actions::parse_door(&action),
        ),
        Cmd::Intercom {
            name_or_uuid,
            action,
            room,
        } => one(
            app,
            &name_or_uuid,
            room.as_deref(),
            actions::parse_intercom(&action),
        ),
        Cmd::Charger {
            name_or_uuid,
            action,
            limit,
            room,
        } => one(
            app,
            &name_or_uuid,
            room.as_deref(),
            actions::parse_charger(&action, limit),
        ),
        Cmd::Lock {
            name_or_uuid,
            reason,
            room,
        } => one(
            app,
            &name_or_uuid,
            room.as_deref(),
            Ok(Action::LockControl(reason)),
        ),
        Cmd::Unlock { name_or_uuid, room } => one(
            app,
            &name_or_uuid,
            room.as_deref(),
            Ok(Action::UnlockControl),
        ),
        Cmd::Run { scene, .. } => {
            if app.scenes.contains(&scene) {
                Ok(Parsed::Scene(scene))
            } else {
                Err(format!("no scene '{}'", scene))
            }
        }
        Cmd::Send {
            secured: Some(_), ..
        } => Err("secured raw commands are CLI-only".into()),
        Cmd::Send {
            name_or_uuid,
            command,
            room,
            ..
        } => one(
            app,
            &name_or_uuid,
            room.as_deref(),
            Ok(Action::Raw(command)),
        ),
        _ => Err("not available in the palette".into()),
    }
}

/// `--all-in-room`: every light in the room (the room row's `<` / `>`).
pub fn all_in_room(app: &App, room: &str, on: bool) -> Result<Parsed, String> {
    let h = &app.house;
    let lr = room.to_lowercase();
    let r = (0..h.rooms.len())
        .find(|r| h.rooms[*r].name.to_lowercase() == lr)
        .or_else(|| (0..h.rooms.len()).find(|r| h.rooms[*r].name.to_lowercase().contains(&lr)))
        .ok_or_else(|| format!("no room '{}'", room))?;
    Ok(Parsed::Plans(room_lights(app, r, on)))
}

/// Plans switching all lights of a room on/off.
pub fn room_lights(app: &App, r: usize, on: bool) -> Vec<Plan> {
    let h = &app.house;
    h.rooms[r]
        .ctrls
        .iter()
        .filter(|c| vm::is_light(h, **c))
        .map(|&cid| {
            let action = match (h.ctrls[cid].kind, on) {
                (super::model::Kind::LightCtl | super::model::Kind::CentralLight, false) => {
                    Action::Mood(actions::MoodCmd::Off)
                }
                (_, true) => Action::On,
                (_, false) => Action::Off,
            };
            Plan {
                cid,
                label: if on { "on".into() } else { "off".into() },
                action,
            }
        })
        .collect()
}

/// `⇥` completion: put the selected control's name into the command line.
pub fn complete(input: &str, name: &str) -> String {
    let quoted = if name.contains(' ') {
        format!("\"{}\"", name)
    } else {
        name.to_string()
    };
    let mut t = split(input);
    let lox = t.first().is_some_and(|w| w == "lox");
    if lox {
        t.remove(0);
    }
    if t.first().is_some_and(|w| VERBS.contains(&w.as_str())) {
        let verb = t[0].clone();
        let sub = matches!(verb.as_str(), "light" | "input");
        let pos = if sub { 2 } else { 1 };
        let mut parts: Vec<String> = t.iter().take(pos).cloned().collect();
        parts.push(quoted);
        let s = format!("{}{} ", if lox { "lox " } else { "" }, parts.join(" "));
        return s;
    }
    name.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_quotes() {
        assert_eq!(
            split(r#"blind "Blind South" pos 30"#),
            ["blind", "Blind South", "pos", "30"]
        );
        assert_eq!(split("on 'a b'  c"), ["on", "a b", "c"]);
        assert_eq!(split(r#"on """#), ["on", ""]);
    }

    #[test]
    fn completion() {
        assert_eq!(
            complete("blind sou", "Blind South"),
            "blind \"Blind South\" "
        );
        assert_eq!(complete("light dim cei", "Ceiling"), "light dim Ceiling ");
        assert_eq!(complete("sou", "Blind South"), "Blind South");
    }
}
