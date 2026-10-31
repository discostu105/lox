//! Lox Logic abstract syntax and canonical text emission.

pub use crate::access::PrincipalKind;
use crate::connectors::{PortUnit, port_unit};
use crate::error::{Error, Result};
use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

mod check;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Module {
    pub items: Vec<Item>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Item {
    Extern(ExternDecl),
    Block(BlockDecl),
    Wire(WireDecl),
    Set(SetDecl),
    /// `target.title|room|category = value` — claim one attribute of an
    /// extern object (D39).
    Attr(AttrDecl),
    Let(LetDecl),
    Removed(RemovedDecl),
    Moved(MovedDecl),
    /// `template <name>(<params>)` … `end` — a reusable statement body
    /// (D23). Declares nothing by itself; each instantiation expands it.
    Template(TemplateDecl),
    /// `<slug> = <template_name>(<param>: <arg>, …)` — a template
    /// instantiation, distinguished from a block declaration by the
    /// lowercase callee. Shares [`BlockDecl`]'s shape (`block_type`
    /// holds the template name; a label string is not allowed).
    Instance(BlockDecl),
    /// `target.Port <- <expr>` — expression sugar (D24): the boolean
    /// expression desugars into managed gate/comparator blocks whose
    /// result is wired onto the extern port. A bare `slug.Port` RHS is a
    /// plain [`Item::Wire`], not an expression.
    ExprWire(ExprWireDecl),
    /// `<slug> = <expr>`, a named signal. It desugars like an inline
    /// expression, but its top block is `<slug>` (hand-owned in the
    /// lockfile). Its sub-blocks are `<slug>__<op><n>` (expression-owned).
    Signal(SignalDecl),
    /// `page "Title"` — the base-config page the following block
    /// declarations are placed on (D28). Positional: governs every block
    /// after it, until the next `page` statement; blocks above the first
    /// `page` statement keep the compile options' default page.
    Page(PageDecl),
    /// `test "Name"` … `end` — a simulator-backed scenario (D36).
    /// `compile` ignores it entirely; `loxl test` compiles the module in
    /// memory and replays the body in the simulator (D68).
    Test(TestDecl),
    /// `tiles <name>` … `end` — a named set of Touch Pure Flex tile
    /// programs (D41).
    Tiles(TilesDecl),
    /// `<device>.tiles = <name>` — write a tiles set onto an extern Touch
    /// Pure Flex (D41).
    TilesClaim(TilesClaim),
    /// `access <slug>` … `end` — who may open an extern NFC Code Touch,
    /// and when (D49).
    Access(AccessDecl),
    /// `schedule <name>` … `end` — a named DayTimer schedule (D53).
    Schedule(ScheduleDecl),
    /// `<timer>.schedule = <name>` — write a schedule onto a DayTimer,
    /// extern or managed (D53).
    ScheduleClaim(ScheduleClaim),
    /// `moods <name>` … `end` — a named set of LightController2 moods
    /// (D58).
    Moods(MoodsDecl),
    /// `<controller>.moods = <name>` — write a mood set onto a
    /// LightController2, extern or managed (D58).
    MoodsClaim(MoodsClaim),
    /// `rule <slug>` … `end` — what an extern Automatic Designer rule does:
    /// active or not, its conditions and its actions (D57).
    Rule(RuleDecl),
    /// A whole-line `#` comment, stored verbatim (text after the `#`) so
    /// formatting is non-destructive. Statements carry their own trailing
    /// comments; argument lists carry theirs as [`ArgItem`]s.
    Comment(String),
}

/// A parameter/assignment value. The variant records how the value was
/// written, so canonical emission never has to guess from the content — a
/// quoted string stays quoted even when it happens to contain digits and
/// signs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    /// Bare numeric literal, kept exactly as written (e.g. `-2.5`).
    /// Always matches `-?[0-9]+(\.[0-9]+)?`.
    Number(String),
    /// Quoted string, stored decoded. (A quoted string that reads as a
    /// number is canonicalized to [`Value::Number`] at parse time, so each
    /// value has exactly one canonical spelling.)
    Str(String),
    /// Bare identifier referencing a `let` constant.
    Ref(String),
    /// A number with a unit suffix (`40s`, `1.5h`, `2700K` — D27), kept as
    /// written; the compiler resolves it to the port's base unit
    /// (`1.5h` → `5400`) when emitting `Def=`.
    Unit { number: String, unit: Unit },
    /// `file("hue.c")` — text read from a file next to the declaring
    /// `.loxl` file (D40); binds only program text (`Code:` on Code16).
    /// `path` is kept as written; `text` is filled by
    /// [`Module::load_files`] (CRLF normalized to LF) and stays `None`
    /// for a module parsed from a string alone.
    File { path: String, text: Option<String> },
    /// A time of day, `HH:MM` or `HH:MM:SS`, kept as written (two-digit
    /// minutes and seconds, hours 0–23); it resolves to seconds since
    /// midnight (`19:00` → `68400`). Binds only time-of-day parameters
    /// (`connectors::clock_params`, `At:` on PulseAt).
    Clock(String),
    /// `on` / `off` — a digital value (v2). Compiles to what the port
    /// stores: `1`/`0` on a connector or numeric attribute, `true`/`false`
    /// on a text-boolean attribute (`connectors::bool_params`, e.g.
    /// `Analog:` on VirtualIn).
    Bool(bool),
}

/// Seconds since midnight of a `HH:MM[:SS]` time of day, `None` unless
/// it is one (00:00 … 23:59:59, minutes and seconds two digits).
pub fn clock_seconds(s: &str) -> Option<u32> {
    let mut parts = s.split(':');
    let (h, m, sec) = (parts.next()?, parts.next()?, parts.next());
    if parts.next().is_some() || h.is_empty() || h.len() > 2 || m.len() != 2 {
        return None;
    }
    let digits = |p: &str| p.bytes().all(|b| b.is_ascii_digit());
    if !digits(h) || !digits(m) || sec.is_some_and(|x| x.len() != 2 || !digits(x)) {
        return None;
    }
    let (h, m): (u32, u32) = (h.parse().ok()?, m.parse().ok()?);
    let sec: u32 = sec.map_or(Some(0), |x| x.parse().ok())?;
    (h < 24 && m < 60 && sec < 60).then_some(h * 3600 + m * 60 + sec)
}

/// The canonical spelling of seconds since midnight: `HH:MM`, or
/// `HH:MM:SS` when the seconds are not zero. `None` beyond a day.
pub fn clock_text(seconds: u32) -> Option<String> {
    if seconds >= 86_400 {
        return None;
    }
    let (h, m, s) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
    Some(if s == 0 {
        format!("{h:02}:{m:02}")
    } else {
        format!("{h:02}:{m:02}:{s:02}")
    })
}

/// A value's unit suffix (D27). Time units scale to Loxone's base unit,
/// seconds; `K` (color temperature) and `%` are annotations with factor 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unit {
    Ms,
    S,
    Min,
    H,
    K,
    Pct,
}

impl Unit {
    pub fn suffix(self) -> &'static str {
        match self {
            Unit::Ms => "ms",
            Unit::S => "s",
            Unit::Min => "min",
            Unit::H => "h",
            Unit::K => "K",
            Unit::Pct => "%",
        }
    }

    pub fn parse(s: &str) -> Option<Unit> {
        Some(match s {
            "ms" => Unit::Ms,
            "s" => Unit::S,
            "min" => Unit::Min,
            "h" => Unit::H,
            "K" => Unit::K,
            "%" => Unit::Pct,
            _ => return None,
        })
    }

    /// `(multiplier, extra decimal digits)`: the literal scales by
    /// `multiplier / 10^shift` into the base unit.
    fn factor(self) -> (u128, u32) {
        match self {
            Unit::Ms => (1, 3),
            Unit::S | Unit::K | Unit::Pct => (1, 0),
            Unit::Min => (60, 0),
            Unit::H => (3600, 0),
        }
    }
}

impl fmt::Display for Unit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.suffix())
    }
}

/// Resolve a suffixed value for a port whose documented unit is
/// `port_unit` (D52): time suffixes scale into the port's own time unit
/// (seconds or minutes), `%` and `K` bind only where they mean what the
/// port is in. `None` (undocumented port) keeps the D27 reading. The
/// error is the reason, without the port's name (callers add it).
pub(crate) fn scale_for_port(
    number: &str,
    unit: Unit,
    port_unit: Option<PortUnit>,
) -> std::result::Result<String, String> {
    let overflow = || format!("`{number}{unit}` overflows its unit");
    let is_time = matches!(unit, Unit::Ms | Unit::S | Unit::Min | Unit::H);
    let mismatch = |pu: PortUnit| {
        Err(format!(
            "`{number}{unit}` does not fit — the port is documented in {}",
            pu.describe()
        ))
    };
    match port_unit {
        None => scale_by_unit(number, unit).ok_or_else(overflow),
        Some(PortUnit::Seconds) if is_time => scale_by_unit(number, unit).ok_or_else(overflow),
        Some(PortUnit::Minutes) if is_time => {
            let seconds = scale_by_unit(number, unit).ok_or_else(overflow)?;
            divide_exact(&seconds, 60).ok_or_else(|| {
                format!(
                    "`{number}{unit}` is no exact decimal number of minutes — \
                     the port is documented in minutes; write it in `min`"
                )
            })
        }
        Some(PortUnit::Percent) if unit == Unit::Pct => Ok(number.to_string()),
        Some(PortUnit::Kelvin) if unit == Unit::K => Ok(number.to_string()),
        Some(pu) => mismatch(pu),
    }
}

/// `decimal / divisor` as an exact decimal literal, or `None` when the
/// quotient does not terminate (`10 / 60`).
fn divide_exact(decimal: &str, divisor: u128) -> Option<String> {
    let (neg, rest) = match decimal.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, decimal),
    };
    let (int, frac) = rest.split_once('.').unwrap_or((rest, ""));
    let mut num: u128 = format!("{int}{frac}").parse().ok()?;
    let mut den = divisor.checked_mul(10u128.checked_pow(frac.len() as u32)?)?;
    let g = gcd(num, den);
    num /= g;
    den /= g;
    // Terminating iff the reduced denominator is 2^a·5^b; then
    // num/den = num·(10^k/den) / 10^k with k = max(a, b).
    let (mut twos, mut fives, mut rest) = (0u32, 0u32, den);
    while rest % 2 == 0 {
        rest /= 2;
        twos += 1;
    }
    while rest % 5 == 0 {
        rest /= 5;
        fives += 1;
    }
    if rest != 1 {
        return None;
    }
    let point = twos.max(fives);
    let num = num.checked_mul(10u128.checked_pow(point)? / den)?;
    let digits = num.to_string();
    let point = point as usize;
    let mut out = if point == 0 {
        digits
    } else {
        let (i, f) = if digits.len() > point {
            let split = digits.len() - point;
            (digits[..split].to_string(), digits[split..].to_string())
        } else {
            ("0".to_string(), format!("{digits:0>point$}"))
        };
        let f = f.trim_end_matches('0');
        if f.is_empty() { i } else { format!("{i}.{f}") }
    };
    if neg && out != "0" {
        out.insert(0, '-');
    }
    Some(out)
}

fn gcd(mut a: u128, mut b: u128) -> u128 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a.max(1)
}

/// Scale a number literal (`-?digits(.digits)?`) by a unit's factor,
/// exactly, rendering a plain number literal (`1.5`×3600 → `5400`,
/// `250`÷1000 → `0.25`). `None` when the digits overflow the exact range.
pub(crate) fn scale_by_unit(number: &str, unit: Unit) -> Option<String> {
    let (mul, shift) = unit.factor();
    let (neg, rest) = match number.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, number),
    };
    let (int, frac) = rest.split_once('.').unwrap_or((rest, ""));
    let mantissa: u128 = format!("{int}{frac}").parse().ok()?;
    let mantissa = mantissa.checked_mul(mul)?;
    let point = frac.len() as u32 + shift;
    let digits = mantissa.to_string();
    let mut out = if point == 0 {
        digits
    } else {
        let point = point as usize;
        let (int, frac) = if digits.len() > point {
            let split = digits.len() - point;
            (digits[..split].to_string(), digits[split..].to_string())
        } else {
            ("0".to_string(), format!("{digits:0>point$}"))
        };
        let frac = frac.trim_end_matches('0');
        if frac.is_empty() {
            int
        } else {
            format!("{int}.{frac}")
        }
    };
    if neg && out != "0" {
        out.insert(0, '-');
    }
    Some(out)
}

impl Value {
    /// Classify a raw literal (e.g. a `Def=` value lifted from XML).
    pub fn from_literal(s: &str) -> Value {
        if is_number_literal(s) {
            Value::Number(s.to_string())
        } else {
            Value::Str(s.to_string())
        }
    }

    /// The literal content this value resolves to: `Number`/`Str` verbatim,
    /// `Unit` scaled into its base unit, `File` its loaded text, `Clock`
    /// its seconds since midnight, `Bool` `1`/`0`; `None` for
    /// a `Ref` (resolve it through [`Module::resolve_value`]) and for a
    /// `File` that was never loaded.
    pub fn literal(&self) -> Option<Cow<'_, str>> {
        match self {
            Value::Number(s) | Value::Str(s) => Some(Cow::Borrowed(s)),
            Value::Ref(_) => None,
            Value::Unit { number, unit } => scale_by_unit(number, *unit).map(Cow::Owned),
            Value::File { text, .. } => text.as_deref().map(Cow::Borrowed),
            Value::Clock(t) => clock_seconds(t).map(|s| Cow::Owned(s.to_string())),
            Value::Bool(b) => Some(Cow::Borrowed(if *b { "1" } else { "0" })),
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Number(n) => f.write_str(n),
            Value::Str(s) => f.write_str(&quote(s)),
            Value::Ref(r) => f.write_str(r),
            Value::Unit { number, unit } => write!(f, "{number}{unit}"),
            Value::File { path, .. } => write!(f, "file({})", quote(path)),
            Value::Clock(t) => f.write_str(t),
            Value::Bool(b) => f.write_str(if *b { "on" } else { "off" }),
        }
    }
}

/// Whether `s` is exactly a number token: `-?[0-9]+(\.[0-9]+)?`.
pub(crate) fn is_number_literal(s: &str) -> bool {
    let s = s.strip_prefix('-').unwrap_or(s);
    let (int, frac) = match s.split_once('.') {
        Some((i, f)) => (i, Some(f)),
        None => (s, None),
    };
    let digits = |p: &str| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit());
    digits(int) && frac.is_none_or(digits)
}

/// `extern slug = Type(matcher: "value")` — a reference to an object owned
/// by Loxone Config (hardware, system blocks, anything unmanaged).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternDecl {
    pub slug: String,
    pub block_type: String,
    pub match_spec: MatchSpec,
    /// Room constraint (`room: "Büro"`): the object's `<IoData Pr=…>`
    /// must point at a `Place` with this title. Narrows `iname`/`title`
    /// matching where titles repeat per room; never combined with
    /// `uuid` (which pins exactly).
    pub room: Option<String>,
    /// Category constraint (`category: "Beleuchtung"`): the object's
    /// `<IoData Cr=…>` must point at a `Category` with this title.
    pub category: Option<String>,
    /// Device constraint (`device: "B0123456"`, D55): the serial number of
    /// the hardware the object belongs to — the `Serial=` of its nearest
    /// enclosing Tree device, extension or Miniserver. Narrows
    /// `iname`/`title` matching where identical devices repeat their I/O
    /// titles; unlike a room or a title, it does not change in the GUI.
    pub device: Option<String>,
    /// Trailing `#` comment on the statement line, verbatim.
    pub comment: Option<String>,
}

impl ExternDecl {
    /// The full match spec as source text: `title: "…", room: "…"`.
    pub fn spec(&self) -> String {
        let mut s = self.match_spec.to_string();
        if let Some(r) = &self.room {
            s.push_str(&format!(", room: {}", quote(r)));
        }
        if let Some(c) = &self.category {
            s.push_str(&format!(", category: {}", quote(c)));
        }
        if let Some(d) = &self.device {
            s.push_str(&format!(", device: {}", quote(d)));
        }
        s
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MatchSpec {
    Uuid(String),
    IName(String),
    Title(String),
    /// `mirrors: <slug>` (D32) — for `InputRef`/`OutputRef` externs only:
    /// matches the ref object whose `Ref=` attribute names the object the
    /// given slug resolves to (a managed block, or another extern of the
    /// module). Where a page holds several refs of the same target, the
    /// declaring file's `page` statement narrows the candidates; a still
    /// ambiguous match is refused (keep `uuid:` for those).
    Mirrors(String),
}

impl fmt::Display for MatchSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MatchSpec::Uuid(v) => write!(f, "uuid: {}", quote(v)),
            MatchSpec::IName(v) => write!(f, "iname: {}", quote(v)),
            MatchSpec::Title(v) => write!(f, "title: {}", quote(v)),
            MatchSpec::Mirrors(v) => write!(f, "mirrors: {v}"),
        }
    }
}

/// `slug = Type("Label", Port: value, Port: source.Q, …)` — a managed block
/// the compiler owns end-to-end. The argument list declares the block's
/// entire input situation in one place: a literal (or constant) binds the
/// port's `Def=` parameter, a `slug.Port` reference wires that source into
/// the port.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockDecl {
    pub slug: String,
    pub block_type: String,
    /// The optional leading string argument: the display title
    /// (defaults to the slug).
    pub title: Option<String>,
    /// Keyword arguments and whole-line comments, in source order.
    pub args: Vec<ArgItem>,
    /// Trailing `#` comment on the header line (after the `(` when the
    /// call spans lines).
    pub comment: Option<String>,
    /// Trailing `#` comment on the closing `)` line, verbatim.
    pub close_comment: Option<String>,
}

impl BlockDecl {
    /// All keyword arguments, in source order.
    pub fn bindings(&self) -> impl Iterator<Item = &Binding> {
        self.args.iter().filter_map(|a| match a {
            ArgItem::Binding(b) => Some(b),
            ArgItem::Comment(_) => None,
        })
    }

    /// The parameter bindings (`Port: value`), emitted as `Def=` on the
    /// corresponding connectors.
    pub fn params(&self) -> impl Iterator<Item = (&str, &Value)> {
        self.bindings().filter_map(|b| match &b.kind {
            BindingKind::Param(v) => Some((b.port.as_str(), v)),
            _ => None,
        })
    }

    /// The wire bindings (`Port: source.Q`) as `(sink_port, source)`.
    pub fn input_wires(&self) -> impl Iterator<Item = (&str, &PortRef)> {
        self.bindings().filter_map(|b| match &b.kind {
            BindingKind::Wire(src) => Some((b.port.as_str(), src)),
            _ => None,
        })
    }

    /// The expression bindings (`Port: a.Q and b.Q`) as `(sink_port, expr)`
    /// — desugared by [`Module::desugar`] like `<-` expressions.
    pub fn expr_bindings(&self) -> impl Iterator<Item = (&str, &Expr)> {
        self.bindings().filter_map(|b| match &b.kind {
            BindingKind::Expr(e) => Some((b.port.as_str(), e)),
            _ => None,
        })
    }
}

/// One entry of a block's argument list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArgItem {
    Binding(Binding),
    /// A whole-line `#` comment inside the argument list, verbatim.
    Comment(String),
}

/// `Port: value` (parameter), `Port: slug.Port` (wire), or
/// `Port: <expr>` (expression sugar, D26) inside a block's argument list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Binding {
    pub port: String,
    pub kind: BindingKind,
    /// Trailing `#` comment on the argument line, verbatim.
    pub comment: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BindingKind {
    /// A literal or constant reference: becomes `Def=` on the port.
    Param(Value),
    /// A `slug.Port` source reference: becomes a wire into the port.
    Wire(PortRef),
    /// A boolean expression (D26): desugars into gate/comparator blocks
    /// whose result is wired into the port — the same machinery as the
    /// `<-` expression statement (D24). A bare `slug.Port` stays a
    /// [`BindingKind::Wire`], a bare value a [`BindingKind::Param`].
    Expr(Expr),
}

impl fmt::Display for BindingKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BindingKind::Param(v) => v.fmt(f),
            BindingKind::Wire(r) => r.fmt(f),
            BindingKind::Expr(e) => e.fmt(f),
        }
    }
}

/// `target.Port <- source.Port` — a wire onto an *extern* port (wires into
/// managed blocks are written in the block's argument list). Recorded in
/// the lockfile so removing the statement removes the wire again without
/// touching wires drawn in Loxone Config.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WireDecl {
    pub to: PortRef,
    pub from: PortRef,
    /// Trailing `#` comment on the statement line, verbatim.
    pub comment: Option<String>,
}

/// `target.Port <- <expr>` — expression sugar on an extern port (D24).
/// [`Module::desugar`] turns the expression into managed gate/comparator
/// blocks (synthetic slugs `<target>_<port>__<op><n>`, locked like
/// hand-written blocks but marked expression-owned) plus a plain wire from
/// the root block's `Q` onto the target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExprWireDecl {
    pub to: PortRef,
    pub expr: Expr,
    /// Trailing `#` comment on the statement line, verbatim.
    pub comment: Option<String>,
}

/// `<slug> = <expr>`, a named signal. [`Module::desugar`] turns the
/// expression into the blocks an inline expression gets, but the top block
/// takes the slug itself. Its output is `<slug>.Q` for a gate or
/// comparator and `<slug>.AQ` for the `Formula` of an arithmetic signal.
///
/// The sub-blocks are `<slug>__<op><n>` and expression-owned. The top block
/// is hand-owned: a deleted signal needs `removed <slug>`, a renamed one
/// `moved`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignalDecl {
    pub slug: String,
    /// Never a bare port or value — the parser refuses those (an alias is
    /// not a block).
    pub expr: Expr,
    /// Trailing `#` comment on the statement line, verbatim.
    pub comment: Option<String>,
}

/// A boolean expression on the RHS of `<-`. Precedence, loosest to
/// tightest: `or` < `and` < `not` < comparison < `+ -` < `* /`.
/// Comparisons do not chain. Arithmetic stands alone (a whole `<-` RHS or
/// a whole argument binding) — mixing it under gates or comparisons is
/// deferred, with parse-time errors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expr {
    Or(Box<Expr>, Box<Expr>),
    And(Box<Expr>, Box<Expr>),
    Not(Box<Expr>),
    Cmp {
        op: CmpOp,
        lhs: Operand,
        rhs: Operand,
    },
    /// Arithmetic (D24 formula backend). Children are always `Atom` or
    /// `Arith` (the parser rejects boolean subtrees); each maximal tree
    /// desugars to ONE `Formula` block.
    Arith {
        op: ArithOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
    /// A bare operand in gate position — a boolean source port. Constants
    /// here are rejected at desugar time (they cannot drive a gate input).
    Atom(Operand),
}

/// Comparison operator; each maps to one verified comparator block type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CmpOp {
    Ge,
    Gt,
    Le,
    Lt,
    Eq,
    Ne,
}

impl CmpOp {
    pub fn symbol(self) -> &'static str {
        match self {
            CmpOp::Ge => ">=",
            CmpOp::Gt => ">",
            CmpOp::Le => "<=",
            CmpOp::Lt => "<",
            CmpOp::Eq => "==",
            CmpOp::Ne => "!=",
        }
    }
}

/// Arithmetic operator; all four land in the same `Formula` block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArithOp {
    Add,
    Sub,
    Mul,
    Div,
}

impl ArithOp {
    pub fn symbol(self) -> &'static str {
        match self {
            ArithOp::Add => "+",
            ArithOp::Sub => "-",
            ArithOp::Mul => "*",
            ArithOp::Div => "/",
        }
    }
}

/// A leaf of an expression: a source port or a constant (number or `let`
/// reference).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Operand {
    Port(PortRef),
    Value(Value),
}

impl fmt::Display for Operand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Operand::Port(p) => p.fmt(f),
            Operand::Value(v) => v.fmt(f),
        }
    }
}

impl Expr {
    /// Every leaf operand, in evaluation (post-order) order.
    pub fn operands(&self) -> Vec<&Operand> {
        let mut out = Vec::new();
        self.collect_operands(&mut out);
        out
    }

    fn collect_operands<'a>(&'a self, out: &mut Vec<&'a Operand>) {
        match self {
            Expr::Or(a, b) | Expr::And(a, b) => {
                a.collect_operands(out);
                b.collect_operands(out);
            }
            Expr::Not(x) => x.collect_operands(out),
            Expr::Cmp { lhs, rhs, .. } => {
                out.push(lhs);
                out.push(rhs);
            }
            Expr::Arith { lhs, rhs, .. } => {
                lhs.collect_operands(out);
                rhs.collect_operands(out);
            }
            Expr::Atom(o) => out.push(o),
        }
    }

    /// Binding strength for canonical (minimal-paren) emission.
    pub(crate) fn prec(&self) -> u8 {
        match self {
            Expr::Or(..) => 1,
            Expr::And(..) => 2,
            Expr::Not(..) => 3,
            Expr::Cmp { .. } => 4,
            Expr::Arith {
                op: ArithOp::Add | ArithOp::Sub,
                ..
            } => 5,
            Expr::Arith { .. } => 6,
            Expr::Atom(_) => 7,
        }
    }

    fn fmt_child(
        &self,
        child: &Expr,
        needs_parens: bool,
        f: &mut fmt::Formatter<'_>,
    ) -> fmt::Result {
        let _ = self;
        if needs_parens {
            write!(f, "({child})")
        } else {
            write!(f, "{child}")
        }
    }
}

impl fmt::Display for Expr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            // Binary operators are left-associative: an equal-precedence
            // LEFT child re-parses to the same tree without parens, an
            // equal-precedence RIGHT child does not.
            Expr::Or(a, b) | Expr::And(a, b) => {
                let kw = if matches!(self, Expr::Or(..)) {
                    "or"
                } else {
                    "and"
                };
                self.fmt_child(a, a.prec() < self.prec(), f)?;
                write!(f, " {kw} ")?;
                self.fmt_child(b, b.prec() <= self.prec(), f)
            }
            // A comparison under `not` gets parens for readability even
            // though `not a >= b` would re-parse identically.
            Expr::Not(x) => {
                f.write_str("not ")?;
                self.fmt_child(x, !matches!(**x, Expr::Atom(_) | Expr::Not(_)), f)
            }
            Expr::Cmp { op, lhs, rhs } => write!(f, "{lhs} {} {rhs}", op.symbol()),
            // Left-associative like `and`/`or`; `+ -` and `* /` are two
            // precedence levels, so `(a + b) * c` keeps its parens.
            Expr::Arith { op, lhs, rhs } => {
                self.fmt_child(lhs, lhs.prec() < self.prec(), f)?;
                write!(f, " {} ", op.symbol())?;
                self.fmt_child(rhs, rhs.prec() <= self.prec(), f)
            }
            Expr::Atom(o) => o.fmt(f),
        }
    }
}

/// `target.Port = value` — write a parameter (`Def=`) on an *extern* port;
/// the original value is preserved in the lockfile and restored when the
/// statement is removed from source. On managed blocks, parameters belong
/// in the argument list — assigning a managed port here is a validation
/// error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetDecl {
    pub target: PortRef,
    pub value: Value,
    /// Trailing `#` comment on the statement line, verbatim.
    pub comment: Option<String>,
    /// The line of the `(a, b, …).Port = value` statement this one was
    /// written in (#199): one statement per object everywhere, written
    /// back as the one line by [`Module::to_text`].
    pub bulk: Option<usize>,
}

pub use crate::lock::Attr;

/// `target.title = "…"`, `target.room = "…"`, `target.category = "…"` —
/// claim one attribute of an *extern* object (D39): its title, or its
/// room/category binding. A room or category is named by the title of its
/// `Place`/`Category`. The original is preserved in the lockfile and
/// restored when the statement leaves the source, like a port assignment;
/// a room/category change carries along the object's descendants that
/// shared its old value (a device's inputs and outputs).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttrDecl {
    pub slug: String,
    pub attr: Attr,
    pub value: Value,
    /// Trailing `#` comment on the statement line, verbatim.
    pub comment: Option<String>,
    /// As [`SetDecl::bulk`]: `(a, b).room = "…"`.
    pub bulk: Option<usize>,
}

impl fmt::Display for AttrDecl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{} = {}", self.slug, self.attr, self.value)
    }
}

/// `let name = value` — a named constant. Referenced by bare identifier in
/// any value position (argument lists, extern assignments). Pure
/// substitution: the compiler resolves references before emitting `Def=`
/// values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LetDecl {
    pub name: String,
    /// Always `Number`, `Str`, or `Unit` (constants cannot reference
    /// constants).
    pub value: Value,
    /// Trailing `#` comment on the statement line, verbatim.
    pub comment: Option<String>,
}

/// `removed slug` — declares that a managed block's absence from source is
/// intentional: the next compile deletes it from the config and moves its
/// lockfile entry to a removal tombstone (D31), which keeps deleting the
/// object from bases predating the deployment. Scoped to one slug and
/// reviewable in the diff (the in-language counterpart of Terraform's
/// `removed` block). The statement may be deleted right after the compile
/// that applies it (the tombstone carries the intent from there) and is
/// tolerated while the tombstone is pending; once the removal has reached
/// the deployed config, a lingering statement is a compile error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemovedDecl {
    pub slug: String,
    /// Trailing `#` comment on the statement line, verbatim.
    pub comment: Option<String>,
}

/// `page "<Title>"` — names the `<C Type="Page">` the block declarations
/// after it are (re)built on (D28). Authoritative: on every compile a
/// governed block is pinned to a base page with this title — a pin that
/// still matches a page so titled is kept (titles need not be unique), any
/// other moves to the first matching page in document order, and a title no
/// page carries is a compile error — unless the statement names a room
/// (`page "<Title>" (room: "…")`): then the compiler creates the page (D56).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageDecl {
    /// Display title of a `<C Type="Page">` in the base config.
    pub title: String,
    /// `room:` — the `Place` a page the compiler creates is assigned to
    /// (D56). Only consulted when no page carries the title.
    pub room: Option<String>,
    /// `category:` — the created page's `Category`; default: the
    /// unassigned one.
    pub category: Option<String>,
    /// Trailing `#` comment on the statement line, verbatim.
    pub comment: Option<String>,
}

/// `moved old_slug -> new_slug` — renames a managed block's lockfile entry
/// so its identity (object and port UUIDs) survives a slug rename in source
/// (the in-language counterpart of Terraform's `moved` block). Idempotent:
/// once applied, a compile that finds `new_slug` already in the lock treats
/// the statement as done.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MovedDecl {
    pub from: String,
    pub to: String,
    /// Trailing `#` comment on the statement line, verbatim.
    pub comment: Option<String>,
}

/// `template <name>(<params>)` … `end` — a reusable body of block, wire,
/// and port-assignment statements (D23). Body slugs are private to the
/// template: instance `sued` expands body block `hoch` to `sued_hoch`.
/// Free identifiers (neither parameter nor body slug) resolve in the
/// module namespace after expansion — a template may capture module
/// externs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateDecl {
    pub name: String,
    pub params: Vec<TemplateParam>,
    /// Block declarations, wires, port assignments, and comments.
    pub body: Vec<Item>,
    /// Trailing `#` comment on the header line, verbatim.
    pub comment: Option<String>,
    /// Trailing `#` comment on the `end` line, verbatim.
    pub end_comment: Option<String>,
}

/// One parameter of a template header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TemplateParam {
    /// `name: Type` — the instance passes an extern or block slug. The
    /// annotation is checked against the slug's declared type when that
    /// is known.
    Object { name: String, block_type: String },
    /// `name = <literal>` — a value parameter with a default; the
    /// instance may override it with a literal or a `let` reference.
    Value { name: String, default: Value },
}

impl TemplateParam {
    pub fn name(&self) -> &str {
        match self {
            TemplateParam::Object { name, .. } | TemplateParam::Value { name, .. } => name,
        }
    }
}

impl fmt::Display for TemplateParam {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TemplateParam::Object { name, block_type } => write!(f, "{name}: {block_type}"),
            TemplateParam::Value { name, default } => write!(f, "{name} = {default}"),
        }
    }
}

/// `test "<name>"` … `end` — an imperative simulator scenario (D36). The
/// body reads top to bottom: port assignments inject signals, `tick`
/// advances simulated time, `expect` asserts a port's value, `clock` sets
/// the simulated wall-clock. `loxl test` groups the statements into
/// steps at the `tick` boundaries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestDecl {
    /// Display name, quoted in source. Unique across the module.
    pub name: String,
    pub body: Vec<TestItem>,
    /// Trailing `#` comment on the header line, verbatim.
    pub comment: Option<String>,
    /// Trailing `#` comment on the `end` line, verbatim.
    pub end_comment: Option<String>,
}

/// One statement of a `test` body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TestItem {
    /// `slug.Port = value` — inject a signal, persistently for the rest
    /// of the scenario. On an output connector the injection overrides
    /// the block (how calendar-driven Mode blocks are driven in the sim).
    Inject(SetDecl),
    /// `tick <n> [dt <seconds>]` — advance the simulation `n` ticks of
    /// `dt` simulated seconds each (default 1s; a unit value like
    /// `dt 100ms` scales). `tick <duration>` (`tick 5min`, D80) advances
    /// that much simulated time, a whole number of ticks.
    Tick(TickDecl),
    /// `expect slug.Port <cmp> <value>` — assert a port's value after the
    /// preceding `tick`.
    Expect(ExpectDecl),
    /// `clock HH:MM[:SS]` or `clock YYYY-MM-DD HH:MM[:SS]` — set the
    /// simulated wall-clock (drives DayTimer/astro blocks); it advances
    /// by `dt` each tick.
    Clock(ClockDecl),
    /// Whole-line `#` comment, verbatim.
    Comment(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TickDecl {
    /// The number of ticks: as written, or the duration divided by dt.
    pub n: u64,
    /// `tick 5min`: the duration as written (a time `Unit` value); `None`
    /// for a plain tick count.
    pub duration: Option<Value>,
    /// Simulated seconds per tick; `None` = the default (1s). Always
    /// `Number` or `Unit`.
    pub dt: Option<Value>,
    /// Trailing `#` comment on the statement line, verbatim.
    pub comment: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpectDecl {
    pub port: PortRef,
    pub cmp: TestCmp,
    /// Always numeric: `Number`, `Unit`, `Bool`, or a `let` reference.
    pub value: Value,
    /// Trailing `#` comment on the statement line, verbatim.
    pub comment: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClockDecl {
    /// `HH:MM[:SS]` or `YYYY-MM-DD HH:MM[:SS]`, canonical (two-digit
    /// hours, no zero seconds — like a time of day value).
    pub spec: String,
    /// Trailing `#` comment on the statement line, verbatim.
    pub comment: Option<String>,
}

/// Comparators the simulator supports in `expect`. `~=` passes within
/// 5% of the expected value; `!=` fails where `==` would pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TestCmp {
    Eq,
    Ne,
    Ge,
    Gt,
    Le,
    Lt,
    Approx,
}

impl TestCmp {
    pub fn symbol(self) -> &'static str {
        match self {
            TestCmp::Eq => "==",
            TestCmp::Ne => "!=",
            TestCmp::Ge => ">=",
            TestCmp::Gt => ">",
            TestCmp::Le => "<=",
            TestCmp::Lt => "<",
            TestCmp::Approx => "~=",
        }
    }
}

impl fmt::Display for TestCmp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.symbol())
    }
}

impl fmt::Display for TestItem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let tail = |c: &Option<String>| c.as_ref().map(|t| format!(" #{t}")).unwrap_or_default();
        match self {
            TestItem::Inject(s) => write!(f, "{} = {}{}", s.target, s.value, tail(&s.comment)),
            TestItem::Tick(t) => {
                match &t.duration {
                    Some(d) => write!(f, "tick {d}")?,
                    None => write!(f, "tick {}", t.n)?,
                }
                if let Some(dt) = &t.dt {
                    write!(f, " dt {dt}")?;
                }
                f.write_str(&tail(&t.comment))
            }
            TestItem::Expect(e) => write!(
                f,
                "expect {} {} {}{}",
                e.port,
                e.cmp,
                e.value,
                tail(&e.comment)
            ),
            TestItem::Clock(c) => write!(f, "clock {}{}", c.spec, tail(&c.comment)),
            TestItem::Comment(text) => write!(f, "#{text}"),
        }
    }
}

/// `tiles <name>` … `end` — a named set of Touch Pure Flex tile programs
/// (D41). Declares nothing in the config by itself; each
/// `<device>.tiles = <name>` claim writes it onto one device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TilesDecl {
    pub name: String,
    pub body: Vec<TilesItem>,
    /// Trailing `#` comment on the header line, verbatim.
    pub comment: Option<String>,
    /// Trailing `#` comment on the `end` line, verbatim.
    pub end_comment: Option<String>,
}

impl TilesDecl {
    pub fn tiles(&self) -> impl Iterator<Item = &TileDecl> {
        self.body.iter().filter_map(|i| match i {
            TilesItem::Tile(t) => Some(t),
            TilesItem::Comment(_) => None,
        })
    }
}

/// One statement of a `tiles` body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TilesItem {
    Tile(TileDecl),
    /// Whole-line `#` comment, verbatim.
    Comment(String),
}

/// `tile <i>(click: "…", up: "…", down: "…", text: "…", to: <slug>, …)` —
/// the program of the existing tile `i="<i>"`. Shares the argument-list
/// shape of a block declaration (multi-line, comments); the keys and
/// their meaning are D41's ([`super::tiles::program`]). `tile <i>()`
/// clears every function of the tile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TileDecl {
    pub index: u32,
    pub args: Vec<ArgItem>,
    /// Trailing `#` comment on the `tile` line, verbatim.
    pub comment: Option<String>,
    /// Trailing `#` comment on the closing `)` line of a multi-line list.
    pub close_comment: Option<String>,
}

impl TileDecl {
    pub fn bindings(&self) -> impl Iterator<Item = &Binding> {
        self.args.iter().filter_map(|a| match a {
            ArgItem::Binding(b) => Some(b),
            ArgItem::Comment(_) => None,
        })
    }
}

/// `<device>.tiles = <set>` — claim the tile programs of an extern Touch
/// Pure Flex (D41). Like a `Code:` program (D40) the claim keeps no lock
/// state: withdrawing it leaves the last pushed programs in place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TilesClaim {
    pub slug: String,
    pub tiles: String,
    /// Trailing `#` comment on the statement line, verbatim.
    pub comment: Option<String>,
}

impl fmt::Display for TilesClaim {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.tiles = {}", self.slug, self.tiles)
    }
}

/// `access <slug>` … `end` — the access rights (`TIM=`) on an extern NFC
/// Code Touch (D49): every user and group that may open it, each with the
/// access schedule that says when. `recipients <slug>` … `end` — the
/// recipients (`ACT=`) of an extern push notification (D51). The
/// declaration owns the block's whole set of these rights; users, groups
/// and schedules stay with Loxone Config and the app and are named by
/// title.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccessDecl {
    /// Which right the declaration owns.
    pub right: RightKind,
    pub slug: String,
    pub body: Vec<AccessItem>,
    /// Trailing `#` comment on the header line, verbatim.
    pub comment: Option<String>,
    /// Trailing `#` comment on the `end` line, verbatim.
    pub end_comment: Option<String>,
}

impl AccessDecl {
    pub fn grants(&self) -> impl Iterator<Item = &AccessGrant> {
        self.body.iter().filter_map(|i| match i {
            AccessItem::Grant(g) => Some(g),
            AccessItem::Comment(_) => None,
        })
    }
}

/// One statement of an `access` body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccessItem {
    Grant(AccessGrant),
    /// Whole-line `#` comment, verbatim.
    Comment(String),
}

/// The right a rights declaration owns: the attribute of `<PRM>` it
/// writes, on which block type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RightKind {
    /// `access`: `TIM=` on an NFC Code Touch (D49).
    Access,
    /// `recipients`: `ACT=` on a push notification (D51).
    Recipients,
}

impl RightKind {
    pub fn keyword(self) -> &'static str {
        match self {
            RightKind::Access => "access",
            RightKind::Recipients => "recipients",
        }
    }

    /// The block type the declaration applies to.
    pub fn block_type(self) -> &'static str {
        match self {
            RightKind::Access => ACCESS_BLOCK_TYPE,
            RightKind::Recipients => crate::connectors::MESSAGE_REF_TARGET,
        }
    }

    /// What the block is called in messages.
    pub fn block_noun(self) -> &'static str {
        match self {
            RightKind::Access => "an NFC Code Touch",
            RightKind::Recipients => "a push notification",
        }
    }
}

/// `user "<title>": "<schedule>"` or `group "<title>": "<schedule>"` in an
/// `access` body; `user "<title>"` or `group "<title>"` in a `recipients`
/// body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccessGrant {
    pub kind: PrincipalKind,
    /// The user's or group's title.
    pub principal: String,
    /// The access schedule's title; `None` in a `recipients` body.
    pub schedule: Option<String>,
    /// Trailing `#` comment, verbatim.
    pub comment: Option<String>,
}

impl fmt::Display for AccessGrant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.kind.as_str(), quote(&self.principal))?;
        match &self.schedule {
            Some(schedule) => write!(f, ": {}", quote(schedule)),
            None => Ok(()),
        }
    }
}

/// `schedule <name>` … `end` — a named DayTimer schedule (D53): per
/// operating mode, the intervals the timer is on and their values. Like a
/// `tiles` set it declares nothing by itself; each
/// `<timer>.schedule = <name>` claim writes it onto one DayTimer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduleDecl {
    pub name: String,
    pub body: Vec<ScheduleItem>,
    /// Trailing `#` comment on the header line, verbatim.
    pub comment: Option<String>,
    /// Trailing `#` comment on the `end` line, verbatim.
    pub end_comment: Option<String>,
}

impl ScheduleDecl {
    pub fn lines(&self) -> impl Iterator<Item = &ScheduleLine> {
        self.body.iter().filter_map(|i| match i {
            ScheduleItem::Line(l) => Some(l),
            ScheduleItem::Comment(_) => None,
        })
    }
}

/// One statement of a `schedule` body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScheduleItem {
    Line(ScheduleLine),
    /// Whole-line `#` comment, verbatim.
    Comment(String),
}

/// `mon tue 06:00-08:00 = 21.5, 17:00-22:00` — the days (operating-mode
/// keywords, kept as written: `weekdays` stays `weekdays`; or modes by
/// title, D69) and their intervals. No intervals (`holiday off`) lists the
/// modes with the timer off all day.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduleLine {
    pub days: Vec<Day>,
    pub intervals: Vec<Interval>,
    /// Trailing `#` comment, verbatim.
    pub comment: Option<String>,
}

/// The days of a schedule line: a built-in keyword (`mon`, `weekdays`,
/// `heating`, …) or an operating mode of the config by its title
/// (`"Party"`, D69).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Day {
    Keyword(String),
    Mode(String),
}

impl fmt::Display for Day {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Day::Keyword(k) => f.write_str(k),
            Day::Mode(title) => f.write_str(&quote(title)),
        }
    }
}

/// `HH:MM-HH:MM [= value] [activate]`, times in minutes of the day
/// (`24:00` = 1440).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Interval {
    pub from: u16,
    pub to: u16,
    /// The output value while on, as a number literal; `None` is a digital
    /// timer's `1`.
    pub value: Option<String>,
    /// `activate` — the interval needs an activation (`NeedActivate`).
    pub activate: bool,
}

impl fmt::Display for Interval {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let hm = |m: u16| format!("{:02}:{:02}", m / 60, m % 60);
        write!(f, "{}-{}", hm(self.from), hm(self.to))?;
        if let Some(v) = &self.value {
            write!(f, " = {v}")?;
        }
        if self.activate {
            f.write_str(" activate")?;
        }
        Ok(())
    }
}

impl fmt::Display for ScheduleLine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let days: Vec<String> = self.days.iter().map(Day::to_string).collect();
        f.write_str(&days.join(" "))?;
        if self.intervals.is_empty() {
            f.write_str(" off")?;
        }
        for (i, iv) in self.intervals.iter().enumerate() {
            write!(f, "{}{iv}", if i == 0 { " " } else { ", " })?;
        }
        if let Some(c) = &self.comment {
            write!(f, " #{c}")?;
        }
        Ok(())
    }
}

/// `<timer>.schedule = <name>` — claim the schedule of a DayTimer (D53).
/// No lock state, like a tiles claim: withdrawing it leaves the last
/// written schedule in place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduleClaim {
    pub slug: String,
    pub schedule: String,
    /// Trailing `#` comment on the statement line, verbatim.
    pub comment: Option<String>,
}

impl fmt::Display for ScheduleClaim {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.schedule = {}", self.slug, self.schedule)
    }
}

/// `moods <name>` … `end` — a named set of LightController2 moods (D58):
/// per mood its name and the value of each lighting circuit. Like a
/// `schedule` it declares nothing by itself; each
/// `<controller>.moods = <name>` claim writes it onto one controller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoodsDecl {
    pub name: String,
    pub body: Vec<MoodsItem>,
    /// Trailing `#` comment on the header line, verbatim.
    pub comment: Option<String>,
    /// Trailing `#` comment on the `end` line, verbatim.
    pub end_comment: Option<String>,
}

impl MoodsDecl {
    pub fn moods(&self) -> impl Iterator<Item = &MoodLine> {
        self.body.iter().filter_map(|i| match i {
            MoodsItem::Mood(m) => Some(m),
            MoodsItem::Comment(_) => None,
        })
    }
}

/// One statement of a `moods` body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MoodsItem {
    Mood(MoodLine),
    /// Whole-line `#` comment, verbatim.
    Comment(String),
}

/// `"Kochen": all on, AQ3 temp(80%, 2700K)` — a mood's name (the app's
/// mood title) and its circuit values, in order. A circuit no setting
/// names is off.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoodLine {
    pub name: String,
    pub settings: Vec<MoodSetting>,
    /// Trailing `#` comment, verbatim.
    pub comment: Option<String>,
}

/// `AQ3 temp(80%, 2700K)` or `all on`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MoodSetting {
    /// The circuit number (`AQ<n>`); `None` is `all` — every circuit.
    pub circuit: Option<u8>,
    pub value: crate::moods::MoodValue,
}

impl fmt::Display for MoodSetting {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.circuit {
            Some(n) => write!(f, "AQ{n} {}", self.value),
            None => write!(f, "all {}", self.value),
        }
    }
}

impl fmt::Display for MoodLine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:", quote(&self.name))?;
        for (i, s) in self.settings.iter().enumerate() {
            write!(f, "{}{s}", if i == 0 { " " } else { ", " })?;
        }
        if let Some(c) = &self.comment {
            write!(f, " #{c}")?;
        }
        Ok(())
    }
}

/// `<controller>.moods = <name>` — claim the moods of a LightController2
/// (D58). Allowed in a template body, so one declaration sets up every
/// room the template stamps out. No lock state: withdrawing it leaves the
/// last written moods in place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoodsClaim {
    pub slug: String,
    pub moods: String,
    /// Trailing `#` comment on the statement line, verbatim.
    pub comment: Option<String>,
}

impl fmt::Display for MoodsClaim {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.moods = {}", self.slug, self.moods)
    }
}

/// `rule <slug>` … `end` — an extern Automatic Designer rule
/// (`AutopilotRule`) as source (D57): whether it is active, the
/// conditions that must all hold, and the commands it then sends. The
/// declaration owns exactly that part of the rule's `APC=` payload; its
/// name, description and every field Lox Logic does not know stay as they
/// are. Like `access` it keeps no lock state (a named rule does, D60).
///
/// `rule <slug> "Title"` declares a rule by its name instead (D60): the
/// base's rule of that title, or a new one the compile creates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleDecl {
    pub slug: String,
    /// The rule's name (`Title=`, the payload's `name`) when the
    /// declaration owns the rule itself; `None` for an extern rule.
    pub title: Option<String>,
    pub body: Vec<RuleItem>,
    /// Trailing `#` comment on the header line, verbatim.
    pub comment: Option<String>,
    /// Trailing `#` comment on the `end` line, verbatim.
    pub end_comment: Option<String>,
}

impl RuleDecl {
    pub fn stmts(&self) -> impl Iterator<Item = &RuleStmt> {
        self.body.iter().filter_map(|i| match i {
            RuleItem::Stmt(s) => Some(s),
            RuleItem::Comment(_) => None,
        })
    }

    /// The objects the body names: action targets, the blocks its
    /// conditions watch and the modes it names by slug (D57, D73).
    pub fn slugs(&self) -> impl Iterator<Item = &str> {
        self.stmts().filter_map(|st| match &st.kind {
            RuleStmtKind::Then { target, .. }
            | RuleStmtKind::State { target, .. }
            | RuleStmtKind::Mode {
                mode: ModeName::Slug(target),
                ..
            }
            | RuleStmtKind::ThenMode {
                mode: ModeName::Slug(target),
                ..
            } => Some(target.as_str()),
            _ => None,
        })
    }
}

/// One line of a `rule` body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuleItem {
    Stmt(RuleStmt),
    /// Whole-line `#` comment, verbatim.
    Comment(String),
}

/// A statement of a `rule` body, with its trailing comment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleStmt {
    pub kind: RuleStmtKind,
    /// Trailing `#` comment, verbatim.
    pub comment: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuleStmtKind {
    /// `active` / `inactive`.
    Active(bool),
    /// `when time HH:MM` — minutes since midnight.
    Time(u16),
    /// `when weekday mon tue …` — day keywords as written (`weekdays`,
    /// `weekend`, `daily` stay as written).
    Weekday(Vec<String>),
    /// `then <slug> "<command>"` — the app command sent to a block.
    Then { target: String, command: String },
    /// `when <slug>.<state> <op> <value>` — a block's state (D73): the
    /// state as the app names it (`active`, `position`, `value`, …), a
    /// comparison and a number, `on` (1) or `off` (0), as written.
    State {
        target: String,
        state: String,
        op: CmpOp,
        value: String,
    },
    /// `when mode <mode>` / `when mode <mode> off` — an operating mode is
    /// active (or not, D73).
    Mode { mode: ModeName, on: bool },
    /// `then mode <mode> on|off` — switch an operating mode (D73).
    ThenMode { mode: ModeName, on: bool },
    /// `when weather <field> <op> <value>` — a weather value (D76).
    Weather {
        field: String,
        op: CmpOp,
        value: String,
    },
    /// `when day <op> <n>` — the day of the month (D76).
    Day { op: CmpOp, value: String },
    /// `when daylight` / `when daylight off` (D76).
    Daylight(bool),
    /// `when sunrise|sunset|dawn|dusk` — the clock's pulse (D76).
    Sun(String),
    /// `when date YYYY-MM-DD` — that day (D76).
    Date(String),
    /// `then notify "<subject>" "<text>"` — a push notification (D76).
    Notify { subject: String, text: String },
    /// `then mail "<to>" "<subject>" "<text>"` — through the cloud
    /// mailer (D76).
    Mail {
        to: String,
        subject: String,
        text: String,
    },
    /// `then wait <duration>` — the duration as written (`5s`, `500ms`,
    /// `2min`, D76).
    Wait(String),
    /// `then message <severity> "<subject>" "<text>" [admins]` — a
    /// system message (D76).
    Message {
        severity: String,
        subject: String,
        text: String,
        admins: bool,
    },
    /// `or` — the conditions after it are another group: the rule runs
    /// when all conditions of one group hold (D73).
    Or,
}

/// An operating mode in a rule: an extern or minted `Mode` by slug, or a
/// mode of the config by its title (as in a schedule, D69).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModeName {
    Slug(String),
    Title(String),
}

impl fmt::Display for ModeName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ModeName::Slug(s) => f.write_str(s),
            ModeName::Title(t) => f.write_str(&quote(t)),
        }
    }
}

impl fmt::Display for RuleStmtKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RuleStmtKind::Active(true) => f.write_str("active"),
            RuleStmtKind::Active(false) => f.write_str("inactive"),
            RuleStmtKind::Time(m) => write!(f, "when time {:02}:{:02}", m / 60, m % 60),
            RuleStmtKind::Weekday(days) => write!(f, "when weekday {}", days.join(" ")),
            // A one-word command is written bare (`then pumpe on`, D80).
            RuleStmtKind::Then { target, command } if is_bare_command(command) => {
                write!(f, "then {target} {command}")
            }
            RuleStmtKind::Then { target, command } => {
                write!(f, "then {target} {}", quote(command))
            }
            RuleStmtKind::State {
                target,
                state,
                op,
                value,
            } => write!(f, "when {target}.{state} {} {value}", op.symbol()),
            RuleStmtKind::Mode { mode, on: true } => write!(f, "when mode {mode}"),
            RuleStmtKind::Mode { mode, on: false } => write!(f, "when mode {mode} off"),
            RuleStmtKind::ThenMode { mode, on } => {
                write!(f, "then mode {mode} {}", if *on { "on" } else { "off" })
            }
            RuleStmtKind::Or => f.write_str("or"),
            RuleStmtKind::Weather { field, op, value } => {
                write!(f, "when weather {field} {} {value}", op.symbol())
            }
            RuleStmtKind::Day { op, value } => write!(f, "when day {} {value}", op.symbol()),
            RuleStmtKind::Daylight(true) => f.write_str("when daylight"),
            RuleStmtKind::Daylight(false) => f.write_str("when daylight off"),
            RuleStmtKind::Sun(w) => write!(f, "when {w}"),
            RuleStmtKind::Date(d) => write!(f, "when date {d}"),
            RuleStmtKind::Notify { subject, text } => {
                write!(f, "then notify {} {}", quote(subject), quote(text))
            }
            RuleStmtKind::Mail { to, subject, text } => {
                write!(
                    f,
                    "then mail {} {} {}",
                    quote(to),
                    quote(subject),
                    quote(text)
                )
            }
            RuleStmtKind::Wait(d) => write!(f, "then wait {d}"),
            RuleStmtKind::Message {
                severity,
                subject,
                text,
                admins,
            } => {
                write!(
                    f,
                    "then message {severity} {} {}",
                    quote(subject),
                    quote(text)
                )?;
                if *admins {
                    f.write_str(" admins")?;
                }
                Ok(())
            }
        }
    }
}

/// A rule command that is one word (`on`, `pulse`, `fullup`): written
/// bare, canonically (D80). Anything with spaces, digits first, or
/// punctuation stays quoted.
pub fn is_bare_command(command: &str) -> bool {
    let mut c = command.chars();
    c.next().is_some_and(|c| c.is_ascii_alphabetic())
        && c.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// The block type a `rule` declaration applies to (D57).
pub const RULE_BLOCK_TYPE: &str = "AutopilotRule";

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct PortRef {
    pub slug: String,
    pub port: String,
}

impl fmt::Display for PortRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.slug, self.port)
    }
}

/// The block type an `access` declaration applies to (D49): the NFC Code
/// Touch block, the only type with schedule rights (`TIM=`) in the
/// observed configs.
pub const ACCESS_BLOCK_TYPE: &str = "NfcCodeTouch";

/// What a declared name refers to, for reference checking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NameKind {
    Extern,
    Block,
    /// A named signal: a managed block once desugared.
    Signal,
    Let,
    Template,
    Instance,
    Tiles,
    Schedule,
    Moods,
}

impl NameKind {
    fn describe(self) -> &'static str {
        match self {
            NameKind::Extern => "an extern",
            NameKind::Block => "a managed block",
            NameKind::Signal => "a named signal",
            NameKind::Let => "a `let` constant",
            NameKind::Template => "a template",
            NameKind::Instance => "a template instance",
            NameKind::Tiles => "a `tiles` set",
            NameKind::Schedule => "a `schedule`",
            NameKind::Moods => "a `moods` set",
        }
    }
}

impl Module {
    pub fn parse(src: &str) -> Result<Module> {
        let module = super::parser::parse(src)?;
        module.validate()?;
        Ok(module)
    }

    /// Parse one file of a multi-file module. Syntax and statement-local
    /// checks run; name resolution does not — a fragment may reference
    /// slugs declared in a sibling file. Concatenate the fragments'
    /// `items` in file order and run [`Module::validate`] on the whole.
    pub fn parse_fragment(src: &str) -> Result<Module> {
        super::parser::parse(src)
    }

    /// Read every `file("…")` value (D40) relative to `dir` — the directory
    /// of the `.loxl` file these items were parsed from — so each fragment
    /// resolves its own paths before the merge. CRLF becomes LF: Loxone's
    /// own program text carries bare LF (the house program: 502 LF, no CR),
    /// so an editor's line endings never reach the config.
    pub fn load_files(&mut self, dir: &std::path::Path) -> Result<()> {
        fn load(items: &mut [Item], dir: &std::path::Path) -> Result<()> {
            for item in items {
                let args = match item {
                    Item::Block(b) | Item::Instance(b) => &mut b.args,
                    Item::Template(t) => {
                        load(&mut t.body, dir)?;
                        continue;
                    }
                    _ => continue,
                };
                for arg in args {
                    if let ArgItem::Binding(Binding {
                        kind: BindingKind::Param(Value::File { path, text }),
                        ..
                    }) = arg
                    {
                        let full = dir.join(&*path);
                        let raw = std::fs::read_to_string(&full).map_err(|e| {
                            Error::Compile(format!(
                                "`file({})`: {}: {e}",
                                quote(path),
                                full.display()
                            ))
                        })?;
                        *text = Some(raw.replace("\r\n", "\n"));
                    }
                }
            }
            Ok(())
        }
        load(&mut self.items, dir)
    }

    pub fn externs(&self) -> impl Iterator<Item = &ExternDecl> {
        self.items.iter().filter_map(|i| match i {
            Item::Extern(e) => Some(e),
            _ => None,
        })
    }

    pub fn blocks(&self) -> impl Iterator<Item = &BlockDecl> {
        self.items.iter().filter_map(|i| match i {
            Item::Block(b) => Some(b),
            _ => None,
        })
    }

    /// Drop every statement that makes `slug` an extern — its `extern`
    /// declaration, the wires, expressions and assignments onto its ports,
    /// its attribute claims — when the object is promoted to a managed
    /// block that states them as arguments. Reports whether anything went.
    pub fn strip_extern(&mut self, slug: &str) -> bool {
        let before = self.items.len();
        self.items.retain(|i| match i {
            Item::Extern(e) => e.slug != slug,
            Item::Wire(w) => w.to.slug != slug,
            Item::ExprWire(w) => w.to.slug != slug,
            Item::Set(s) => s.target.slug != slug,
            // `.room`/`.category` hold on a managed block (D63).
            Item::Attr(a) => a.slug != slug || a.attr != Attr::Title,
            // An extern rule's edits (D57): a promoted rule is declared
            // by name, with its whole payload.
            Item::Rule(r) => r.title.is_some() || r.slug != slug,
            _ => true,
        });
        self.items.len() != before
    }

    /// The `<-` statements (wires onto extern ports).
    pub fn extern_wires(&self) -> impl Iterator<Item = &WireDecl> {
        self.items.iter().filter_map(|i| match i {
            Item::Wire(w) => Some(w),
            _ => None,
        })
    }

    /// The `<-` statements with an expression RHS (desugared by
    /// [`Module::desugar`]).
    pub fn expr_wires(&self) -> impl Iterator<Item = &ExprWireDecl> {
        self.items.iter().filter_map(|i| match i {
            Item::ExprWire(w) => Some(w),
            _ => None,
        })
    }

    /// The named signals (`<slug> = <expr>`).
    pub fn signals(&self) -> impl Iterator<Item = &SignalDecl> {
        self.items.iter().filter_map(|i| match i {
            Item::Signal(x) => Some(x),
            _ => None,
        })
    }

    /// The `=` port statements (`Def=` writes on extern ports).
    pub fn sets(&self) -> impl Iterator<Item = &SetDecl> {
        self.items.iter().filter_map(|i| match i {
            Item::Set(s) => Some(s),
            _ => None,
        })
    }

    /// The attribute assignments on externs (D39).
    pub fn attrs(&self) -> impl Iterator<Item = &AttrDecl> {
        self.items.iter().filter_map(|i| match i {
            Item::Attr(a) => Some(a),
            _ => None,
        })
    }

    pub fn tiles_sets(&self) -> impl Iterator<Item = &TilesDecl> {
        self.items.iter().filter_map(|i| match i {
            Item::Tiles(t) => Some(t),
            _ => None,
        })
    }

    pub fn schedules(&self) -> impl Iterator<Item = &ScheduleDecl> {
        self.items.iter().filter_map(|i| match i {
            Item::Schedule(s) => Some(s),
            _ => None,
        })
    }

    pub fn mood_sets(&self) -> impl Iterator<Item = &MoodsDecl> {
        self.items.iter().filter_map(|i| match i {
            Item::Moods(m) => Some(m),
            _ => None,
        })
    }

    pub fn moods_claims(&self) -> impl Iterator<Item = &MoodsClaim> {
        self.items.iter().filter_map(|i| match i {
            Item::MoodsClaim(c) => Some(c),
            _ => None,
        })
    }

    pub fn schedule_claims(&self) -> impl Iterator<Item = &ScheduleClaim> {
        self.items.iter().filter_map(|i| match i {
            Item::ScheduleClaim(c) => Some(c),
            _ => None,
        })
    }

    pub fn tiles_claims(&self) -> impl Iterator<Item = &TilesClaim> {
        self.items.iter().filter_map(|i| match i {
            Item::TilesClaim(c) => Some(c),
            _ => None,
        })
    }

    pub fn rule_decls(&self) -> impl Iterator<Item = &RuleDecl> {
        self.items.iter().filter_map(|i| match i {
            Item::Rule(r) => Some(r),
            _ => None,
        })
    }

    pub fn access_decls(&self) -> impl Iterator<Item = &AccessDecl> {
        self.items.iter().filter_map(|i| match i {
            Item::Access(a) => Some(a),
            _ => None,
        })
    }

    pub fn lets(&self) -> impl Iterator<Item = &LetDecl> {
        self.items.iter().filter_map(|i| match i {
            Item::Let(l) => Some(l),
            _ => None,
        })
    }

    pub fn removed(&self) -> impl Iterator<Item = &RemovedDecl> {
        self.items.iter().filter_map(|i| match i {
            Item::Removed(r) => Some(r),
            _ => None,
        })
    }

    pub fn moved(&self) -> impl Iterator<Item = &MovedDecl> {
        self.items.iter().filter_map(|i| match i {
            Item::Moved(m) => Some(m),
            _ => None,
        })
    }

    /// Every wire in the module as `(source, sink)`, in source order: a
    /// block's wire bindings sink into the block itself; `<-` statements
    /// sink onto extern ports.
    pub fn wire_pairs(&self) -> Vec<(PortRef, PortRef)> {
        let mut out = Vec::new();
        for item in &self.items {
            match item {
                Item::Block(b) => {
                    for (port, src) in b.input_wires() {
                        out.push((
                            src.clone(),
                            PortRef {
                                slug: b.slug.clone(),
                                port: port.to_string(),
                            },
                        ));
                    }
                }
                Item::Wire(w) => out.push((w.from.clone(), w.to.clone())),
                _ => {}
            }
        }
        out
    }

    /// Resolve a value to the literal string that becomes `Def=`: literals
    /// resolve to themselves, unit values scale into their base unit
    /// (`1.5h` → `5400`), `Ref`s resolve through the module's `let`
    /// constants.
    pub fn resolve_value<'m>(&'m self, value: &'m Value) -> Result<Cow<'m, str>> {
        match value {
            Value::Ref(name) => self
                .lets()
                .find(|l| l.name == *name)
                .and_then(|l| l.value.literal())
                .ok_or_else(|| Error::Compile(format!("undeclared constant `{name}`"))),
            Value::File { path, text: None } => Err(Error::Compile(format!(
                "`file({})` was not loaded — parse the module from its files \
                 (`Module::load_files`), which reads it relative to the `.loxl` file",
                quote(path)
            ))),
            other => other
                .literal()
                .ok_or_else(|| Error::Compile(format!("value `{other}` overflows its unit"))),
        }
    }

    /// [`Self::resolve_value`] for a value bound to `port` of a block of
    /// `block_type`: a suffixed value — written inline or through a `let`
    /// — scales into the port's documented unit (D52), and a suffix that
    /// cannot mean what the port is in is refused; `on`/`off` becomes
    /// `true`/`false` on a text-boolean attribute (D80). `slug` names the
    /// binding in the error.
    pub fn resolve_value_for<'m>(
        &'m self,
        value: &'m Value,
        block_type: &str,
        slug: &str,
        port: &str,
    ) -> Result<Cow<'m, str>> {
        let suffixed = match value {
            Value::Unit { number, unit } => Some((number, *unit)),
            Value::Ref(name) => {
                self.lets()
                    .find(|l| l.name == *name)
                    .and_then(|l| match &l.value {
                        Value::Unit { number, unit } => Some((number, *unit)),
                        _ => None,
                    })
            }
            _ => None,
        };
        // `on`/`off` on a text-boolean attribute (`Analog:` on VirtualIn)
        // is stored as `true`/`false`; everywhere else as `1`/`0`.
        let boolean = match value {
            Value::Bool(b) => Some(*b),
            Value::Ref(name) => self
                .lets()
                .find(|l| l.name == *name)
                .and_then(|l| match l.value {
                    Value::Bool(b) => Some(b),
                    _ => None,
                }),
            _ => None,
        };
        if let Some(b) = boolean
            && crate::connectors::bool_params(block_type).contains(&port)
        {
            return Ok(Cow::Borrowed(if b { "true" } else { "false" }));
        }
        match suffixed {
            Some((number, unit)) => scale_for_port(number, unit, port_unit(block_type, port))
                .map(Cow::Owned)
                .map_err(|why| Error::Compile(format!("`{slug}.{port}`: {why}"))),
            None => self.resolve_value(value),
        }
    }

    /// Canonical text form. `parse(to_text(m)) == m`.
    pub fn to_text(&self) -> String {
        // Blank lines separate statement families; plain and expression
        // wires are one family (both read `sink <- …`).
        fn family(item: &Item) -> u8 {
            match item {
                Item::Extern(_) => 0,
                // A named signal declares a block, like a declaration.
                Item::Block(_) | Item::Signal(_) => 1,
                Item::Wire(_) | Item::ExprWire(_) => 2,
                Item::Set(_)
                | Item::Attr(_)
                | Item::TilesClaim(_)
                | Item::ScheduleClaim(_)
                | Item::MoodsClaim(_) => 3,
                Item::Let(_) => 4,
                Item::Removed(_) => 5,
                Item::Moved(_) => 6,
                Item::Template(_) => 7,
                Item::Instance(_) => 8,
                Item::Comment(_) => 9,
                // A `page` statement heads a placement section — the blank
                // line on both sides comes from the family change.
                Item::Page(_) => 10,
                Item::Test(_) => 11,
                Item::Tiles(_) => 12,
                Item::Access(_) => 13,
                Item::Schedule(_) => 14,
                Item::Moods(_) => 16,
                Item::Rule(_) => 15,
            }
        }
        let mut out = String::new();
        let mut prev: Option<u8> = None;
        // A multi-line call is visually dense; separate it from the next
        // item even when the kind does not change.
        let mut prev_multiline = false;
        let mut skip = 0;
        for (i, item) in self.items.iter().enumerate() {
            if skip > 0 {
                skip -= 1;
                continue;
            }
            let disc = family(item);
            if prev.is_some_and(|p| p != disc) || prev_multiline {
                out.push('\n');
            }
            prev = Some(disc);
            (prev_multiline, skip) = self.write_item(i, item, &mut out);
        }
        out
    }

    /// Writes one item in canonical form (the body of [`Module::to_text`]).
    /// `i` is its index in `self.items`. Returns whether it was written
    /// over several lines and how many following items a bulk line
    /// absorbed.
    fn write_item(&self, i: usize, item: &Item, out: &mut String) -> (bool, usize) {
        let mut multiline = false;
        let mut skip = 0;
        match item {
            Item::Extern(e) => {
                out.push_str(&format!(
                    "extern {} = {}({}){}\n",
                    e.slug,
                    e.block_type,
                    e.spec(),
                    tail(&e.comment)
                ));
            }
            Item::Block(b) | Item::Instance(b) => {
                multiline = write_block(b, out);
            }
            Item::Wire(w) => {
                out.push_str(&format!("{} <- {}{}\n", w.to, w.from, tail(&w.comment)));
            }
            Item::ExprWire(w) => {
                out.push_str(&format!("{} <- {}{}\n", w.to, w.expr, tail(&w.comment)));
            }
            Item::Signal(x) => {
                out.push_str(&format!("{} = {}{}\n", x.slug, x.expr, tail(&x.comment)));
            }
            // The statements of one bulk line go back to that line.
            Item::Set(SetDecl {
                bulk: Some(line),
                target,
                value,
                comment,
            }) => {
                let slugs = bulk_run(&self.items[i..], *line, |x| match x {
                    Item::Set(s) if s.target.port == target.port && s.value == *value => {
                        Some((s.bulk, s.target.slug.as_str()))
                    }
                    _ => None,
                });
                skip = slugs.len() - 1;
                out.push_str(&format!(
                    "({}).{} = {value}{}\n",
                    slugs.join(", "),
                    target.port,
                    tail(comment)
                ));
            }
            Item::Attr(AttrDecl {
                bulk: Some(line),
                attr,
                value,
                comment,
                ..
            }) => {
                let slugs = bulk_run(&self.items[i..], *line, |x| match x {
                    Item::Attr(a) if a.attr == *attr && a.value == *value => {
                        Some((a.bulk, a.slug.as_str()))
                    }
                    _ => None,
                });
                skip = slugs.len() - 1;
                out.push_str(&format!(
                    "({}).{attr} = {value}{}\n",
                    slugs.join(", "),
                    tail(comment)
                ));
            }
            Item::Set(s) => {
                out.push_str(&format!("{} = {}{}\n", s.target, s.value, tail(&s.comment)));
            }
            Item::Attr(a) => {
                out.push_str(&format!("{a}{}\n", tail(&a.comment)));
            }
            Item::Let(l) => {
                out.push_str(&format!(
                    "let {} = {}{}\n",
                    l.name,
                    l.value,
                    tail(&l.comment)
                ));
            }
            Item::Removed(r) => {
                out.push_str(&format!("removed {}{}\n", r.slug, tail(&r.comment)));
            }
            Item::Moved(m) => {
                out.push_str(&format!(
                    "moved {} -> {}{}\n",
                    m.from,
                    m.to,
                    tail(&m.comment)
                ));
            }
            Item::Template(t) => {
                write_template(t, out);
                multiline = true;
            }
            Item::Page(p) => {
                let mut args = Vec::new();
                if let Some(r) = &p.room {
                    args.push(format!("room: {}", quote(r)));
                }
                if let Some(c) = &p.category {
                    args.push(format!("category: {}", quote(c)));
                }
                let args = if args.is_empty() {
                    String::new()
                } else {
                    format!(" ({})", args.join(", "))
                };
                out.push_str(&format!(
                    "page {}{args}{}\n",
                    quote(&p.title),
                    tail(&p.comment)
                ));
            }
            Item::Test(t) => {
                out.push_str(&format!("test {}{}\n", quote(&t.name), tail(&t.comment)));
                for stmt in &t.body {
                    out.push_str(&format!("\t{stmt}\n"));
                }
                out.push_str(&format!("end{}\n", tail(&t.end_comment)));
                multiline = true;
            }
            Item::Tiles(t) => {
                write_tiles(t, out);
                multiline = true;
            }
            Item::TilesClaim(c) => {
                out.push_str(&format!("{c}{}\n", tail(&c.comment)));
            }
            Item::Access(a) => {
                out.push_str(&format!(
                    "{} {}{}\n",
                    a.right.keyword(),
                    a.slug,
                    tail(&a.comment)
                ));
                for stmt in &a.body {
                    match stmt {
                        AccessItem::Grant(g) => {
                            out.push_str(&format!("\t{g}{}\n", tail(&g.comment)));
                        }
                        AccessItem::Comment(text) => out.push_str(&format!("\t#{text}\n")),
                    }
                }
                out.push_str(&format!("end{}\n", tail(&a.end_comment)));
                multiline = true;
            }
            Item::Schedule(t) => {
                out.push_str(&format!("schedule {}{}\n", t.name, tail(&t.comment)));
                for stmt in &t.body {
                    match stmt {
                        ScheduleItem::Line(l) => out.push_str(&format!("\t{l}\n")),
                        ScheduleItem::Comment(text) => out.push_str(&format!("\t#{text}\n")),
                    }
                }
                out.push_str(&format!("end{}\n", tail(&t.end_comment)));
                multiline = true;
            }
            Item::ScheduleClaim(c) => {
                out.push_str(&format!("{c}{}\n", tail(&c.comment)));
            }
            Item::Moods(t) => {
                out.push_str(&format!("moods {}{}\n", t.name, tail(&t.comment)));
                for stmt in &t.body {
                    match stmt {
                        MoodsItem::Mood(m) => out.push_str(&format!("\t{m}\n")),
                        MoodsItem::Comment(text) => out.push_str(&format!("\t#{text}\n")),
                    }
                }
                out.push_str(&format!("end{}\n", tail(&t.end_comment)));
                multiline = true;
            }
            Item::MoodsClaim(c) => {
                out.push_str(&format!("{c}{}\n", tail(&c.comment)));
            }
            Item::Rule(r) => {
                let title = r.title.as_deref().map(|t| format!(" {}", quote(t)));
                out.push_str(&format!(
                    "rule {}{}{}\n",
                    r.slug,
                    title.unwrap_or_default(),
                    tail(&r.comment)
                ));
                for stmt in &r.body {
                    match stmt {
                        RuleItem::Stmt(st) => {
                            out.push_str(&format!("\t{}{}\n", st.kind, tail(&st.comment)));
                        }
                        RuleItem::Comment(text) => out.push_str(&format!("\t#{text}\n")),
                    }
                }
                out.push_str(&format!("end{}\n", tail(&r.end_comment)));
                multiline = true;
            }
            Item::Comment(text) => {
                out.push_str(&format!("#{text}\n"));
            }
        }
        (multiline, skip)
    }
}

/// ` #comment` for a trailing comment, empty without one.
fn tail(c: &Option<String>) -> String {
    c.as_ref().map(|t| format!(" #{t}")).unwrap_or_default()
}

/// Canonical text of a block or instance call: on one line without
/// arguments, else one argument per line. True when multi-line.
fn write_block(b: &BlockDecl, out: &mut String) -> bool {
    let mut multiline = false;
    out.push_str(&format!("{} = {}(", b.slug, b.block_type));
    if b.args.is_empty() {
        // Single line: `slug = Type()` or `slug = Type("Label")`.
        if let Some(t) = &b.title {
            out.push_str(&quote(t));
        }
        out.push(')');
        out.push_str(&tail(&b.comment));
    } else {
        out.push_str(&tail(&b.comment));
        out.push('\n');
        if let Some(t) = &b.title {
            out.push_str(&format!("\t{},\n", quote(t)));
        }
        for arg in &b.args {
            match arg {
                ArgItem::Binding(x) => {
                    out.push_str(&format!("\t{}: {},{}\n", x.port, x.kind, tail(&x.comment)))
                }
                ArgItem::Comment(text) => {
                    out.push_str(&format!("\t#{text}\n"));
                }
            }
        }
        out.push(')');
        out.push_str(&tail(&b.close_comment));
        multiline = true;
    }
    out.push('\n');
    multiline
}

/// Canonical text of a template: its head, the body indented one level, `end`.
fn write_template(t: &TemplateDecl, out: &mut String) {
    let params = t
        .params
        .iter()
        .map(|p| p.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    out.push_str(&format!(
        "template {}({params}){}\n",
        t.name,
        tail(&t.comment)
    ));
    // The body is a module in miniature — reuse its
    // canonical form, indented one level.
    let body = Module {
        items: t.body.clone(),
    }
    .to_text();
    for line in body.lines() {
        if !line.is_empty() {
            out.push('\t');
            out.push_str(line);
        }
        out.push('\n');
    }
    out.push_str(&format!("end{}\n", tail(&t.end_comment)));
}

/// Canonical text of a `tiles` set: one `tile` call per tile, then `end`.
fn write_tiles(t: &TilesDecl, out: &mut String) {
    out.push_str(&format!("tiles {}{}\n", t.name, tail(&t.comment)));
    for stmt in &t.body {
        match stmt {
            TilesItem::Tile(tile) => {
                out.push_str(&format!("\ttile {}(", tile.index));
                if tile.args.is_empty() {
                    out.push(')');
                    out.push_str(&tail(&tile.comment));
                } else {
                    out.push_str(&tail(&tile.comment));
                    out.push('\n');
                    for arg in &tile.args {
                        match arg {
                            ArgItem::Binding(x) => out.push_str(&format!(
                                "\t\t{}: {},{}\n",
                                x.port,
                                x.kind,
                                tail(&x.comment)
                            )),
                            ArgItem::Comment(text) => {
                                out.push_str(&format!("\t\t#{text}\n"));
                            }
                        }
                    }
                    out.push_str("\t)");
                    out.push_str(&tail(&tile.close_comment));
                }
                out.push('\n');
            }
            TilesItem::Comment(text) => out.push_str(&format!("\t#{text}\n")),
        }
    }
    out.push_str(&format!("end{}\n", tail(&t.end_comment)));
}

fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The slugs of the bulk statement (#199) that starts `items`: the run of
/// statements carrying its line and the same setting and value.
fn bulk_run<'a>(
    items: &'a [Item],
    line: usize,
    member: impl Fn(&'a Item) -> Option<(Option<usize>, &'a str)>,
) -> Vec<&'a str> {
    items
        .iter()
        .map_while(|x| member(x).filter(|(l, _)| *l == Some(line)).map(|(_, s)| s))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn divide_exact_terminates_or_refuses() {
        assert_eq!(divide_exact("7200", 60).as_deref(), Some("120"));
        assert_eq!(divide_exact("90", 60).as_deref(), Some("1.5"));
        assert_eq!(divide_exact("7.5", 60).as_deref(), Some("0.125"));
        assert_eq!(divide_exact("-1800", 60).as_deref(), Some("-30"));
        assert_eq!(divide_exact("0.25", 60), None);
        assert_eq!(divide_exact("10", 60), None);
        assert_eq!(divide_exact("0", 60).as_deref(), Some("0"));
    }

    #[test]
    fn suffixes_scale_into_the_documented_unit() {
        let seconds = Some(PortUnit::Seconds);
        let minutes = Some(PortUnit::Minutes);
        assert_eq!(scale_for_port("1.5", Unit::H, seconds).unwrap(), "5400");
        assert_eq!(scale_for_port("2", Unit::H, minutes).unwrap(), "120");
        assert_eq!(scale_for_port("90", Unit::S, minutes).unwrap(), "1.5");
        assert_eq!(scale_for_port("-30", Unit::Min, minutes).unwrap(), "-30");
        let err = scale_for_port("10", Unit::S, minutes).unwrap_err();
        assert!(err.contains("documented in minutes"), "{err}");

        assert_eq!(
            scale_for_port("70", Unit::Pct, Some(PortUnit::Percent)).unwrap(),
            "70"
        );
        assert_eq!(
            scale_for_port("2700", Unit::K, Some(PortUnit::Kelvin)).unwrap(),
            "2700"
        );
        let err = scale_for_port("70", Unit::Pct, seconds).unwrap_err();
        assert!(err.contains("documented in seconds"), "{err}");
        let err = scale_for_port("5", Unit::S, Some(PortUnit::Percent)).unwrap_err();
        assert!(err.contains("`5s` does not fit"), "{err}");
        assert!(scale_for_port("5", Unit::S, Some(PortUnit::Other("°"))).is_err());

        // Undocumented ports keep the D27 reading.
        assert_eq!(scale_for_port("45", Unit::Min, None).unwrap(), "2700");
        assert_eq!(scale_for_port("80", Unit::Pct, None).unwrap(), "80");
    }
}
