//! Adopting the project or the Miniserver (`Document`, `LoxLIVE`): they
//! stay externs — Lox Logic never rebuilds them — and adoption lifts
//! their settings into `x.Setting = value` claims, each the value the
//! config holds now, so the next compile changes nothing.

use std::collections::BTreeSet;

use crate::doc::{LoxoneDoc, ObjectSummary};
use crate::error::{Error, Result};
use crate::lock::{LockedExternal, Lockfile, sha256_hex};
use crate::logic::adopt::AdoptedBlock;
use crate::logic::adopt_tree::value_of;
use crate::logic::ast::{ExternDecl, Item, MatchSpec, Module, PortRef, SetDecl, Value};
use crate::settings::{SystemKind, SystemSetting, UPDATE_MODES, system_settings};
use crate::xml::Element;

/// The value `s` holds on `el`, as source writes it; `None` where no
/// claim would reproduce the bytes (a setting a save has not written
/// yet, or a value the pane does not offer).
pub(crate) fn lift_setting(el: &Element, s: &SystemSetting) -> Option<Value> {
    let raw = el.attr_decoded(s.attr).map(|v| v.into_owned());
    match s.kind {
        SystemKind::Text | SystemKind::AddableText => raw.map(|v| value_of(&v)),
        SystemKind::Switch { on, off } => {
            if raw.as_deref() == on {
                Some(Value::Bool(true))
            } else if raw.as_deref() == off {
                Some(Value::Bool(false))
            } else {
                None
            }
        }
        SystemKind::MinuteOfDay => {
            let m: u32 = raw?.parse().ok()?;
            (m < 24 * 60).then(|| Value::Clock(format!("{:02}:{:02}", m / 60, m % 60)))
        }
        SystemKind::Updates => {
            let aue = el.attr("AUE");
            let word = match raw.as_deref() {
                Some(aum) => {
                    UPDATE_MODES
                        .iter()
                        .find(|(_, e, m)| *m == aum && *e == aue)?
                        .0
                }
                // A 17.2 save: `AUE="true"` alone is automatic, which the
                // compile leaves in that form. Without either, a claim
                // would add `AUM="0"`.
                None if aue == Some("true") => "auto",
                None => return None,
            };
            Some(Value::Str(word.to_string()))
        }
    }
}

/// [`super::adopt::adopt_one`] for the project or the Miniserver.
pub(super) fn adopt_system(
    doc: &LoxoneDoc,
    o: &ObjectSummary,
    ident: &str,
    slug: &str,
    module: &Module,
    lock: &mut Lockfile,
) -> Result<AdoptedBlock> {
    let fail = |m: String| Error::Compile(m);
    let el = doc.element_at(&o.path).expect("path from objects()");
    let mut items = Vec::new();
    let mut new_externs = Vec::new();
    // An extern of the module already: its claims are added to it.
    let pinned = lock
        .externals
        .iter()
        .find(|(_, le)| le.uuid == o.uuid)
        .map(|(s, _)| s.clone());
    let slug = match pinned {
        Some(x) if x != slug => {
            return Err(fail(format!(
                "{ident} is extern `{x}` of this module. Adopt it with `--as {x}`."
            )));
        }
        Some(x) => x,
        None => {
            let taken = module
                .externs()
                .map(|e| e.slug.as_str())
                .chain(module.blocks().map(|b| b.slug.as_str()))
                .chain(module.lets().map(|l| l.name.as_str()))
                .any(|s| s == slug)
                || lock.objects.contains_key(slug);
            if taken {
                return Err(fail(format!("`{slug}` is already used in this module")));
            }
            items.push(Item::Extern(ExternDecl {
                slug: slug.to_string(),
                block_type: o.block_type.clone(),
                match_spec: MatchSpec::Uuid(o.uuid.clone()),
                room: None,
                category: None,
                device: None,
                comment: None,
                args: Vec::new(),
                close_comment: None,
            }));
            lock.externals.insert(
                slug.to_string(),
                LockedExternal {
                    uuid: o.uuid.clone(),
                    matched_by: "uuid".to_string(),
                    title_at_match: o.title.clone(),
                    iname_at_match: o.iname.clone(),
                    owned: false,
                },
            );
            new_externs.push(slug.to_string());
            slug.to_string()
        }
    };
    // Settings the module claims already keep their lines.
    let claimed: BTreeSet<&str> = module
        .sets()
        .filter(|s| s.target.slug == slug)
        .map(|s| s.target.port.as_str())
        .collect();
    let mut lifted = 0;
    for s in system_settings(&o.block_type) {
        if claimed.contains(s.name) {
            continue;
        }
        if let Some(value) = lift_setting(el, s) {
            // The originals a compile records for the claim, so the
            // next one leaves the lock as written. Automatic in a 17.2
            // save's form is left alone, and records none.
            let attrs: &[&str] = match s.kind {
                SystemKind::Updates if el.attr("AUM").is_none() => &[],
                SystemKind::Updates => &["AUE", s.attr],
                _ => std::slice::from_ref(&s.attr),
            };
            for a in attrs {
                lock.set_originals
                    .entry(crate::settings::key(&o.uuid, a))
                    .or_insert_with(|| el.attr_decoded(a).map(|v| v.into_owned()));
            }
            items.push(Item::Set(SetDecl {
                target: PortRef {
                    slug: slug.clone(),
                    port: s.name.to_string(),
                },
                value,
                comment: None,
                bulk: None,
            }));
            lifted += 1;
        }
    }
    if lifted == 0 {
        return Err(fail(format!(
            "{ident}: every setting Lox Logic knows is claimed already (or not saved yet). There is nothing to adopt."
        )));
    }
    lock.target.config_version = doc.config_version();
    lock.target.source_config_sha256 = Some(sha256_hex(&doc.to_bytes()));
    lock.target.semantic_fingerprint = Some(crate::diff::semantic_fingerprint(doc));
    Ok(AdoptedBlock {
        items,
        page_title: "Miniserver".to_string(),
        new_externs,
        promoted: Vec::new(),
        on_page: false,
    })
}
