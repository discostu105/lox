//! `defaults <Type>(Port: value, …)`: parameter values shared by every
//! managed block of a type. [`Module::expand`] applies them, so
//! everything after expansion — validation, the compile, the lockfile —
//! sees only blocks that bind each port themselves, exactly as if every
//! block had written the value. [`factor`] goes the other way for
//! decompile and adopt.

use std::collections::BTreeMap;

use crate::error::{Error, Result};
use crate::logic::ast::{ArgItem, Binding, BindingKind, BlockDecl, DefaultsDecl, Item, Value};

/// The `defaults` statements of `items` by block type. A type takes one.
pub(crate) fn by_type(items: &[Item]) -> Result<BTreeMap<&str, &DefaultsDecl>> {
    let mut out: BTreeMap<&str, &DefaultsDecl> = BTreeMap::new();
    for item in items {
        if let Item::Defaults(d) = item
            && out.insert(d.block_type.as_str(), d).is_some()
        {
            return Err(Error::Compile(format!(
                "two `defaults {ty}(…)` statements. A type takes one: merge their arguments \
                 into one statement.",
                ty = d.block_type
            )));
        }
    }
    Ok(out)
}

/// The ports of `d` that `block` does not bind itself, with their values:
/// what the defaults add to it.
pub fn taken<'d>(
    d: &'d DefaultsDecl,
    block: &BlockDecl,
) -> impl Iterator<Item = (&'d str, &'d Value)> {
    let bound: Vec<&str> = block.bindings().map(|b| b.port.as_str()).collect();
    d.params().filter(move |(p, _)| !bound.contains(p))
}

/// Give every block of `items` the defaults of its type for the ports it
/// does not bind, behind its own arguments.
pub(crate) fn apply(items: &mut [Item], defaults: &BTreeMap<&str, &DefaultsDecl>) {
    if defaults.is_empty() {
        return;
    }
    for item in items {
        let Item::Block(b) = item else { continue };
        let Some(d) = defaults.get(b.block_type.as_str()) else {
            continue;
        };
        let add: Vec<ArgItem> = taken(d, b)
            .map(|(port, v)| {
                ArgItem::Binding(Binding {
                    port: port.to_string(),
                    kind: BindingKind::Param(v.clone()),
                    comment: None,
                })
            })
            .collect();
        b.args.extend(add);
    }
}

/// The smallest number of blocks of one type that decompile factors into
/// a `defaults` statement.
pub(crate) const FACTOR_MIN_BLOCKS: usize = 3;

/// Factor shared parameter values of `blocks` into `defaults`
/// statements, the inverse of [`apply`]: for each type with at least
/// [`FACTOR_MIN_BLOCKS`] blocks, each port that every block of the type
/// binds (a value, or a wire that keeps the default off it) and whose
/// most common value is held by more than half of them becomes a default.
/// Blocks holding that value drop their binding; the others keep theirs.
/// Types are returned in the order their first block appears.
///
/// Applying the result restores every dropped binding with its value, so
/// a factored module compiles to the same bytes.
pub(crate) fn factor(blocks: &mut [&mut BlockDecl]) -> Vec<DefaultsDecl> {
    let mut types: Vec<String> = Vec::new();
    for b in blocks.iter() {
        if !types.contains(&b.block_type) {
            types.push(b.block_type.clone());
        }
    }
    let mut out = Vec::new();
    for ty in types {
        let of_type: Vec<usize> = (0..blocks.len())
            .filter(|&i| blocks[i].block_type == ty)
            .collect();
        if of_type.len() < FACTOR_MIN_BLOCKS {
            continue;
        }
        // Ports in the order the first block binds them.
        let mut ports: Vec<String> = Vec::new();
        for &i in &of_type {
            for b in blocks[i].bindings() {
                if !ports.contains(&b.port) {
                    ports.push(b.port.clone());
                }
            }
        }
        let mut args = Vec::new();
        for port in ports {
            if !factorable(&ty, &port) {
                continue;
            }
            // Every block binds the port, so the default changes none.
            let kinds: Vec<Option<&BindingKind>> = of_type
                .iter()
                .map(|&i| {
                    blocks[i]
                        .bindings()
                        .find(|b| b.port == port)
                        .map(|b| &b.kind)
                })
                .collect();
            if kinds.iter().any(Option::is_none) {
                continue;
            }
            // The most common value; the first to reach the top count wins.
            let mut counts: Vec<(&Value, usize)> = Vec::new();
            for k in kinds.iter().flatten() {
                if let BindingKind::Param(v) = k {
                    match counts.iter_mut().find(|(x, _)| *x == v) {
                        Some((_, n)) => *n += 1,
                        None => counts.push((v, 1)),
                    }
                }
            }
            let Some((value, n)) = counts
                .iter()
                .fold(None::<(&Value, usize)>, |best, &(v, n)| match best {
                    Some((_, m)) if m >= n => best,
                    _ => Some((v, n)),
                })
            else {
                continue;
            };
            if n * 2 <= of_type.len() {
                continue;
            }
            let value = value.clone();
            for &i in &of_type {
                blocks[i].args.retain(|a| {
                    !matches!(a, ArgItem::Binding(b)
                        if b.port == port && b.kind == BindingKind::Param(value.clone())
                            && b.comment.is_none())
                });
            }
            args.push(ArgItem::Binding(Binding {
                port,
                kind: BindingKind::Param(value),
                comment: None,
            }));
        }
        if !args.is_empty() {
            out.push(DefaultsDecl {
                block_type: ty,
                args,
                comment: None,
                close_comment: None,
            });
        }
    }
    out
}

/// Ports a default may carry: values, not identity (`device:`,
/// `mirrors:`).
fn factorable(block_type: &str, port: &str) -> bool {
    !(crate::periphery::is_device_binding(block_type, port)
        || (port == "mirrors" && super::validate::is_ref_type(block_type)))
}

/// Fit the items adopted into `module` (one block, or a device with its
/// I/Os) to its `defaults`: a binding that repeats its type's default
/// goes, and a block the default would give a value it does not have is
/// refused. Blocks of a type the module has neither a `defaults` nor
/// blocks for are factored among themselves ([`factor`]); the new
/// statements lead `items`.
pub(crate) fn fit_adopted(items: &mut Vec<Item>, module: &super::ast::Module) -> Result<()> {
    let existing = by_type(&module.items)?;
    for item in items.iter_mut() {
        let Item::Block(b) = item else { continue };
        let Some(d) = existing.get(b.block_type.as_str()) else {
            continue;
        };
        if let Some((port, v)) = taken(d, b).next() {
            return Err(Error::Compile(format!(
                "cannot adopt `{slug}`: the config leaves its `{port}` unset, but \
                 `defaults {ty}(…)` would set it to `{v}`. Take `{port}` out of the `defaults` \
                 statement and write it on the blocks that need it, then adopt again.",
                slug = b.slug,
                ty = b.block_type
            )));
        }
        b.args.retain(|a| match a {
            ArgItem::Binding(x) => {
                x.comment.is_some()
                    || !d
                        .params()
                        .any(|(p, v)| p == x.port && x.kind == BindingKind::Param(v.clone()))
            }
            ArgItem::Comment(_) => true,
        });
    }
    // A new `defaults` would reach every block of its type: only where the
    // module has none yet (template bodies included).
    let Ok(expanded) = module.expand() else {
        return Ok(());
    };
    let mut fresh: Vec<&mut BlockDecl> = items
        .iter_mut()
        .filter_map(|i| match i {
            Item::Block(b)
                if !existing.contains_key(b.block_type.as_str())
                    && !expanded.blocks().any(|x| x.block_type == b.block_type) =>
            {
                Some(b)
            }
            _ => None,
        })
        .collect();
    let found = factor(&mut fresh);
    let lead: Vec<Item> = found.into_iter().map(Item::Defaults).collect();
    items.splice(0..0, lead);
    Ok(())
}
