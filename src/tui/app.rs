//! App state, messages and effects (§8.2). `update.rs` is the only writer.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

use ratatui::layout::Rect;
use serde::{Deserialize, Serialize};

use super::data::{
    BusLan, Device, Diag, DiagSample, LogLine, MsInfo, NetSample, Series, SiteStatus,
};
use super::model::{Cid, House};
use super::store::Store;
use super::theme::Theme;
use super::vm::{Choice, Plan, SetSpec};
use crate::actions::Action;
use crate::stream::StateEvent;

// ── Screens and per-screen state ────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Screen {
    #[default]
    Home,
    Rooms,
    Events,
    Energy,
    System,
    Sites,
}

impl Screen {
    pub const ALL: [Screen; 6] = [
        Screen::Home,
        Screen::Rooms,
        Screen::Events,
        Screen::Energy,
        Screen::System,
        Screen::Sites,
    ];
    pub fn from_num(n: u8) -> Option<Screen> {
        Screen::ALL.get((n as usize).wrapping_sub(1)).copied()
    }
    pub fn num(self) -> u8 {
        Screen::ALL.iter().position(|s| *s == self).unwrap_or(0) as u8 + 1
    }
    pub fn title(self) -> &'static str {
        match self {
            Screen::Home => "home",
            Screen::Rooms => "rooms",
            Screen::Events => "events",
            Screen::Energy => "energy",
            Screen::System => "system",
            Screen::Sites => "sites",
        }
    }
    pub fn parse(s: &str) -> Option<Screen> {
        let s = s.to_lowercase();
        Screen::ALL
            .into_iter()
            .find(|x| x.title() == s || s.parse::<u8>().ok() == Some(x.num()))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GroupBy {
    #[default]
    Room,
    Category,
    Type,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RoomSort {
    #[default]
    Name,
    Activity,
    Temp,
}

/// An entry of the Rooms left list.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum GKey {
    Favorites,
    All,
    Room(usize),
    Cat(usize),
    Type(String),
    Unassigned,
}

/// A list filter value (§4.5a). Values of one dimension combine with OR,
/// dimensions with AND: `room ∈ {Kitchen, Hall} ∧ type = LightController`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Facet {
    Room(usize),
    Cat(usize),
    /// Type label (versions folded, as in group-by type)
    Type(String),
    On,
    Off,
    Moving,
    /// Loxone favorite or pinned
    Fav,
    Attention,
    /// Changed in the last 10 minutes
    Recent,
    /// A control (top level) — events only
    Ctrl(Cid),
    /// Events without a control (global states, system)
    System,
}

/// Facet dimensions, in picker order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Dim {
    Flag,
    State,
    Room,
    Category,
    Type,
    Control,
    Source,
}

impl Dim {
    pub fn title(self) -> &'static str {
        match self {
            Dim::Flag => "FLAGS",
            Dim::State => "STATE",
            Dim::Room => "ROOM",
            Dim::Category => "CATEGORY",
            Dim::Type => "TYPE",
            Dim::Control => "CONTROL",
            Dim::Source => "SOURCE",
        }
    }
}

impl Facet {
    pub fn dim(&self) -> Dim {
        match self {
            Facet::Room(_) => Dim::Room,
            Facet::Cat(_) => Dim::Category,
            Facet::Type(_) => Dim::Type,
            Facet::On | Facet::Off | Facet::Moving => Dim::State,
            Facet::Fav | Facet::Attention | Facet::Recent => Dim::Flag,
            Facet::Ctrl(_) => Dim::Control,
            Facet::System => Dim::Source,
        }
    }
}

/// Lists that take facets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FacetList {
    Controls,
    Events,
}

#[derive(Debug, Default)]
pub struct RoomsState {
    pub group: GroupBy,
    /// Facets over the controls (`f`)
    pub facets: Vec<Facet>,
    pub sort: RoomSort,
    /// 0 = groups, 1 = controls, 2 = inspector
    pub pane: u8,
    pub sel_group: Option<GKey>,
    /// Selected control (UUID) per group — back restores place (§4.1 rule 10)
    pub sel_ctrl: HashMap<GKey, String>,
    pub filter_groups: String,
    pub filter_ctrls: String,
    /// Live sorts are frozen while the list has focus (§7.2)
    pub frozen: Option<Vec<GKey>>,
    pub marks: HashSet<Cid>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HomePane {
    #[default]
    Rooms,
    Attention,
    Energy,
    Pinned,
    Quick,
    Live,
}

impl HomePane {
    pub const ORDER: [HomePane; 6] = [
        HomePane::Rooms,
        HomePane::Attention,
        HomePane::Energy,
        HomePane::Pinned,
        HomePane::Quick,
        HomePane::Live,
    ];
}

#[derive(Debug, Default)]
pub struct HomeState {
    pub pane: HomePane,
    /// Card order (refreshed every 30 s while the cards don't have focus)
    pub order: Vec<usize>,
    pub order_at: f64,
    pub sel_room: usize,
    pub sel_attention: usize,
    pub sel_pin: usize,
    pub sel_quick: usize,
}

#[derive(Debug)]
pub struct EventsState {
    pub follow: bool,
    /// Selected event (identity)
    pub sel: Option<u64>,
    /// Newest seq seen when follow was turned off (for the "↓ N new" notch)
    pub seen: u64,
    /// Facets over the events (`f`)
    pub facets: Vec<Facet>,
    pub show_muted: bool,
    pub filter: String,
    /// 0 = list, 1 = detail
    pub pane: u8,
}

impl Default for EventsState {
    fn default() -> Self {
        EventsState {
            follow: true,
            sel: None,
            seen: 0,
            facets: Vec::new(),
            show_muted: false,
            filter: String::new(),
            pane: 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ERange {
    #[default]
    Now,
    Today,
    Week,
    Month,
}

impl ERange {
    pub const ALL: [ERange; 4] = [ERange::Now, ERange::Today, ERange::Week, ERange::Month];
    pub fn title(self) -> &'static str {
        match self {
            ERange::Now => "now",
            ERange::Today => "today",
            ERange::Week => "7d",
            ERange::Month => "30d",
        }
    }
}

#[derive(Debug, Default)]
pub struct EnergyState {
    pub range: ERange,
    pub sel_meter: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum SysView {
    #[default]
    Overview,
    Devices,
    BusLan,
    Log,
    Config,
    Update,
}

impl SysView {
    pub const ALL: [SysView; 6] = [
        SysView::Overview,
        SysView::Devices,
        SysView::BusLan,
        SysView::Log,
        SysView::Config,
        SysView::Update,
    ];
    pub fn title(self) -> &'static str {
        match self {
            SysView::Overview => "overview",
            SysView::Devices => "devices",
            SysView::BusLan => "bus & lan",
            SysView::Log => "log",
            SysView::Config => "config",
            SysView::Update => "update",
        }
    }
}

#[derive(Debug, Default)]
pub struct SystemState {
    pub view: SysView,
    pub sel: HashMap<SysView, usize>,
    pub log_search: String,
    /// Config: selected commit; its diff shows in the right pane
    pub diff_scroll: usize,
    /// Config/Log: 0 = list, 1 = detail
    pub pane: u8,
}

#[derive(Debug, Default)]
pub struct SitesState {
    pub sel: usize,
}

// ── Overlays ────────────────────────────────────────────────────────────────

/// A single-line text editor (palette, filters, value input).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Line {
    pub buf: String,
    /// Cursor as a char index
    pub cur: usize,
    /// A prefilled value shown as selected: the first typed character
    /// replaces it, any other edit keeps it (`=` then `30⏎` sets 30)
    pub fresh: bool,
}

impl Line {
    pub fn new(s: &str) -> Line {
        Line {
            buf: s.to_string(),
            cur: s.chars().count(),
            fresh: false,
        }
    }
    pub fn prefilled(s: &str) -> Line {
        Line {
            fresh: !s.is_empty(),
            ..Line::new(s)
        }
    }
    fn byte(&self, ci: usize) -> usize {
        self.buf
            .char_indices()
            .nth(ci)
            .map(|(b, _)| b)
            .unwrap_or(self.buf.len())
    }
    pub fn insert(&mut self, s: &str) {
        // Paste is text, never a submit (§4.1 rule 9): newlines become spaces
        let s: String = s
            .chars()
            .map(|c| {
                if c == '\n' || c == '\r' || c == '\t' {
                    ' '
                } else {
                    c
                }
            })
            .filter(|c| !c.is_control())
            .collect();
        if std::mem::take(&mut self.fresh) {
            self.kill();
        }
        let b = self.byte(self.cur);
        self.buf.insert_str(b, &s);
        self.cur += s.chars().count();
    }
    pub fn backspace(&mut self) {
        if self.cur > 0 {
            let (a, b) = (self.byte(self.cur - 1), self.byte(self.cur));
            self.buf.replace_range(a..b, "");
            self.cur -= 1;
        }
    }
    pub fn delete(&mut self) {
        if self.cur < self.buf.chars().count() {
            let (a, b) = (self.byte(self.cur), self.byte(self.cur + 1));
            self.buf.replace_range(a..b, "");
        }
    }
    pub fn left(&mut self) {
        self.cur = self.cur.saturating_sub(1);
    }
    pub fn right(&mut self) {
        self.cur = (self.cur + 1).min(self.buf.chars().count());
    }
    pub fn home(&mut self) {
        self.cur = 0;
    }
    pub fn end(&mut self) {
        self.cur = self.buf.chars().count();
    }
    pub fn kill(&mut self) {
        self.buf.clear();
        self.cur = 0;
    }
    /// Ctrl-w: delete the word before the cursor
    pub fn kill_word(&mut self) {
        let chars: Vec<char> = self.buf.chars().collect();
        let mut i = self.cur;
        while i > 0 && chars[i - 1] == ' ' {
            i -= 1;
        }
        while i > 0 && chars[i - 1] != ' ' {
            i -= 1;
        }
        let (a, b) = (self.byte(i), self.byte(self.cur));
        self.buf.replace_range(a..b, "");
        self.cur = i;
    }
    pub fn set(&mut self, s: &str) {
        *self = Line::new(s);
    }
}

/// What a filter input edits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterTarget {
    Groups,
    Ctrls,
    Events,
    Log,
    Help,
}

#[derive(Debug, Clone)]
pub enum InputKind {
    Filter {
        target: FilterTarget,
        prev: String,
    },
    Value {
        cids: Vec<Cid>,
        spec: SetSpec,
    },
    /// `+` in the history chart: add a series (fuzzy control name)
    ChartAdd,
    /// Masked PIN for secured alarm commands (never stored)
    Pin {
        plans: Vec<Plan>,
    },
}

/// One palette result.
#[derive(Debug, Clone, PartialEq)]
pub enum PalItem {
    Ctrl(Cid),
    Room(usize),
    Scene(String),
    Screen(Screen),
    Site(String),
    /// A parsed command line ready to run
    Run {
        label: String,
        plans: Vec<Plan>,
    },
    RunScene(String),
    /// Explanation (not runnable), e.g. "use the Rooms screen"
    Note(String),
}

#[derive(Debug, Clone, Default)]
pub struct PaletteState {
    pub line: Line,
    pub sel: usize,
    /// Browsing history with ↑/↓ in an empty palette
    pub hist_pos: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct Confirm {
    pub title: String,
    pub body: Vec<String>,
    pub plans: Vec<Plan>,
    /// Non-control effect to run on yes (reboot / install)
    pub effect: Option<Box<Effect>>,
    /// Typed confirmation: the word to type (the context name)
    pub typed: Option<String>,
    pub input: Line,
}

#[derive(Debug, Clone)]
pub struct WiringState {
    /// Center block UUID
    pub center: String,
    /// Selected wire row (inputs then outputs)
    pub sel: usize,
    /// Navigation history for Esc/h
    pub back: Vec<String>,
}

#[derive(Debug, Clone)]
pub enum Overlay {
    Help {
        scroll: usize,
        filter: String,
    },
    Palette(PaletteState),
    /// `a`: every action for the item
    Menu {
        cids: Vec<Cid>,
        /// A room's menu (all lights / blinds)
        room: Option<usize>,
        items: Vec<(String, Option<char>, Action)>,
        sel: usize,
    },
    /// `m`: moods / modes
    Picker {
        cids: Vec<Cid>,
        title: String,
        choices: Vec<Choice>,
        sel: usize,
    },
    Confirm(Confirm),
    Input {
        kind: InputKind,
        line: Line,
        err: Option<String>,
    },
    Contexts {
        sel: usize,
    },
    /// `f`: facet picker (typing narrows the values)
    Facets {
        list: FacetList,
        line: Line,
        sel: usize,
    },
    MsgLog {
        scroll: usize,
    },
    Wiring(WiringState),
    /// History chart with timeframes
    Chart(ChartState),
    /// Full value of a state (pretty-printed JSON), scrollable
    Value {
        cid: Cid,
        state: String,
        scroll: usize,
    },
    /// Full text of a list row (log line, diff line), scrollable
    Text {
        title: String,
        sub: String,
        text: String,
        scroll: usize,
    },
    /// Compact inspector (narrow layouts, Home, palette)
    Inspector {
        cid: Cid,
        scroll: usize,
    },
}

// ── Feedback ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    Ok,
    Info,
    Err,
}

#[derive(Debug, Clone)]
pub struct Toast {
    pub kind: ToastKind,
    pub text: String,
    pub until: f64,
}

#[derive(Debug, Clone)]
pub struct LogMsg {
    pub t: f64,
    pub err: bool,
    pub what: String,
    pub detail: String,
    /// The `lox` command that failed (if any)
    pub cli: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PendState {
    /// Sent, waiting for the stream (`⋯`)
    Sent,
    /// No confirmation within 3 s (`?`) — may still have worked
    Unconfirmed,
    Failed(String),
}

#[derive(Debug, Clone)]
pub struct Pending {
    pub req: u64,
    pub since: f64,
    pub label: String,
    /// The value the control will have (for chained `+`/`-`)
    pub optimistic: Option<f64>,
    pub state: PendState,
    /// HTTP call returned OK
    pub acked: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Conn {
    Connecting,
    Live,
    Reconnecting { attempt: u32, at: f64 },
    Offline(String),
    OutOfService,
}

// ── History chart ───────────────────────────────────────────────────────────

/// Timeframe of the history chart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum Span {
    H6,
    #[default]
    H24,
    D7,
    D30,
    Y1,
}

impl Span {
    pub const ALL: [Span; 5] = [Span::H6, Span::H24, Span::D7, Span::D30, Span::Y1];

    pub fn secs(self) -> i64 {
        match self {
            Span::H6 => 6 * 3600,
            Span::H24 => 86_400,
            Span::D7 => 7 * 86_400,
            Span::D30 => 30 * 86_400,
            Span::Y1 => 365 * 86_400,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Span::H6 => "6h",
            Span::H24 => "24h",
            Span::D7 => "7d",
            Span::D30 => "30d",
            Span::Y1 => "1y",
        }
    }
}

/// One fetched window: a control over `span`, `back` periods before now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ChartKey {
    pub cid: Cid,
    pub span: Span,
    pub back: i64,
}

/// Fetched chart data: the window (unix seconds) and its points.
#[derive(Debug, Clone)]
pub struct ChartData {
    pub from: i64,
    pub to: i64,
    pub series: Series,
}

/// `c`: the full-screen history chart (§5.10).
#[derive(Debug, Clone, PartialEq)]
pub struct ChartState {
    /// The control, then added series (`+`)
    pub cids: Vec<Cid>,
    pub span: Span,
    /// Periods before now (`←`/`→`)
    pub back: i64,
    /// Previous period as a faint line
    pub compare: bool,
    /// Cursor column (cells from the left of the plot)
    pub cursor: Option<usize>,
}

impl ChartState {
    pub fn key(&self, cid: Cid, prev: bool) -> ChartKey {
        ChartKey {
            cid,
            span: self.span,
            back: self.back + prev as i64,
        }
    }
}

// ── Polling ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PollKind {
    Diag,
    Info,
    BusLan,
    Devices,
    Log,
    Sites,
    /// Miniserver statistics for a control (inspector history)
    History(Cid),
    /// A window of statistics for the history chart
    Chart(ChartKey),
    /// Today's PV / consumption from meter statistics
    EnergyDay,
    ConfigLog,
    /// Diff of a gitops commit against its parent
    ConfigDiff(String),
}

#[derive(Debug, Clone)]
pub enum Polled {
    Diag(Diag),
    Info(MsInfo),
    BusLan(BusLan),
    Devices(Vec<Device>),
    Log(Vec<LogLine>),
    Sites(Vec<SiteStatus>),
    History(Cid, Series),
    Chart(ChartKey, ChartData),
    EnergyDay {
        pv: Vec<f64>,
        usage: Vec<f64>,
    },
    /// history, and where pulls store `config.Loxone` (for the footer)
    ConfigLog(Result<Vec<Commit>, String>, Option<String>),
    ConfigDiff(String, Vec<String>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Commit {
    pub hash: String,
    pub date: String,
    pub subject: String,
    /// When the config was saved in Loxone Config (the backup's date)
    pub saved: Option<String>,
    /// Config version, e.g. `v273`
    pub version: Option<String>,
    /// One-line summary of the commit body (`+ Registriertes Gerät (PuDe)`, `+60 −2`)
    pub summary: String,
}

impl Commit {
    /// From `git log`: subject `Config backup 2026-09-25 18:39:01 (v273)` (as
    /// `lox config pull` writes it) and a body listing the changed controls.
    pub fn new(hash: &str, date: &str, subject: &str, body: &str) -> Commit {
        let saved = subject
            .strip_prefix("Config backup ")
            .and_then(|r| r.get(..16))
            .map(str::to_string);
        let version = subject
            .rsplit_once("(v")
            .and_then(|(_, v)| v.strip_suffix(')'))
            .map(|v| format!("v{}", v));
        let lines: Vec<&str> = body
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .collect();
        let (add, rem) = (
            lines.iter().filter(|l| l.starts_with("+ ")).count(),
            lines.iter().filter(|l| l.starts_with("- ")).count(),
        );
        let summary = match lines.as_slice() {
            [] => String::new(),
            [one] if one.starts_with("No structural") => "no structural changes".into(),
            // `+ Added control: "Name" (Type)` → `+ Name (Type)`
            [one] => one
                .replace("Added control: ", "")
                .replace("Removed control: ", "")
                .replace("Changed control: ", "")
                .replace('"', ""),
            _ if add + rem > 0 => format!("+{} −{} controls", add, rem),
            [first, ..] => first.to_string(),
        };
        Commit {
            hash: hash.into(),
            date: date.into(),
            subject: subject.into(),
            saved,
            version,
            summary,
        }
    }
}

#[derive(Debug, Default)]
pub struct PollState {
    /// last request time per kind
    pub last: HashMap<PollKind, f64>,
    /// kinds whose last poll failed (errors are logged once, not every interval)
    pub failing: HashSet<PollKind>,
    /// in-flight request per kind (newer supersedes older)
    pub inflight: HashMap<PollKind, u64>,
}

#[derive(Debug, Clone)]
pub enum WiringDoc {
    None,
    Loading,
    Ready(std::sync::Arc<crate::logic::Logic>, String),
    Failed(String),
    /// No `.Loxone` available; offer an FTP download
    Missing,
}

// ── Msg / Effect ────────────────────────────────────────────────────────────

// Polled results are the big variant; messages are few per frame, so boxing
// would only add noise at every construction site.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone)]
pub enum Msg {
    Key(crossterm::event::KeyEvent),
    Paste(String),
    Mouse(crossterm::event::MouseEvent),
    Resize(u16, u16),
    Tick {
        now: f64,
    },
    States {
        epoch: u64,
        batch: Vec<StateEvent>,
    },
    Conn {
        epoch: u64,
        conn: Conn,
    },
    Polled {
        epoch: u64,
        req: u64,
        kind: PollKind,
        result: Result<Polled, String>,
    },
    CmdDone {
        epoch: u64,
        req: u64,
        cid: Cid,
        result: Result<(), String>,
    },
    Wiring {
        epoch: u64,
        doc: WiringDoc,
    },
    /// A new house after a context switch or structure refresh
    NewHouse {
        epoch: u64,
        house: Box<House>,
        ctx: String,
    },
    Toast(ToastKind, String),
    Log(LogMsg),
    /// `P`: `lox config pull` finished (Ok(true) = a new commit)
    ConfigPulled(Result<bool, String>),
}

#[derive(Debug, Clone)]
pub enum Effect {
    Send {
        req: u64,
        cid: Cid,
        uuid: String,
        cmds: Vec<String>,
        cli: String,
        coalesce: bool,
        /// Masked out of error text (alarm PIN, sent in the URL path)
        secret: Option<String>,
    },
    Poll {
        req: u64,
        kind: PollKind,
    },
    /// OSC 52 clipboard
    Copy(String),
    SwitchContext(String),
    /// Ctrl-r: refetch the structure and reconnect
    Refresh,
    SaveState(UiState),
    RunScene(String),
    LoadWiring {
        download: bool,
    },
    ConfigPull,
    Reboot,
    Install,
    Suspend,
    Quit,
}

// ── Persisted preferences and UI state ──────────────────────────────────────

/// `~/.lox/tui.yaml`
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    pub theme: Option<String>,
    pub icons: Option<String>,
    pub motion: Option<bool>,
    pub mouse: Option<bool>,
    pub transparent: Option<bool>,
    /// Energy role overrides: control name or UUID → grid|production|storage|load
    pub roles: BTreeMap<String, String>,
}

/// `~/.lox/contexts/<n>/tui-state.yaml`
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct UiState {
    pub pins: Vec<String>,
    pub mutes: Vec<String>,
    pub palette_history: Vec<String>,
    pub last_screen: Option<Screen>,
    /// Last timeframe of the history chart
    pub chart_span: Option<Span>,
}

// ── Render-time caches (written by render, read by update for mouse / paging)

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Hit {
    Tab(Screen),
    /// A list row: (list id, index)
    Row(u8, usize),
    Pane(u8),
    SubTab(usize),
}

/// Mouse text selection (drag), copied on release like a terminal would.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Drag {
    /// (column, row) where the button went down, and where it is now
    pub from: (u16, u16),
    pub to: (u16, u16),
    /// Columns the selection stays within (the pane under the press)
    pub clip: (u16, u16),
    pub moved: bool,
}

impl Drag {
    /// Selected cells per row, reading order: `(row, first col, last col)` inclusive.
    pub fn spans(&self) -> Vec<(u16, u16, u16)> {
        let (a, b) = if (self.from.1, self.from.0) <= (self.to.1, self.to.0) {
            (self.from, self.to)
        } else {
            (self.to, self.from)
        };
        let (lo, hi) = (self.clip.0, self.clip.1.saturating_sub(1));
        (a.1..=b.1)
            .filter_map(|y| {
                let c0 = if y == a.1 { a.0 } else { lo }.clamp(lo, hi);
                let c1 = if y == b.1 { b.0 } else { hi }.clamp(lo, hi);
                (c0 <= c1).then_some((y, c0, c1))
            })
            .collect()
    }
}

#[derive(Debug, Default)]
pub struct UiCache {
    pub hits: Vec<(Rect, Hit)>,
    /// Visible rows of the focused list (for half-page moves)
    pub page: usize,
    /// Scroll offsets per list id (kept so the selection stays in view)
    pub offsets: HashMap<u8, usize>,
    /// Columns of the Home card grid
    pub home_cols: usize,
    /// Plot width of the history chart (cursor bounds)
    pub chart_w: usize,
    /// The last frame while a mouse selection is active (to copy its text)
    pub frame: Option<ratatui::buffer::Buffer>,
}

// ── App ─────────────────────────────────────────────────────────────────────

pub struct Opts {
    pub read_only: bool,
    pub demo: bool,
    pub mouse: bool,
    pub motion: bool,
    pub nerd: bool,
}

pub struct App {
    pub house: House,
    pub store: Store,
    pub th: Theme,
    pub opts: Opts,
    pub ctx_name: String,
    pub contexts: Vec<String>,
    /// Context generation: bumped on switch / reconnect; stale results are dropped
    pub epoch: u64,
    pub next_req: u64,
    /// Unix seconds
    pub now: f64,
    /// Local time offset (seconds) for display
    pub tz: i64,
    pub started: f64,
    pub conn: Conn,
    pub conn_since: f64,
    pub paused: bool,
    pub screen: Screen,
    pub size: (u16, u16),
    pub rooms: RoomsState,
    pub home: HomeState,
    pub events: EventsState,
    pub energy: EnergyState,
    pub system: SystemState,
    pub sites_ui: SitesState,
    pub overlays: Vec<Overlay>,
    pub toasts: Vec<Toast>,
    pub msglog: VecDeque<LogMsg>,
    pub unread_errors: usize,
    pub pending: HashMap<Cid, Pending>,
    pub last_dir: HashMap<Cid, i8>,
    pub ui_state: UiState,
    pub scenes: Vec<String>,
    pub polls: PollState,
    // polled data (time received, value)
    pub diag: Option<(f64, Diag)>,
    pub diag_hist: VecDeque<DiagSample>,
    /// LAN / CAN packet rates between bus & LAN polls
    pub net_hist: VecDeque<NetSample>,
    pub info: Option<MsInfo>,
    pub buslan: Option<(f64, BusLan)>,
    pub buslan_prev: Option<BusLan>,
    /// Error counters whose delta was non-zero recently (name → time)
    pub buslan_flash: HashMap<String, f64>,
    pub devices: Option<(f64, Vec<Device>)>,
    pub log: Option<(f64, Vec<LogLine>)>,
    pub sites: Vec<SiteStatus>,
    pub history: HashMap<Cid, Series>,
    /// History chart windows
    pub charts: HashMap<ChartKey, ChartData>,
    /// Selected state row of the inspector (Rooms pane 3)
    pub insp_sel: usize,
    pub energy_day: Option<(Vec<f64>, Vec<f64>)>,
    pub commits: Option<Result<Vec<Commit>, String>>,
    pub diffs: HashMap<String, Vec<String>>,
    /// Diffs that failed (hash → error), shown instead of "loading"
    pub diff_errs: HashMap<String, String>,
    /// `P`: when the pull started or finished; `None` result = still running
    pub pull: Option<(f64, Option<Result<bool, String>>)>,
    /// `config.Loxone` in the config git repo (from the history poll)
    pub config_file: Option<String>,
    /// Mouse text selection in progress or just made
    pub drag: Option<Drag>,
    pub wiring: WiringDoc,
    /// Last key press (code, time) for auto-repeat suppression (§4.1 rule 8)
    pub last_key: Option<(String, f64)>,
    /// State batches held back while paused (`p`)
    pub paused_buf: Vec<StateEvent>,
    pub ui: RefCell<UiCache>,
    pub frame: u64,
    pub quit: bool,
}

impl App {
    pub fn new(house: House, th: Theme, opts: Opts, ctx_name: String, now: f64) -> App {
        App {
            house,
            store: Store::new(),
            th,
            opts,
            ctx_name,
            contexts: Vec::new(),
            epoch: 1,
            next_req: 1,
            now,
            tz: 0,
            started: now,
            conn: Conn::Connecting,
            conn_since: now,
            paused: false,
            screen: Screen::Home,
            size: (120, 36),
            rooms: RoomsState::default(),
            home: HomeState::default(),
            events: EventsState::default(),
            energy: EnergyState::default(),
            system: SystemState::default(),
            sites_ui: SitesState::default(),
            overlays: Vec::new(),
            toasts: Vec::new(),
            msglog: VecDeque::new(),
            unread_errors: 0,
            pending: HashMap::new(),
            last_dir: HashMap::new(),
            ui_state: UiState::default(),
            scenes: Vec::new(),
            polls: PollState::default(),
            diag: None,
            diag_hist: VecDeque::new(),
            net_hist: VecDeque::new(),
            info: None,
            buslan: None,
            buslan_prev: None,
            buslan_flash: HashMap::new(),
            devices: None,
            log: None,
            sites: Vec::new(),
            history: HashMap::new(),
            charts: HashMap::new(),
            insp_sel: 0,
            energy_day: None,
            commits: None,
            diffs: HashMap::new(),
            diff_errs: HashMap::new(),
            pull: None,
            config_file: None,
            drag: None,
            wiring: WiringDoc::None,
            last_key: None,
            paused_buf: Vec::new(),
            ui: RefCell::new(UiCache::default()),
            frame: 0,
            quit: false,
        }
    }

    pub fn req(&mut self) -> u64 {
        self.next_req += 1;
        self.next_req
    }

    /// Apply the persisted UI state (pins, mutes, history, last screen).
    pub fn load_ui_state(&mut self, st: UiState) {
        self.store.mutes = st.mutes.iter().cloned().collect();
        self.ui_state = st;
    }

    pub fn is_pinned(&self, cid: Cid) -> bool {
        self.ui_state.pins.contains(&self.house.ctrls[cid].uuid)
    }

    pub fn pinned(&self) -> Vec<Cid> {
        self.ui_state
            .pins
            .iter()
            .filter_map(|u| self.house.by_uuid.get(u).copied())
            .collect()
    }

    /// Seconds of local day (for "today" charts and the clock).
    pub fn local_secs(&self) -> i64 {
        (self.now as i64 + self.tz).rem_euclid(86_400)
    }

    pub fn clock(&self) -> String {
        let s = self.local_secs();
        format!("{:02}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60)
    }

    pub fn hhmm(&self, t: f64) -> String {
        let s = (t as i64 + self.tz).rem_euclid(86_400);
        format!("{:02}:{:02}", s / 3600, (s / 60) % 60)
    }

    pub fn hhmmss(&self, t: f64) -> String {
        let s = (t as i64 + self.tz).rem_euclid(86_400);
        format!("{:02}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60)
    }

    pub fn live(&self) -> bool {
        self.conn == Conn::Live
    }

    pub fn top_overlay(&self) -> Option<&Overlay> {
        self.overlays.last()
    }

    pub fn toast(&mut self, kind: ToastKind, text: impl Into<String>) {
        let text = text.into();
        self.toasts.retain(|t| t.text != text);
        self.toasts.push(Toast {
            kind,
            text,
            until: self.now + if kind == ToastKind::Err { 6.0 } else { 3.0 },
        });
        if self.toasts.len() > 3 {
            self.toasts.remove(0);
        }
    }

    /// Record a failure: toast + message log (a toast is never the only record, §5.1).
    pub fn fail(
        &mut self,
        what: impl Into<String>,
        detail: impl Into<String>,
        cli: Option<String>,
    ) {
        let what = what.into();
        let detail = detail.into();
        self.toast(ToastKind::Err, format!("{} — {}", what, detail));
        self.log_msg(true, what, detail, cli);
    }

    pub fn log_msg(
        &mut self,
        err: bool,
        what: impl Into<String>,
        detail: impl Into<String>,
        cli: Option<String>,
    ) {
        self.msglog.push_back(LogMsg {
            t: self.now,
            err,
            what: what.into(),
            detail: detail.into(),
            cli,
        });
        if err {
            self.unread_errors += 1;
        }
        while self.msglog.len() > 500 {
            self.msglog.pop_front();
        }
    }
}
