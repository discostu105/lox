//! Rewrite a v2 module's spellings to v3: what `loxl fmt --write` does
//! before it formats, so a v2 module migrates with one command.
//!
//! [`migrate_port_names`] renames the port keys to the source names
//! (`InputTrigger` → `Trigger`, `I1` → `In1`). A key is renamed where
//! its type is known: a block's own arguments and `defaults` by the block
//! type, a `slug.Port` reference by the declared type of `slug` (an
//! extern, a block, a template's object parameter or body block).
//! Everything else stays as written.
//!
//! [`migrate_test_steps`] works on the text, before the parse: a test's
//! `tick <n>` with a bare count is a parse error in v3, and becomes
//! `step <n>`. A calendar entry's `on:` becomes `every:` in
//! [`crate::logic::ModeSpellings::apply`], which `fmt` runs too.

use super::ast::{ArgItem, BindingKind, Expr, Item, Module, Operand, PortRef, TestItem};
use crate::port_names::renamed;
use std::collections::BTreeMap;

/// Rename every v2 port key of `module` whose port the table knows,
/// looking up types in `whole` (the module's fragments, `module` among
/// them). Returns how many keys changed.
pub fn migrate_port_names(module: &mut Module, whole: &[&Module]) -> usize {
    let mut types: BTreeMap<String, String> = BTreeMap::new();
    for m in whole {
        for item in &m.items {
            match item {
                Item::Extern(e) => {
                    types.insert(e.slug.clone(), e.block_type.clone());
                }
                Item::Block(b) => {
                    types.insert(b.slug.clone(), b.block_type.clone());
                }
                _ => {}
            }
        }
    }
    let mut n = 0;
    for item in &mut module.items {
        if let Item::Template(t) = item {
            // The body's own names shadow the module's.
            let mut local = types.clone();
            for p in &t.params {
                if let super::ast::TemplateParam::Object { name, block_type } = p {
                    local.insert(name.clone(), block_type.clone());
                }
            }
            for body in &t.body {
                if let Item::Block(b) = body {
                    local.insert(b.slug.clone(), b.block_type.clone());
                }
            }
            for body in &mut t.body {
                n += migrate_item(body, &local);
            }
        } else {
            n += migrate_item(item, &types);
        }
    }
    n
}

fn migrate_item(item: &mut Item, types: &BTreeMap<String, String>) -> usize {
    let mut n = 0;
    let ty = |slug: &str| types.get(slug).cloned().unwrap_or_default();
    let mut port = |r: &mut PortRef| {
        let t = ty(&r.slug);
        n += fix_ref(r, &t);
    };
    match item {
        Item::Block(b) => {
            let bt = b.block_type.clone();
            n += migrate_args(&mut b.args, &bt, types);
        }
        Item::Defaults(d) => {
            let bt = d.block_type.clone();
            n += migrate_args(&mut d.args, &bt, types);
        }
        Item::Wire(w) => {
            port(&mut w.to);
            port(&mut w.from);
        }
        Item::ExprWire(w) => {
            port(&mut w.to);
            n += fix_expr(&mut w.expr, types);
        }
        Item::Signal(x) => n += fix_expr(&mut x.expr, types),
        Item::Set(s) => port(&mut s.target),
        Item::Test(t) => {
            for stmt in &mut t.body {
                match stmt {
                    TestItem::Inject(s) => port(&mut s.target),
                    TestItem::Expect(e) => port(&mut e.port),
                    _ => {}
                }
            }
        }
        _ => {}
    }
    n
}

fn migrate_args(args: &mut [ArgItem], block_type: &str, types: &BTreeMap<String, String>) -> usize {
    let mut n = 0;
    for arg in args {
        let ArgItem::Binding(b) = arg else { continue };
        if let Some(name) = renamed(block_type, &b.port) {
            b.port = name.to_string();
            n += 1;
        }
        match &mut b.kind {
            BindingKind::Wire(r) => {
                let t = types.get(&r.slug).cloned().unwrap_or_default();
                n += fix_ref(r, &t);
            }
            BindingKind::Expr(e) => n += fix_expr(e, types),
            BindingKind::Param(_) => {}
        }
    }
    n
}

fn fix_ref(r: &mut PortRef, block_type: &str) -> usize {
    if r.port.is_empty() {
        return 0;
    }
    match renamed(block_type, &r.port) {
        Some(name) => {
            r.port = name.to_string();
            1
        }
        None => 0,
    }
}

fn fix_expr(e: &mut Expr, types: &BTreeMap<String, String>) -> usize {
    let mut n = 0;
    let mut fix_operand = |o: &mut Operand| match o {
        Operand::Port(p) => {
            let t = types.get(&p.slug).cloned().unwrap_or_default();
            n += fix_ref(p, &t);
        }
        Operand::Arith(x) => n += fix_expr(x, types),
        Operand::Value(_) => {}
    };
    match e {
        Expr::Or(a, b) | Expr::And(a, b) | Expr::Arith { lhs: a, rhs: b, .. } => {
            n += fix_expr(a, types);
            n += fix_expr(b, types);
        }
        Expr::Not(x) => n += fix_expr(x, types),
        Expr::Cmp { lhs, rhs, .. } => {
            fix_operand(lhs);
            fix_operand(rhs);
        }
        Expr::Atom(o) => fix_operand(o),
    }
    n
}

/// `tick <n>` with a bare count becomes `step <n>` (`tick 3 dt 0.5` →
/// `step 3 dt 0.5`), line by line. `tick` is a reserved word that only a
/// test body takes, so a line that starts with it is that statement.
/// Comments and the rest of the line stay as written.
pub fn migrate_test_steps(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    for line in src.split_inclusive('\n') {
        let body = line.trim_start();
        let indent = &line[..line.len() - body.len()];
        let migrated = body.strip_prefix("tick").and_then(|rest| {
            let after = rest.trim_start();
            if after.len() == rest.len() {
                return None; // `tickle`, `tick_x`
            }
            let digits = after.trim_start_matches(|c: char| c.is_ascii_digit());
            let is_count = digits.len() < after.len()
                && digits
                    .chars()
                    .next()
                    .is_none_or(|c| c.is_whitespace() || c == '#');
            is_count.then(|| format!("{indent}step{rest}"))
        });
        match migrated {
            Some(l) => out.push_str(&l),
            None => out.push_str(line),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_tick_counts_become_steps() {
        let src = "test \"t\"\n\
                   \ttick 3 # three cycles\n\
                   \ttick 2 dt 0.5\n\
                   \ttick 5min\n\
                   \ttick 1s dt 500ms\n\
                   \ttick warten\n\
                   \t# tick 7 in a comment stays\n\
                   end\n";
        let out = migrate_test_steps(src);
        assert_eq!(
            out,
            "test \"t\"\n\
             \tstep 3 # three cycles\n\
             \tstep 2 dt 0.5\n\
             \ttick 5min\n\
             \ttick 1s dt 500ms\n\
             \ttick warten\n\
             \t# tick 7 in a comment stays\n\
             end\n"
        );
        assert!(Module::parse_fragment(&out).is_ok());
        assert_eq!(migrate_test_steps(&out), out);
        assert_eq!(migrate_test_steps("tick 5"), "step 5");
        assert_eq!(migrate_test_steps("ticks 5\n"), "ticks 5\n");
    }

    #[test]
    fn keys_become_names_where_the_type_is_known() {
        let mut m = Module::parse_fragment(
            "extern s = VirtualIn(iname: \"VI1\")\n\
             extern jal = AutoJalousie(title: \"J\")\n\
             a = Monoflop(InputTrigger: s.Q, Time: 2s)\n\
             b = And(I1: a.Q, I2: s.Q)\n\
             jal.InputTriggerUp <- b.Q\n\
             jal.TimeEnd = 70\n\
             x = s.Q and a.Q\n\
             template t(j: AutoJalousie)\n\
             \tm = Monoflop(InputTrigger: j.OutputUp)\n\
             end\n\
             test \"t\"\n\
             \ta.InputTrigger = 1\n\
             \tstep 1\n\
             \texpect b.Q == 1\n\
             end\n",
        )
        .unwrap();
        let whole = [m.clone()];
        let n = migrate_port_names(&mut m, &whole.iter().collect::<Vec<_>>());
        let text = m.to_text();
        assert!(
            text.contains("a = Monoflop(Trigger: s.Q, Time: 2s)"),
            "{text}"
        );
        assert!(text.contains("b = And(In1: a.Q, In2: s.Q)"), "{text}");
        assert!(text.contains("jal.TriggerUp <- b.Q"), "{text}");
        assert!(text.contains("jal.OpenTime = 70"), "{text}");
        assert!(text.contains("m = Monoflop(Trigger: j.MotorUp)"), "{text}");
        assert!(text.contains("a.Trigger = 1"), "{text}");
        assert_eq!(n, 8, "{text}");
        // A second run changes nothing.
        assert_eq!(migrate_port_names(&mut m, &[]), 0);
    }
}
