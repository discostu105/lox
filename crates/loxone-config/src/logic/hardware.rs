//! Hardware declarations: `taster = TreeDevice("Taster Flur", serial: "…")`
//! and `klingel = TreeSensor("Klingel", device: taster, iname: "I0", …)`
//! take over an existing device or I/O ([`crate::hardware`]). Expansion
//! lowers each into an extern of its object, the wires and port values of
//! its argument list as extern statements, and an [`OwnedDecl`] with the
//! attributes the source owns. Nothing below the expansion knows the
//! declaration form.

use std::collections::BTreeMap;

use crate::error::{Error, Result};
use crate::hardware::{STATISTICS, is_device, is_hardware, is_io, parents, stats_value};
use crate::logic::ast::{
    Attr, BindingKind, BlockDecl, ExprWireDecl, ExternDecl, Item, MatchSpec, OwnedDecl, PortRef,
    SetDecl, Value, WireDecl,
};

/// Whether `key` names an owned setting of `block_type` in a declaration.
pub(crate) fn is_owned_setting(block_type: &str, key: &str) -> bool {
    crate::settings::setting(block_type, key).is_some()
        || (key == STATISTICS && stats_value(block_type).is_some())
}

/// The identity arguments of a hardware declaration.
pub(crate) fn is_identity(key: &str) -> bool {
    matches!(
        key,
        "serial" | "uuid" | "iname" | "device" | "kind" | "tree" | "branch"
    )
}

/// A string argument, written quoted or (a serial of digits only) as a
/// number.
fn text(v: &Value) -> Option<&str> {
    match v {
        Value::Str(s) | Value::Number(s) => Some(s),
        _ => None,
    }
}

struct Identity {
    serial: Option<String>,
    uuid: Option<String>,
    iname: Option<String>,
    device: Option<String>,
    /// A device to create: its kind, Tree extension and branch.
    kind: Option<String>,
    tree: Option<String>,
    branch: Option<String>,
}

fn identity(b: &BlockDecl) -> Result<Identity> {
    let fail = |m: String| Err(Error::Compile(m));
    let mut id = Identity {
        serial: None,
        uuid: None,
        iname: None,
        device: None,
        kind: None,
        tree: None,
        branch: None,
    };
    for bind in b.bindings().filter(|x| is_identity(&x.port)) {
        let key = bind.port.as_str();
        let allowed = match key {
            "serial" => is_device(&b.block_type),
            "kind" | "tree" | "branch" => b.block_type == "TreeDevice",
            "iname" | "device" => is_io(&b.block_type),
            _ => true,
        };
        if !allowed {
            let what = if b.block_type == "TreeDevice" {
                "a Tree device takes `serial:`, `uuid:` or `kind:` with `tree:`"
            } else if is_device(&b.block_type) {
                "a device takes `serial:` or `uuid:`"
            } else {
                "an input or output takes `device:` with `iname:` or `uuid:`"
            };
            return fail(format!("`{}`: `{key}:` does not apply, {what}.", b.slug));
        }
        let BindingKind::Param(v) = &bind.kind else {
            return fail(format!("`{}`: `{key}:` takes a value, not a wire.", b.slug));
        };
        let slot = match key {
            "serial" => &mut id.serial,
            "uuid" => &mut id.uuid,
            "iname" => &mut id.iname,
            "kind" => &mut id.kind,
            "tree" => &mut id.tree,
            "branch" => &mut id.branch,
            _ => &mut id.device,
        };
        let value = if key == "device" || key == "tree" {
            match v {
                Value::Ref(d) => d.clone(),
                _ if key == "tree" => {
                    return fail(format!(
                        "`{}`: `tree:` names the Tree extension's declaration, e.g. `tree: tree1`.",
                        b.slug
                    ));
                }
                _ => {
                    return fail(format!(
                        "`{}`: `device:` names the device's declaration, e.g. `device: taster`.",
                        b.slug
                    ));
                }
            }
        } else {
            match text(v) {
                Some(t) => t.to_string(),
                None => return fail(format!("`{}`: `{key}:` takes a quoted string.", b.slug)),
            }
        };
        if slot.replace(value).is_some() {
            return fail(format!("`{}` gives `{key}:` twice. Keep one.", b.slug));
        }
    }
    Ok(id)
}

/// Lower every hardware declaration of the expanded `items` in place.
pub(super) fn lower(items: &mut Vec<Item>) -> Result<()> {
    let hw: BTreeMap<String, (String, Identity)> = items
        .iter()
        .filter_map(|i| match i {
            Item::Block(b) if is_hardware(&b.block_type) => Some(b),
            _ => None,
        })
        .map(|b| Ok((b.slug.clone(), (b.block_type.clone(), identity(b)?))))
        .collect::<Result<_>>()?;
    if hw.is_empty() {
        return Ok(());
    }
    // What a declaration states in its argument list is written there
    // only: one spelling per fact.
    let mut rooms: BTreeMap<String, (Option<Value>, Option<Value>)> = BTreeMap::new();
    for item in items.iter() {
        let (slug, what) = match item {
            Item::Wire(w) => (&w.to.slug, format!("`{}.{} <- …`", w.to.slug, w.to.port)),
            Item::ExprWire(w) => (&w.to.slug, format!("`{}.{} <- …`", w.to.slug, w.to.port)),
            Item::Set(s) => (
                &s.target.slug,
                format!("`{}.{} = …`", s.target.slug, s.target.port),
            ),
            Item::Attr(a) if a.attr == Attr::Title && hw.contains_key(&a.slug) => {
                return Err(Error::Compile(format!(
                    "`{}.title`: a hardware declaration's title is its label. Write it as \
                     `{} = {}(\"…\", …)`.",
                    a.slug, a.slug, hw[&a.slug].0
                )));
            }
            Item::Attr(a) if hw.contains_key(&a.slug) => {
                let slot = rooms.entry(a.slug.clone()).or_default();
                match a.attr {
                    Attr::Room => slot.0 = Some(a.value.clone()),
                    _ => slot.1 = Some(a.value.clone()),
                }
                continue;
            }
            _ => continue,
        };
        if hw.contains_key(slug) {
            return Err(Error::Compile(format!(
                "{what}: `{slug}` is a hardware declaration. Write the port or setting in its \
                 argument list."
            )));
        }
    }
    let mut out = Vec::with_capacity(items.len());
    for item in items.drain(..) {
        match item {
            Item::Block(b) if is_hardware(&b.block_type) => {
                let (room, category) = rooms.remove(&b.slug).unwrap_or_default();
                out.extend(lower_one(b, &hw, room, category)?);
            }
            Item::Attr(a) if hw.contains_key(&a.slug) => {}
            other => out.push(other),
        }
    }
    *items = out;
    Ok(())
}

/// The matcher of a device to create, checked against the kind table.
fn create_spec(slug: &str, kind: &str, id: &Identity) -> Result<MatchSpec> {
    if crate::tree_kinds::kind(kind).is_none() {
        return Err(Error::Compile(format!(
            "`{slug}`: Lox Logic cannot create a `{kind}`. It creates {}. Declare others by \
             `serial:` once Loxone Config has paired them.",
            crate::tree_kinds::names().join(", ")
        )));
    }
    let Some(tree) = &id.tree else {
        return Err(Error::Compile(format!(
            "`{slug}`: name the Tree extension the device goes on, e.g. `tree: tree1`."
        )));
    };
    if let Some(b) = &id.branch
        && b != "left"
        && b != "right"
    {
        return Err(Error::Compile(format!(
            "`{slug}`: `branch:` is \"left\" or \"right\", not \"{b}\"."
        )));
    }
    Ok(MatchSpec::Create {
        kind: kind.to_string(),
        tree: tree.clone(),
        branch: id.branch.clone(),
    })
}

fn lower_one(
    b: BlockDecl,
    hw: &BTreeMap<String, (String, Identity)>,
    room: Option<Value>,
    category: Option<Value>,
) -> Result<Vec<Item>> {
    let fail = |m: String| Err(Error::Compile(m));
    let id = &hw[&b.slug].1;
    let ty = b.block_type.as_str();
    let (match_spec, device_serial) = if is_device(ty) {
        match (&id.serial, &id.uuid, &id.kind) {
            (Some(s), None, None) => (MatchSpec::Serial(s.clone()), None),
            (None, Some(u), None) => (MatchSpec::Uuid(u.clone()), None),
            (None, None, Some(k)) => (create_spec(&b.slug, k, id)?, None),
            _ if ty == "TreeDevice" => {
                return fail(format!(
                    "`{}`: say which {ty} this is with `serial: \"…\"` or `uuid: \"…\"`, or \
                     create one with `kind: \"…\", tree: <extension>`.",
                    b.slug
                ));
            }
            _ => {
                return fail(format!(
                    "`{}`: say which {ty} this is with `serial: \"…\"` or `uuid: \"…\"`.",
                    b.slug
                ));
            }
        }
    } else {
        let Some(dev) = &id.device else {
            return fail(format!(
                "`{}`: name the device the {ty} belongs to, e.g. `device: taster`.",
                b.slug
            ));
        };
        let parent = match hw.get(dev) {
            Some((t, pid)) if parents(ty).contains(&t.as_str()) => pid,
            Some((t, _)) => {
                return fail(format!(
                    "`{}`: a {ty} hangs below a {}, not a {t} (`{dev}`).",
                    b.slug,
                    parents(ty).join(" or ")
                ));
            }
            None => {
                return fail(format!(
                    "`{}`: `device: {dev}` names no hardware declaration. Declare the device, \
                     e.g. `{dev} = {}(\"…\", serial: \"…\")`.",
                    b.slug,
                    parents(ty).join("|")
                ));
            }
        };
        match (&id.iname, &id.uuid) {
            (Some(i), None) if parent.kind.is_some() => (
                MatchSpec::Within {
                    device: dev.clone(),
                    iname: i.clone(),
                },
                None,
            ),
            (Some(i), None) => match &parent.serial {
                Some(s) => (MatchSpec::IName(i.clone()), Some(s.clone())),
                None => {
                    return fail(format!(
                        "`{}`: `iname:` needs its device matched by `serial:`. Match this one \
                         by `uuid:`.",
                        b.slug
                    ));
                }
            },
            (None, Some(u)) => (MatchSpec::Uuid(u.clone()), None),
            _ => {
                return fail(format!(
                    "`{}`: say which {ty} of `{dev}` this is with `iname: \"…\"` or `uuid: \"…\"`.",
                    b.slug
                ));
            }
        }
    };
    let mut items = vec![Item::Extern(ExternDecl {
        slug: b.slug.clone(),
        block_type: b.block_type.clone(),
        match_spec,
        room: None,
        category: None,
        device: device_serial,
        comment: None,
    })];
    let mut settings: Vec<(String, Value)> = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for bind in b.bindings() {
        let key = bind.port.as_str();
        if is_identity(key) {
            continue;
        }
        if !seen.insert(key) {
            return fail(format!("`{}` binds `{key}` twice. Keep one.", b.slug));
        }
        let at = PortRef {
            slug: b.slug.clone(),
            port: key.to_string(),
        };
        match &bind.kind {
            BindingKind::Param(v) if is_owned_setting(ty, key) => {
                if key == STATISTICS && !matches!(v, Value::Bool(_)) {
                    return fail(format!("`{}`: `{STATISTICS}:` is `on` or `off`.", b.slug));
                }
                settings.push((key.to_string(), v.clone()));
            }
            BindingKind::Param(v) => items.push(Item::Set(SetDecl {
                target: at,
                value: v.clone(),
                comment: None,
                bulk: None,
            })),
            BindingKind::Wire(r) => items.push(Item::Wire(WireDecl {
                to: at,
                from: r.clone(),
                comment: None,
            })),
            BindingKind::Expr(e) => items.push(Item::ExprWire(ExprWireDecl {
                to: at,
                expr: e.clone(),
                comment: None,
            })),
        }
    }
    items.push(Item::Owned(OwnedDecl {
        slug: b.slug.clone(),
        block_type: b.block_type,
        device: id.device.clone(),
        title: b.title,
        room,
        category,
        settings,
    }));
    Ok(items)
}

#[cfg(test)]
mod tests {
    use crate::logic::ast::{Item, Module};

    fn lowered(src: &str) -> crate::error::Result<Module> {
        Module::parse(src)?.expand()
    }

    #[test]
    fn a_declaration_becomes_an_extern_and_what_it_owns() {
        let m = lowered(
            r#"
taster = TreeDevice("Taster Flur", serial: "B0A1B2C3", Installation: "Wand")
klingel = TreeSensor("Klingel", device: taster, iname: "I0", Desc: "Klingel", Statistics: on)
led = TreeAactor("LED", device: taster, iname: "AQ0", I: 30)
taster.room = "Flur"
"#,
        )
        .unwrap();
        let text: Vec<String> = m
            .items
            .iter()
            .map(|i| match i {
                Item::Extern(e) => format!("extern {} {}", e.slug, e.spec()),
                Item::Set(s) => format!("set {}.{}", s.target.slug, s.target.port),
                Item::Owned(o) => format!(
                    "owned {} {:?} {:?} {:?}",
                    o.slug,
                    o.title,
                    o.room.as_ref().map(|v| v.to_string()),
                    o.settings
                        .iter()
                        .map(|(k, _)| k.as_str())
                        .collect::<Vec<_>>()
                ),
                other => format!("{other:?}"),
            })
            .collect();
        assert_eq!(
            text,
            [
                r#"extern taster serial: "B0A1B2C3""#,
                r#"owned taster Some("Taster Flur") Some("\"Flur\"") ["Installation"]"#,
                r#"extern klingel iname: "I0", device: "B0A1B2C3""#,
                r#"owned klingel Some("Klingel") None ["Desc", "Statistics"]"#,
                r#"extern led iname: "AQ0", device: "B0A1B2C3""#,
                "set led.I",
                r#"owned led Some("LED") None []"#,
            ]
        );
    }

    #[test]
    fn refusals_name_the_fix() {
        let err = |src: &str| lowered(src).unwrap_err().to_string();
        assert!(err(r#"t = TreeDevice("T")"#).contains("serial:"));
        assert!(
            err(r#"t = TreeDevice("T", serial: "B0")
k = TreeSensor("K", device: t)"#)
            .contains("iname:")
        );
        assert!(
            err(r#"k = TreeSensor("K", device: t, iname: "I0")"#).contains("names no hardware")
        );
        // Air devices are never created: their MAC comes from pairing (#322).
        assert!(
            err(r#"t = LoxAIRDevice("T", kind: "PresenceSensorTree")"#)
                .contains("`kind:` does not apply")
        );
        assert!(
            err(r#"t = LoxAIRDevice("T", serial: "B0")
k = TreeSensor("K", device: t, iname: "I0")"#)
            .contains("hangs below a TreeDevice")
        );
        assert!(
            err(r#"t = TreeDevice("T", uuid: "u")
k = TreeSensor("K", device: t, iname: "I0")"#)
            .contains("by `uuid:`")
        );
        assert!(
            err(r#"t = TreeDevice("T", serial: "B0")
t.title = "X""#)
            .contains("label")
        );
        assert!(
            err(r#"t = TreeDevice("T", serial: "B0")
t.Installation = "X""#)
            .contains("argument list")
        );
        assert!(
            err(r#"t = TreeDevice("T", serial: "B0")
k = TreeSensor("K", device: t, iname: "I0", Statistics: 7)"#)
            .contains("`on` or `off`")
        );
    }
}
