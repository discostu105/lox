//! Slug rename across a module and its lockfile — the engine of
//! `lxir rename`.
//!
//! A rename is a refactoring: the compiled output must not change, except
//! for display titles the slug itself feeds — a block without an explicit
//! label uses its slug as `Title=`, and D24 expression blocks label
//! themselves with their sub-expression text. The CLI verifies exactly
//! that before writing anything.
//!
//! The lockfile side cannot be a plain rekey of `old` alone: template
//! expansion and expression desugaring derive *synthetic* slugs from
//! declared ones (`<instance>_<body>`, `<sink>_<port>__<op><n>`), so
//! renaming an instance or an expression sink shifts lock keys the source
//! never spells out. [`lock_rekeys`] therefore expands and desugars the
//! module before and after the rename and pairs the two item lists
//! positionally — the structure is identical, only names differ.

use super::ast::{
    AccessItem, ArgItem, BindingKind, BlockDecl, Expr, Item, MatchSpec, Module, MoodsItem, Operand,
    RuleItem, RuleStmtKind, ScheduleItem, TemplateDecl, TestItem, TilesItem, Value,
};
use crate::error::{Error, Result};
use crate::lock::Lockfile;

/// Which lockfile map a rekey touches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RekeyKind {
    Object,
    Extern,
}

/// One lockfile key move implied by a rename: `(old, new, kind)`.
pub type Rekey = (String, String, RekeyKind);

/// The slug rules the parser enforces, without a source line: lowercase
/// `[a-z][a-z0-9_]*`, not a reserved word.
pub fn valid_slug(s: &str) -> Result<()> {
    if super::decompile::RESERVED.contains(&s) {
        return Err(Error::Compile(format!(
            "`{s}` is a reserved word and cannot be used as a name"
        )));
    }
    let ok = s.chars().next().is_some_and(|c| c.is_ascii_lowercase())
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    if ok {
        Ok(())
    } else {
        Err(Error::Compile(format!(
            "invalid slug `{s}` (expected [a-z][a-z0-9_]*)"
        )))
    }
}

/// Rename every occurrence of the module-level name `old` to `new`:
/// declarations, port references, wire endpoints, expression operands,
/// `mirrors:` targets, constant references, template-body captures (unless
/// the template's local namespace shadows `old`), `moved` targets — and
/// word-boundary mentions inside comments, so prose stays truthful.
pub fn rename_slug(module: &mut Module, old: &str, new: &str) {
    for item in &mut module.items {
        rename_item(item, old, new);
    }
}

fn rename_item(item: &mut Item, old: &str, new: &str) {
    let sub = |s: &mut String| {
        if s == old {
            *s = new.to_string();
        }
    };
    let sub_comment = |c: &mut Option<String>| {
        if let Some(text) = c {
            sub_text(text, old, new);
        }
    };
    match item {
        Item::Extern(e) => {
            sub(&mut e.slug);
            if let MatchSpec::Mirrors(target) = &mut e.match_spec {
                sub(target);
            }
            sub_comment(&mut e.comment);
        }
        Item::Block(b) => {
            sub(&mut b.slug);
            rename_block_args(b, old, new);
        }
        Item::Instance(call) => {
            sub(&mut call.slug);
            // The callee is a template name — same module namespace.
            sub(&mut call.block_type);
            rename_block_args(call, old, new);
        }
        Item::Wire(w) => {
            sub(&mut w.to.slug);
            sub(&mut w.from.slug);
            sub_comment(&mut w.comment);
        }
        Item::ExprWire(w) => {
            sub(&mut w.to.slug);
            rename_expr(&mut w.expr, old, new);
            sub_comment(&mut w.comment);
        }
        Item::Set(s) => {
            sub(&mut s.target.slug);
            rename_value(&mut s.value, old, new);
            sub_comment(&mut s.comment);
        }
        Item::Attr(a) => {
            sub(&mut a.slug);
            rename_value(&mut a.value, old, new);
            sub_comment(&mut a.comment);
        }
        Item::Let(l) => {
            sub(&mut l.name);
            sub_comment(&mut l.comment);
        }
        // A `removed` slug names a lockfile entry of the past, never a
        // declared name (validate forbids the overlap) — leave it.
        Item::Removed(r) => sub_comment(&mut r.comment),
        // `moved from` likewise names an old lock key; the target is the
        // declared block.
        Item::Moved(m) => {
            sub(&mut m.to);
            sub_comment(&mut m.comment);
        }
        Item::Template(t) => {
            sub(&mut t.name);
            // Body identifiers resolve template-locally first: when a
            // parameter or body slug shadows `old`, the body never sees
            // the module-level name and must stay untouched.
            if !shadows(t, old) {
                for body_item in &mut t.body {
                    rename_item(body_item, old, new);
                }
            }
            sub_comment(&mut t.comment);
            sub_comment(&mut t.end_comment);
        }
        Item::Page(p) => sub_comment(&mut p.comment),
        Item::Test(t) => {
            for stmt in &mut t.body {
                match stmt {
                    TestItem::Inject(s) => {
                        sub(&mut s.target.slug);
                        rename_value(&mut s.value, old, new);
                        sub_comment(&mut s.comment);
                    }
                    TestItem::Expect(e) => {
                        sub(&mut e.port.slug);
                        rename_value(&mut e.value, old, new);
                        sub_comment(&mut e.comment);
                    }
                    TestItem::Tick(t) => sub_comment(&mut t.comment),
                    TestItem::Clock(c) => sub_comment(&mut c.comment),
                    TestItem::Comment(text) => sub_text(text, old, new),
                }
            }
            sub_comment(&mut t.comment);
            sub_comment(&mut t.end_comment);
        }
        // `to:` targets are slugs; every other tile value is a quoted
        // string, which `rename_value` leaves alone.
        Item::Tiles(t) => {
            sub(&mut t.name);
            for stmt in &mut t.body {
                match stmt {
                    TilesItem::Tile(tile) => {
                        for arg in &mut tile.args {
                            match arg {
                                ArgItem::Binding(b) => {
                                    if let BindingKind::Param(v) = &mut b.kind {
                                        rename_value(v, old, new);
                                    }
                                    sub_comment(&mut b.comment);
                                }
                                ArgItem::Comment(text) => sub_text(text, old, new),
                            }
                        }
                        sub_comment(&mut tile.comment);
                        sub_comment(&mut tile.close_comment);
                    }
                    TilesItem::Comment(text) => sub_text(text, old, new),
                }
            }
            sub_comment(&mut t.comment);
            sub_comment(&mut t.end_comment);
        }
        Item::TilesClaim(c) => {
            sub(&mut c.slug);
            sub(&mut c.tiles);
            sub_comment(&mut c.comment);
        }
        // Principals and schedules are titles, never slugs.
        Item::Access(a) => {
            sub(&mut a.slug);
            for stmt in &mut a.body {
                match stmt {
                    AccessItem::Grant(g) => sub_comment(&mut g.comment),
                    AccessItem::Comment(text) => sub_text(text, old, new),
                }
            }
            sub_comment(&mut a.comment);
            sub_comment(&mut a.end_comment);
        }
        Item::Schedule(t) => {
            sub(&mut t.name);
            for stmt in &mut t.body {
                match stmt {
                    ScheduleItem::Line(l) => sub_comment(&mut l.comment),
                    ScheduleItem::Comment(text) => sub_text(text, old, new),
                }
            }
            sub_comment(&mut t.comment);
            sub_comment(&mut t.end_comment);
        }
        Item::Rule(r) => {
            sub(&mut r.slug);
            for stmt in &mut r.body {
                match stmt {
                    RuleItem::Stmt(st) => {
                        if let RuleStmtKind::Then { target, .. } = &mut st.kind {
                            sub(target);
                        }
                        sub_comment(&mut st.comment);
                    }
                    RuleItem::Comment(text) => sub_text(text, old, new),
                }
            }
            sub_comment(&mut r.comment);
            sub_comment(&mut r.end_comment);
        }
        Item::ScheduleClaim(c) => {
            sub(&mut c.slug);
            sub(&mut c.schedule);
            sub_comment(&mut c.comment);
        }
        Item::Moods(t) => {
            sub(&mut t.name);
            for stmt in &mut t.body {
                match stmt {
                    MoodsItem::Mood(m) => sub_comment(&mut m.comment),
                    MoodsItem::Comment(text) => sub_text(text, old, new),
                }
            }
            sub_comment(&mut t.comment);
            sub_comment(&mut t.end_comment);
        }
        Item::MoodsClaim(c) => {
            sub(&mut c.slug);
            sub(&mut c.moods);
            sub_comment(&mut c.comment);
        }
        Item::Comment(text) => sub_text(text, old, new),
    }
}

fn shadows(t: &TemplateDecl, name: &str) -> bool {
    t.params.iter().any(|p| p.name() == name)
        || t.body
            .iter()
            .any(|i| matches!(i, Item::Block(b) if b.slug == name))
}

fn rename_block_args(b: &mut BlockDecl, old: &str, new: &str) {
    for arg in &mut b.args {
        match arg {
            ArgItem::Binding(binding) => {
                // `binding.port` is a connector or template-parameter name
                // — a different namespace, never renamed.
                match &mut binding.kind {
                    BindingKind::Param(v) => rename_value(v, old, new),
                    BindingKind::Wire(p) => {
                        if p.slug == old {
                            p.slug = new.to_string();
                        }
                    }
                    BindingKind::Expr(e) => rename_expr(e, old, new),
                }
                if let Some(text) = &mut binding.comment {
                    sub_text(text, old, new);
                }
            }
            ArgItem::Comment(text) => sub_text(text, old, new),
        }
    }
    if let Some(text) = &mut b.comment {
        sub_text(text, old, new);
    }
    if let Some(text) = &mut b.close_comment {
        sub_text(text, old, new);
    }
}

fn rename_expr(e: &mut Expr, old: &str, new: &str) {
    match e {
        Expr::Or(a, b) | Expr::And(a, b) => {
            rename_expr(a, old, new);
            rename_expr(b, old, new);
        }
        Expr::Not(x) => rename_expr(x, old, new),
        Expr::Cmp { lhs, rhs, .. } => {
            rename_operand(lhs, old, new);
            rename_operand(rhs, old, new);
        }
        Expr::Arith { lhs, rhs, .. } => {
            rename_expr(lhs, old, new);
            rename_expr(rhs, old, new);
        }
        Expr::Atom(o) => rename_operand(o, old, new),
    }
}

fn rename_operand(o: &mut Operand, old: &str, new: &str) {
    match o {
        Operand::Port(p) => {
            if p.slug == old {
                p.slug = new.to_string();
            }
        }
        Operand::Value(v) => rename_value(v, old, new),
    }
}

fn rename_value(v: &mut Value, old: &str, new: &str) {
    if let Value::Ref(r) = v
        && r == old
    {
        *r = new.to_string();
    }
}

/// Replace word-boundary occurrences of `old` in free text (comments):
/// an occurrence counts when neither neighbor is a slug character, so
/// `alarm` matches in "the alarm fires" and `alarm.Q`, but not inside
/// `voralarm` or `alarm_2`.
fn sub_text(text: &mut String, old: &str, new: &str) {
    let is_slug_char = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_';
    let s = text.as_str();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    let mut changed = false;
    while let Some(rel) = s[i..].find(old) {
        let start = i + rel;
        let end = start + old.len();
        let before_ok = !s[..start].chars().next_back().is_some_and(is_slug_char);
        let after_ok = !s[end..].chars().next().is_some_and(is_slug_char);
        out.push_str(&s[i..start]);
        if before_ok && after_ok {
            out.push_str(new);
            changed = true;
        } else {
            out.push_str(old);
        }
        i = end;
    }
    if changed {
        out.push_str(&s[i..]);
        *text = out;
    }
}

/// The lockfile key moves a rename implies. Expands and desugars the
/// module before and after the rename and pairs the item lists
/// positionally — this catches synthetic slugs (template bodies,
/// expression blocks) whose lock keys shift with a declared name.
pub fn lock_rekeys(old_module: &Module, new_module: &Module) -> Result<Vec<Rekey>> {
    let flatten = |m: &Module| -> Result<Vec<(RekeyKind, String)>> {
        let (desugared, _) = m.expand()?.desugar()?;
        Ok(desugared
            .items
            .iter()
            .filter_map(|i| match i {
                Item::Block(b) => Some((RekeyKind::Object, b.slug.clone())),
                Item::Extern(e) => Some((RekeyKind::Extern, e.slug.clone())),
                _ => None,
            })
            .collect())
    };
    let before = flatten(old_module)?;
    let after = flatten(new_module)?;
    if before.len() != after.len() || !before.iter().zip(&after).all(|((ka, _), (kb, _))| ka == kb)
    {
        return Err(Error::Compile(
            "internal: the rename changed the module's structure".into(),
        ));
    }
    Ok(before
        .into_iter()
        .zip(after)
        .filter(|((_, a), (_, b))| a != b)
        .map(|((kind, a), (_, b))| (a, b, kind))
        .collect())
}

/// Apply the rekeys to a lockfile. Entries the lock does not carry are
/// skipped — a block or extern that has never been compiled has no
/// identity to keep. Returns how many entries moved.
pub fn apply_rekeys(lock: &mut Lockfile, rekeys: &[Rekey]) -> Result<usize> {
    let mut moved = 0;
    for (old, new, kind) in rekeys {
        match kind {
            RekeyKind::Object => {
                if lock.objects.contains_key(old.as_str()) {
                    lock.rename_object(old, new)?;
                    moved += 1;
                }
            }
            RekeyKind::Extern => {
                if let Some(entry) = lock.externals.remove(old.as_str()) {
                    if lock.externals.contains_key(new.as_str()) {
                        return Err(Error::Lock(format!(
                            "extern slug `{new}` already exists in lock"
                        )));
                    }
                    lock.externals.insert(new.clone(), entry);
                    moved += 1;
                }
            }
        }
    }
    Ok(moved)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sub_text_respects_boundaries() {
        let mut t = "alarm fires; voralarm and alarm_2 stay; see alarm.Q".to_string();
        sub_text(&mut t, "alarm", "sirene");
        assert_eq!(t, "sirene fires; voralarm and alarm_2 stay; see sirene.Q");
    }

    #[test]
    fn rename_covers_references_and_comments() {
        let src = "\
extern sonne = VirtualIn(iname: \"VI3\") # sonne feeds the gate
extern sonne_ref = InputRef(mirrors: sonne)

gate = And(
\tI1: sonne.Q, # from sonne
\tI2: sonne_ref.Q,
)

sonne_ref.AI <- sonne.AQ
";
        let mut m = Module::parse(src).unwrap();
        rename_slug(&mut m, "sonne", "sonnenschein");
        let text = m.to_text();
        assert!(!text.contains("sonne "), "stale slug in:\n{text}");
        assert!(text.contains("extern sonnenschein = VirtualIn"));
        assert!(text.contains("mirrors: sonnenschein"));
        assert!(text.contains("# sonnenschein feeds the gate"));
        assert!(text.contains("sonne_ref.AI <- sonnenschein.AQ"));
        // The prefixed sibling slug is untouched.
        assert!(text.contains("extern sonne_ref = InputRef"));
    }

    #[test]
    fn template_shadowing_protects_bodies() {
        let src = "\
extern takt = VirtualIn(iname: \"VI1\")

template blinker(takt: VirtualIn)
\tgate = And(
\t\tI1: takt.Q,
\t)
end

b1 = blinker(takt: takt)
";
        let mut m = Module::parse(src).unwrap();
        rename_slug(&mut m, "takt", "sekundentakt");
        let text = m.to_text();
        // The template parameter and its body use stay `takt`; the module
        // extern and the instance ARGUMENT are renamed.
        assert!(text.contains("extern sekundentakt = VirtualIn"));
        assert!(text.contains("template blinker(takt: VirtualIn)"));
        assert!(text.contains("I1: takt.Q"));
        assert!(
            text.contains("takt: sekundentakt,"),
            "instance arg:\n{text}"
        );
    }

    #[test]
    fn rekeys_track_synthetic_slugs() {
        let src = "\
extern a = VirtualIn(iname: \"VI1\")
extern b = VirtualIn(iname: \"VI2\")
extern sink = VirtualIn(iname: \"VI3\")

sink.I <- a.Q and b.Q
";
        let old = Module::parse(src).unwrap();
        let mut renamed = old.clone();
        rename_slug(&mut renamed, "sink", "ziel");
        let rekeys = lock_rekeys(&old, &renamed).unwrap();
        // The extern itself plus the expression-owned And block whose
        // synthetic slug carries the sink's name.
        assert!(
            rekeys
                .iter()
                .any(|(o, n, k)| o == "sink" && n == "ziel" && *k == RekeyKind::Extern)
        );
        assert!(
            rekeys.iter().any(|(o, n, k)| o.starts_with("sink_")
                && n.starts_with("ziel_")
                && *k == RekeyKind::Object),
            "expression block not rekeyed: {rekeys:?}"
        );
    }
}
