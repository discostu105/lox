//! IR abstract syntax and canonical text emission.

pub use crate::access::PrincipalKind;
use crate::connectors::{PortUnit, port_unit};
use crate::error::{Error, Result};
use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

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
    /// `page "Title"` — the base-config page the following block
    /// declarations are placed on (D28). Positional: governs every block
    /// after it, until the next `page` statement; blocks above the first
    /// `page` statement keep the compile options' default page.
    Page(PageDecl),
    /// `test "Name"` … `end` — a simulator-backed scenario (D36).
    /// `compile` ignores it entirely; `lxir test` compiles the module in
    /// memory and replays the body against `lox sim run` (lox-cli).
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
    /// `.lxir` file (D40); binds only program text (`Code:` on Code16).
    /// `path` is kept as written; `text` is filled by
    /// [`Module::load_files`] (CRLF normalized to LF) and stays `None`
    /// for a module parsed from a string alone.
    File { path: String, text: Option<String> },
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
    /// `Unit` scaled into its base unit, `File` its loaded text; `None` for
    /// a `Ref` (resolve it through [`Module::resolve_value`]) and for a
    /// `File` that was never loaded.
    pub fn literal(&self) -> Option<Cow<'_, str>> {
        match self {
            Value::Number(s) | Value::Str(s) => Some(Cow::Borrowed(s)),
            Value::Ref(_) => None,
            Value::Unit { number, unit } => scale_by_unit(number, *unit).map(Cow::Owned),
            Value::File { text, .. } => text.as_deref().map(Cow::Borrowed),
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
}

/// An object attribute an extern assignment can claim (D39). Lowercase
/// names after the dot: no connector in the corpus carries these keys, and
/// a port of that name could not be wired or set anyway (`=`/`<-` on it
/// always mean the attribute).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum Attr {
    /// The object's `Title=`.
    Title,
    /// The room binding: `Pr=` on the object's `<IoData>` (a `Place` UUID).
    Room,
    /// The category binding: `Cr=` on the object's `<IoData>` (a
    /// `Category` UUID).
    Category,
}

impl Attr {
    pub const ALL: [Attr; 3] = [Attr::Title, Attr::Room, Attr::Category];

    pub fn name(self) -> &'static str {
        match self {
            Attr::Title => "title",
            Attr::Room => "room",
            Attr::Category => "category",
        }
    }

    pub fn parse(s: &str) -> Option<Attr> {
        Attr::ALL.into_iter().find(|a| a.name() == s)
    }
}

impl fmt::Display for Attr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

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
/// the simulated wall-clock. `lxir test` groups the statements into
/// `lox sim` steps at the `tick` boundaries.
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
    /// `dt 100ms` scales).
    Tick(TickDecl),
    /// `expect slug.Port <cmp> <value>` — assert a port's value after the
    /// preceding `tick`.
    Expect(ExpectDecl),
    /// `clock "HH:MM[:SS]"` or `clock "YYYY-MM-DD HH:MM[:SS]"` — set the
    /// simulated wall-clock (drives DayTimer/astro blocks); it advances
    /// by `dt` each tick.
    Clock(ClockDecl),
    /// Whole-line `#` comment, verbatim.
    Comment(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TickDecl {
    pub n: u64,
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
    /// Always numeric: `Number`, `Unit`, or a `let` reference.
    pub value: Value,
    /// Trailing `#` comment on the statement line, verbatim.
    pub comment: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClockDecl {
    /// `"HH:MM[:SS]"` or `"YYYY-MM-DD HH:MM[:SS]"`, verbatim.
    pub spec: String,
    /// Trailing `#` comment on the statement line, verbatim.
    pub comment: Option<String>,
}

/// Comparators the simulator supports in `expect` (there is no `!=`).
/// `~=` passes within 5% of the expected value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TestCmp {
    Eq,
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
                write!(f, "tick {}", t.n)?;
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
            TestItem::Clock(c) => write!(f, "clock {}{}", quote(&c.spec), tail(&c.comment)),
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
/// keywords, kept as written: `weekdays` stays `weekdays`) and their
/// intervals. No intervals (`holiday off`) lists the modes with the timer
/// off all day.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduleLine {
    pub days: Vec<String>,
    pub intervals: Vec<Interval>,
    /// Trailing `#` comment, verbatim.
    pub comment: Option<String>,
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
        f.write_str(&self.days.join(" "))?;
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
/// name, description and every field lxir does not know stay as they
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
}

impl fmt::Display for RuleStmtKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RuleStmtKind::Active(true) => f.write_str("active"),
            RuleStmtKind::Active(false) => f.write_str("inactive"),
            RuleStmtKind::Time(m) => write!(f, "when time {:02}:{:02}", m / 60, m % 60),
            RuleStmtKind::Weekday(days) => write!(f, "when weekday {}", days.join(" ")),
            RuleStmtKind::Then { target, command } => {
                write!(f, "then {target} {}", quote(command))
            }
        }
    }
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
    /// of the `.lxir` file these items were parsed from — so each fragment
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
            Item::Attr(a) => a.slug != slug,
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
                 (`Module::load_files`), which reads it relative to the `.lxir` file",
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
    /// cannot mean what the port is in is refused. `slug` names the
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
        match suffixed {
            Some((number, unit)) => scale_for_port(number, unit, port_unit(block_type, port))
                .map(Cow::Owned)
                .map_err(|why| Error::Compile(format!("`{slug}.{port}`: {why}"))),
            None => self.resolve_value(value),
        }
    }

    /// Name uniqueness, reference resolution, and statement-level
    /// consistency (no base config needed). Port existence and directions
    /// on managed blocks are checked separately by
    /// [`super::validate_ports`].
    pub fn validate(&self) -> Result<()> {
        let compile_err = |msg: String| Err(Error::Compile(msg));

        // One namespace for externs, blocks, and constants.
        let mut names: BTreeMap<&str, NameKind> = BTreeMap::new();
        for item in &self.items {
            let (name, kind) = match item {
                Item::Extern(e) => (e.slug.as_str(), NameKind::Extern),
                Item::Block(b) => (b.slug.as_str(), NameKind::Block),
                Item::Let(l) => (l.name.as_str(), NameKind::Let),
                Item::Template(t) => (t.name.as_str(), NameKind::Template),
                Item::Instance(b) => (b.slug.as_str(), NameKind::Instance),
                Item::Tiles(t) => (t.name.as_str(), NameKind::Tiles),
                Item::Schedule(t) => (t.name.as_str(), NameKind::Schedule),
                Item::Moods(t) => (t.name.as_str(), NameKind::Moods),
                _ => continue,
            };
            if names.insert(name, kind).is_some() {
                return compile_err(format!("duplicate name `{name}`"));
            }
        }
        // Tests are named by display string, in a namespace of their own.
        let mut test_names: BTreeSet<&str> = BTreeSet::new();
        for item in &self.items {
            if let Item::Test(t) = item
                && !test_names.insert(t.name.as_str())
            {
                return compile_err(format!("duplicate test name {}", quote(&t.name)));
            }
        }
        let block_type = |slug: &str| -> &str {
            self.blocks()
                .find(|b| b.slug == slug)
                .map_or("…", |b| &b.block_type)
        };

        let object_ref = |r: &PortRef| -> Result<()> {
            match names.get(r.slug.as_str()) {
                None => compile_err(format!(
                    "reference to undeclared slug `{}` (in `{r}`)",
                    r.slug
                )),
                Some(NameKind::Let) => compile_err(format!(
                    "`{}` is a `let` constant, not a block or extern (in `{r}`)",
                    r.slug
                )),
                Some(NameKind::Template) => compile_err(format!(
                    "`{slug}` is a template, not an object — instantiate it first \
                     (`<slug> = {slug}(…)`) (in `{r}`)",
                    slug = r.slug
                )),
                Some(NameKind::Instance) => compile_err(format!(
                    "`{slug}` is a template instance and names no object itself — \
                     reference one of its blocks by its expanded name \
                     (`{slug}_<block>`) (in `{r}`)",
                    slug = r.slug
                )),
                Some(NameKind::Tiles) => compile_err(format!(
                    "`{}` is a `tiles` set, not an object (in `{r}`)",
                    r.slug
                )),
                Some(_) => Ok(()),
            }
        };
        let value_refs = |value: &Value| -> Result<()> {
            let Value::Ref(name) = value else {
                return Ok(());
            };
            match names.get(name.as_str()) {
                Some(NameKind::Let) => Ok(()),
                Some(kind) => compile_err(format!(
                    "`{name}` is {}, not a `let` constant (quote the value if a \
                     string was intended)",
                    kind.describe()
                )),
                None => {
                    let hint = super::validate::suggest(name, self.lets().map(|l| l.name.as_str()));
                    compile_err(format!(
                        "undeclared constant `{name}`{hint} (declare it with \
                         `let {name} = …`, or quote the value if a string was intended)"
                    ))
                }
            }
        };
        // Whether every template and expression has already been lowered
        // into plain blocks. Test bodies may reference the slugs those
        // lowerings mint, so their references only resolve on this form.
        let flattened = !self.items.iter().any(|i| match i {
            Item::Template(_) | Item::Instance(_) | Item::ExprWire(_) => true,
            Item::Block(b) => b.bindings().any(|x| matches!(x.kind, BindingKind::Expr(_))),
            _ => false,
        });

        // Expression operands, shared by `<-` expressions and expression
        // bindings in argument lists: ports must resolve, constants must be
        // numbers or `let` references.
        let check_expr_operands = |e: &Expr, sink: &str| -> Result<()> {
            for operand in e.operands() {
                match operand {
                    Operand::Port(p) => object_ref(p)?,
                    Operand::Value(Value::Str(s)) => {
                        return compile_err(format!(
                            "string {} in the expression on `{sink}` — \
                             expressions compare numbers and ports only",
                            quote(s),
                        ));
                    }
                    Operand::Value(v) => value_refs(v)?,
                }
            }
            Ok(())
        };

        let mut attrs_seen: BTreeSet<(&str, Attr)> = BTreeSet::new();
        let mut tiles_claimed: BTreeSet<&str> = BTreeSet::new();
        let mut access_declared: BTreeSet<&str> = BTreeSet::new();
        let mut schedules_claimed: BTreeSet<&str> = BTreeSet::new();
        let mut moods_claimed: BTreeSet<&str> = BTreeSet::new();
        let mut rules_declared: BTreeSet<&str> = BTreeSet::new();
        let mut rule_titles: BTreeSet<&str> = BTreeSet::new();
        for item in &self.items {
            match item {
                Item::Block(b) => {
                    let mut params_seen: BTreeSet<&str> = BTreeSet::new();
                    let mut wires_seen: BTreeSet<(&str, &PortRef)> = BTreeSet::new();
                    let mut exprs_seen: BTreeSet<(&str, String)> = BTreeSet::new();
                    for binding in b.bindings() {
                        match &binding.kind {
                            BindingKind::Param(v) => {
                                // `mirrors:` on a minted ref names an object,
                                // not a constant (D33) — its target is
                                // checked by `validate_ports`. On any other
                                // type the key is a mistake worth a message
                                // clearer than the constant-resolution one.
                                if binding.port == "mirrors" {
                                    if !matches!(
                                        b.block_type.as_str(),
                                        "InputRef" | "OutputRef" | "OutputRefLM"
                                    ) {
                                        return compile_err(format!(
                                            "`mirrors:` applies to InputRef/\
                                             OutputRef/OutputRefLM only, not {} (in `{} = \
                                             {}(…)`)",
                                            b.block_type, b.slug, b.block_type
                                        ));
                                    }
                                } else {
                                    value_refs(v)?;
                                }
                                if !params_seen.insert(&binding.port) {
                                    return compile_err(format!(
                                        "duplicate parameter `{}` in `{} = {}(…)`",
                                        binding.port, b.slug, b.block_type
                                    ));
                                }
                            }
                            BindingKind::Wire(src) => {
                                object_ref(src)?;
                                if !wires_seen.insert((&binding.port, src)) {
                                    return compile_err(format!(
                                        "duplicate wire `{}: {src}` in `{} = {}(…)`",
                                        binding.port, b.slug, b.block_type
                                    ));
                                }
                            }
                            BindingKind::Expr(e) => {
                                check_expr_operands(e, &format!("{}.{}", b.slug, binding.port))?;
                                if !exprs_seen.insert((&binding.port, e.to_string())) {
                                    return compile_err(format!(
                                        "duplicate expression `{}: {e}` in `{} = {}(…)`",
                                        binding.port, b.slug, b.block_type
                                    ));
                                }
                            }
                        }
                    }
                }
                Item::Wire(w) => {
                    object_ref(&w.to)?;
                    object_ref(&w.from)?;
                    if names.get(w.to.slug.as_str()) == Some(&NameKind::Block) {
                        return compile_err(format!(
                            "`{to} <- {from}` targets managed block `{slug}` — wire it \
                             in the argument list instead (`{port}: {from}` inside \
                             `{slug} = {ty}(…)`); `<-` is for extern ports only",
                            to = w.to,
                            from = w.from,
                            slug = w.to.slug,
                            port = w.to.port,
                            ty = block_type(&w.to.slug),
                        ));
                    }
                }
                Item::ExprWire(w) => {
                    object_ref(&w.to)?;
                    if names.get(w.to.slug.as_str()) == Some(&NameKind::Block) {
                        return compile_err(format!(
                            "`{to} <- …` targets managed block `{slug}` — wire the \
                             expression's result in the argument list instead; `<-` \
                             is for extern ports only",
                            to = w.to,
                            slug = w.to.slug,
                        ));
                    }
                    check_expr_operands(&w.expr, &w.to.to_string())?;
                }
                Item::Set(s) => {
                    object_ref(&s.target)?;
                    if names.get(s.target.slug.as_str()) == Some(&NameKind::Block) {
                        return compile_err(format!(
                            "`{target} = …` targets managed block `{slug}` — bind the \
                             parameter in the argument list instead (`{port}: …` inside \
                             `{slug} = {ty}(…)`); port assignment is for extern ports only",
                            target = s.target,
                            slug = s.target.slug,
                            port = s.target.port,
                            ty = block_type(&s.target.slug),
                        ));
                    }
                    value_refs(&s.value)?;
                }
                Item::Attr(a) => {
                    let target = format!("{}.{}", a.slug, a.attr);
                    match names.get(a.slug.as_str()) {
                        Some(NameKind::Extern) => {}
                        Some(NameKind::Block) => {
                            let instead = match a.attr {
                                Attr::Title => format!(
                                    "give it a label instead (`{} = {}(\"…\", …)`)",
                                    a.slug,
                                    block_type(&a.slug)
                                ),
                                Attr::Room | Attr::Category => format!(
                                    "a managed block's {} stays with Loxone Config \
                                     (carried forward from the base, D19)",
                                    a.attr
                                ),
                            };
                            return compile_err(format!(
                                "`{target} = …` targets managed block `{}` — {instead}; \
                                 attribute assignment is for externs only",
                                a.slug
                            ));
                        }
                        _ => object_ref(&PortRef {
                            slug: a.slug.clone(),
                            port: a.attr.name().to_string(),
                        })?,
                    }
                    // An assignment must not rewrite what its own extern is
                    // matched by: once deployed, the match would no longer
                    // find the object it pinned.
                    if let Some(e) = self.externs().find(|e| e.slug == a.slug) {
                        let matched_by_it = match a.attr {
                            Attr::Title => matches!(e.match_spec, MatchSpec::Title(_)),
                            Attr::Room => e.room.is_some(),
                            Attr::Category => e.category.is_some(),
                        };
                        if matched_by_it {
                            return compile_err(format!(
                                "`{target} = …` rewrites what extern `{}` is matched by \
                                 (`{}`) — after the push the match would no longer find \
                                 the object; pin it by `uuid:` (or `iname:`) instead",
                                a.slug,
                                e.spec()
                            ));
                        }
                    }
                    if !attrs_seen.insert((a.slug.as_str(), a.attr)) {
                        return compile_err(format!("duplicate assignment `{target} = …`"));
                    }
                    if let Value::Unit { .. } = a.value {
                        return compile_err(format!(
                            "`{a}` — a {} is text, not a unit value",
                            a.attr
                        ));
                    }
                    value_refs(&a.value)?;
                }
                // Unlike `Set`, a test may target managed blocks — driving
                // their inputs (and overriding outputs) is the point. Slug
                // references are deferred to the flattened view: tests may
                // name template-expanded blocks (`frost_alarm`) and
                // expression-owned blocks (`…__and2`), which only exist
                // after expansion and desugaring — compile/check/test all
                // re-validate that form.
                Item::Test(t) => {
                    for stmt in &t.body {
                        let numeric = |target: &dyn fmt::Display, v: &Value| -> Result<()> {
                            if let Value::Str(s) = v {
                                return compile_err(format!(
                                    "`{target}` gets string {} in test {} — the \
                                     simulator handles numbers only",
                                    quote(s),
                                    quote(&t.name)
                                ));
                            }
                            value_refs(v)
                        };
                        match stmt {
                            TestItem::Inject(s) => {
                                if flattened {
                                    object_ref(&s.target)?;
                                }
                                numeric(&s.target, &s.value)?;
                            }
                            TestItem::Expect(e) => {
                                if flattened {
                                    object_ref(&e.port)?;
                                }
                                numeric(&e.port, &e.value)?;
                            }
                            TestItem::Tick(_) | TestItem::Clock(_) | TestItem::Comment(_) => {}
                        }
                    }
                }
                Item::Template(t) => {
                    // Parameters and body slugs share one template-local
                    // namespace. Body references are resolved per
                    // instantiation, not here.
                    let mut local: BTreeSet<&str> = BTreeSet::new();
                    for p in &t.params {
                        if !local.insert(p.name()) {
                            return compile_err(format!(
                                "template `{}`: duplicate parameter `{}`",
                                t.name,
                                p.name()
                            ));
                        }
                    }
                    for item in &t.body {
                        match item {
                            Item::Block(b) => {
                                if !local.insert(&b.slug) {
                                    return compile_err(format!(
                                        "template `{}`: duplicate name `{}` in the body",
                                        t.name, b.slug
                                    ));
                                }
                                let mut seen: BTreeSet<&str> = BTreeSet::new();
                                for binding in b.bindings() {
                                    if matches!(binding.kind, BindingKind::Param(_))
                                        && !seen.insert(&binding.port)
                                    {
                                        return compile_err(format!(
                                            "template `{}`: duplicate parameter `{}` in \
                                             `{} = {}(…)`",
                                            t.name, binding.port, b.slug, b.block_type
                                        ));
                                    }
                                }
                            }
                            Item::Wire(_)
                            | Item::ExprWire(_)
                            | Item::Set(_)
                            | Item::MoodsClaim(_)
                            | Item::Comment(_) => {}
                            _ => {
                                return compile_err(format!(
                                    "template `{}`: only block declarations, wires, port \
                                     assignments, and comments are allowed in a body \
                                     (attribute assignments like `x.room = …` are \
                                     top-level statements, D39)",
                                    t.name
                                ));
                            }
                        }
                    }
                }
                Item::Instance(call) => {
                    let Some(t) = self.items.iter().find_map(|i| match i {
                        Item::Template(t) if t.name == call.block_type => Some(t),
                        _ => None,
                    }) else {
                        let hint = super::validate::suggest(
                            &call.block_type,
                            self.items.iter().filter_map(|i| match i {
                                Item::Template(t) => Some(t.name.as_str()),
                                _ => None,
                            }),
                        );
                        return compile_err(format!(
                            "`{} = {name}(…)` instantiates unknown template `{name}`{hint}",
                            call.slug,
                            name = call.block_type
                        ));
                    };
                    if call.title.is_some() {
                        return compile_err(format!(
                            "`{} = {}(…)`: an instantiation takes no label string — \
                             titles belong on the template's blocks",
                            call.slug, call.block_type
                        ));
                    }
                    let mut seen: BTreeSet<&str> = BTreeSet::new();
                    let body_blocks = t
                        .body
                        .iter()
                        .filter(|i| matches!(i, Item::Block(_)))
                        .count();
                    for binding in call.bindings() {
                        let Some(p) = t.params.iter().find(|p| p.name() == binding.port) else {
                            // A binding naming no parameter forwards as a
                            // port binding onto the body's single block
                            // (D23); like a block's feeds it may repeat, and
                            // the expanded module re-validates its refs.
                            if body_blocks == 1 {
                                if let BindingKind::Param(v) = &binding.kind {
                                    value_refs(v)?;
                                }
                                continue;
                            }
                            let hint = super::validate::suggest(
                                &binding.port,
                                t.params.iter().map(|p| p.name()),
                            );
                            return compile_err(format!(
                                "`{} = {}(…)`: `{}` names no parameter of `{}`, and \
                                 port bindings forward only when the template body \
                                 declares exactly one block (this one declares \
                                 {body_blocks}){hint}",
                                call.slug, call.block_type, binding.port, call.block_type
                            ));
                        };
                        if !seen.insert(&binding.port) {
                            return compile_err(format!(
                                "`{} = {}(…)`: duplicate argument `{}`",
                                call.slug, call.block_type, binding.port
                            ));
                        }
                        match (p, &binding.kind) {
                            (TemplateParam::Object { .. }, BindingKind::Param(Value::Ref(_))) => {}
                            (TemplateParam::Object { name, .. }, _) => {
                                return compile_err(format!(
                                    "`{} = {}(…)`: object parameter `{name}` takes an \
                                     extern or block slug (`{name}: <slug>`)",
                                    call.slug, call.block_type
                                ));
                            }
                            (TemplateParam::Value { .. }, BindingKind::Param(v)) => {
                                value_refs(v)?;
                            }
                            (
                                TemplateParam::Value { name, .. },
                                BindingKind::Wire(_) | BindingKind::Expr(_),
                            ) => {
                                return compile_err(format!(
                                    "`{} = {}(…)`: value parameter `{name}` takes a \
                                     number, string, or constant — not a port or \
                                     expression",
                                    call.slug, call.block_type
                                ));
                            }
                        }
                    }
                    for p in &t.params {
                        if let TemplateParam::Object { name, .. } = p
                            && !seen.contains(name.as_str())
                        {
                            return compile_err(format!(
                                "`{} = {}(…)`: object parameter `{name}` must be given",
                                call.slug, call.block_type
                            ));
                        }
                    }
                }
                // `to:` targets name blocks, which may be template-expanded
                // (`<instance>_jal`) — an unknown slug is judged on the
                // flattened view only, like test references.
                Item::Tiles(t) => {
                    let mut indices: BTreeSet<u32> = BTreeSet::new();
                    for tile in t.tiles() {
                        let ctx = format!("tiles {}, tile {}", t.name, tile.index);
                        if !indices.insert(tile.index) {
                            return compile_err(format!(
                                "`tiles {}`: tile {} is programmed twice",
                                t.name, tile.index
                            ));
                        }
                        let program = super::tiles::program(tile)
                            .map_err(|m| Error::Compile(format!("{ctx}: {m}")))?;
                        for slug in program.targets() {
                            match names.get(slug) {
                                Some(NameKind::Extern | NameKind::Block) => {}
                                None if !flattened => {}
                                None => {
                                    let hint = super::validate::suggest(
                                        slug,
                                        self.externs()
                                            .map(|e| e.slug.as_str())
                                            .chain(self.blocks().map(|b| b.slug.as_str())),
                                    );
                                    return compile_err(format!(
                                        "{ctx}: `to: {slug}` names an undeclared slug{hint}"
                                    ));
                                }
                                Some(NameKind::Instance) => {
                                    return compile_err(format!(
                                        "{ctx}: `to: {slug}` names a template instance — \
                                         name the block that receives the command by its \
                                         expanded name (`{slug}_<block>`)"
                                    ));
                                }
                                Some(kind) => {
                                    return compile_err(format!(
                                        "{ctx}: `to: {slug}` names {}, not a block",
                                        kind.describe()
                                    ));
                                }
                            }
                        }
                    }
                }
                Item::TilesClaim(c) => {
                    match names.get(c.slug.as_str()) {
                        Some(NameKind::Extern) => {
                            let ty = self
                                .externs()
                                .find(|e| e.slug == c.slug)
                                .map_or("", |e| e.block_type.as_str());
                            if ty != "TreeDevice" {
                                return compile_err(format!(
                                    "`{c}`: tiles belong to a Touch Pure Flex, a \
                                     `TreeDevice` — `{}` is declared as `{ty}`",
                                    c.slug
                                ));
                            }
                        }
                        Some(kind) => {
                            return compile_err(format!(
                                "`{c}`: `{}` is {} — tiles are claimed on an extern \
                                 Touch Pure Flex (`extern {} = TreeDevice(uuid: \"…\")`)",
                                c.slug,
                                kind.describe(),
                                c.slug
                            ));
                        }
                        None => {
                            return compile_err(format!(
                                "`{c}`: undeclared slug `{}` — declare the device as \
                                 `extern {} = TreeDevice(uuid: \"…\")`",
                                c.slug, c.slug
                            ));
                        }
                    }
                    match names.get(c.tiles.as_str()) {
                        Some(NameKind::Tiles) => {}
                        Some(kind) => {
                            return compile_err(format!(
                                "`{c}`: `{}` is {}, not a `tiles` set",
                                c.tiles,
                                kind.describe()
                            ));
                        }
                        None => {
                            let hint = super::validate::suggest(
                                &c.tiles,
                                self.tiles_sets().map(|t| t.name.as_str()),
                            );
                            return compile_err(format!(
                                "`{c}`: no `tiles {}` set is declared{hint}",
                                c.tiles
                            ));
                        }
                    }
                    if !tiles_claimed.insert(&c.slug) {
                        return compile_err(format!(
                            "duplicate assignment `{}.tiles = …` — a device takes one \
                             tiles set",
                            c.slug
                        ));
                    }
                }
                Item::Access(a) => {
                    let (kw, ty_want, noun) = (
                        a.right.keyword(),
                        a.right.block_type(),
                        a.right.block_noun(),
                    );
                    let ctx = format!("`{kw} {}`", a.slug);
                    let what = match a.right {
                        RightKind::Access => "access rights with a schedule belong to",
                        RightKind::Recipients => "recipients belong to",
                    };
                    match names.get(a.slug.as_str()) {
                        // A managed block works as well: an adopted Code
                        // Touch keeps its rights in the source.
                        Some(kind @ (NameKind::Extern | NameKind::Block)) => {
                            let ty = if matches!(kind, NameKind::Extern) {
                                self.externs()
                                    .find(|e| e.slug == a.slug)
                                    .map_or("", |e| e.block_type.as_str())
                            } else {
                                self.blocks()
                                    .find(|b| b.slug == a.slug)
                                    .map_or("", |b| b.block_type.as_str())
                            };
                            if ty != ty_want {
                                return compile_err(format!(
                                    "{ctx}: {what} {noun}, a `{ty_want}` — `{}` is declared \
                                     as `{ty}`",
                                    a.slug
                                ));
                            }
                        }
                        Some(kind) => {
                            return compile_err(format!(
                                "{ctx}: `{}` is {} — {kw} is declared on an extern \
                                 (`extern {} = {ty_want}(…)`)",
                                a.slug,
                                kind.describe(),
                                a.slug
                            ));
                        }
                        None => {
                            let hint = super::validate::suggest(
                                &a.slug,
                                self.externs().map(|e| e.slug.as_str()),
                            );
                            return compile_err(format!(
                                "{ctx}: undeclared slug `{}`{hint} — declare the block as \
                                 `extern {} = {ty_want}(…)`",
                                a.slug, a.slug
                            ));
                        }
                    }
                    if !access_declared.insert(&a.slug) {
                        return compile_err(format!(
                            "{ctx} is declared twice — one declaration holds all {} of \
                             the block",
                            match a.right {
                                RightKind::Access => "access rights",
                                RightKind::Recipients => "recipients",
                            }
                        ));
                    }
                    let mut principals: BTreeSet<(PrincipalKind, &str)> = BTreeSet::new();
                    for g in a.grants() {
                        match (a.right, &g.schedule) {
                            (RightKind::Access, Some(s))
                                if !g.principal.is_empty() && !s.is_empty() => {}
                            (RightKind::Access, _) => {
                                return compile_err(format!(
                                    "{ctx}: `{g}` — name the {} and the access schedule by \
                                     their titles (`{} \"<title>\": \"<schedule>\"`)",
                                    g.kind.as_str(),
                                    g.kind.as_str()
                                ));
                            }
                            (RightKind::Recipients, None) if !g.principal.is_empty() => {}
                            (RightKind::Recipients, None) => {
                                return compile_err(format!(
                                    "{ctx}: `{g}` — name the {} by its title",
                                    g.kind.as_str()
                                ));
                            }
                            (RightKind::Recipients, Some(_)) => {
                                return compile_err(format!(
                                    "{ctx}: `{g}` — a recipient takes no schedule; write \
                                     `{} {}`",
                                    g.kind.as_str(),
                                    quote(&g.principal)
                                ));
                            }
                        }
                        if !principals.insert((g.kind, &g.principal)) {
                            return compile_err(format!(
                                "{ctx}: {} {} is listed twice",
                                g.kind.as_str(),
                                quote(&g.principal),
                            ));
                        }
                    }
                }
                Item::Schedule(t) => {
                    super::schedule::program(t)
                        .map_err(|m| Error::Compile(format!("`schedule {}`: {m}", t.name)))?;
                }
                Item::ScheduleClaim(c) => {
                    match names.get(c.slug.as_str()) {
                        Some(NameKind::Extern) => {
                            let ty = self
                                .externs()
                                .find(|e| e.slug == c.slug)
                                .map_or("", |e| e.block_type.as_str());
                            if ty != "DayTimer" {
                                return compile_err(format!(
                                    "`{c}`: a schedule belongs to a `DayTimer` — `{}` is \
                                     declared as `{ty}`",
                                    c.slug
                                ));
                            }
                        }
                        Some(NameKind::Block) => {
                            let ty = block_type(&c.slug);
                            if ty != "DayTimer" {
                                return compile_err(format!(
                                    "`{c}`: a schedule belongs to a `DayTimer` — `{}` is \
                                     a `{ty}`",
                                    c.slug
                                ));
                            }
                        }
                        None if !flattened => {}
                        Some(kind) => {
                            return compile_err(format!(
                                "`{c}`: `{}` is {} — a schedule is claimed on a DayTimer \
                                 (an extern or a managed block)",
                                c.slug,
                                kind.describe()
                            ));
                        }
                        None => {
                            return compile_err(format!(
                                "`{c}`: undeclared slug `{}` — declare the timer as \
                                 `extern {} = DayTimer(…)` or as a block",
                                c.slug, c.slug
                            ));
                        }
                    }
                    match names.get(c.schedule.as_str()) {
                        Some(NameKind::Schedule) => {}
                        Some(kind) => {
                            return compile_err(format!(
                                "`{c}`: `{}` is {}, not a `schedule`",
                                c.schedule,
                                kind.describe()
                            ));
                        }
                        None => {
                            let hint = super::validate::suggest(
                                &c.schedule,
                                self.schedules().map(|t| t.name.as_str()),
                            );
                            return compile_err(format!(
                                "`{c}`: no `schedule {}` is declared{hint}",
                                c.schedule
                            ));
                        }
                    }
                    if !schedules_claimed.insert(&c.slug) {
                        return compile_err(format!(
                            "duplicate assignment `{}.schedule = …` — a timer takes one \
                             schedule",
                            c.slug
                        ));
                    }
                }
                Item::Moods(t) => {
                    let mut names: BTreeSet<&str> = BTreeSet::new();
                    for m in t.moods() {
                        let ctx = format!("`moods {}`, mood {}", t.name, quote(&m.name));
                        if m.name.is_empty() {
                            return compile_err(format!("`moods {}`: a mood needs a name", t.name));
                        }
                        if !names.insert(&m.name) {
                            return compile_err(format!(
                                "{ctx}: listed twice — a controller's mood names are unique"
                            ));
                        }
                        let mut circuits: BTreeSet<Option<u8>> = BTreeSet::new();
                        for (i, setting) in m.settings.iter().enumerate() {
                            if setting.circuit.is_none() && i != 0 {
                                return compile_err(format!(
                                    "{ctx}: `{setting}` must come first — it sets every \
                                     circuit, and the settings after it name the exceptions"
                                ));
                            }
                            if !circuits.insert(setting.circuit) {
                                return compile_err(format!(
                                    "{ctx}: `{setting}` sets a circuit twice"
                                ));
                            }
                        }
                    }
                }
                Item::MoodsClaim(c) => {
                    let ty = match names.get(c.slug.as_str()) {
                        Some(NameKind::Extern) => self
                            .externs()
                            .find(|e| e.slug == c.slug)
                            .map_or("", |e| e.block_type.as_str()),
                        Some(NameKind::Block) => block_type(&c.slug),
                        None if !flattened => "LightController2",
                        Some(kind) => {
                            return compile_err(format!(
                                "`{c}`: `{}` is {} — moods are claimed on a \
                                 LightController2 (an extern or a managed block)",
                                c.slug,
                                kind.describe()
                            ));
                        }
                        None => {
                            return compile_err(format!(
                                "`{c}`: undeclared slug `{}` — declare the controller as \
                                 `extern {} = LightController2(…)` or as a block",
                                c.slug, c.slug
                            ));
                        }
                    };
                    if ty != "LightController2" {
                        return compile_err(format!(
                            "`{c}`: moods belong to a `LightController2` — `{}` is a `{ty}`",
                            c.slug
                        ));
                    }
                    match names.get(c.moods.as_str()) {
                        Some(NameKind::Moods) => {}
                        Some(kind) => {
                            return compile_err(format!(
                                "`{c}`: `{}` is {}, not a `moods` set",
                                c.moods,
                                kind.describe()
                            ));
                        }
                        None => {
                            let hint = super::validate::suggest(
                                &c.moods,
                                self.mood_sets().map(|t| t.name.as_str()),
                            );
                            return compile_err(format!(
                                "`{c}`: no `moods {}` is declared{hint}",
                                c.moods
                            ));
                        }
                    }
                    if !moods_claimed.insert(&c.slug) {
                        return compile_err(format!(
                            "duplicate assignment `{}.moods = …` — a controller takes one \
                             mood set",
                            c.slug
                        ));
                    }
                }
                Item::Rule(r) => {
                    let ctx = format!("`rule {}`", r.slug);
                    match (&r.title, names.get(r.slug.as_str())) {
                        // A titled rule names itself (D60): its slug is
                        // its own, like a managed block's.
                        (Some(title), None) => {
                            if title.trim().is_empty() {
                                return compile_err(format!(
                                    "{ctx}: the rule's name is empty — the app lists rules \
                                     by name"
                                ));
                            }
                            if !rule_titles.insert(title.as_str()) {
                                return compile_err(format!(
                                    "{ctx}: another rule is named {title:?} — the name is the \
                                     rule's identity"
                                ));
                            }
                        }
                        (Some(_), Some(kind)) => {
                            return compile_err(format!(
                                "{ctx}: `{}` is already {} — a named rule (`rule {} \"…\"`) \
                                 declares its own slug; for an existing rule's extern write \
                                 `rule {}` without the name",
                                r.slug,
                                kind.describe(),
                                r.slug,
                                r.slug
                            ));
                        }
                        (None, Some(NameKind::Extern)) => {
                            let ty = self
                                .externs()
                                .find(|e| e.slug == r.slug)
                                .map_or("", |e| e.block_type.as_str());
                            if ty != RULE_BLOCK_TYPE {
                                return compile_err(format!(
                                    "{ctx}: a rule belongs to an Automatic Designer rule, a \
                                     `{RULE_BLOCK_TYPE}` — `{}` is declared as `{ty}`",
                                    r.slug
                                ));
                            }
                        }
                        (None, Some(kind)) => {
                            return compile_err(format!(
                                "{ctx}: `{}` is {} — a rule is declared on an extern \
                                 (`extern {} = {RULE_BLOCK_TYPE}(…)`)",
                                r.slug,
                                kind.describe(),
                                r.slug
                            ));
                        }
                        (None, None) => {
                            let hint = super::validate::suggest(
                                &r.slug,
                                self.externs().map(|e| e.slug.as_str()),
                            );
                            return compile_err(format!(
                                "{ctx}: undeclared slug `{}`{hint} — declare the rule as \
                                 `extern {} = {RULE_BLOCK_TYPE}(title: \"…\")`, or name a \
                                 new one: `rule {} \"Name\"`",
                                r.slug, r.slug, r.slug
                            ));
                        }
                    }
                    if !rules_declared.insert(&r.slug) {
                        return compile_err(format!(
                            "{ctx} is declared twice — one declaration holds the whole rule"
                        ));
                    }
                    let (mut active, mut when, mut then) = (0, 0, 0);
                    for st in r.stmts() {
                        match &st.kind {
                            RuleStmtKind::Active(_) => active += 1,
                            RuleStmtKind::Time(_) => when += 1,
                            RuleStmtKind::Weekday(days) => {
                                when += 1;
                                let mut seen = BTreeSet::new();
                                for d in days {
                                    let Some(ix) = super::rule::weekdays(d) else {
                                        return compile_err(format!(
                                            "{ctx}: unknown weekday `{d}` — days are mon, tue, \
                                             wed, thu, fri, sat, sun, weekdays, weekend, daily"
                                        ));
                                    };
                                    if ix.iter().any(|i| !seen.insert(*i)) {
                                        return compile_err(format!(
                                            "{ctx}: `{}` names a day twice",
                                            st.kind
                                        ));
                                    }
                                }
                            }
                            RuleStmtKind::Then { target, command } => {
                                then += 1;
                                if command.is_empty() {
                                    return compile_err(format!(
                                        "{ctx}: `{}` — the command is the app's command text \
                                         (`on`, `off`, `pulse`, …)",
                                        st.kind
                                    ));
                                }
                                match names.get(target.as_str()) {
                                    Some(NameKind::Extern | NameKind::Block) => {}
                                    None if !flattened => {}
                                    Some(kind) => {
                                        return compile_err(format!(
                                            "{ctx}: `{}` — `{target}` is {}; a rule sends its \
                                             command to a block (an extern or a managed one)",
                                            st.kind,
                                            kind.describe()
                                        ));
                                    }
                                    None => {
                                        return compile_err(format!(
                                            "{ctx}: `{}` — undeclared slug `{target}`",
                                            st.kind
                                        ));
                                    }
                                }
                            }
                        }
                    }
                    if active != 1 {
                        return compile_err(format!(
                            "{ctx}: say once whether the rule is `active` or `inactive` \
                             ({active} lines do)"
                        ));
                    }
                    if when == 0 || then == 0 {
                        return compile_err(format!(
                            "{ctx}: a rule needs at least one `when` condition and one \
                             `then` action"
                        ));
                    }
                }
                Item::Page(p) if p.title.is_empty() => {
                    return compile_err("`page \"\"`: the page title must not be empty".into());
                }
                _ => {}
            }
        }

        // Lifecycle statements must not contradict declarations or each
        // other.
        let mut removed_seen: BTreeMap<&str, ()> = BTreeMap::new();
        for r in self.removed() {
            if let Some(kind) = names.get(r.slug.as_str()) {
                return compile_err(format!(
                    "`removed {slug}` contradicts the declaration of {} `{slug}` — \
                     delete the `removed` line or the declaration",
                    kind.describe(),
                    slug = r.slug,
                ));
            }
            if removed_seen.insert(&r.slug, ()).is_some() {
                return compile_err(format!("duplicate `removed {}`", r.slug));
            }
        }
        let mut moved_from: BTreeMap<&str, ()> = BTreeMap::new();
        let mut moved_to: BTreeMap<&str, ()> = BTreeMap::new();
        for m in self.moved() {
            if m.from == m.to {
                return compile_err(format!("`moved {0} -> {0}` moves a slug to itself", m.from));
            }
            if let Some(kind) = names.get(m.from.as_str()) {
                return compile_err(format!(
                    "`moved {from} -> {to}` conflicts with the declaration of {} \
                     `{from}` — the old slug must no longer be declared",
                    kind.describe(),
                    from = m.from,
                    to = m.to,
                ));
            }
            if matches!(
                names.get(m.to.as_str()),
                Some(NameKind::Extern | NameKind::Let)
            ) {
                return compile_err(format!(
                    "`moved {} -> {to}` — `{to}` is not a managed block",
                    m.from,
                    to = m.to,
                ));
            }
            if moved_from.insert(&m.from, ()).is_some() {
                return compile_err(format!("duplicate `moved` from `{}`", m.from));
            }
            if moved_to.insert(&m.to, ()).is_some() {
                return compile_err(format!("duplicate `moved` to `{}`", m.to));
            }
            if removed_seen.contains_key(m.from.as_str())
                || removed_seen.contains_key(m.to.as_str())
            {
                return compile_err(format!(
                    "`moved {} -> {}` conflicts with a `removed` of the same slug",
                    m.from, m.to
                ));
            }
        }
        // No chains: a move's target must not be another move's source.
        for m in self.moved() {
            if moved_from.contains_key(m.to.as_str()) {
                return compile_err(format!(
                    "chained `moved` through `{}` — collapse into one `moved` statement",
                    m.to
                ));
            }
        }
        Ok(())
    }

    /// Canonical text form. `parse(to_text(m)) == m`.
    pub fn to_text(&self) -> String {
        // Blank lines separate statement families; plain and expression
        // wires are one family (both read `sink <- …`).
        fn family(item: &Item) -> u8 {
            match item {
                Item::Extern(_) => 0,
                Item::Block(_) => 1,
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
        for item in &self.items {
            let disc = family(item);
            if prev.is_some_and(|p| p != disc) || prev_multiline {
                out.push('\n');
            }
            prev = Some(disc);
            prev_multiline = false;
            let tail =
                |c: &Option<String>| c.as_ref().map(|t| format!(" #{t}")).unwrap_or_default();
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
                                ArgItem::Binding(x) => out.push_str(&format!(
                                    "\t{}: {},{}\n",
                                    x.port,
                                    x.kind,
                                    tail(&x.comment)
                                )),
                                ArgItem::Comment(text) => {
                                    out.push_str(&format!("\t#{text}\n"));
                                }
                            }
                        }
                        out.push(')');
                        out.push_str(&tail(&b.close_comment));
                        prev_multiline = true;
                    }
                    out.push('\n');
                }
                Item::Wire(w) => {
                    out.push_str(&format!("{} <- {}{}\n", w.to, w.from, tail(&w.comment)));
                }
                Item::ExprWire(w) => {
                    out.push_str(&format!("{} <- {}{}\n", w.to, w.expr, tail(&w.comment)));
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
                    prev_multiline = true;
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
                    prev_multiline = true;
                }
                Item::Tiles(t) => {
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
                    prev_multiline = true;
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
                    prev_multiline = true;
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
                    prev_multiline = true;
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
                    prev_multiline = true;
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
                    prev_multiline = true;
                }
                Item::Comment(text) => {
                    out.push_str(&format!("#{text}\n"));
                }
            }
        }
        out
    }
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
