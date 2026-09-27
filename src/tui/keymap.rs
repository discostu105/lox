//! The one keymap table (§4.1 rule 6): dispatch, border hints and the `?`
//! overlay are all generated from [`BINDINGS`].

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::vm::Verb;

/// Where a binding applies. More specific contexts are checked first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Ctx {
    Global,
    /// A control (or room) is selected: the action vocabulary
    Item,
    Rooms,
    Events,
    System,
    Log,
    Config,
    Update,
}

impl Ctx {
    pub fn title(self) -> &'static str {
        match self {
            Ctx::Global => "Global",
            Ctx::Item => "Selected item",
            Ctx::Rooms => "Rooms",
            Ctx::Events => "Events",
            Ctx::System => "System",
            Ctx::Log => "System › Log",
            Ctx::Config => "System › Config",
            Ctx::Update => "System › Update",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Cmd {
    Screen(u8),
    NextPane,
    PrevPane,
    Left,
    Right,
    Down,
    Up,
    Top,
    Bottom,
    PageDown,
    PageUp,
    PrevSub,
    NextSub,
    Filter,
    Palette,
    Help,
    Contexts,
    MsgLog,
    Pause,
    Refresh,
    Quit,
    Back,
    Verb(Verb),
    Set,
    Mode,
    Menu,
    Inspect,
    Wiring,
    Pin,
    ShowEvents,
    Yank,
    YankUuid,
    Chart,
    Mark,
    MarkAll,
    GroupBy,
    Facets,
    Sort,
    Mute,
    ShowMuted,
    Follow,
    NextMatch,
    PrevMatch,
    Pull,
    Reboot,
    Install,
}

pub struct Binding {
    pub ctx: Ctx,
    /// Key codes as produced by [`code`]; the first one is shown in help
    pub keys: &'static [&'static str],
    pub cmd: Cmd,
    pub help: &'static str,
    /// Auto-repeat allowed (movement, steps). Everything else ignores repeats.
    pub repeat: bool,
}

macro_rules! b {
    ($ctx:ident, [$($k:expr),+], $cmd:expr, $help:expr) => {
        Binding { ctx: Ctx::$ctx, keys: &[$($k),+], cmd: $cmd, help: $help, repeat: false }
    };
    ($ctx:ident, [$($k:expr),+], $cmd:expr, $help:expr, repeat) => {
        Binding { ctx: Ctx::$ctx, keys: &[$($k),+], cmd: $cmd, help: $help, repeat: true }
    };
}

pub static BINDINGS: &[Binding] = &[
    // Global — navigation never changes the house (§4.1 rule 1)
    b!(Global, ["1"], Cmd::Screen(1), "Home"),
    b!(Global, ["2"], Cmd::Screen(2), "Rooms"),
    b!(Global, ["3"], Cmd::Screen(3), "Events"),
    b!(Global, ["4"], Cmd::Screen(4), "Energy"),
    b!(Global, ["5"], Cmd::Screen(5), "System"),
    b!(Global, ["6"], Cmd::Screen(6), "Sites"),
    b!(Global, ["Tab"], Cmd::NextPane, "next pane"),
    b!(Global, ["S-Tab"], Cmd::PrevPane, "previous pane"),
    b!(Global, ["h", "Left"], Cmd::Left, "focus pane left"),
    b!(Global, ["l", "Right"], Cmd::Right, "focus pane right"),
    b!(Global, ["j", "Down"], Cmd::Down, "move down", repeat),
    b!(Global, ["k", "Up"], Cmd::Up, "move up", repeat),
    b!(Global, ["g", "Home"], Cmd::Top, "first"),
    b!(Global, ["G", "End"], Cmd::Bottom, "last"),
    b!(
        Global,
        ["C-d", "PageDown"],
        Cmd::PageDown,
        "half page down",
        repeat
    ),
    b!(
        Global,
        ["C-u", "PageUp"],
        Cmd::PageUp,
        "half page up",
        repeat
    ),
    b!(Global, ["["], Cmd::PrevSub, "previous sub-view"),
    b!(Global, ["]"], Cmd::NextSub, "next sub-view"),
    b!(Global, ["/"], Cmd::Filter, "filter"),
    b!(Global, [":", "C-k"], Cmd::Palette, "palette: go to / run"),
    b!(Global, ["?"], Cmd::Help, "help"),
    b!(Global, ["C"], Cmd::Contexts, "switch context"),
    b!(Global, ["!"], Cmd::MsgLog, "message log"),
    b!(Global, ["p"], Cmd::Pause, "pause live updates"),
    b!(
        Global,
        ["C-r"],
        Cmd::Refresh,
        "refresh structure + reconnect"
    ),
    b!(Global, ["q", "C-c"], Cmd::Quit, "quit"),
    b!(Global, ["Esc"], Cmd::Back, "back"),
    // The action vocabulary (§4.3)
    b!(Item, ["Space"], Cmd::Verb(Verb::Primary), "primary action"),
    b!(Item, ["+"], Cmd::Verb(Verb::Plus), "step up", repeat),
    b!(Item, ["-"], Cmd::Verb(Verb::Minus), "step down", repeat),
    b!(Item, ["<"], Cmd::Verb(Verb::Min), "minimum"),
    b!(Item, [">"], Cmd::Verb(Verb::Max), "maximum"),
    b!(Item, ["="], Cmd::Set, "set exact value"),
    b!(Item, ["s"], Cmd::Verb(Verb::Stop), "stop"),
    b!(Item, ["m"], Cmd::Mode, "mode / mood"),
    b!(Item, ["a"], Cmd::Menu, "all actions"),
    b!(Item, ["Enter"], Cmd::Inspect, "inspect / open"),
    b!(Item, ["w"], Cmd::Wiring, "wiring"),
    b!(Item, ["*"], Cmd::Pin, "pin to Home"),
    b!(Item, ["e"], Cmd::ShowEvents, "events of this item"),
    b!(Item, ["c"], Cmd::Chart, "history chart (timeframes)"),
    b!(Item, ["y"], Cmd::Yank, "copy lox command"),
    b!(Item, ["Y"], Cmd::YankUuid, "copy UUID"),
    b!(Item, ["v"], Cmd::Mark, "mark"),
    b!(Item, ["V"], Cmd::MarkAll, "mark all visible"),
    // Screen-specific (§4.5)
    b!(
        Rooms,
        ["b"],
        Cmd::GroupBy,
        "group by room · category · type"
    ),
    b!(Rooms, ["f"], Cmd::Facets, "facets: room, type, state…"),
    b!(
        Rooms,
        ["o"],
        Cmd::Sort,
        "sort rooms: name · activity · temperature"
    ),
    b!(Events, ["f"], Cmd::Facets, "facets: room, control, type…"),
    b!(Events, ["x"], Cmd::Mute, "mute this control"),
    b!(Events, ["X"], Cmd::ShowMuted, "show muted / noisy"),
    b!(Events, ["F"], Cmd::Follow, "follow newest"),
    b!(Log, ["n"], Cmd::NextMatch, "next match"),
    b!(Log, ["N"], Cmd::PrevMatch, "previous match"),
    b!(Config, ["n"], Cmd::NextMatch, "next change"),
    b!(Config, ["N"], Cmd::PrevMatch, "previous change"),
    b!(
        Config,
        ["P"],
        Cmd::Pull,
        "pull the newest config backup (lox config pull)"
    ),
    b!(Update, ["R"], Cmd::Reboot, "reboot (typed confirmation)"),
    b!(
        Update,
        ["U"],
        Cmd::Install,
        "install update (typed confirmation)"
    ),
];

/// Normalize a key event to a code string: `j`, `G`, `C-d`, `S-Tab`, `Space`, `Enter`.
pub fn code(k: &KeyEvent) -> String {
    let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
    let base = match k.code {
        KeyCode::Char(' ') => "Space".to_string(),
        KeyCode::Char(c) => c.to_string(),
        KeyCode::Enter => "Enter".into(),
        KeyCode::Esc => "Esc".into(),
        KeyCode::Tab => "Tab".into(),
        KeyCode::BackTab => return "S-Tab".into(),
        KeyCode::Backspace => "Backspace".into(),
        KeyCode::Left => "Left".into(),
        KeyCode::Right => "Right".into(),
        KeyCode::Up => "Up".into(),
        KeyCode::Down => "Down".into(),
        KeyCode::Home => "Home".into(),
        KeyCode::End => "End".into(),
        KeyCode::PageUp => "PageUp".into(),
        KeyCode::PageDown => "PageDown".into(),
        KeyCode::Delete => "Delete".into(),
        KeyCode::F(n) => format!("F{}", n),
        _ => "?unknown".into(),
    };
    if ctrl {
        format!("C-{}", base.to_lowercase())
    } else {
        base
    }
}

/// Find the binding for a key in the active contexts (most specific first).
pub fn lookup(ctxs: &[Ctx], code: &str) -> Option<&'static Binding> {
    for c in ctxs.iter().chain(std::iter::once(&Ctx::Global)) {
        if let Some(b) = BINDINGS
            .iter()
            .find(|b| b.ctx == *c && b.keys.contains(&code))
        {
            return Some(b);
        }
    }
    None
}

/// Display form of a key code: `Space` → `␣`, `Enter` → `⏎`, `C-d` → `^d`.
pub fn display(code: &str) -> String {
    match code {
        "Space" => "␣".into(),
        "Enter" => "⏎".into(),
        "Tab" => "⇥".into(),
        "S-Tab" => "⇤".into(),
        "Left" => "←".into(),
        "Right" => "→".into(),
        "Up" => "↑".into(),
        "Down" => "↓".into(),
        "Esc" => "Esc".into(),
        c if c.starts_with("C-") => format!("^{}", &c[2..]),
        c => c.into(),
    }
}

/// The key reference in COMMANDS.md, generated from [`BINDINGS`].
#[cfg(test)]
pub fn markdown() -> String {
    let mut out = String::new();
    let mut ctx = None;
    for b in BINDINGS {
        if ctx != Some(b.ctx) {
            ctx = Some(b.ctx);
            out.push_str(&format!("\n| {} | Action |\n|---|---|\n", b.ctx.title()));
        }
        let keys: Vec<String> = b.keys.iter().map(|k| format!("`{}`", display(k))).collect();
        out.push_str(&format!(
            "| {} | {} |\n",
            keys.join(" "),
            b.help.replace('|', "\\|")
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// COMMANDS.md carries the generated key table; regenerate with
    /// `LOX_WRITE_KEYS=1 cargo test commands_md_key_table`.
    #[test]
    fn commands_md_key_table() {
        const START: &str = "<!-- keys:start (generated from src/tui/keymap.rs) -->";
        const END: &str = "<!-- keys:end -->";
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/COMMANDS.md");
        // Windows checkouts may have CRLF line endings
        let doc = std::fs::read_to_string(path).unwrap().replace("\r\n", "\n");
        let (a, rest) = doc
            .split_once(START)
            .expect("COMMANDS.md: keys start marker");
        let (_, b) = rest.split_once(END).expect("COMMANDS.md: keys end marker");
        let want = format!("{}{}{}\n{}{}", a, START, markdown(), END, b);
        if std::env::var("LOX_WRITE_KEYS").is_ok() {
            std::fs::write(path, &want).unwrap();
        }
        assert!(
            doc == want,
            "COMMANDS.md key table is out of date: LOX_WRITE_KEYS=1 cargo test commands_md_key_table"
        );
    }

    #[test]
    fn no_collisions_within_a_context_or_with_global() {
        for ctx in [
            Ctx::Global,
            Ctx::Item,
            Ctx::Rooms,
            Ctx::Events,
            Ctx::System,
            Ctx::Log,
            Ctx::Config,
            Ctx::Update,
        ] {
            let mut seen = HashSet::new();
            for b in BINDINGS
                .iter()
                .filter(|b| b.ctx == ctx || b.ctx == Ctx::Global)
            {
                for k in b.keys {
                    assert!(seen.insert(*k), "key {k} bound twice in {ctx:?}");
                }
            }
        }
        // Item keys must not collide with screen keys that are active at the same time
        for scr in [Ctx::Rooms, Ctx::Events] {
            let mut seen = HashSet::new();
            for b in BINDINGS
                .iter()
                .filter(|b| b.ctx == Ctx::Item || b.ctx == scr)
            {
                for k in b.keys {
                    assert!(seen.insert(*k), "key {k} collides between Item and {scr:?}");
                }
            }
        }
    }

    #[test]
    fn every_binding_has_help() {
        assert!(
            BINDINGS
                .iter()
                .all(|b| !b.help.is_empty() && !b.keys.is_empty())
        );
    }

    #[test]
    fn key_codes() {
        let k = |c, m| KeyEvent::new(c, m);
        assert_eq!(code(&k(KeyCode::Char('d'), KeyModifiers::CONTROL)), "C-d");
        assert_eq!(code(&k(KeyCode::Char('G'), KeyModifiers::SHIFT)), "G");
        assert_eq!(code(&k(KeyCode::Char(' '), KeyModifiers::NONE)), "Space");
        assert_eq!(code(&k(KeyCode::BackTab, KeyModifiers::SHIFT)), "S-Tab");
        assert_eq!(
            lookup(&[Ctx::Item], "Space").unwrap().cmd,
            Cmd::Verb(Verb::Primary)
        );
        assert_eq!(lookup(&[Ctx::Rooms], "f").unwrap().cmd, Cmd::Facets);
        assert_eq!(lookup(&[Ctx::Events], "f").unwrap().cmd, Cmd::Facets);
        assert_eq!(lookup(&[], "q").unwrap().cmd, Cmd::Quit);
        assert!(lookup(&[], "z").is_none());
    }
}
