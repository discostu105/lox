//! Hand-rolled parser for the Lox Logic text format. Line-oriented; only a block
//! declaration's `( … )` argument list spans lines.

use super::ast::*;
use crate::error::{Error, Result};

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Ident(String),
    Str(String),
    Num(String),
    /// A number with a unit suffix (`40s`, `1.5h`).
    UnitNum(String, Unit),
    Colon,
    Dot,
    Comma,
    Arrow,
    LArrow,
    Eq,
    LParen,
    RParen,
    LBrace,
    RBrace,
    /// Comparison operators (expression sugar).
    Cmp(CmpOp),
    /// `~=` — approximately-equal, valid only in a test `expect`.
    Approx,
    /// Arithmetic operators (formula backend). `-` is only lexed as
    /// binary minus after a token that can end an operand — everywhere
    /// else it starts a negative number.
    Plus,
    Minus,
    Star,
    Slash,
}

fn lex(line: &str, lineno: usize) -> Result<(Vec<Tok>, Option<String>)> {
    let err = |msg: String| Error::Parse { line: lineno, msg };
    let mut toks = Vec::new();
    let mut comment = None;
    let mut chars = line.chars().peekable();
    while let Some(&c) = chars.peek() {
        match c {
            '#' => {
                chars.next();
                comment = Some(chars.collect::<String>());
                break;
            }
            c if c.is_whitespace() => {
                chars.next();
            }
            ':' => {
                chars.next();
                toks.push(Tok::Colon);
            }
            '.' => {
                chars.next();
                toks.push(Tok::Dot);
            }
            ',' => {
                chars.next();
                toks.push(Tok::Comma);
            }
            '=' => {
                chars.next();
                if chars.peek() == Some(&'=') {
                    chars.next();
                    toks.push(Tok::Cmp(CmpOp::Eq));
                } else {
                    toks.push(Tok::Eq);
                }
            }
            '>' => {
                chars.next();
                if chars.peek() == Some(&'=') {
                    chars.next();
                    toks.push(Tok::Cmp(CmpOp::Ge));
                } else {
                    toks.push(Tok::Cmp(CmpOp::Gt));
                }
            }
            '!' => {
                chars.next();
                if chars.peek() == Some(&'=') {
                    chars.next();
                    toks.push(Tok::Cmp(CmpOp::Ne));
                } else {
                    return Err(err("unexpected `!` (not-equal is `!=`)".into()));
                }
            }
            '~' => {
                chars.next();
                if chars.peek() == Some(&'=') {
                    chars.next();
                    toks.push(Tok::Approx);
                } else {
                    return Err(err("unexpected `~` (approximately-equal is `~=`)".into()));
                }
            }
            '+' => {
                chars.next();
                toks.push(Tok::Plus);
            }
            '*' => {
                chars.next();
                toks.push(Tok::Star);
            }
            '/' => {
                chars.next();
                toks.push(Tok::Slash);
            }
            '(' => {
                chars.next();
                toks.push(Tok::LParen);
            }
            ')' => {
                chars.next();
                toks.push(Tok::RParen);
            }
            '{' => {
                chars.next();
                toks.push(Tok::LBrace);
            }
            '}' => {
                chars.next();
                toks.push(Tok::RBrace);
            }
            '<' => {
                chars.next();
                // `<-` wins over `<` followed by a negative number: write
                // `x < -5` with a space (canonical form) to compare.
                match chars.peek() {
                    Some(&'-') => {
                        chars.next();
                        toks.push(Tok::LArrow);
                    }
                    Some(&'=') => {
                        chars.next();
                        toks.push(Tok::Cmp(CmpOp::Le));
                    }
                    _ => toks.push(Tok::Cmp(CmpOp::Lt)),
                }
            }
            '-' => {
                chars.next();
                if chars.peek() == Some(&'>') {
                    chars.next();
                    toks.push(Tok::Arrow);
                } else if matches!(
                    toks.last(),
                    Some(Tok::Ident(_) | Tok::Num(_) | Tok::UnitNum(..) | Tok::RParen)
                ) {
                    // Binary minus: only after something that can end an
                    // operand (`a.AQ - 5`, `(x + 1) - y.AQ`). After `:`,
                    // `=`, a comparison etc. `-` keeps starting a negative
                    // number (`SSoff: -30`, `x < -5`).
                    toks.push(Tok::Minus);
                } else {
                    // negative number
                    let mut s = String::from('-');
                    while let Some(&d) = chars.peek() {
                        if d.is_ascii_digit() || d == '.' {
                            s.push(d);
                            chars.next();
                        } else {
                            break;
                        }
                    }
                    if s == "-" {
                        return Err(err("unexpected `-`".into()));
                    }
                    toks.push(number(s, &mut chars, lineno)?);
                }
            }
            '"' => {
                chars.next();
                let mut s = String::new();
                loop {
                    match chars.next() {
                        Some('"') => break,
                        Some('\\') => match chars.next() {
                            Some('"') => s.push('"'),
                            Some('\\') => s.push('\\'),
                            Some('n') => s.push('\n'),
                            other => {
                                return Err(err(format!("invalid escape `\\{}`", opt(other))));
                            }
                        },
                        Some(c) => s.push(c),
                        None => return Err(err("unterminated string".into())),
                    }
                }
                toks.push(Tok::Str(s));
            }
            c if c.is_ascii_digit() => {
                let mut s = String::new();
                while let Some(&d) = chars.peek() {
                    if d.is_ascii_digit() || d == '.' {
                        s.push(d);
                        chars.next();
                    } else {
                        break;
                    }
                }
                toks.push(number(s, &mut chars, lineno)?);
            }
            c if c.is_alphanumeric() || c == '_' => {
                let mut s = String::new();
                while let Some(&d) = chars.peek() {
                    if d.is_alphanumeric() || d == '_' {
                        s.push(d);
                        chars.next();
                    } else {
                        break;
                    }
                }
                toks.push(Tok::Ident(s));
            }
            other => return Err(err(format!("unexpected character `{other}`"))),
        }
    }
    Ok((join_scoped(toks), comment))
}

/// `instance.block.Port`: the scoped name `instance.block` becomes
/// one identifier, so every `<slug>.<Port>` form takes it unchanged. Only
/// a lowercase pair followed by another `.` joins — `a.b` alone stays a
/// port reference, and the forms that take a bare scoped name
/// (`removed a.b`, `to: a.b`, …) join it themselves.
fn join_scoped(toks: Vec<Tok>) -> Vec<Tok> {
    let lower =
        |t: &Tok| matches!(t, Tok::Ident(s) if s.starts_with(|c: char| c.is_ascii_lowercase()));
    let mut out: Vec<Tok> = Vec::with_capacity(toks.len());
    let mut i = 0;
    while i < toks.len() {
        if lower(&toks[i])
            && toks.get(i + 1) == Some(&Tok::Dot)
            && toks.get(i + 2).is_some_and(lower)
            && toks.get(i + 3) == Some(&Tok::Dot)
            && out.last() != Some(&Tok::Dot)
        {
            let (Tok::Ident(a), Tok::Ident(b)) = (&toks[i], &toks[i + 2]) else {
                unreachable!("guarded")
            };
            out.push(Tok::Ident(format!("{a}.{b}")));
            i += 3;
            continue;
        }
        out.push(toks[i].clone());
        i += 1;
    }
    out
}

/// `instance.block`, a body block of a template instance named in a
/// position that takes an object.
fn scoped(a: &str, b: &str, lineno: usize) -> Result<String> {
    Ok(format!(
        "{}.{}",
        check_slug(a, lineno)?,
        check_slug(b, lineno)?
    ))
}

/// A slug or a scoped `instance.block`: a name that may be part of
/// a reference.
fn check_ref(s: &str, lineno: usize) -> Result<String> {
    match s.split_once('.') {
        Some((a, b)) => scoped(a, b, lineno),
        None => check_slug(s, lineno),
    }
}

/// A lexed digit-and-dot run must be exactly `-?digits(.digits)?`. An
/// immediately following letter run or `%` is a unit suffix:
/// `40s`, `1.5h`, `2700K`, `70%`.
fn number(
    s: String,
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
    lineno: usize,
) -> Result<Tok> {
    let err = |msg: String| Error::Parse { line: lineno, msg };
    if !is_number_literal(&s) {
        return Err(err(format!(
            "invalid number literal `{s}` (expected digits with at most one `.`)"
        )));
    }
    let mut suffix = String::new();
    if chars.peek() == Some(&'%') {
        chars.next();
        suffix.push('%');
    } else {
        while let Some(&c) = chars.peek() {
            if c.is_ascii_alphabetic() {
                suffix.push(c);
                chars.next();
            } else {
                break;
            }
        }
    }
    if suffix.is_empty() {
        return Ok(Tok::Num(s));
    }
    let Some(unit) = Unit::parse(&suffix) else {
        return Err(err(format!(
            "unknown unit `{suffix}` on `{s}{suffix}` (known units: ms, s, min, h, K, %)"
        )));
    };
    if scale_by_unit(&s, unit).is_none() {
        return Err(err(format!("`{s}{suffix}` is too large to scale exactly")));
    }
    Ok(Tok::UnitNum(s, unit))
}

fn opt(c: Option<char>) -> String {
    c.map(String::from).unwrap_or_else(|| "<eol>".into())
}

/// The statement sink: an open `template` body, or the module itself.
fn sink<'a>(
    open_template: &'a mut Option<TemplateDecl>,
    items: &'a mut Vec<Item>,
) -> &'a mut Vec<Item> {
    match open_template {
        Some(t) => &mut t.body,
        None => items,
    }
}

/// A closed `tile <i>(…)` argument list becomes the tile.
fn finish_tile(call: BlockDecl, index: u32, lineno: usize) -> Result<TileDecl> {
    if call.title.is_some() {
        return Err(Error::Parse {
            line: lineno,
            msg: format!(
                "`tile {index}(…)` takes no label string. Set the status text with `text: \"…\"`."
            ),
        });
    }
    // `to: licht.lc` names a body block of a template instance: an
    // object, written like a port reference.
    let mut args = call.args;
    for arg in &mut args {
        if let ArgItem::Binding(b) = arg
            && (b.port == "to" || b.port.ends_with("_to"))
            && let BindingKind::Wire(r) = &b.kind
            && r.port.starts_with(|c: char| c.is_ascii_lowercase())
        {
            b.kind = BindingKind::Param(Value::Ref(scoped(&r.slug, &r.port, lineno)?));
        }
    }
    Ok(TileDecl {
        index,
        args,
        comment: call.comment,
        close_comment: call.close_comment,
    })
}

/// A closed `defaults <Type>(…)` argument list: values only, each port
/// once, at least one.
fn finish_defaults(call: BlockDecl, lineno: usize) -> Result<DefaultsDecl> {
    let ty = &call.block_type;
    let err = |msg: String| Error::Parse { line: lineno, msg };
    if call.title.is_some() {
        return Err(err(format!(
            "`defaults {ty}(…)` takes no label string: a label names one block."
        )));
    }
    let mut seen = std::collections::BTreeSet::new();
    for b in call.bindings() {
        let p = &b.port;
        match &b.kind {
            BindingKind::Param(Value::Ref(_))
                if crate::periphery::is_device_binding(ty, p)
                    || (p == "mirrors" && super::validate::is_ref_type(ty)) =>
            {
                return Err(err(format!(
                    "`defaults {ty}(…)`: `{p}:` says which object a block is, not a value. \
                     Write it on each block."
                )));
            }
            BindingKind::Param(_) => {}
            BindingKind::Wire(_) | BindingKind::Expr(_) => {
                return Err(err(format!(
                    "`defaults {ty}(…)`: `{p}: {}` is not a value. A default is a value \
                     (a number, string, `on`/`off`, a duration or a constant); wire the port on \
                     each block.",
                    b.kind
                )));
            }
        }
        if !seen.insert(p.as_str()) {
            return Err(err(format!(
                "`defaults {ty}(…)` binds `{p}` twice. Keep one of them."
            )));
        }
    }
    if seen.is_empty() {
        return Err(err(format!(
            "`defaults {ty}()` sets nothing. Give it `<Port>: <value>` arguments, or delete it."
        )));
    }
    Ok(DefaultsDecl {
        block_type: call.block_type,
        args: call.args,
        comment: call.comment,
        close_comment: call.close_comment,
    })
}

/// A closed call becomes a block declaration, or a template instantiation
/// when the callee is lowercase.
fn finish_call(mut call: BlockDecl, in_template: bool, lineno: usize) -> Result<Item> {
    let lower = call
        .block_type
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_lowercase());
    if !lower {
        // `mirrors: inst.block` names a block of a template instance: an
        // object, written like a port reference (as a tile's `to:`).
        for arg in &mut call.args {
            if let ArgItem::Binding(b) = arg
                && b.port == "mirrors"
                && let BindingKind::Wire(r) = &b.kind
                && r.port.starts_with(|c: char| c.is_ascii_lowercase())
            {
                b.kind = BindingKind::Param(Value::Ref(scoped(&r.slug, &r.port, lineno)?));
            }
        }
        return Ok(Item::Block(call));
    }
    let err = |msg: String| Error::Parse { line: lineno, msg };
    if in_template {
        return Err(err(format!(
            "`{} = {}(…)`: a template body cannot instantiate another template",
            call.slug, call.block_type
        )));
    }
    if call.title.is_some() {
        return Err(err(format!(
            "`{} = {}(…)`: an instantiation takes no label string. Put titles on the template's blocks.",
            call.slug, call.block_type
        )));
    }
    Ok(Item::Instance(call))
}

/// Every statement kind a line may start with, for the parse error of a
/// line that starts with none of them.
const STATEMENTS: &str = "expected a statement: `let <name> = <value>`, \
     `extern <slug> = <Type>(…)`, `<slug> = <Type>(…)` (a block), \
     `<slug> = <template>(…)` (an instance), `<slug> = <expression>` (a named \
     signal), `<slug>.<Port> <- <source>`, \
     `<slug>.<Port> = <value>`, `<slug>.title|room|category = <value>`, \
     `(<slug>, …).<Port> = <value>`, `<slug>.tiles|schedule|moods|access|recipients = <name>`, \
     `page \"<title>\"`, `template <name>(…)`, `test \"<name>\"`, `tiles <name>`, \
     `schedule <name>`, `moods <name>`, `rule <slug>`, `access <name>`, \
     `recipients <name>`, `defaults <Type>(…)`, `removed <slug>`, or \
     `moved <old> -> <new>`";

pub fn parse(src: &str) -> Result<Module> {
    let mut parser = Parser::default();
    for (i, raw) in src.lines().enumerate() {
        let lineno = i + 1;
        let (toks, comment) = lex(raw, lineno)?;
        parser.line(&toks, comment, lineno)?;
    }
    parser.finish(src.lines().count())
}

/// The parser's state between lines: the items so far and the bodies
/// still open.
#[derive(Default)]
struct Parser {
    items: Vec<Item>,
    /// A block declaration whose `( … )` argument list is still open.
    open_call: Option<BlockDecl>,
    /// A `template` whose body is still open (until `end`).
    open_template: Option<TemplateDecl>,
    /// A `test` whose body is still open (until `end`).
    open_test: Option<TestDecl>,
    /// A named set whose body is still open (until `end`).
    open_set: Option<OpenSet>,
    /// The index of the `tile` whose argument list `open_call` carries.
    open_tile: Option<u32>,
    /// `open_call` carries a `defaults <Type>(…)` argument list.
    open_defaults: bool,
    /// A `rule` whose body is still open (until `end`).
    open_rule: Option<RuleDecl>,
}

/// A named set being parsed: every kind opens with `<kind> <name>` and
/// closes with `end`; only its body lines differ.
enum OpenSet {
    Tiles(TilesDecl),
    Schedule(ScheduleDecl),
    Moods(MoodsDecl),
    Access(AccessDecl),
}

impl OpenSet {
    /// `<kind> <name>` opens an empty set.
    fn open(kind: SetKind, name: String, comment: Option<String>) -> OpenSet {
        match kind {
            SetKind::Tiles => OpenSet::Tiles(TilesDecl {
                name,
                body: Vec::new(),
                comment,
                end_comment: None,
            }),
            SetKind::Schedule => OpenSet::Schedule(ScheduleDecl {
                name,
                body: Vec::new(),
                comment,
                end_comment: None,
            }),
            SetKind::Moods => OpenSet::Moods(MoodsDecl {
                name,
                body: Vec::new(),
                comment,
                end_comment: None,
            }),
            SetKind::Access | SetKind::Recipients => OpenSet::Access(AccessDecl {
                right: kind.right().expect("a rights kind"),
                name,
                body: Vec::new(),
                comment,
                end_comment: None,
            }),
        }
    }

    /// `<kind> <name>`, for messages.
    fn head(&self) -> String {
        let (kind, name) = match self {
            OpenSet::Tiles(t) => ("tiles", &t.name),
            OpenSet::Schedule(t) => ("schedule", &t.name),
            OpenSet::Moods(t) => ("moods", &t.name),
            OpenSet::Access(a) => (a.right.keyword(), &a.name),
        };
        format!("{kind} {name}")
    }

    /// A whole-line comment inside the body.
    fn comment(&mut self, text: String) {
        match self {
            OpenSet::Tiles(t) => t.body.push(TilesItem::Comment(text)),
            OpenSet::Schedule(t) => t.body.push(ScheduleItem::Comment(text)),
            OpenSet::Moods(t) => t.body.push(MoodsItem::Comment(text)),
            OpenSet::Access(a) => a.body.push(AccessItem::Comment(text)),
        }
    }

    /// The finished set, with the comment of its `end` line.
    fn close(self, end_comment: Option<String>) -> Item {
        match self {
            OpenSet::Tiles(mut t) => {
                t.end_comment = end_comment;
                Item::Tiles(t)
            }
            OpenSet::Schedule(mut t) => {
                t.end_comment = end_comment;
                Item::Schedule(t)
            }
            OpenSet::Moods(mut t) => {
                t.end_comment = end_comment;
                Item::Moods(t)
            }
            OpenSet::Access(mut a) => {
                a.end_comment = end_comment;
                Item::Access(a)
            }
        }
    }
}

/// `<kind> <name>` — a line that opens a named set. Each kind is a
/// keyword only there: followed by `=`, `.` or `<-` it is a statement
/// about an object of that name (`access = Switch()`), which the reserved
/// kinds then refuse as a name.
fn set_head(toks: &[Tok]) -> Option<SetKind> {
    match toks {
        [Tok::Ident(kw), next, ..] if !matches!(next, Tok::Eq | Tok::Dot | Tok::LArrow) => {
            SetKind::parse(kw)
        }
        [Tok::Ident(kw)] => SetKind::parse(kw),
        _ => None,
    }
}

impl Parser {
    /// One source line, lexed.
    fn line(&mut self, toks: &[Tok], comment: Option<String>, lineno: usize) -> Result<()> {
        if self.open_call.is_some() {
            return self.call_line(toks, comment, lineno);
        }
        if toks.is_empty() {
            self.comment_line(comment);
            return Ok(());
        }
        if self.in_body() {
            return self.body_line(toks, comment, lineno);
        }
        self.refuse_in_template(toks, lineno)?;
        self.statement(toks, comment, lineno)
    }

    /// Inside an open argument list: arguments, comments, or `)`.
    fn call_line(&mut self, toks: &[Tok], comment: Option<String>, lineno: usize) -> Result<()> {
        let call = self.open_call.as_mut().expect("an argument list is open");
        if toks.is_empty() {
            if let Some(text) = comment {
                call.args.push(ArgItem::Comment(text));
            }
            return Ok(());
        }
        let outcome = parse_call_args(call, toks, lineno)?;
        if outcome.closed {
            call.close_comment = comment;
            if let Some(index) = self.open_tile.take() {
                let tile = finish_tile(self.open_call.take().unwrap(), index, lineno)?;
                let Some(OpenSet::Tiles(tiles)) = self.open_set.as_mut() else {
                    unreachable!("a tile is open only in a tiles set");
                };
                tiles.body.push(TilesItem::Tile(tile));
                return Ok(());
            }
            if std::mem::take(&mut self.open_defaults) {
                let d = finish_defaults(self.open_call.take().unwrap(), lineno)?;
                self.items.push(Item::Defaults(d));
                return Ok(());
            }
            let in_template = self.open_template.is_some();
            let item = finish_call(self.open_call.take().unwrap(), in_template, lineno)?;
            sink(&mut self.open_template, &mut self.items).push(item);
        } else if let Some(text) = comment {
            match call.args.last_mut() {
                Some(ArgItem::Binding(b)) if outcome.pushed_binding => b.comment = Some(text),
                _ => call.args.push(ArgItem::Comment(text)),
            }
        }
        Ok(())
    }

    /// Blank line, or a whole-line comment — kept verbatim as an item so
    /// formatting is non-destructive.
    fn comment_line(&mut self, comment: Option<String>) {
        if let Some(text) = comment {
            if let Some(set) = self.open_set.as_mut() {
                set.comment(text);
                return;
            }
            if let Some(r) = self.open_rule.as_mut() {
                r.body.push(RuleItem::Comment(text));
                return;
            }
            match self.open_test.as_mut() {
                Some(t) => t.body.push(TestItem::Comment(text)),
                None => sink(&mut self.open_template, &mut self.items).push(Item::Comment(text)),
            }
        }
    }

    /// Whether a named set, a `rule` or a `test` body is open.
    fn in_body(&self) -> bool {
        self.open_set.is_some() || self.open_rule.is_some() || self.open_test.is_some()
    }

    /// A line inside an open body: one of the body's lines, or `end`.
    fn body_line(&mut self, toks: &[Tok], comment: Option<String>, lineno: usize) -> Result<()> {
        let is_end = matches!(toks, [Tok::Ident(kw)] if kw == "end");
        if self.open_set.is_some() {
            if is_end {
                let set = self.open_set.take().unwrap();
                self.items.push(set.close(comment));
                return Ok(());
            }
            return self.set_line(toks, comment, lineno);
        }

        // Inside a `rule` body every line is a rule statement (or `end`).
        if let Some(r) = self.open_rule.as_mut() {
            if is_end {
                let mut r = self.open_rule.take().unwrap();
                r.end_comment = comment;
                self.items.push(Item::Rule(r));
            } else {
                r.body
                    .push(RuleItem::Stmt(parse_rule_stmt(toks, comment, lineno)?));
            }
            return Ok(());
        }

        // Inside a `test` body every line is a test statement (or `end`).
        if let Some(t) = self.open_test.as_mut() {
            if is_end {
                let mut t = self.open_test.take().unwrap();
                t.end_comment = comment;
                self.items.push(Item::Test(t));
            } else {
                t.body.push(parse_test_stmt(toks, comment, lineno)?);
            }
            return Ok(());
        }
        unreachable!("body_line runs only inside an open body")
    }

    /// One line of an open named set, other than `end`: a schedule line, a
    /// mood, a `tile`, or a grant.
    fn set_line(&mut self, toks: &[Tok], comment: Option<String>, lineno: usize) -> Result<()> {
        let err = |msg: String| Error::Parse { line: lineno, msg };
        match self.open_set.as_mut().expect("a set body is open") {
            OpenSet::Schedule(t) => {
                t.body.push(ScheduleItem::Line(parse_schedule_line(
                    toks, comment, lineno,
                )?));
            }
            OpenSet::Moods(m) => {
                m.body
                    .push(MoodsItem::Mood(parse_mood_line(toks, comment, lineno)?));
            }
            OpenSet::Tiles(t) => match toks {
                [Tok::Ident(kw), Tok::Num(n), Tok::LParen, rest @ ..] if kw == "tile" => {
                    let index = match n.parse::<u32>() {
                        Ok(i) if i > 0 => i,
                        _ => {
                            return Err(err(format!(
                                "`tile {n}`: a tile index is a positive whole number (the tile's `i=` in the config)"
                            )));
                        }
                    };
                    let mut call = BlockDecl {
                        slug: "tile".into(),
                        block_type: n.clone(),
                        title: None,
                        args: Vec::new(),
                        comment: None,
                        close_comment: None,
                    };
                    let outcome = parse_call_args(&mut call, rest, lineno)?;
                    call.comment = comment;
                    if outcome.closed {
                        t.body
                            .push(TilesItem::Tile(finish_tile(call, index, lineno)?));
                    } else {
                        self.open_call = Some(call);
                        self.open_tile = Some(index);
                    }
                }
                _ => {
                    return Err(err(format!(
                        "expected `tile <i>(click: \"…\", up: \"…\", down: \"…\", \
                             text: \"…\", to: <slug>, …)` or `end` in `tiles {}`",
                        t.name
                    )));
                }
            },
            OpenSet::Access(a) => match toks {
                [Tok::Ident(kw), Tok::Str(principal), rest @ ..]
                    if (kw == "user" || kw == "group")
                        && matches!(rest, [] | [Tok::Colon, Tok::Str(_)]) =>
                {
                    a.body.push(AccessItem::Grant(AccessGrant {
                        kind: if kw == "user" {
                            PrincipalKind::User
                        } else {
                            PrincipalKind::Group
                        },
                        principal: principal.clone(),
                        schedule: match rest {
                            [_, Tok::Str(schedule)] => Some(schedule.clone()),
                            _ => None,
                        },
                        comment,
                    }));
                }
                _ => {
                    return Err(err(match a.right {
                        RightKind::Access => format!(
                            "expected `user \"<title>\": \"<schedule>\"`, \
                                 `group \"<title>\": \"<schedule>\"` or `end` in `access {}`",
                            a.name
                        ),
                        RightKind::Recipients => format!(
                            "expected `user \"<title>\"`, `group \"<title>\"` or `end` in \
                                 `recipients {}`",
                            a.name
                        ),
                    }));
                }
            },
        }
        Ok(())
    }

    /// Module-level statements are refused inside a template body.
    fn refuse_in_template(&self, toks: &[Tok], lineno: usize) -> Result<()> {
        let err = |msg: String| Error::Parse { line: lineno, msg };
        if self.open_template.is_none() {
            return Ok(());
        }
        // `test = Not()` in a body is a reserved name, as at the top level.
        if let [Tok::Ident(name), Tok::Eq, ..] = toks {
            check_slug(name, lineno)?;
        }
        // A named set is declared once, at the top level; a body claims one
        // through a set parameter.
        if let Some(kind) = set_head(toks) {
            let kw = kind.keyword();
            return Err(err(format!(
                "`{kw}` sets are declared at the top level, not inside a template body. \
                 Pass one to the template with a set parameter (`<name>: {kw}`) and claim \
                 it in the body (`<block>.{kw} = <name>`)."
            )));
        }
        if let Tok::Ident(kw) = &toks[0]
            && (matches!(
                kw.as_str(),
                "let"
                    | "extern"
                    | "removed"
                    | "moved"
                    | "page"
                    | "test"
                    | "tick"
                    | "expect"
                    | "clock"
                    | "tile"
            ) || (matches!(kw.as_str(), "rule" | "defaults")
                && matches!(toks.get(1), Some(Tok::Ident(_)))))
        {
            if kw == "defaults" {
                return Err(err(
                    "`defaults` is a top-level statement: it applies to the blocks of the whole \
                     module, template bodies included. Move it out of the template, or give the \
                     template a parameter with a default value."
                        .into(),
                ));
            }
            return Err(err(format!(
                "`{kw}` is not allowed inside a template body. A body holds only block declarations, named signals, wires, port assignments and comments."
            )));
        }
        Ok(())
    }

    /// A statement outside any body: a body opener, a declaration, or a
    /// statement about an object.
    fn statement(&mut self, toks: &[Tok], comment: Option<String>, lineno: usize) -> Result<()> {
        let err = |msg: String| Error::Parse { line: lineno, msg };
        // `tick = …`: a declaration named by a reserved word says so,
        // whatever the word would start elsewhere.
        if let [Tok::Ident(name), Tok::Eq, ..] = toks {
            check_slug(name, lineno)?;
        }
        match &toks[0] {
            Tok::Ident(kw) if kw == "template" => {
                if self.open_template.is_some() {
                    return Err(err(
                        "nested `template`. Close the open one with `end`.".into()
                    ));
                }
                self.open_template = Some(parse_template_head(toks, comment, lineno)?);
            }
            Tok::Ident(kw) if kw == "test" => {
                if self.open_template.is_some() {
                    return Err(err(
                        "`test` is not allowed inside a template body. Close the template with `end` first."
                            .into(),
                    ));
                }
                let [_, Tok::Str(name)] = toks else {
                    return Err(err("expected `test \"<name>\"`".into()));
                };
                self.open_test = Some(TestDecl {
                    name: name.clone(),
                    body: Vec::new(),
                    comment,
                    end_comment: None,
                });
            }
            // `<kind> <name>` opens a named set, the same way for every kind.
            Tok::Ident(_) if set_head(toks).is_some() => {
                let kind = set_head(toks).expect("guarded");
                self.open_set = Some(parse_set_head(kind, toks, comment, lineno)?);
            }
            // `rule` is a keyword only in front of a slug, like the set kinds.
            Tok::Ident(kw) if kw == "rule" && matches!(toks.get(1), Some(Tok::Ident(_))) => {
                self.open_rule = Some(parse_rule_head(toks, comment, lineno)?);
            }
            // `defaults` is a keyword only in front of a type, like `rule`.
            Tok::Ident(kw) if kw == "defaults" && matches!(toks.get(1), Some(Tok::Ident(_))) => {
                let [_, Tok::Ident(ty), Tok::LParen, rest @ ..] = toks else {
                    return Err(err("expected `defaults <Type>(<Port>: <value>, …)`".into()));
                };
                if ty.starts_with(|c: char| c.is_ascii_lowercase()) {
                    return Err(err(format!(
                        "`defaults {ty}(…)`: `defaults` takes a block type. A template's \
                         defaults are its parameter defaults (`template {ty}(<param> = <value>)`)."
                    )));
                }
                let mut call = BlockDecl {
                    slug: "defaults".into(),
                    block_type: check_type(ty, lineno)?,
                    title: None,
                    args: Vec::new(),
                    comment: None,
                    close_comment: None,
                };
                let outcome = parse_call_args(&mut call, rest, lineno)?;
                call.comment = comment;
                if outcome.closed {
                    self.items
                        .push(Item::Defaults(finish_defaults(call, lineno)?));
                } else {
                    self.open_call = Some(call);
                    self.open_defaults = true;
                }
            }
            Tok::Ident(kw) if kw == "tile" => {
                return Err(err(
                    "`tile` is only allowed inside a `tiles <name>` … `end` body".into(),
                ));
            }
            Tok::Ident(kw) if matches!(kw.as_str(), "tick" | "expect" | "clock") => {
                return Err(err(format!(
                    "`{kw}` is only allowed inside a `test \"…\"` … `end` body"
                )));
            }
            Tok::Ident(kw) if kw == "end" => match toks {
                [_] => {
                    let Some(mut t) = self.open_template.take() else {
                        return Err(err("`end` without an open `template` or `test`".into()));
                    };
                    t.end_comment = comment;
                    self.items.push(Item::Template(t));
                }
                _ => return Err(err("expected `end` on its own line".into())),
            },
            Tok::Ident(kw) if kw == "use" => {
                return Err(err(
                    "the `use` keyword is old syntax. Instantiate a template as `<slug> = <template_name>(<param>: <arg>, …)`."
                        .into(),
                ));
            }
            Tok::Ident(kw) if kw == "let" => {
                self.items
                    .push(Item::Let(parse_let(toks, comment, lineno)?));
            }
            Tok::Ident(kw) if kw == "extern" => {
                self.items
                    .push(Item::Extern(parse_extern(toks, comment, lineno)?));
            }
            Tok::Ident(kw) if kw == "removed" => {
                let slug = match toks {
                    [_, Tok::Ident(slug)] => check_slug(slug, lineno)?,
                    [_, Tok::Ident(a), Tok::Dot, Tok::Ident(b)] => scoped(a, b, lineno)?,
                    _ => return Err(err("expected `removed <slug>`".into())),
                };
                self.items
                    .push(Item::Removed(RemovedDecl { slug, comment }));
            }
            Tok::Ident(kw) if kw == "moved" => {
                let Some(at) = toks.iter().position(|t| *t == Tok::Arrow) else {
                    return Err(err("expected `moved <old_slug> -> <new_slug>`".into()));
                };
                let name = |part: &[Tok]| match part {
                    [Tok::Ident(slug)] => check_slug(slug, lineno),
                    [Tok::Ident(a), Tok::Dot, Tok::Ident(b)] => scoped(a, b, lineno),
                    _ => Err(err("expected `moved <old_slug> -> <new_slug>`".into())),
                };
                self.items.push(Item::Moved(MovedDecl {
                    from: name(&toks[1..at])?,
                    to: name(&toks[at + 1..])?,
                    comment,
                }));
            }
            Tok::Ident(kw) if kw == "page" => {
                self.items
                    .push(Item::Page(parse_page(toks, comment, lineno)?));
            }
            // v0 keywords: reserved, with migration guidance.
            Tok::Ident(kw) if kw == "block" => {
                return Err(err(
                    "the `block` keyword is old syntax. Declare a block as `<slug> = <Type>(…)` with its inputs and parameters as arguments."
                        .into(),
                ));
            }
            Tok::Ident(kw) if kw == "wire" => {
                return Err(err(
                    "the `wire` keyword is old syntax. Wire a block input in its argument list (`<Port>: <source>.<Port>`), an extern port with `<extern>.<Port> <- <source>.<Port>`."
                        .into(),
                ));
            }
            Tok::Ident(kw) if kw == "set" => {
                return Err(err(
                    "the `set` keyword is old syntax. Write `<extern>.<Port> = <value>`.".into(),
                ));
            }
            Tok::LBrace | Tok::RBrace => {
                return Err(err(
                    "`{ … }` block bodies are old syntax. Write an argument list: `<slug> = <Type>(…)`."
                        .into(),
                ));
            }
            // `(a, b).Setting = value` (#199) shares the claim forms' parser.
            Tok::Ident(_) | Tok::LParen => {
                let in_template = self.open_template.is_some();
                parse_ident_statement(
                    toks,
                    comment,
                    lineno,
                    sink(&mut self.open_template, &mut self.items),
                    &mut self.open_call,
                    in_template,
                )?;
            }
            _ => {
                return Err(err(STATEMENTS.into()));
            }
        }
        Ok(())
    }

    /// End of input: every body and argument list must be closed.
    fn finish(self, line_count: usize) -> Result<Module> {
        if let Some(call) = self.open_call {
            let what = match self.open_tile {
                Some(i) => format!("tile {i}(…"),
                None if self.open_defaults => format!("defaults {}(…", call.block_type),
                None => format!("{} = {}(…", call.slug, call.block_type),
            };
            return Err(Error::Parse {
                line: line_count,
                msg: format!("unclosed `(` in `{what}`"),
            });
        }
        if let Some(set) = self.open_set {
            return Err(Error::Parse {
                line: line_count,
                msg: format!("unclosed `{}`. Close it with `end`.", set.head()),
            });
        }
        if let Some(r) = self.open_rule {
            return Err(Error::Parse {
                line: line_count,
                msg: format!("unclosed `rule {}`. Close it with `end`.", r.slug),
            });
        }
        if let Some(t) = self.open_template {
            return Err(Error::Parse {
                line: line_count,
                msg: format!("unclosed `template {}`. Close it with `end`.", t.name),
            });
        }
        if let Some(t) = self.open_test {
            return Err(Error::Parse {
                line: line_count,
                msg: format!("unclosed `test \"{}\"`. Close it with `end`.", t.name),
            });
        }
        Ok(Module { items: self.items })
    }
}

/// `template <name>(<param>: <Type> | <param>: <set kind> [= <set>] |
/// <param> = <literal>, …) [-> <block>]` opens a template body.
fn parse_template_head(
    toks: &[Tok],
    comment: Option<String>,
    lineno: usize,
) -> Result<TemplateDecl> {
    let err = |msg: String| Error::Parse { line: lineno, msg };
    let (toks, primary) = match toks {
        [head @ .., Tok::RParen, Tok::Arrow, Tok::Ident(block)] => {
            let mut head = head.to_vec();
            head.push(Tok::RParen);
            (head, Some(check_slug(block, lineno)?))
        }
        [.., Tok::Arrow] | [.., Tok::Arrow, _] => {
            return Err(err(
                "expected `-> <block>` after the parameter list: the body block \
                 `<instance>.<Port>` means"
                    .into(),
            ));
        }
        _ => (toks.to_vec(), None),
    };
    let [_, Tok::Ident(name), Tok::LParen, rest @ .., Tok::RParen] = toks.as_slice() else {
        return Err(err(
            "expected `template <name>(<param>: <Type> | <param> = <literal>, …)`".into(),
        ));
    };
    let mut params = Vec::new();
    if !rest.is_empty() {
        for param in rest.split(|t| *t == Tok::Comma) {
            match param {
                [Tok::Ident(n), Tok::Colon, Tok::Ident(kind), tail @ ..]
                    if SetKind::parse(kind).is_some() =>
                {
                    let default = match tail {
                        [] => None,
                        [Tok::Eq, Tok::Ident(set)] => Some(check_slug(set, lineno)?),
                        [Tok::Eq, Tok::Str(set)] => {
                            return Err(err(format!(
                                "a set is named by its bare name. Write `{n}: {kind} = {set}` \
                                 instead of `{n}: {kind} = \"{set}\"`."
                            )));
                        }
                        _ => {
                            return Err(err(format!(
                                "expected `{n}: {kind}` or `{n}: {kind} = <name>` (the default \
                                 `{kind}` set)"
                            )));
                        }
                    };
                    params.push(TemplateParam::Set {
                        name: check_slug(n, lineno)?,
                        kind: SetKind::parse(kind).expect("guarded"),
                        default,
                    });
                }
                [Tok::Ident(n), Tok::Colon, Tok::Ident(ty)] => {
                    params.push(TemplateParam::Object {
                        name: check_slug(n, lineno)?,
                        // `any` takes an object of every type.
                        block_type: if ty == "any" {
                            ty.clone()
                        } else {
                            check_type(ty, lineno)?
                        },
                    });
                }
                [Tok::Ident(n), Tok::Colon, Tok::Ident(ty), Tok::Eq, ..] => {
                    return Err(err(format!(
                        "`{n}: {ty}` takes no default: an object parameter is given at \
                         every call (only set parameters, `{n}: moods = <name>`, have one)"
                    )));
                }
                [Tok::Ident(n), Tok::Eq, v]
                    if matches!(v, Tok::Num(_) | Tok::UnitNum(..) | Tok::Str(_))
                        || matches!(v, Tok::Ident(b) if b == "on" || b == "off") =>
                {
                    params.push(TemplateParam::Value {
                        name: check_slug(n, lineno)?,
                        default: value_of(v, lineno)?,
                    });
                }
                [] => return Err(err("expected a parameter between the commas".into())),
                _ => {
                    return Err(err(format!(
                        "expected `<name>: <Type>` (object parameter), \
                         `<name>: {} [= <set>]` (set parameter) or \
                         `<name> = <literal>` (value parameter with default: a \
                         number, `on`/`off` or a quoted string)",
                        SetKind::choices()
                    )));
                }
            }
        }
    }
    Ok(TemplateDecl {
        name: check_slug(name, lineno)?,
        params,
        primary,
        body: Vec::new(),
        comment,
        end_comment: None,
    })
}

/// `rule <slug>` (an existing rule's extern) or `rule <slug> "Name"`
/// opens a rule body.
fn parse_rule_head(toks: &[Tok], comment: Option<String>, lineno: usize) -> Result<RuleDecl> {
    let err = |msg: String| Error::Parse { line: lineno, msg };
    let (slug, title) = match toks {
        [_, Tok::Ident(slug)] => (slug, None),
        [_, Tok::Ident(slug), Tok::Str(title)] => (slug, Some(title.clone())),
        _ => {
            return Err(err(
                "expected `rule <slug>` (the rule's extern) or `rule <slug> \"Name\"`".into(),
            ));
        }
    };
    Ok(RuleDecl {
        slug: check_slug(slug, lineno)?,
        title,
        body: Vec::new(),
        comment,
        end_comment: None,
    })
}

/// `<kind> <name>` opens a named set of any kind.
fn parse_set_head(
    kind: SetKind,
    toks: &[Tok],
    comment: Option<String>,
    lineno: usize,
) -> Result<OpenSet> {
    let kw = kind.keyword();
    let [_, Tok::Ident(name)] = toks else {
        return Err(Error::Parse {
            line: lineno,
            msg: format!(
                "expected `{kw} <name>`, a named `{kw}` set that claims \
                 (`<block>.{kw} = <name>`) write onto blocks"
            ),
        });
    };
    Ok(OpenSet::open(kind, check_slug(name, lineno)?, comment))
}

/// `let <name> = <number | "string">`.
fn parse_let(toks: &[Tok], comment: Option<String>, lineno: usize) -> Result<LetDecl> {
    let err = |msg: String| Error::Parse { line: lineno, msg };
    match toks {
        [_, Tok::Ident(name), Tok::Eq, val] => {
            let value = match value_of(val, lineno)? {
                Value::Ref(other) => {
                    return Err(err(format!(
                        "`let {name} = {other}`: a constant's value is a number, `on`/`off` or a quoted string, not another name"
                    )));
                }
                literal => literal,
            };
            Ok(LetDecl {
                name: check_slug(name, lineno)?,
                value,
                comment,
            })
        }
        // `let abends = 19:00` — a time of day.
        [_, Tok::Ident(name), Tok::Eq, clock @ ..] if clock_toks(clock).is_some() => Ok(LetDecl {
            name: check_slug(name, lineno)?,
            value: clock_value(clock, name, lineno)?,
            comment,
        }),
        _ => Err(err(
            "expected `let <name> = <number | on | off | \"string\" | HH:MM>`".into(),
        )),
    }
}

/// `extern <slug> = <Type>(<matcher>[, <constraint>…])`.
fn parse_extern(toks: &[Tok], comment: Option<String>, lineno: usize) -> Result<ExternDecl> {
    let err = |msg: String| Error::Parse { line: lineno, msg };
    match toks {
        [
            _,
            Tok::Ident(slug),
            Tok::Eq,
            Tok::Ident(ty),
            Tok::LParen,
            rest @ ..,
            Tok::RParen,
        ] => {
            // `kind: "value"` pairs, comma-separated: one primary
            // matcher (uuid|iname|title|mirrors) first, then
            // optional `room:` / `category:` constraints. Only
            // `mirrors:` takes a bare slug; everything else is a
            // quoted string.
            let mut pairs = Vec::new();
            // `mirrors: inst.block` names a block of a template instance.
            let mut joined: Vec<Tok> = Vec::with_capacity(rest.len());
            let mut i = 0;
            while i < rest.len() {
                match (&rest[i], rest.get(i + 1), rest.get(i + 2)) {
                    (Tok::Ident(a), Some(Tok::Dot), Some(Tok::Ident(b)))
                        if joined.last() == Some(&Tok::Colon) =>
                    {
                        joined.push(Tok::Ident(format!("{a}.{b}")));
                        i += 3;
                    }
                    (t, _, _) => {
                        joined.push(t.clone());
                        i += 1;
                    }
                }
            }
            let mut it = joined.iter();
            loop {
                match (it.next(), it.next(), it.next()) {
                    (Some(Tok::Ident(k)), Some(Tok::Colon), Some(Tok::Str(v))) => {
                        pairs.push((k.as_str(), v.clone(), false));
                    }
                    (Some(Tok::Ident(k)), Some(Tok::Colon), Some(Tok::Ident(v))) => {
                        pairs.push((k.as_str(), v.clone(), true));
                    }
                    _ => {
                        return Err(err(
                            "expected `extern <slug> = <Type>(uuid|iname|title|serial: \"…\" \
                                     | mirrors: <slug>[, room: \"…\"] [, category: \"…\"] \
                                     [, device: \"…\"])`"
                                .into(),
                        ));
                    }
                }
                match it.next() {
                    None => break,
                    Some(Tok::Comma) => continue,
                    Some(_) => {
                        return Err(err("expected `,` between matchers".into()));
                    }
                }
            }
            let mut pairs = pairs.into_iter();
            let match_spec = match pairs.next() {
                Some(("mirrors", v, true)) => MatchSpec::Mirrors(check_ref(&v, lineno)?),
                Some(("mirrors", _, false)) => {
                    return Err(err("`mirrors:` takes a slug, not a string. Write it without quotes: `mirrors: status_alarm`."
                        .into()));
                }
                Some((k, _, true)) => {
                    return Err(err(format!("`{k}:` takes a quoted string")));
                }
                Some(("uuid", v, _)) => MatchSpec::Uuid(v),
                Some(("iname", v, _)) => MatchSpec::IName(v),
                Some(("title", v, _)) => MatchSpec::Title(v),
                Some(("serial", v, _)) => MatchSpec::Serial(v),
                Some((other, _, _)) => {
                    return Err(err(format!(
                        "unknown matcher `{other}`. Start with uuid, iname, title, serial or mirrors, then narrow with room, category or device."
                    )));
                }
                None => return Err(err("empty matcher list".into())),
            };
            let (mut room, mut category, mut device) = (None, None, None);
            for (k, v, is_ident) in pairs {
                if is_ident {
                    return Err(err(format!("`{k}:` takes a quoted string")));
                }
                let slot = match k {
                    "room" => &mut room,
                    "category" => &mut category,
                    "device" => &mut device,
                    other => {
                        return Err(err(format!(
                            "unknown constraint `{other}` (expected room, category \
                                     or device)"
                        )));
                    }
                };
                if slot.replace(v).is_some() {
                    return Err(err(format!("duplicate `{k}:` constraint")));
                }
            }
            let constrained = room.is_some() || category.is_some() || device.is_some();
            if let (MatchSpec::Uuid(_) | MatchSpec::Serial(_), true) = (&match_spec, constrained) {
                let k = if matches!(match_spec, MatchSpec::Uuid(_)) {
                    "uuid"
                } else {
                    "serial"
                };
                return Err(err(format!(
                    "`{k}:` matches exactly, so room, category and device are not allowed with it"
                )));
            }
            if matches!(match_spec, MatchSpec::Mirrors(_)) && constrained {
                return Err(err(
                    "room, category and device are not allowed with `mirrors:`. The file's `page` statement narrows it."
                        .into(),
                ));
            }
            Ok(ExternDecl {
                slug: check_slug(slug, lineno)?,
                block_type: check_type(ty, lineno)?,
                match_spec,
                room,
                category,
                device,
                comment,
            })
        }
        _ => Err(err(
            "expected `extern <slug> = <Type>(uuid|iname|title|serial: \"…\")`".into(),
        )),
    }
}

/// `page "<Title>"`, optionally `(room: "…"[, category: "…"])` to
/// create it.
fn parse_page(toks: &[Tok], comment: Option<String>, lineno: usize) -> Result<PageDecl> {
    let err = |msg: String| Error::Parse { line: lineno, msg };
    match toks {
        [_, Tok::Str(title)] => Ok(PageDecl {
            title: title.clone(),
            room: None,
            category: None,
            comment,
        }),
        [_, Tok::Str(title), Tok::LParen, args @ .., Tok::RParen] => {
            // `page "T" (room: "…"[, category: "…"])` — what a page
            // the compiler creates is assigned to.
            let (mut room, mut category) = (None, None);
            for arg in args.split(|t| matches!(t, Tok::Comma)) {
                let [Tok::Ident(k), Tok::Colon, Tok::Str(v)] = arg else {
                    return Err(err(
                        "expected `page \"<Title>\" (room: \"…\"[, category: \"…\"])`".into(),
                    ));
                };
                let slot = match k.as_str() {
                    "room" => &mut room,
                    "category" => &mut category,
                    other => {
                        return Err(err(format!(
                            "unknown page argument `{other}` (expected room or category)"
                        )));
                    }
                };
                if slot.replace(v.clone()).is_some() {
                    return Err(err(format!("duplicate `{k}:` page argument")));
                }
            }
            if room.is_none() {
                return Err(err(
                    "a page argument list needs `room:`, the room the created page belongs to"
                        .into(),
                ));
            }
            Ok(PageDecl {
                title: title.clone(),
                room,
                category,
                comment,
            })
        }
        _ => {
            Err(err(
                        "expected `page \"<Title>\"` with the title of a page in the base config. To create a new page, add `(room: \"…\"[, category: \"…\"])`."
                            .into(),
                    ))
        }
    }
}

/// An operating mode in a rule line: a slug or a quoted title.
fn mode_name(tok: &Tok, lineno: usize, usage: &str) -> Result<ModeName> {
    match tok {
        Tok::Ident(s) => Ok(ModeName::Slug(check_slug(s, lineno)?)),
        Tok::Str(t) => Ok(ModeName::Title(t.clone())),
        _ => Err(Error::Parse {
            line: lineno,
            msg: format!("a mode is a slug or a quoted title. {usage}"),
        }),
    }
}

/// One statement of a `rule` body: `active`/`inactive`,
/// `when time HH:MM`, `when weekday <days>`, `when mode <mode> [off]`,
/// `when <slug>.<state> <op> <value>`, the weather, day, daylight, sun
/// and date conditions, `or`, `then <slug> "<command>"`,
/// `then mode <mode> on|off`, `then notify|mail|wait|message`.
/// The action words of `then <word> …`: in that place they are the action,
/// never a block.
const RULE_ACTIONS: &[&str] = &["wait", "mode", "notify", "mail", "message"];

fn parse_rule_stmt(toks: &[Tok], comment: Option<String>, lineno: usize) -> Result<RuleStmt> {
    let err = |msg: String| Error::Parse { line: lineno, msg };
    let usage = "A rule line is `active`, `inactive`, `when time HH:MM`, \
                 `when weekday <days>`, `when mode <mode> [off]`, \
                 `when <slug>.<state> <op> <value>`, `when weather …`, `when day …`, \
                 `when daylight`, `when sunset`, `when date …`, `or`, \
                 `then <slug> <command>`, `then mode <mode> on|off`, `then notify …`, \
                 `then mail …`, `then wait <duration>`, `then message …` or `end`";
    let kind = match toks {
        [Tok::Ident(w)] if w == "active" => RuleStmtKind::Active(true),
        [Tok::Ident(w)] if w == "inactive" => RuleStmtKind::Active(false),
        [Tok::Ident(w)] if w == "or" => RuleStmtKind::Or,
        [Tok::Ident(w), ..] if w == "when" => parse_rule_when(toks, lineno, usage)?,
        [Tok::Ident(w), ..] if w == "then" => parse_rule_then(toks, lineno, usage)?,
        _ => return Err(err(format!("unexpected rule line. {usage}"))),
    };
    Ok(RuleStmt { kind, comment })
}

/// A `when …` condition line of a `rule` body (see [`parse_rule_stmt`]).
fn parse_rule_when(toks: &[Tok], lineno: usize, usage: &str) -> Result<RuleStmtKind> {
    let err = |msg: String| Error::Parse { line: lineno, msg };
    let kind = match toks {
        // `when <slug>.<state>` first: the dot tells a block named like a
        // condition word (`mode`, `day`) from the condition.
        [
            Tok::Ident(w),
            Tok::Ident(target),
            Tok::Dot,
            Tok::Ident(state),
            Tok::Cmp(op),
            value,
        ] if w == "when" => {
            let value = match value {
                Tok::Num(n) => n.clone(),
                Tok::Ident(v) if v == "on" || v == "off" => v.clone(),
                _ => {
                    return Err(err(format!(
                        "`when {target}.{state} {}` compares with a number, `on` or `off`",
                        op.symbol()
                    )));
                }
            };
            RuleStmtKind::State {
                target: check_ref(target, lineno)?,
                state: state.clone(),
                op: *op,
                value,
            }
        }
        // The same time-of-day lexeme as `At: 19:00` and a test's `clock`.
        [Tok::Ident(w), Tok::Ident(k), Tok::Str(t)] if w == "when" && k == "time" => {
            return Err(err(
                match super::ast::clock_seconds(t).and_then(super::ast::clock_text) {
                    Some(c) => format!(
                        "a time takes no quotes. Write `when time {c}` instead of `when time \"{t}\"`."
                    ),
                    None => format!("expected `when time HH:MM` (`when time 07:30`). {usage}"),
                },
            ));
        }
        [Tok::Ident(w), Tok::Ident(k), time @ ..]
            if w == "when" && k == "time" && clock_toks(time).is_some() =>
        {
            let Value::Clock(text) = clock_value(time, "when time", lineno)? else {
                unreachable!("clock_value returns a clock")
            };
            let secs = super::ast::clock_seconds(&text).expect("canonical");
            if !secs.is_multiple_of(60) {
                return Err(err(format!(
                    "`when time {text}`: a rule's time condition is a whole minute. Write `HH:MM`."
                )));
            }
            RuleStmtKind::Time((secs / 60) as u16)
        }
        [Tok::Ident(w), Tok::Ident(k), days @ ..] if w == "when" && k == "weekday" => {
            let mut words = Vec::new();
            for d in days {
                match d {
                    Tok::Ident(d) => words.push(d.clone()),
                    Tok::Comma => {}
                    _ => {
                        return Err(err(format!(
                            "expected day keywords after `when weekday`. {usage}"
                        )));
                    }
                }
            }
            if words.is_empty() {
                return Err(err(format!(
                    "`when weekday` needs at least one day. {usage}"
                )));
            }
            RuleStmtKind::Weekday(words)
        }
        [Tok::Ident(w), Tok::Ident(k), mode, rest @ ..] if w == "when" && k == "mode" => {
            let on = match rest {
                [] => true,
                [Tok::Ident(v)] if v == "on" => true,
                [Tok::Ident(v)] if v == "off" => false,
                _ => {
                    return Err(err(format!(
                        "`when mode <mode>` ends with nothing, `on` or `off`. {usage}"
                    )));
                }
            };
            RuleStmtKind::Mode {
                mode: mode_name(mode, lineno, usage)?,
                on,
            }
        }
        [
            Tok::Ident(w),
            Tok::Ident(k),
            Tok::Ident(field),
            Tok::Cmp(op),
            value,
        ] if w == "when" && k == "weather" => {
            let value = match value {
                Tok::Num(n) => n.clone(),
                Tok::Ident(v) if v == "on" || v == "off" => v.clone(),
                _ => {
                    return Err(err(format!(
                        "`when weather {field} {}` compares with a number, `on` or `off`",
                        op.symbol()
                    )));
                }
            };
            RuleStmtKind::Weather {
                field: field.clone(),
                op: *op,
                value,
            }
        }
        [Tok::Ident(w), Tok::Ident(k), Tok::Cmp(op), Tok::Num(n)] if w == "when" && k == "day" => {
            RuleStmtKind::Day {
                op: *op,
                value: n.clone(),
            }
        }
        [Tok::Ident(w), Tok::Ident(k), rest @ ..] if w == "when" && k == "daylight" => match rest {
            [] => RuleStmtKind::Daylight(true),
            [Tok::Ident(v)] if v == "on" => RuleStmtKind::Daylight(true),
            [Tok::Ident(v)] if v == "off" => RuleStmtKind::Daylight(false),
            _ => {
                return Err(err(format!(
                    "`when daylight` ends with nothing, `on` or `off`. {usage}"
                )));
            }
        },
        [Tok::Ident(w), Tok::Ident(k)]
            if w == "when" && matches!(k.as_str(), "sunrise" | "sunset" | "dawn" | "dusk") =>
        {
            RuleStmtKind::Sun(k.clone())
        }
        [Tok::Ident(w), Tok::Ident(k), Tok::Str(d)] if w == "when" && k == "date" => {
            return Err(err(if is_date(d) {
                format!(
                    "a date takes no quotes. Write `when date {d}` instead of `when date \"{d}\"`."
                )
            } else {
                format!(
                    "`{d}` is not a date. Write it as YYYY-MM-DD without quotes: `when date 2026-12-24`."
                )
            }));
        }
        [Tok::Ident(w), Tok::Ident(k), date @ ..] if w == "when" && k == "date" => {
            match date_toks(date, lineno)? {
                Some(d) => RuleStmtKind::Date(d),
                None => {
                    return Err(err(
                        "expected `when date YYYY-MM-DD` (`when date 2026-12-24`)".into(),
                    ));
                }
            }
        }
        _ => {
            return Err(err(format!(
                "Lox Logic knows the conditions `when time`, `when weekday`, `when mode`, \
                 `when <slug>.<state> <op> <value>`, `when weather <field> <op> <value>`, \
                 `when day <op> <n>`, `when daylight`, `when sunrise|sunset|dawn|dusk` and \
                 `when date YYYY-MM-DD`. {usage}"
            )));
        }
    };
    Ok(kind)
}

/// A `then …` action line of a `rule` body (see [`parse_rule_stmt`]).
fn parse_rule_then(toks: &[Tok], lineno: usize, usage: &str) -> Result<RuleStmtKind> {
    let err = |msg: String| Error::Parse { line: lineno, msg };
    let kind = match toks {
        [
            Tok::Ident(w),
            Tok::Ident(k),
            Tok::Str(subject),
            Tok::Str(text),
        ] if w == "then" && k == "notify" => RuleStmtKind::Notify {
            subject: subject.clone(),
            text: text.clone(),
        },
        [
            Tok::Ident(w),
            Tok::Ident(k),
            Tok::Str(to),
            Tok::Str(subject),
            Tok::Str(text),
        ] if w == "then" && k == "mail" => RuleStmtKind::Mail {
            to: to.clone(),
            subject: subject.clone(),
            text: text.clone(),
        },
        [Tok::Ident(w), Tok::Ident(k), d] if w == "then" && k == "wait" => {
            let text = match d {
                Tok::UnitNum(n, u) if matches!(u, Unit::Ms | Unit::S | Unit::Min | Unit::H) => {
                    format!("{n}{}", u.suffix())
                }
                Tok::Ident(_) | Tok::Str(_) => {
                    return Err(err(
                        "`then wait` takes a duration: `5s`, `500ms`, `2min`, `1h`. A block \
                         named `wait` cannot be a rule target: rename the block \
                         (`loxl rename wait <new>`)."
                            .into(),
                    ));
                }
                _ => {
                    return Err(err(
                        "`then wait` takes a duration: `5s`, `500ms`, `2min`, `1h`".into(),
                    ));
                }
            };
            if crate::logic::rule::wait_ms(&text).is_none() {
                return Err(err(format!(
                    "`then wait {text}`: the app waits 0.1 s to 24 h, in whole milliseconds"
                )));
            }
            RuleStmtKind::Wait(text)
        }
        [
            Tok::Ident(w),
            Tok::Ident(k),
            Tok::Ident(sev),
            Tok::Str(subject),
            Tok::Str(text),
            rest @ ..,
        ] if w == "then" && k == "message" => {
            if crate::logic::rule::severity(sev).is_none() {
                return Err(err(format!(
                    "`{sev}` is no severity: `info`, `warning`, `important` or `critical`"
                )));
            }
            let admins = match rest {
                [] => false,
                [Tok::Ident(a)] if a == "admins" => true,
                _ => return Err(err("`then message` ends with nothing or `admins`".into())),
            };
            RuleStmtKind::Message {
                severity: sev.clone(),
                subject: subject.clone(),
                text: text.clone(),
                admins,
            }
        }
        [Tok::Ident(w), Tok::Ident(k), mode, Tok::Ident(v)]
            if w == "then" && k == "mode" && (v == "on" || v == "off") =>
        {
            RuleStmtKind::ThenMode {
                mode: mode_name(mode, lineno, usage)?,
                on: v == "on",
            }
        }
        // `then pumpe on` — a one-word command is bare; `then x "on"`
        // canonicalizes to it.
        // The action words win their place: a block named like one
        // cannot be the target of `then <slug> <command>`.
        [Tok::Ident(w), Tok::Ident(target), _]
            if w == "then" && RULE_ACTIONS.contains(&target.as_str()) =>
        {
            return Err(err(format!(
                "`then {target} …` is the {target} action, so a block named `{target}` cannot \
                 be a rule target. Rename the block (`loxl rename {target} <new>`)."
            )));
        }
        [
            Tok::Ident(w),
            Tok::Ident(target),
            Tok::Str(command) | Tok::Ident(command),
        ] if w == "then" => RuleStmtKind::Then {
            target: check_slug(target, lineno)?,
            command: command.clone(),
        },
        // `then licht.lc on` — a body block of a template instance.
        [
            Tok::Ident(w),
            Tok::Ident(a),
            Tok::Dot,
            Tok::Ident(b),
            Tok::Str(command) | Tok::Ident(command),
        ] if w == "then" && b.starts_with(|c: char| c.is_ascii_lowercase()) => RuleStmtKind::Then {
            target: scoped(a, b, lineno)?,
            command: command.clone(),
        },
        _ => return Err(err(format!("unexpected rule line. {usage}"))),
    };
    Ok(kind)
}

/// One statement of a `test` body: a port injection, `tick`,
/// `expect`, or `clock`.
/// One line of a `moods` body: `"<name>": <setting>, …`, a
/// setting being `all <value>` or `AQ<n> <value>`, a value `on`, `off`,
/// `<n>%`, `rgb(<r>, <g>, <b>)` (percent), `temp(<n>%, <k>K)` or a bare
/// number (the stored value verbatim).
fn parse_mood_line(toks: &[Tok], comment: Option<String>, lineno: usize) -> Result<MoodLine> {
    use crate::moods::MoodValue;
    let err = |msg: String| Error::Parse { line: lineno, msg };
    let usage = "A mood is `\"<name>\": all <value>, AQ<n> <value>, …` with the values `on`, \
                 `off`, `<n>%`, `rgb(<r>, <g>, <b>)` or `temp(<n>%, <k>K)`.";
    let [Tok::Str(name), Tok::Colon, rest @ ..] = toks else {
        return Err(err(format!("expected a mood. {usage}")));
    };
    if rest.is_empty() {
        return Err(err(format!(
            "mood \"{name}\" sets no circuit. Write `all off` for a dark mood."
        )));
    }
    let pct = |t: &Tok| -> Result<u8> {
        let n = match t {
            Tok::UnitNum(n, Unit::Pct) | Tok::Num(n) => n,
            _ => return Err(err(format!("expected a percentage (0 … 100). {usage}"))),
        };
        n.parse::<u8>()
            .ok()
            .filter(|p| *p <= 100)
            .ok_or_else(|| err(format!("`{n}` is not a whole percentage (0 … 100)")))
    };
    // Settings split at top-level commas (`rgb(…)` carries its own).
    let mut parts: Vec<&[Tok]> = Vec::new();
    let (mut depth, mut start) = (0usize, 0usize);
    for (i, t) in rest.iter().enumerate() {
        match t {
            Tok::LParen => depth += 1,
            Tok::RParen => depth = depth.saturating_sub(1),
            Tok::Comma if depth == 0 => {
                parts.push(&rest[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&rest[start..]);
    let mut settings = Vec::new();
    for part in parts {
        let (circuit, value) = match part {
            [Tok::Ident(c), value @ ..] if c == "all" => (None, value),
            [Tok::Ident(c), value @ ..] if c.starts_with("AQ") => match c[2..].parse::<u8>() {
                Ok(n) if n >= 1 && !c[2..].starts_with('0') => (Some(n), value),
                _ => return Err(err(format!("`{c}` is not a circuit (`AQ1`, `AQ2`, …)"))),
            },
            [] => return Err(err(format!("empty setting. {usage}"))),
            _ => return Err(err(format!("expected `all` or a circuit `AQ<n>`. {usage}"))),
        };
        let value = match value {
            [Tok::Ident(v)] if v == "on" => MoodValue::On,
            [Tok::Ident(v)] if v == "off" => MoodValue::Off,
            [p @ Tok::UnitNum(_, Unit::Pct)] => MoodValue::Pct(pct(p)?),
            [Tok::Num(n)] => MoodValue::Raw(n.parse().map_err(|_| {
                err(format!(
                    "`{n}` is not a valid mood value (a whole number below 2³²). Write a percentage with `%`."
                ))
            })?),
            [
                Tok::Ident(f),
                Tok::LParen,
                r,
                Tok::Comma,
                g,
                Tok::Comma,
                b,
                Tok::RParen,
            ] if f == "rgb" => MoodValue::Rgb(pct(r)?, pct(g)?, pct(b)?),
            [
                Tok::Ident(f),
                Tok::LParen,
                b,
                Tok::Comma,
                Tok::UnitNum(k, Unit::K),
                Tok::RParen,
            ] if f == "temp" => {
                let kelvin = k
                    .parse::<u16>()
                    .ok()
                    .filter(|k| (1000..=9999).contains(k))
                    .ok_or_else(|| {
                        err(format!(
                            "`{k}K` is not a colour temperature (1000K … 9999K)"
                        ))
                    })?;
                MoodValue::Temp(pct(b)?, kelvin)
            }
            [Tok::Ident(f), ..] if f == "rgb" => {
                return Err(err(
                    "expected `rgb(<r>, <g>, <b>)`, each 0 … 100 percent".into()
                ));
            }
            [Tok::Ident(f), ..] if f == "temp" => {
                return Err(err(
                    "expected `temp(<n>%, <k>K)`, e.g. `temp(80%, 2700K)`".into()
                ));
            }
            _ => return Err(err(format!("expected a value. {usage}"))),
        };
        settings.push(MoodSetting { circuit, value });
    }
    Ok(MoodLine {
        name: name.clone(),
        settings,
        comment,
    })
}

/// One line of a `schedule` body: day keywords, then either `off`
/// or comma-separated intervals `HH:MM-HH:MM [= <number>] [activate]`.
fn parse_schedule_line(
    toks: &[Tok],
    comment: Option<String>,
    lineno: usize,
) -> Result<ScheduleLine> {
    let err = |msg: String| Error::Parse { line: lineno, msg };
    let usage = "A schedule line is `<days> HH:MM-HH:MM[ = <value>][ activate], …` or \
                 `<days> off`";
    let mut days = Vec::new();
    let mut i = 0;
    loop {
        match toks.get(i) {
            Some(Tok::Ident(w)) if w == "off" || w == "activate" => break,
            Some(Tok::Ident(w)) if super::schedule::is_day(w) => {
                days.push(Day::Keyword(w.clone()));
            }
            // Any other word is a `Mode`'s slug; validate resolves it.
            Some(Tok::Ident(w)) if super::rename::valid_slug(w).is_ok() => {
                days.push(Day::Slug(w.clone()));
            }
            Some(Tok::Ident(w)) => {
                return Err(err(super::schedule::unknown_day(w, std::iter::empty())));
            }
            Some(Tok::Str(s)) if s.trim().is_empty() => {
                return Err(err("an operating mode's title cannot be empty".into()));
            }
            Some(Tok::Str(s)) => days.push(Day::Title(s.clone())),
            _ => break,
        }
        i += 1;
    }
    if days.is_empty() {
        return Err(err(format!("expected a day first. {usage}")));
    }
    let rest = &toks[i..];
    if let [Tok::Ident(w)] = rest
        && w == "off"
    {
        return Ok(ScheduleLine {
            days,
            intervals: Vec::new(),
            comment,
        });
    }
    let time = |h: &str, m: &str| -> Result<u16> {
        let bad = || err(format!("`{h}:{m}` is not a time of day (00:00 … 24:00)"));
        if m.len() != 2 || h.is_empty() || h.len() > 2 {
            return Err(bad());
        }
        let (h, m): (u16, u16) = (h.parse().map_err(|_| bad())?, m.parse().map_err(|_| bad())?);
        if m > 59 || h > 24 || (h == 24 && m != 0) {
            return Err(bad());
        }
        Ok(h * 60 + m)
    };
    let mut intervals = Vec::new();
    for part in rest.split(|t| *t == Tok::Comma) {
        let (from, to, tail) = match part {
            [
                Tok::Num(h1),
                Tok::Colon,
                Tok::Num(m1),
                Tok::Minus,
                Tok::Num(h2),
                Tok::Colon,
                Tok::Num(m2),
                tail @ ..,
            ] => (time(h1, m1)?, time(h2, m2)?, tail),
            [] => return Err(err(format!("empty interval. {usage}"))),
            _ => return Err(err(format!("expected an interval `HH:MM-HH:MM`. {usage}"))),
        };
        let (value, tail) = match tail {
            [Tok::Eq, Tok::Num(v), tail @ ..] => (Some(v.clone()), tail),
            [Tok::Eq, ..] => {
                return Err(err(format!(
                    "expected a number after `=` (the timer's value in the interval). {usage}"
                )));
            }
            _ => (None, tail),
        };
        let activate = match tail {
            [] => false,
            [Tok::Ident(w)] if w == "activate" => true,
            _ => return Err(err(format!("unexpected tokens after an interval. {usage}"))),
        };
        intervals.push(Interval {
            from,
            to,
            value,
            activate,
        });
    }
    Ok(ScheduleLine {
        days,
        intervals,
        comment,
    })
}

fn parse_test_stmt(toks: &[Tok], comment: Option<String>, lineno: usize) -> Result<TestItem> {
    let err = |msg: String| Error::Parse { line: lineno, msg };
    match toks {
        [Tok::Ident(kw), rest @ ..] if kw == "tick" => {
            let (n_tok, dt) = match rest {
                [n] => (n, None),
                [n, Tok::Ident(kw), v] if kw == "dt" => {
                    let dt = match value_of(v, lineno)? {
                        v @ (Value::Number(_) | Value::Unit { .. } | Value::Ref(_)) => v,
                        other => return Err(err(super::ast::dt_error(&other))),
                    };
                    (n, Some(dt))
                }
                _ => {
                    return Err(err(
                        "expected `tick <n>`, `tick <duration>` (`tick 5min`) or either \
                         with `dt <seconds>`"
                            .into(),
                    ));
                }
            };
            let amount = match n_tok {
                Tok::Num(_) | Tok::UnitNum(..) | Tok::Ident(_) => value_of(n_tok, lineno)?,
                _ => {
                    return Err(err(
                        "expected `tick <n>` with a plain tick count, or `tick <duration>` \
                         (`tick 90s`, `tick 5min`)"
                            .into(),
                    ));
                }
            };
            let tick = TickDecl {
                amount,
                dt,
                comment,
            };
            // Literals are checked here; a `let` constant when the test runs.
            tick.count(|_| None).map_err(err)?;
            Ok(TestItem::Tick(tick))
        }
        [Tok::Ident(kw), rest @ ..] if kw == "expect" => {
            let [Tok::Ident(slug), Tok::Dot, Tok::Ident(port), cmp_tok, val] = rest else {
                return Err(err(
                    "expected `expect <slug>.<Port> <cmp> <value>` with cmp one of \
                     `==`, `!=`, `>=`, `>`, `<=`, `<`, `~=`"
                        .into(),
                ));
            };
            let cmp = match cmp_tok {
                Tok::Cmp(CmpOp::Eq) => TestCmp::Eq,
                Tok::Cmp(CmpOp::Ge) => TestCmp::Ge,
                Tok::Cmp(CmpOp::Gt) => TestCmp::Gt,
                Tok::Cmp(CmpOp::Le) => TestCmp::Le,
                Tok::Cmp(CmpOp::Lt) => TestCmp::Lt,
                Tok::Approx => TestCmp::Approx,
                Tok::Cmp(CmpOp::Ne) => TestCmp::Ne,
                _ => {
                    return Err(err(
                        "expected a comparator after the port: `==`, `!=`, `>=`, `>`, \
                         `<=`, `<`, or `~=`"
                            .into(),
                    ));
                }
            };
            Ok(TestItem::Expect(ExpectDecl {
                port: PortRef {
                    slug: slug.clone(),
                    port: port.clone(),
                },
                cmp,
                value: value_of(val, lineno)?,
                comment,
            }))
        }
        [Tok::Ident(kw), rest @ ..] if kw == "clock" => Ok(TestItem::Clock(ClockDecl {
            spec: clock_spec(rest, lineno)?,
            comment,
        })),
        [Tok::Ident(slug), Tok::Dot, Tok::Ident(port), Tok::Eq, val] => {
            Ok(TestItem::Inject(SetDecl {
                target: PortRef {
                    slug: slug.clone(),
                    port: port.clone(),
                },
                value: value_of(val, lineno)?,
                comment,
                bulk: None,
            }))
        }
        [
            Tok::Ident(slug),
            Tok::Dot,
            Tok::Ident(port),
            Tok::LArrow,
            ..,
        ] => Err(err(format!(
            "`{slug}.{port} <- …`: a test sets values and draws no wires. Write `{slug}.{port} = 1`."
        ))),
        _ => Err(err(
            "expected a test statement: `<slug>.<Port> = <value>`, `tick <n> [dt <s>]`, \
             `tick <duration>`, `expect <slug>.<Port> <cmp> <value>`, `clock HH:MM`, \
             `clock YYYY-MM-DD HH:MM`, or `end`"
                .into(),
        )),
    }
}

/// The claim `<slug>.<kind> = <set>`, as the item of its kind.
fn claim(kind: SetKind, slug: String, set: String, comment: Option<String>) -> Item {
    match kind {
        SetKind::Tiles => Item::TilesClaim(TilesClaim {
            slug,
            tiles: set,
            comment,
        }),
        SetKind::Schedule => Item::ScheduleClaim(ScheduleClaim {
            slug,
            schedule: set,
            comment,
        }),
        SetKind::Moods => Item::MoodsClaim(MoodsClaim {
            slug,
            moods: set,
            comment,
        }),
        SetKind::Access | SetKind::Recipients => Item::AccessClaim(AccessClaim {
            slug,
            right: kind.right().expect("a rights kind"),
            set,
            comment,
        }),
    }
}

/// Statements that start with a plain identifier: block declarations
/// (`slug = Type(…)`) and extern port statements (`slug.Port <- …` /
/// `slug.Port = …`).
fn parse_ident_statement(
    toks: &[Tok],
    comment: Option<String>,
    lineno: usize,
    items: &mut Vec<Item>,
    open_call: &mut Option<BlockDecl>,
    in_template: bool,
) -> Result<()> {
    let err = |msg: String| Error::Parse { line: lineno, msg };
    match toks {
        // slug = Type( … — a block declaration (possibly spanning lines);
        // a lowercase callee is a template instantiation.
        [
            Tok::Ident(slug),
            Tok::Eq,
            Tok::Ident(ty),
            Tok::LParen,
            rest @ ..,
        ] if ty != "not" => {
            let callee = if ty.chars().next().is_some_and(|c| c.is_ascii_lowercase()) {
                check_slug(ty, lineno)?
            } else {
                check_type(ty, lineno)?
            };
            let mut call = BlockDecl {
                slug: check_slug(slug, lineno)?,
                block_type: callee,
                title: None,
                args: Vec::new(),
                comment: None,
                close_comment: None,
            };
            let outcome = parse_call_args(&mut call, rest, lineno)?;
            // The header line's comment stays on the header — for a call
            // closed on this same line it is the statement comment.
            call.comment = comment;
            if outcome.closed {
                items.push(finish_call(call, in_template, lineno)?);
            } else {
                *open_call = Some(call);
            }
            Ok(())
        }
        [Tok::Ident(slug), Tok::Eq, Tok::Ident(ty)]
            if ty.starts_with(|c: char| c.is_ascii_uppercase()) =>
        {
            Err(err(format!(
                "expected `(` after the type. Declare a block as `{slug} = {ty}(…)`, a constant as `let {slug} = …`."
            )))
        }
        [Tok::Ident(slug), Tok::Eq, Tok::Num(v)] => Err(err(format!(
            "`{slug} = {v}`: did you mean the constant `let {slug} = {v}`? An extern port is set with `{slug}.<Port> = {v}`."
        ))),
        [Tok::Ident(slug), Tok::Eq, Tok::Str(_)] => Err(err(format!(
            "`{slug} = \"…\"`: did you mean `let {slug} = \"…\"`?"
        ))),
        // slug = <expr> — a named signal (D81).
        [Tok::Ident(slug), Tok::Eq, rest @ ..] if !rest.is_empty() => {
            items.push(parse_signal(slug, rest, comment, lineno)?);
            Ok(())
        }
        // slug.Port <- … — a plain wire (`slug.Port` RHS) or an
        // expression that desugars into gate/comparator blocks.
        [
            Tok::Ident(ts),
            Tok::Dot,
            Tok::Ident(tp),
            Tok::LArrow,
            rest @ ..,
        ] => {
            items.push(parse_wire(ts, tp, rest, comment, lineno)?);
            Ok(())
        }
        // slug.Port = value — a Def write on an extern port.
        [
            Tok::Ident(ts),
            Tok::Dot,
            Tok::Ident(tp),
            Tok::Eq,
            Tok::Ident(vs),
            Tok::Dot,
            Tok::Ident(vp),
        ] => Err(err(format!(
            "`{ts}.{tp} = {vs}.{vp}`: wire a port with `<-`, as in `{ts}.{tp} <- {vs}.{vp}`"
        ))),
        // slug.<kind> = name — the claim of a named set, the same for every
        // kind (`tiles`, `schedule`, `moods`, `access`, `recipients`). Like
        // other statements on a block it may sit in a template body, where the
        // set is usually a set parameter.
        [Tok::Ident(ts), Tok::Dot, Tok::Ident(tp), Tok::Eq, rest @ ..]
            if SetKind::parse(tp).is_some() =>
        {
            let kind = SetKind::parse(tp).expect("guarded");
            let [Tok::Ident(set)] = rest else {
                return Err(err(format!(
                    "expected `{ts}.{tp} = <name>`, the name of a `{tp}` set (in a template \
                     body: the set parameter that passes one)"
                )));
            };
            items.push(claim(kind, ts.clone(), set.clone(), comment));
            Ok(())
        }
        // (a, b, …).Setting = value — one setting or attribute for several
        // objects (#199): a statement per object, each remembering the line.
        [
            Tok::LParen,
            inner @ ..,
            Tok::RParen,
            Tok::Dot,
            Tok::Ident(tp),
            Tok::Eq,
            val,
        ] => {
            items.extend(parse_bulk(inner, tp, val, comment, lineno)?);
            Ok(())
        }
        // slug.title|room|category = value — an attribute claim.
        [Tok::Ident(ts), Tok::Dot, Tok::Ident(tp), Tok::Eq, val] if Attr::parse(tp).is_some() => {
            items.push(Item::Attr(AttrDecl {
                slug: ts.clone(),
                attr: Attr::parse(tp).expect("guarded"),
                value: value_of(val, lineno)?,
                comment,
                bulk: None,
            }));
            Ok(())
        }
        [Tok::Ident(ts), Tok::Dot, Tok::Ident(tp), Tok::Eq, val] => {
            items.push(Item::Set(SetDecl {
                target: PortRef {
                    slug: ts.clone(),
                    port: tp.clone(),
                },
                value: value_of(val, lineno)?,
                comment,
                bulk: None,
            }));
            Ok(())
        }
        [Tok::Ident(ts), Tok::Dot, ..] => Err(err(format!(
            "expected `{ts}.<Port> <- <slug>.<Port>` or `{ts}.<Port> = <value>`"
        ))),
        _ => Err(err(STATEMENTS.into())),
    }
}

/// `slug.Port <- …`: a plain wire (`slug.Port` RHS) or an expression
/// that desugars into gate/comparator blocks.
fn parse_wire(
    ts: &str,
    tp: &str,
    rest: &[Tok],
    comment: Option<String>,
    lineno: usize,
) -> Result<Item> {
    let err = |msg: String| Error::Parse { line: lineno, msg };
    if let Some(attr) = Attr::parse(tp) {
        return Err(err(format!(
            "`{ts}.{attr}` is an attribute, not a port, so it cannot be wired. Assign it: `{ts}.{attr} = \"…\"`."
        )));
    }
    if tp == "tiles" {
        return Err(err(format!(
            "`{ts}.tiles` takes a tiles set (`{ts}.tiles = <name>`), never a wire"
        )));
    }
    if tp == "schedule" {
        return Err(err(format!(
            "`{ts}.schedule` takes a schedule (`{ts}.schedule = <name>`), never a wire"
        )));
    }
    if tp == "moods" {
        return Err(err(format!(
            "`{ts}.moods` takes a mood set (`{ts}.moods = <name>`), never a wire"
        )));
    }
    let to = PortRef {
        slug: ts.to_string(),
        port: tp.to_string(),
    };
    if rest.is_empty() {
        return Err(err(format!(
            "expected a source after `<-`: a port (`{ts}.{tp} <- <slug>.<Port>`) or an expression (`{ts}.{tp} <- a.Q and b.AQ >= 28`). To assign a value, write `{ts}.{tp} = <value>`."
        )));
    }
    match parse_expr(rest, lineno)? {
        Expr::Atom(Operand::Port(from)) => Ok(Item::Wire(WireDecl { to, from, comment })),
        Expr::Atom(Operand::Value(v)) => Err(err(format!(
            "`{to} <- {v}` wires a constant. To assign a value, write `{to} = {v}`."
        ))),
        expr => Ok(Item::ExprWire(ExprWireDecl { to, expr, comment })),
    }
}

/// `slug = <expr>`: a named signal. The expression must be one —
/// a bare port or value names no block of its own.
fn parse_signal(slug: &str, rest: &[Tok], comment: Option<String>, lineno: usize) -> Result<Item> {
    let err = |msg: String| Error::Parse { line: lineno, msg };
    let slug = check_slug(slug, lineno)?;
    match parse_expr(rest, lineno)? {
        Expr::Atom(Operand::Port(p)) => Err(err(format!(
            "`{slug} = {p}` is a bare port: an alias is not a block. Write `{p}` \
             where you would write `{slug}.{port}`.",
            port = p.port,
        ))),
        Expr::Atom(Operand::Value(v)) => Err(err(format!(
            "`{slug} = {v}` is a value, not a signal. Did you mean `let {slug} = {v}`?"
        ))),
        expr => Ok(Item::Signal(SignalDecl {
            slug,
            expr,
            comment,
        })),
    }
}

/// `(a, b, …).Setting = value`: one setting or attribute for several
/// objects (#199) — a statement per object, each remembering the line.
fn parse_bulk(
    inner: &[Tok],
    tp: &str,
    val: &Tok,
    comment: Option<String>,
    lineno: usize,
) -> Result<Vec<Item>> {
    let err = |msg: String| Error::Parse { line: lineno, msg };
    let mut items = Vec::new();
    let mut slugs: Vec<String> = Vec::new();
    for part in inner.split(|t| *t == Tok::Comma) {
        // A slug, or `inst.block` for a block of a template instance.
        let slug = match part {
            [Tok::Ident(s)] => s.clone(),
            [Tok::Ident(a), Tok::Dot, Tok::Ident(b)] => scoped(a, b, lineno)?,
            _ => {
                return Err(err(format!(
                    "expected `(<slug>, <slug>, …).{tp} = <value>`"
                )));
            }
        };
        if slugs.contains(&slug) {
            return Err(err(format!("`{slug}` is listed twice")));
        }
        slugs.push(slug);
    }
    if slugs.is_empty() {
        return Err(err(format!(
            "expected `(<slug>, <slug>, …).{tp} = <value>`"
        )));
    }
    if matches!(tp, "tiles" | "schedule" | "moods") {
        return Err(err(format!(
            "`.{tp}` takes one object. Write one statement per object."
        )));
    }
    let value = value_of(val, lineno)?;
    for (k, s) in slugs.into_iter().enumerate() {
        let comment = if k == 0 { comment.clone() } else { None };
        items.push(match Attr::parse(tp) {
            Some(attr) => Item::Attr(AttrDecl {
                slug: s.clone(),
                attr,
                value: value.clone(),
                comment,
                bulk: Some(lineno),
            }),
            None => Item::Set(SetDecl {
                target: PortRef {
                    slug: s.clone(),
                    port: tp.to_string(),
                },
                value: value.clone(),
                comment,
                bulk: Some(lineno),
            }),
        });
    }
    Ok(items)
}

struct ArgOutcome {
    /// The closing `)` was consumed.
    closed: bool,
    /// At least one binding was appended by this line (comment attach).
    pushed_binding: bool,
}

/// Parse argument-list tokens (label string, `Port: value` bindings,
/// commas, closing `)`), appending to `call`.
fn parse_call_args(call: &mut BlockDecl, toks: &[Tok], lineno: usize) -> Result<ArgOutcome> {
    let err = |msg: String| Error::Parse { line: lineno, msg };
    let mut i = 0;
    let mut pushed_binding = false;
    loop {
        match toks.get(i) {
            None => {
                return Ok(ArgOutcome {
                    closed: false,
                    pushed_binding,
                });
            }
            Some(Tok::RParen) => {
                if i + 1 != toks.len() {
                    return Err(err("nothing may follow `)` on the line".into()));
                }
                return Ok(ArgOutcome {
                    closed: true,
                    pushed_binding,
                });
            }
            Some(Tok::Str(s)) => {
                if call.title.is_some() {
                    return Err(err(
                        "duplicate label. A block takes one label string.".into()
                    ));
                }
                if !call.args.iter().any(|a| matches!(a, ArgItem::Binding(_))) {
                    call.title = Some(s.clone());
                } else {
                    return Err(err("the label string must be the first argument".into()));
                }
                i += 1;
            }
            Some(Tok::Ident(port)) => {
                if toks.get(i + 1) != Some(&Tok::Colon) {
                    return Err(err(format!(
                        "expected `:` after argument name `{port}` (`{port}: <value>`)"
                    )));
                }
                // The binding value runs to the next top-level comma or the
                // call's closing `)` — one value token, a `slug.Port`
                // source, or an expression.
                let start = i + 2;
                let mut end = start;
                let mut depth = 0usize;
                while let Some(t) = toks.get(end) {
                    match t {
                        Tok::LParen => depth += 1,
                        Tok::RParen if depth == 0 => break,
                        Tok::RParen => depth -= 1,
                        Tok::Comma if depth == 0 => break,
                        _ => {}
                    }
                    end += 1;
                }
                let kind = binding_kind(port, &toks[start..end], lineno)?;
                if let BindingKind::Param(Value::Str(text)) = &kind
                    && (text == "true" || text == "false")
                    && crate::connectors::bool_params(&call.block_type).contains(&port.as_str())
                {
                    return Err(err(bool_hint(port, text)));
                }
                call.args.push(ArgItem::Binding(Binding {
                    port: port.clone(),
                    kind,
                    comment: None,
                }));
                pushed_binding = true;
                i = end;
            }
            Some(_) => {
                return Err(err(
                    "expected `Port: value`, a label string, or `)` in the argument list".into(),
                ));
            }
        }
        // Separator: a comma, or the end of the list / line.
        match toks.get(i) {
            Some(Tok::Comma) => i += 1,
            Some(Tok::RParen) | None => {}
            Some(_) => return Err(err("expected `,` between arguments".into())),
        }
    }
}

/// Classify one binding's value tokens: a literal or constant is a
/// parameter, a `slug.Port` a wire, anything longer an expression.
/// A parenthesized bare port or value canonicalizes to the plain form —
/// one spelling per fact.
fn binding_kind(port: &str, toks: &[Tok], lineno: usize) -> Result<BindingKind> {
    let err = |msg: String| Error::Parse { line: lineno, msg };
    Ok(match toks {
        [] => {
            return Err(err(format!(
                "expected a value after `{port}:`: a number, quoted string, constant, source port (`<slug>.<Port>`) or an expression like `a.Q and b.AQ >= 28`"
            )));
        }
        // A quoted string that reads as a number canonicalizes to the bare
        // number — one canonical spelling per value.
        [Tok::Str(s)] => BindingKind::Param(Value::from_literal(s)),
        [Tok::Num(n)] => BindingKind::Param(Value::Number(n.clone())),
        [Tok::UnitNum(n, u)] => BindingKind::Param(Value::Unit {
            number: n.clone(),
            unit: *u,
        }),
        [Tok::Ident(b)] if b == "on" || b == "off" => BindingKind::Param(Value::Bool(b == "on")),
        [Tok::Ident(name)] if !matches!(name.as_str(), "and" | "or" | "not") => {
            BindingKind::Param(Value::Ref(name.clone()))
        }
        // `At: 19:00` — a time of day.
        clock if clock_toks(clock).is_some() => {
            BindingKind::Param(clock_value(clock, port, lineno)?)
        }
        // `file("hue.c")` — program text from a file; validate
        // decides which ports may take it.
        [Tok::Ident(f), Tok::LParen, Tok::Str(path), Tok::RParen] if f == "file" => {
            if path.is_empty() {
                return Err(err(format!("`{port}: file(\"\")` names no file")));
            }
            BindingKind::Param(Value::File {
                path: path.clone(),
                text: None,
            })
        }
        [Tok::Ident(f), Tok::LParen, ..] if f == "file" => {
            return Err(err(format!(
                "expected `{port}: file(\"<path>\")` with one quoted path, relative to this .loxl file"
            )));
        }
        [Tok::Ident(slug), Tok::Dot, Tok::Ident(src_port)] => BindingKind::Wire(PortRef {
            slug: slug.clone(),
            port: src_port.clone(),
        }),
        [Tok::Ident(slug), Tok::Dot] => {
            return Err(err(format!("expected a port after `{slug}.`")));
        }
        _ => match parse_expr(toks, lineno)? {
            Expr::Atom(Operand::Port(p)) => BindingKind::Wire(p),
            Expr::Atom(Operand::Value(v)) => BindingKind::Param(v),
            e => BindingKind::Expr(e),
        },
    })
}

/// A `YYYY-MM-DD` token run (`2026`, `-`, `01`, `-`, `03`), validated;
/// `None` for any other shape, an error for a malformed or impossible
/// date.
fn date_toks(toks: &[Tok], lineno: usize) -> Result<Option<String>> {
    let [
        Tok::Num(y),
        Tok::Minus,
        Tok::Num(m),
        Tok::Minus,
        Tok::Num(d),
    ] = toks
    else {
        return Ok(None);
    };
    let date = format!("{y}-{m}-{d}");
    if !is_date(&date) {
        return Err(Error::Parse {
            line: lineno,
            msg: format!("`{date}` is not a date (YYYY-MM-DD)"),
        });
    }
    Ok(Some(date))
}

/// `YYYY-MM-DD`, exactly, and a day of the calendar.
fn is_date(d: &str) -> bool {
    d.len() == 10 && crate::logic::rule::parse_date(d).is_some()
}

/// The canonical spec of a test's `clock`: `HH:MM[:SS]` or
/// `YYYY-MM-DD HH:MM[:SS]`, written bare like `At: 19:00`. The v1 quoted
/// form is refused with its replacement.
fn clock_spec(toks: &[Tok], lineno: usize) -> Result<String> {
    let err = |msg: String| Error::Parse { line: lineno, msg };
    let usage = "expected `clock HH:MM[:SS]` or `clock YYYY-MM-DD HH:MM[:SS]`";
    if let [Tok::Str(old)] = toks {
        let (date, time) = match old.trim().split_once(' ') {
            Some((d, t)) => (Some(d), t.trim()),
            None => (None, old.trim()),
        };
        let time = clock_seconds(time).and_then(clock_text);
        return Err(err(match time {
            Some(t) if date.is_none_or(is_date) => {
                let new = date.map_or(t.clone(), |d| format!("{d} {t}"));
                format!(
                    "a test clock takes no quotes. Write `clock {new}` instead of `clock \"{old}\"`."
                )
            }
            _ => format!("`clock \"{old}\"`: a test clock takes no quotes ({usage})"),
        }));
    }
    let (date, time) = match date_toks(toks.get(..5).unwrap_or(&[]), lineno)? {
        Some(d) => (Some(d), &toks[5..]),
        None => (None, toks),
    };
    if clock_toks(time).is_none() {
        return Err(err(usage.into()));
    }
    let t = match clock_value(time, "clock", lineno)? {
        Value::Clock(t) => t,
        other => other.to_string(),
    };
    Ok(date.map_or(t.clone(), |d| format!("{d} {t}")))
}

/// The v2 replacement for a quoted boolean on a text-boolean attribute
/// (`Analog: "true"` → `Analog: on`).
pub(crate) fn bool_hint(port: &str, text: &str) -> String {
    let word = if text == "true" { "on" } else { "off" };
    format!("`{port}:` is on or off. Write `{port}: {word}` instead of `{port}: \"{text}\"`.")
}

/// The `HH:MM[:SS]` text of a time-of-day token run (`19`, `:`, `00`),
/// unvalidated; `None` for any other shape.
fn clock_toks(toks: &[Tok]) -> Option<String> {
    match toks {
        [Tok::Num(h), Tok::Colon, Tok::Num(m)] => Some(format!("{h}:{m}")),
        [
            Tok::Num(h),
            Tok::Colon,
            Tok::Num(m),
            Tok::Colon,
            Tok::Num(s),
        ] => Some(format!("{h}:{m}:{s}")),
        _ => None,
    }
}

/// A time-of-day value, refused unless it is one (00:00 … 23:59:59).
fn clock_value(toks: &[Tok], name: &str, lineno: usize) -> Result<Value> {
    let written = clock_toks(toks).expect("checked by the caller");
    // One spelling per value: `6:58` reads as `06:58`, `19:00:00` as `19:00`.
    match super::ast::clock_seconds(&written).and_then(super::ast::clock_text) {
        Some(text) => Ok(Value::Clock(text)),
        None => Err(Error::Parse {
            line: lineno,
            msg: format!(
                "`{name}`: `{written}` is not a time of day. Write `HH:MM` or `HH:MM:SS`, from 00:00 to 23:59:59."
            ),
        }),
    }
}

fn value_of(tok: &Tok, lineno: usize) -> Result<Value> {
    match tok {
        Tok::Num(n) => Ok(Value::Number(n.clone())),
        Tok::UnitNum(n, u) => Ok(Value::Unit {
            number: n.clone(),
            unit: *u,
        }),
        // A quoted string that reads as a number canonicalizes to the bare
        // number — one canonical spelling per value.
        Tok::Str(s) => Ok(Value::from_literal(s)),
        Tok::Ident(s) if s == "on" || s == "off" => Ok(Value::Bool(s == "on")),
        Tok::Ident(s) => Ok(Value::Ref(s.clone())),
        _ => Err(Error::Parse {
            line: lineno,
            msg: "expected a number, `on`/`off`, quoted string, or constant name".into(),
        }),
    }
}

fn check_slug(s: &str, lineno: usize) -> Result<String> {
    if super::decompile::RESERVED.contains(&s) {
        return Err(Error::Parse {
            line: lineno,
            msg: format!("`{s}` is a reserved word and cannot be used as a name"),
        });
    }
    let ok = s.chars().next().is_some_and(|c| c.is_ascii_lowercase())
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    if ok {
        Ok(s.to_string())
    } else {
        Err(Error::Parse {
            line: lineno,
            msg: format!("invalid slug `{s}` (expected [a-z][a-z0-9_]*)"),
        })
    }
}

/// `text` read as a whole expression, or `None`: decompile reads a block's
/// label back as the expression it was desugared from.
pub(crate) fn expr_text(text: &str) -> Option<Expr> {
    if text.contains('#') {
        return None;
    }
    let (toks, _) = lex(text, 1).ok()?;
    parse_expr(&toks, 1).ok()
}

/// Parse the RHS of `<-` as an expression. Precedence, loosest to
/// tightest: `or` < `and` < `not` < comparison < `+ -` < `* /`; parens
/// group; comparisons take plain operands and do not chain. Arithmetic
/// stands alone — under a gate or a comparison it is deferred (v1) with a
/// parse error. Must consume every token.
fn parse_expr(toks: &[Tok], lineno: usize) -> Result<Expr> {
    let mut p = ExprParser {
        toks,
        pos: 0,
        lineno,
    };
    let e = p.or_level()?;
    if p.pos != toks.len() {
        return Err(p.err("unexpected trailing tokens after the expression".into()));
    }
    Ok(e)
}

struct ExprParser<'t> {
    toks: &'t [Tok],
    pos: usize,
    lineno: usize,
}

impl ExprParser<'_> {
    fn err(&self, msg: String) -> Error {
        Error::Parse {
            line: self.lineno,
            msg,
        }
    }

    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }

    fn keyword(&self, kw: &str) -> bool {
        matches!(self.peek(), Some(Tok::Ident(s)) if s == kw)
    }

    /// Arithmetic mixed under `or`/`and`/`not` is deferred: the analog
    /// `AQ` of a `Formula` feeding a boolean gate input is behavior we have
    /// not verified against the Miniserver.
    fn no_arith(&self, e: &Expr, kw: &str) -> Result<()> {
        if matches!(e, Expr::Arith { .. }) {
            return Err(self.err(format!(
                "arithmetic under `{kw}` is not supported. Declare a `Formula` block and use its `AQ`."
            )));
        }
        Ok(())
    }

    /// A parenthesized group taking part in arithmetic must itself be
    /// arithmetic — gates and comparisons produce on/off, not numbers.
    fn ensure_arith(&self, e: &Expr) -> Result<()> {
        if !matches!(e, Expr::Atom(_) | Expr::Arith { .. }) {
            return Err(self.err(
                "a boolean group cannot take part in arithmetic, since gates and comparisons produce on/off, not numbers"
                    .into(),
            ));
        }
        Ok(())
    }

    fn add_op(&self) -> Option<ArithOp> {
        match self.peek() {
            Some(Tok::Plus) => Some(ArithOp::Add),
            Some(Tok::Minus) => Some(ArithOp::Sub),
            _ => None,
        }
    }

    fn mul_op(&self) -> Option<ArithOp> {
        match self.peek() {
            Some(Tok::Star) => Some(ArithOp::Mul),
            Some(Tok::Slash) => Some(ArithOp::Div),
            _ => None,
        }
    }

    fn arith_op(&self) -> Option<ArithOp> {
        self.add_op().or_else(|| self.mul_op())
    }

    fn or_level(&mut self) -> Result<Expr> {
        let mut e = self.and_level()?;
        while self.keyword("or") {
            self.no_arith(&e, "or")?;
            self.pos += 1;
            let rhs = self.and_level()?;
            self.no_arith(&rhs, "or")?;
            e = Expr::Or(Box::new(e), Box::new(rhs));
        }
        Ok(e)
    }

    fn and_level(&mut self) -> Result<Expr> {
        let mut e = self.unary()?;
        while self.keyword("and") {
            self.no_arith(&e, "and")?;
            self.pos += 1;
            let rhs = self.unary()?;
            self.no_arith(&rhs, "and")?;
            e = Expr::And(Box::new(e), Box::new(rhs));
        }
        Ok(e)
    }

    fn unary(&mut self) -> Result<Expr> {
        if self.keyword("not") {
            self.pos += 1;
            let x = self.unary()?;
            self.no_arith(&x, "not")?;
            return Ok(Expr::Not(Box::new(x)));
        }
        self.primary()
    }

    fn primary(&mut self) -> Result<Expr> {
        let e = if matches!(self.peek(), Some(Tok::LParen)) {
            self.pos += 1;
            let e = self.or_level()?;
            if !matches!(self.peek(), Some(Tok::RParen)) {
                return Err(self.err("missing `)` in the expression".into()));
            }
            self.pos += 1;
            if let Some(Tok::Cmp(op)) = self.peek() {
                return Err(self.err(format!(
                    "`(…) {}`: a comparison takes ports, numbers or constants, not parenthesized expressions",
                    op.symbol()
                )));
            }
            if self.arith_op().is_none() {
                return Ok(e);
            }
            // `(…) * x` — the group is the first factor of an arithmetic
            // expression.
            self.ensure_arith(&e)?;
            self.sum_from(e)?
        } else {
            self.sum()?
        };
        if let Some(Tok::Cmp(op)) = self.peek() {
            let op = *op;
            let Expr::Atom(lhs) = e else {
                return Err(self.err(format!(
                    "arithmetic under `{}` is not supported. Declare a `Formula` block and compare its `AQ`.",
                    op.symbol()
                )));
            };
            self.pos += 1;
            let rhs = self.operand()?;
            if self.arith_op().is_some() {
                return Err(self.err(format!(
                    "arithmetic under `{}` is not supported. Declare a `Formula` block and compare its `AQ`.",
                    op.symbol()
                )));
            }
            if matches!(self.peek(), Some(Tok::Cmp(_))) {
                return Err(self.err(
                    "comparisons do not chain (`a < b < c`). Write two comparisons joined with `and`."
                        .into(),
                ));
            }
            return Ok(Expr::Cmp { op, lhs, rhs });
        }
        Ok(e)
    }

    /// `+`/`-` level; `*`/`/` bind tighter. A single operand comes back as
    /// a plain `Atom`, so non-arithmetic callers are unaffected.
    fn sum(&mut self) -> Result<Expr> {
        let first = self.factor()?;
        self.sum_from(first)
    }

    /// Continue a sum whose first factor is already parsed (also the entry
    /// point after a parenthesized group at primary level).
    fn sum_from(&mut self, first: Expr) -> Result<Expr> {
        let mut e = self.mul_rest(first)?;
        while let Some(op) = self.add_op() {
            self.pos += 1;
            let rhs = self.mul()?;
            e = Expr::Arith {
                op,
                lhs: Box::new(e),
                rhs: Box::new(rhs),
            };
        }
        Ok(e)
    }

    fn mul(&mut self) -> Result<Expr> {
        let first = self.factor()?;
        self.mul_rest(first)
    }

    fn mul_rest(&mut self, mut e: Expr) -> Result<Expr> {
        while let Some(op) = self.mul_op() {
            self.pos += 1;
            let rhs = self.factor()?;
            e = Expr::Arith {
                op,
                lhs: Box::new(e),
                rhs: Box::new(rhs),
            };
        }
        Ok(e)
    }

    fn factor(&mut self) -> Result<Expr> {
        if matches!(self.peek(), Some(Tok::LParen)) {
            self.pos += 1;
            let e = self.or_level()?;
            if !matches!(self.peek(), Some(Tok::RParen)) {
                return Err(self.err("missing `)` in the expression".into()));
            }
            self.pos += 1;
            self.ensure_arith(&e)?;
            return Ok(e);
        }
        Ok(Expr::Atom(self.operand()?))
    }

    fn operand(&mut self) -> Result<Operand> {
        match (
            self.toks.get(self.pos),
            self.toks.get(self.pos + 1),
            self.toks.get(self.pos + 2),
        ) {
            (Some(Tok::Ident(kw)), _, _) if matches!(kw.as_str(), "and" | "or" | "not") => {
                Err(self.err(format!("expected an operand, found `{kw}`")))
            }
            (Some(Tok::Ident(s)), Some(Tok::Dot), Some(Tok::Ident(p))) => {
                let r = Operand::Port(PortRef {
                    slug: s.clone(),
                    port: p.clone(),
                });
                self.pos += 3;
                Ok(r)
            }
            (Some(Tok::Ident(s)), _, _) => {
                let r = Operand::Value(if s == "on" || s == "off" {
                    Value::Bool(s == "on")
                } else {
                    Value::Ref(s.clone())
                });
                self.pos += 1;
                Ok(r)
            }
            (Some(Tok::Num(n)), _, _) => {
                let r = Operand::Value(Value::Number(n.clone()));
                self.pos += 1;
                Ok(r)
            }
            (Some(Tok::UnitNum(n, u)), _, _) => {
                let r = Operand::Value(Value::Unit {
                    number: n.clone(),
                    unit: *u,
                });
                self.pos += 1;
                Ok(r)
            }
            (Some(Tok::Str(_)), _, _) => {
                Err(self.err("an expression cannot contain strings, only numbers and ports".into()))
            }
            _ => Err(self.err("expected an operand: `slug.Port`, a number, or a constant".into())),
        }
    }
}

fn check_type(s: &str, lineno: usize) -> Result<String> {
    let ok = s.chars().next().is_some_and(|c| c.is_ascii_uppercase())
        && s.chars().all(|c| c.is_ascii_alphanumeric());
    if ok {
        Ok(s.to_string())
    } else {
        Err(Error::Parse {
            line: lineno,
            msg: format!("invalid block type `{s}` (expected PascalCase, e.g. GreaterEqual)"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
# Beschattung
extern sonne = VirtualIn(iname: "VI3")
extern jal = AutoJalousie(title: "Beschattung Süd")

let schwelle = 28

temp_hoch = GreaterEqual(
	"Temp über 28",
	Input1: sonne.Q,
	Input2: schwelle,
)
beschatten = And(I1: temp_hoch.Q, I2: sonne.Q)

jal.AutoShade <- beschatten.Q
jal.TargetPos = 70
"#;

    #[test]
    fn parses_sample() {
        let m = Module::parse(SAMPLE).unwrap();
        assert_eq!(m.externs().count(), 2);
        assert_eq!(m.blocks().count(), 2);
        assert_eq!(m.extern_wires().count(), 1);
        assert_eq!(m.wire_pairs().len(), 4);
        assert_eq!(m.sets().count(), 1);
        assert_eq!(m.lets().count(), 1);
        let block = m.blocks().next().unwrap();
        assert_eq!(block.title.as_deref(), Some("Temp über 28"));
        let params: Vec<_> = block.params().collect();
        assert_eq!(params, vec![("Input2", &Value::Ref("schwelle".into()))]);
        assert_eq!(m.resolve_value(params[0].1).unwrap(), "28");
        let wires: Vec<_> = block.input_wires().collect();
        assert_eq!(wires.len(), 1);
        assert_eq!(wires[0].0, "Input1");
        assert_eq!(wires[0].1.to_string(), "sonne.Q");
    }

    #[test]
    fn text_roundtrip() {
        let m = Module::parse(SAMPLE).unwrap();
        let text = m.to_text();
        let again = Module::parse(&text).unwrap();
        assert_eq!(m, again);
        assert_eq!(again.to_text(), text, "canonical form is a fixpoint");
    }

    #[test]
    fn short_calls_go_on_one_line_long_ones_one_argument_per_line() {
        let m = Module::parse(
            "extern x = VirtualIn(iname: \"VI1\")\nb = And(\n\tI1: x.Q,\n\tI2: x.Qm,\n)\n",
        )
        .unwrap();
        let text = m.to_text();
        assert!(text.contains("b = And(I1: x.Q, I2: x.Qm)\n"), "{text}");
        let again = Module::parse(&text).unwrap();
        assert_eq!(m, again);
        assert_eq!(again.to_text(), text);
        // Four arguments (the label counts), a comment in the list, or a
        // line over 100 columns: one argument per line.
        for src in [
            "b = And(\"L\", I1: x.Q, I2: x.Qm, Off: 1)\n",
            "b = And(\n\tI1: x.Q, # why\n\tI2: x.Qm,\n)\n",
            "b = And(\n\t# why\n\tI1: x.Q,\n)\n",
            "b = And(\n\tI1: x.Q,\n) # why\n",
            "b = And(\"A label long enough to push this call past the limit of one hundred columns\", I1: x.Q, I2: x.Qm)\n",
        ] {
            let text = Module::parse_fragment(src).unwrap().to_text();
            assert!(text.contains("b = And(\n"), "{src}: {text}");
        }
        // A trailing comment after a one-line call stays on its line.
        let src = "b = And(I1: x.Q) # why\n";
        assert_eq!(Module::parse_fragment(src).unwrap().to_text(), src);
        // Indentation counts: in a template body a tab is four columns.
        let call = format!("g = And(\"{}\", I1: x.Q, I2: x.Qm)", "l".repeat(68));
        assert_eq!(call.len(), 98);
        let top = Module::parse_fragment(&format!("{call}\n"))
            .unwrap()
            .to_text();
        assert_eq!(top, format!("{call}\n"));
        let body = format!("template t()\n\t{call}\nend\n");
        let text = Module::parse_fragment(&body).unwrap().to_text();
        assert!(text.contains("\tg = And(\n\t\t\"lll"), "{text}");
        assert_eq!(Module::parse_fragment(&text).unwrap().to_text(), text);
        // Argument-free calls stay on one line.
        let m = Module::parse("b = And()\nc = Or(\"Oder\")\n").unwrap();
        let text = m.to_text();
        assert!(text.contains("b = And()\n"), "{text}");
        assert!(text.contains("c = Or(\"Oder\")\n"), "{text}");
    }

    #[test]
    fn comments_are_preserved() {
        let src = "\
# header
extern sonne = VirtualIn(iname: \"VI3\") # the sun
t = GreaterEqual( # threshold block
\t# arg note
\tInput1: sonne.Q, # main wire
\tInput2: 28, # degrees
) # done

sonne.Qm = 30 # override
";
        let m = Module::parse(src).unwrap();
        let text = m.to_text();
        for needle in [
            "# header",
            "\"VI3\") # the sun",
            "GreaterEqual( # threshold block",
            "\t# arg note",
            "sonne.Q, # main wire",
            "28, # degrees",
            ") # done",
            "= 30 # override",
        ] {
            assert!(text.contains(needle), "missing {needle:?} in:\n{text}");
        }
        // Canonical form is a fixpoint.
        let again = Module::parse(&text).unwrap();
        assert_eq!(m, again);
        assert_eq!(again.to_text(), text);
    }

    #[test]
    fn string_values_never_emit_as_unparsable_bare_tokens() {
        // Regression: values like "5+" or "1-2" read as bare-number-ish
        // character runs; a content-sniffing emitter once printed them bare,
        // breaking `parse(to_text(m)) == m`.
        for tricky in ["5+", "1-2", "--1", "1.2.3", "+", "."] {
            let src = format!("extern s = VirtualIn(iname: \"VI1\")\ns.Q = \"{tricky}\"\n");
            let m = Module::parse(&src).unwrap();
            let text = m.to_text();
            let again = Module::parse(&text).unwrap_or_else(|e| {
                panic!("canonical form for {tricky:?} does not re-parse: {e}\n{text}")
            });
            assert_eq!(m, again, "value {tricky:?} must survive the round trip");
        }
        // A quoted number canonicalizes to the bare spelling — one canonical
        // form per value, in port assignments and argument lists alike.
        let m = Module::parse("extern s = VirtualIn(iname: \"VI1\")\ns.Q = \"28\"\n").unwrap();
        assert!(m.to_text().contains("s.Q = 28"), "{}", m.to_text());
        let m = Module::parse("b = GreaterEqual(Input2: \"28\")\n").unwrap();
        assert!(m.to_text().contains("Input2: 28)"), "{}", m.to_text());
    }

    #[test]
    fn unit_values_parse_scale_and_roundtrip() {
        // Canonical form keeps the unit spelling; resolution scales into
        // the base unit, exactly.
        let m = Module::parse(
            "let nachlauf = 90min\n\
             t = Monoflop(Time: 1.5h)\n\
             p = PulseGen(TimeHigh: 250ms, TimeLow: 2s)\n",
        )
        .unwrap();
        let text = m.to_text();
        for needle in ["nachlauf = 90min", "Time: 1.5h)", "TimeHigh: 250ms,"] {
            assert!(text.contains(needle), "missing {needle:?} in:\n{text}");
        }
        assert_eq!(Module::parse(&text).unwrap(), m);
        assert_eq!(Module::parse(&text).unwrap().to_text(), text, "fixpoint");
        let resolved: Vec<String> = m
            .blocks()
            .flat_map(|b| {
                b.params()
                    .map(|(_, v)| m.resolve_value(v).unwrap().into_owned())
            })
            .collect();
        assert_eq!(resolved, ["5400", "0.25", "2"]);
        let nachlauf = m.lets().next().unwrap();
        assert_eq!(
            m.resolve_value(&Value::Ref(nachlauf.name.clone())).unwrap(),
            "5400"
        );

        // K and % are annotations with factor 1; negatives keep the sign.
        for (src, want) in [
            ("2700K", "2700"),
            ("70%", "70"),
            ("-30s", "-30"),
            ("0.3s", "0.3"),
            ("1000ms", "1"),
            ("0ms", "0"),
        ] {
            let m = Module::parse(&format!("t = Monoflop(Time: {src})\n")).unwrap();
            let (_, v) = m.blocks().next().unwrap().params().next().unwrap();
            assert_eq!(m.resolve_value(v).unwrap(), want, "{src}");
            assert!(m.to_text().contains(&format!("Time: {src})")), "{src}");
        }

        // Units work in expressions and template defaults.
        let m = Module::parse(
            "extern a = VirtualIn(iname: \"VI1\")\n\
             extern j = AutoJalousie(title: \"J\")\n\
             j.AutoShade <- a.AQ >= 1.5h\n",
        )
        .unwrap();
        let (plain, _) = m.desugar().unwrap();
        let ge = plain.blocks().next().unwrap();
        assert_eq!(ge.title.as_deref(), Some("a.AQ >= 1.5h"));
        let (_, v) = ge.params().next().unwrap();
        assert_eq!(plain.resolve_value(v).unwrap(), "5400");
        Module::parse("template t(x: VirtualIn, zeit = 5min)\n\ty = Monoflop(Time: zeit)\nend\n")
            .unwrap();

        // Unknown or malformed units are refused at parse time.
        let e = Module::parse("t = Monoflop(Time: 5x)\n").unwrap_err();
        assert!(e.to_string().contains("unknown unit `x`"), "{e}");
        let e = Module::parse("t = Monoflop(Time: 5sec)\n").unwrap_err();
        assert!(e.to_string().contains("unknown unit `sec`"), "{e}");
        // A quoted "40s" is a string, not a unit value — two spellings,
        // two meanings.
        let m = Module::parse("t = Monoflop(Time: \"40s\")\n").unwrap();
        let (_, v) = m.blocks().next().unwrap().params().next().unwrap();
        assert_eq!(v, &Value::Str("40s".into()));
    }

    #[test]
    fn malformed_numbers_are_rejected() {
        for bad in ["1.2.3", "5.", "-5."] {
            let src = format!("b = GreaterEqual(Input2: {bad})\n");
            let e = Module::parse(&src).unwrap_err();
            assert!(
                e.to_string().contains("invalid number literal"),
                "{bad}: {e}"
            );
        }
    }

    #[test]
    fn let_statements_parse_and_resolve() {
        let m = Module::parse(
            "let schwelle = 28\nlet gruss = \"hallo\"\nb = GreaterEqual(Input2: schwelle)\n",
        )
        .unwrap();
        assert_eq!(m.lets().count(), 2);
        let (_, v) = m.blocks().next().unwrap().params().next().unwrap();
        assert_eq!(m.resolve_value(v).unwrap(), "28");

        // Constants cannot reference constants.
        let e = Module::parse("let a = 1\nlet b = a\n").unwrap_err();
        assert!(e.to_string().contains("not another name"), "{e}");

        // Undeclared references are errors, with a suggestion.
        let e =
            Module::parse("let schwelle = 28\nb = GreaterEqual(Input2: schwele)\n").unwrap_err();
        assert!(
            e.to_string().contains("undeclared constant `schwele`"),
            "{e}"
        );
        assert!(e.to_string().contains("did you mean `schwelle`?"), "{e}");

        // Constants share the slug namespace and cannot be wired.
        let e = Module::parse("let x = 1\nx = And()\n").unwrap_err();
        assert!(e.to_string().contains("duplicate name `x`"), "{e}");
        let e = Module::parse("let x = 1\nb = And(I1: x.Q)\n").unwrap_err();
        assert!(e.to_string().contains("not a block or extern"), "{e}");
    }

    #[test]
    fn removed_and_moved_parse_and_are_checked() {
        let m = Module::parse("removed old_block # gone\nmoved a -> b\n").unwrap();
        assert_eq!(m.removed().count(), 1);
        assert_eq!(m.moved().count(), 1);
        let text = m.to_text();
        assert!(text.contains("removed old_block # gone"), "{text}");
        assert!(text.contains("moved a -> b"), "{text}");
        assert_eq!(Module::parse(&text).unwrap(), m);

        let e = Module::parse("a = And()\nremoved a\n").unwrap_err();
        assert!(e.to_string().contains("contradicts the declaration"), "{e}");
        let e = Module::parse("removed a\nremoved a\n").unwrap_err();
        assert!(e.to_string().contains("duplicate `removed a`"), "{e}");

        let e = Module::parse("a = And()\nmoved a -> b\n").unwrap_err();
        assert!(e.to_string().contains("must no longer be declared"), "{e}");
        let e = Module::parse("moved a -> a\n").unwrap_err();
        assert!(e.to_string().contains("to itself"), "{e}");
        let e = Module::parse("moved a -> b\nmoved b -> c\n").unwrap_err();
        assert!(e.to_string().contains("chained `moved`"), "{e}");
        let e = Module::parse("moved a -> b\nremoved a\n").unwrap_err();
        assert!(e.to_string().contains("conflicts with a `removed`"), "{e}");
        let e = Module::parse("extern e = VirtualIn(iname: \"VI1\")\nmoved a -> e\n").unwrap_err();
        assert!(e.to_string().contains("not a managed block"), "{e}");

        let e = Module::parse("removed Bad\n").unwrap_err();
        assert!(e.to_string().contains("invalid slug"), "{e}");
        let e = Module::parse("moved a b\n").unwrap_err();
        assert!(
            e.to_string().contains("moved <old_slug> -> <new_slug>"),
            "{e}"
        );
    }

    #[test]
    fn port_statements_on_managed_blocks_are_rejected() {
        // `=` on a managed port: parameters belong in the argument list.
        let e = Module::parse("b = And()\nb.I1 = 3\n").unwrap_err();
        let msg = e.to_string();
        assert!(msg.contains("targets managed block `b`"), "{msg}");
        assert!(msg.contains("argument list"), "{msg}");
        // `<-` on a managed port: wires belong in the argument list.
        let e = Module::parse("a = And()\nb = And()\nb.I1 <- a.Q\n").unwrap_err();
        let msg = e.to_string();
        assert!(msg.contains("targets managed block `b`"), "{msg}");
        assert!(msg.contains("I1: a.Q"), "{msg}");
    }

    #[test]
    fn duplicate_arguments_are_rejected() {
        let e = Module::parse("b = GreaterEqual(Input2: 1, Input2: 2)\n").unwrap_err();
        assert!(
            e.to_string().contains("duplicate parameter `Input2`"),
            "{e}"
        );
        let e = Module::parse("extern x = VirtualIn(iname: \"VI1\")\nb = And(I1: x.Q, I1: x.Q)\n")
            .unwrap_err();
        assert!(e.to_string().contains("duplicate wire `I1: x.Q`"), "{e}");
        // Two different sources into one port (fan-in) are representable.
        Module::parse("extern x = VirtualIn(iname: \"VI1\")\nb = And(I1: x.Q, I1: x.Qm)\n")
            .unwrap();
    }

    #[test]
    fn label_must_come_first() {
        let e = Module::parse("extern x = VirtualIn(iname: \"VI1\")\nb = And(I1: x.Q, \"L\")\n")
            .unwrap_err();
        assert!(e.to_string().contains("first argument"), "{e}");
        let e = Module::parse("b = And(\"L\", \"M\")\n").unwrap_err();
        assert!(e.to_string().contains("one label"), "{e}");
    }

    #[test]
    fn v0_keywords_carry_migration_guidance() {
        let e = Module::parse("block b: And\n").unwrap_err();
        assert!(e.to_string().contains("old syntax"), "{e}");
        assert!(e.to_string().contains("= <Type>("), "{e}");
        let e = Module::parse("wire a.Q -> b.I1\n").unwrap_err();
        assert!(e.to_string().contains("argument list"), "{e}");
        assert!(e.to_string().contains("<-"), "{e}");
        let e = Module::parse("set a.B = 3\n").unwrap_err();
        assert!(e.to_string().contains("<extern>.<Port> = <value>"), "{e}");
        let e = Module::parse("extern s: VirtualIn match iname \"VI1\"\n").unwrap_err();
        assert!(e.to_string().contains("uuid|iname|title"), "{e}");
    }

    #[test]
    fn wrong_arrow_and_value_forms_are_guided() {
        let e = Module::parse("extern j = AutoJalousie(title: \"J\")\nj.Pos = j.Q\n").unwrap_err();
        assert!(e.to_string().contains("wire a port with `<-`"), "{e}");
        let e = Module::parse("extern j = AutoJalousie(title: \"J\")\nj.Pos <- 5\n").unwrap_err();
        assert!(e.to_string().contains("write `j.Pos = 5`"), "{e}");
        let e = Module::parse("x = 28\n").unwrap_err();
        assert!(e.to_string().contains("let x = 28"), "{e}");
    }

    #[test]
    fn errors_carry_line_numbers() {
        let e = Module::parse("t.Q <- \n").unwrap_err();
        assert!(e.to_string().contains("line 1"), "{e}");
        let e = Module::parse("Bad = And()\n").unwrap_err();
        assert!(e.to_string().contains("invalid slug"), "{e}");
        let e = Module::parse("ok = And(\nI1: x.Q,\n").unwrap_err();
        assert!(e.to_string().contains("unclosed"), "{e}");
    }

    #[test]
    fn undeclared_reference_is_rejected() {
        let e = Module::parse("a.Q <- b.I\n").unwrap_err();
        assert!(e.to_string().contains("undeclared slug"), "{e}");
    }

    #[test]
    fn page_statements_parse_and_roundtrip() {
        let src = "extern wind = VirtualIn(iname: \"VI2\")\n\n\
                   page \"Beschattung\" # Süd\n\n\
                   gate = Not(I: wind.Q)\n\n\
                   page \"Regeln\"\n\n\
                   halt = Or(I1: gate.Q)\n";
        let m = Module::parse(src).unwrap();
        assert_eq!(m.to_text(), src);
        let pages: Vec<_> = m
            .items
            .iter()
            .filter_map(|i| match i {
                Item::Page(p) => Some(p.title.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(pages, ["Beschattung", "Regeln"]);

        // The title is a quoted string — a bare identifier is guided.
        let e = Module::parse("page Regeln\n").unwrap_err();
        assert!(e.to_string().contains("page \"<Title>\""), "{e}");
        // Placement is per-module, not per-template.
        let e = Module::parse("template t(a: VirtualIn)\n\tpage \"X\"\nend\n").unwrap_err();
        assert!(
            e.to_string().contains("not allowed inside a template"),
            "{e}"
        );
        // An empty title names no page.
        let e = Module::parse("page \"\"\n").unwrap_err();
        assert!(e.to_string().contains("must not be empty"), "{e}");
    }

    #[test]
    fn on_off_are_digital_literals() {
        let src = "let an = on\n\n\
                   b = And(I1: x.Q, I2: on)\n\
                   c = GreaterEqual(Input1: x.AQ, Input2: off)\n\n\
                   x.Value = an\n";
        let m = Module::parse_fragment(src).unwrap();
        assert_eq!(m.to_text(), src);
        let lets: Vec<_> = m.lets().map(|l| l.value.clone()).collect();
        assert_eq!(lets, [Value::Bool(true)]);
        // A connector stores 1/0; a text-boolean attribute true/false.
        let on = Value::Bool(true);
        assert_eq!(m.resolve_value(&on).unwrap(), "1");
        assert_eq!(m.resolve_value(&Value::Bool(false)).unwrap(), "0");
        assert_eq!(m.resolve_value_for(&on, "And", "b", "I2").unwrap(), "1");
        assert_eq!(
            m.resolve_value_for(&on, "VirtualIn", "v", "Analog")
                .unwrap(),
            "true"
        );
        let via_let = Value::Ref("an".into());
        assert_eq!(
            m.resolve_value_for(&via_let, "VirtualUdpInCmd", "v", "Signed")
                .unwrap(),
            "true"
        );
        // A number stays a number: no 1 -> on canonicalization.
        let m = Module::parse_fragment("b = And(I2: 1)\n").unwrap();
        assert!(m.to_text().contains("I2: 1)"));
        // Reserved: neither can name anything.
        for src in ["on = And()\n", "let off = 1\n"] {
            let e = Module::parse_fragment(src).unwrap_err();
            assert!(e.to_string().contains("reserved word"), "{src}: {e}");
        }
        // v1's quoted boolean on a text-boolean attribute names its
        // replacement; elsewhere a quoted "true" stays a string.
        let e = Module::parse_fragment("v = VirtualIn(Analog: \"false\")\n").unwrap_err();
        assert!(
            e.to_string()
                .contains("Write `Analog: off` instead of `Analog: \"false\"`"),
            "{e}"
        );
        assert!(Module::parse_fragment("f = Formula(Formula: \"true\")\n").is_ok());
        // Template value parameters take on/off as defaults.
        let src = "template t(an = on)\n\tg = And(I1: x.Q, I2: an)\nend\n";
        assert_eq!(Module::parse_fragment(src).unwrap().to_text(), src);
    }

    #[test]
    fn test_time_tick_clock_and_not_equal() {
        let src = "test \"t\"\n\
                   \tclock 07:30\n\
                   \tclock 2026-01-03 07:30:15\n\
                   \tx.Q = on\n\
                   \ttick 5min\n\
                   \ttick 2s dt 500ms\n\
                   \ttick 3 dt 0.5\n\
                   \texpect x.Q != off\n\
                   \texpect x.AQ == 1\n\
                   end\n";
        let m = Module::parse_fragment(src).unwrap();
        assert_eq!(m.to_text(), src);
        let Item::Test(t) = &m.items[0] else {
            panic!("a test");
        };
        let ticks: Vec<u64> = t
            .body
            .iter()
            .filter_map(|i| match i {
                TestItem::Tick(t) => t.count(|_| None).unwrap(),
                _ => None,
            })
            .collect();
        assert_eq!(ticks, [300, 4, 3]);
        // A time of day canonicalizes like `At:`.
        let m = Module::parse_fragment("test \"t\"\n\tclock 7:30:00\n\ttick 1\nend\n").unwrap();
        assert!(m.to_text().contains("\tclock 07:30\n"), "{}", m.to_text());

        let err = |body: &str| {
            Module::parse_fragment(&format!("test \"t\"\n{body}\nend\n"))
                .unwrap_err()
                .to_string()
        };
        // The v1 quoted clock names its replacement.
        let e = err("\tclock \"2026-01-03 7:30\"");
        assert!(
            e.contains("Write `clock 2026-01-03 07:30` instead of `clock \"2026-01-03 7:30\"`"),
            "{e}"
        );
        assert!(err("\tclock \"spät\"").contains("takes no quotes"));
        assert!(err("\tclock 2026-02-30 07:00").contains("not a date"));
        assert!(err("\tclock 25:00").contains("not a time of day"));
        // A duration is a whole number of ticks.
        let e = err("\ttick 1s dt 300ms");
        assert!(e.contains("not a whole number of `dt 300ms` ticks"), "{e}");
        assert!(err("\ttick 0s").contains("advances nothing"));
        assert!(err("\ttick 5%").contains("duration"));
    }

    #[test]
    fn rule_one_word_commands_and_dates_are_bare() {
        let src = "rule r\n\
                   \twhen date 2026-12-24\n\
                   \tthen pumpe on\n\
                   \tthen licht \"Mood 2\"\n\
                   \tthen x \"+\"\n\
                   end\n";
        let m = Module::parse_fragment(src).unwrap();
        assert_eq!(m.to_text(), src);
        // A quoted one-word command canonicalizes, like `"28"` → `28`.
        let quoted = Module::parse_fragment("rule r\n\tthen pumpe \"pulse\"\nend\n").unwrap();
        assert_eq!(quoted.to_text(), "rule r\n\tthen pumpe pulse\nend\n");
        let e = Module::parse_fragment("rule r\n\twhen date \"2026-12-24\"\nend\n").unwrap_err();
        assert!(
            e.to_string()
                .contains("Write `when date 2026-12-24` instead of `when date \"2026-12-24\"`"),
            "{e}"
        );
    }

    #[test]
    fn the_statement_error_lists_every_statement() {
        let e = Module::parse_fragment("42\n").unwrap_err().to_string();
        for kind in [
            "`let",
            "`extern",
            "`page",
            "`template",
            "`test",
            "`tiles",
            "`schedule",
            "`moods",
            "`rule",
            "`access",
            "`recipients",
            "`removed",
            "`moved",
        ] {
            assert!(e.contains(kind), "{kind} missing: {e}");
        }
    }
}
