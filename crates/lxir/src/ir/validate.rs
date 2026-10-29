//! Static semantic validation against the verified builtin table.
//!
//! Everything here needs only the module and the compiled-in connector
//! table — no base config, no lockfile — so `lxir check` can catch the
//! whole class of wrong-type / wrong-port / wrong-direction mistakes before
//! a compile is ever attempted. `compile` runs the same checks first, so the
//! two entry points cannot drift apart.

use super::ast::{Module, Value};
use crate::connectors::{
    BUILTIN_TYPES, MESSAGE_REF_TARGET, PortDir, attr_params, builtin, file_params, message_params,
};
use crate::error::{Error, Result};
use crate::port_meta::port_meta;

/// `InputRef`/`OutputRef`/`OutputRefLM` — block types whose managed form
/// mirrors another object (`mirrors:` binding, design.md D33, D51).
pub(super) fn is_ref_type(block_type: &str) -> bool {
    matches!(block_type, "InputRef" | "OutputRef" | "OutputRefLM")
}

/// The one input port that may be a wire source: the `AI` of an
/// `OutputRef` mirroring an `ApiActor`, read by a block's API connector
/// `OutputAPI`. The GUI records an API link drawn through such a mirror
/// twice — forward on the ref's `AI`, and in reverse on the block's
/// `OutputAPI`, naming the ref's `AI` (docs/loxone-format.md, "API
/// distribution wires are recorded in reverse"; 61 of 61 such reverse
/// wires in the corpus and the house config sit on `OutputAPI`). Callers
/// check the sink side.
pub(super) fn api_mirror_entry(ref_type: &str, port: &str, mirrored: Option<&str>) -> bool {
    ref_type == "OutputRef" && port == "AI" && mirrored == Some("ApiActor")
}

/// The declared type of a minted ref's `mirrors:` target, when it is an
/// extern (an `ApiActor` never is a managed block).
fn mirrored_type<'m>(module: &'m Module, block: &super::ast::BlockDecl) -> Option<&'m str> {
    let target = block.params().find_map(|(k, v)| match (k, v) {
        ("mirrors", Value::Ref(t)) => Some(t),
        _ => None,
    })?;
    module
        .externs()
        .find(|e| &e.slug == target)
        .map(|e| e.block_type.as_str())
}

/// Check every managed block's type, port names, and wire directions
/// against the builtin table. Extern ports are open-world (they exist or
/// not in the base config) and are checked by `compile` instead.
pub fn validate_ports(module: &Module) -> Result<()> {
    // Types and parameter bindings.
    for block in module.blocks() {
        if builtin(&block.block_type).is_none() {
            let hint = suggest(&block.block_type, BUILTIN_TYPES.iter().copied());
            return Err(Error::Compile(format!(
                "block `{}`: type `{}` is not in the verified builtin table and \
                 cannot be created{hint}",
                block.slug, block.block_type
            )));
        }
        let mut mirrors = None;
        for (key, v) in block.params() {
            // A file holds program text (D40) — nothing else takes one.
            if let Value::File { .. } = v
                && !file_params(&block.block_type).contains(&key)
            {
                return Err(Error::Compile(format!(
                    "block `{}`: `{key}: {v}` — a file binds only program text \
                     (`Code:` on a Code16 program block); write other values inline",
                    block.slug
                )));
            }
            // Attribute parameters (e.g. `Formula:` on Formula blocks)
            // bind like any parameter but are not connectors.
            if attr_params(&block.block_type).contains(&key) {
                continue;
            }
            // Message parameters (D51) live on the `<LoggerMailer>` child:
            // texts, and a duration for `MinimumTime`.
            if message_params(&block.block_type).contains(&key) {
                if key == "MinimumTime"
                    && !matches!(v, Value::Number(_) | Value::Unit { .. } | Value::Ref(_))
                {
                    return Err(Error::Compile(format!(
                        "block `{}`: `MinimumTime: {v}` takes a duration \
                         (`MinimumTime: 1h`, or seconds as a number)",
                        block.slug
                    )));
                }
                continue;
            }
            // `mirrors:` on a minted ref names the mirrored object — an
            // identity binding, not a connector (D33).
            if is_ref_type(&block.block_type) && key == "mirrors" {
                let Value::Ref(target) = v else {
                    return Err(Error::Compile(format!(
                        "block `{}`: `mirrors:` takes the name of an extern or \
                         managed block, not a literal",
                        block.slug
                    )));
                };
                if !module.externs().any(|e| &e.slug == target)
                    && !module.blocks().any(|b| &b.slug == target)
                {
                    return Err(Error::Compile(format!(
                        "block `{}` mirrors `{target}`, but no extern or managed \
                         block of this module has that name",
                        block.slug
                    )));
                }
                mirrors = Some(target);
                continue;
            }
            known_port(module, &block.slug, key)?;
            // A suffixed value must fit the port's documented unit (D52).
            module.resolve_value_for(v, &block.block_type, &block.slug, key)?;
        }
        if is_ref_type(&block.block_type) {
            if mirrors.is_none() {
                return Err(Error::Compile(format!(
                    "block `{}`: a minted {} mirrors another object — declare the \
                     target with `mirrors: <name>` (to reference an existing ref \
                     in the base instead, use `extern`)",
                    block.slug, block.block_type
                )));
            }
            // A message ref (D51) shows a notification and nothing else.
            if block.block_type == "OutputRefLM"
                && let Some(target) = mirrors
            {
                let t = module
                    .externs()
                    .find(|e| &e.slug == target)
                    .map(|e| e.block_type.as_str());
                if t != Some(MESSAGE_REF_TARGET) {
                    return Err(Error::Compile(format!(
                        "block `{}`: an OutputRefLM mirrors a push notification — \
                         `{target}` must be an extern `{MESSAGE_REF_TARGET}(…)`",
                        block.slug
                    )));
                }
            }
            // Oracle session 11: a save rewrites a ref's Title= to the
            // mirrored object's title — a label of our own would be a lie.
            if block.title.is_some() {
                return Err(Error::Compile(format!(
                    "block `{}`: a {}'s title is derived from its `mirrors:` \
                     target by Loxone Config — remove the label",
                    block.slug, block.block_type
                )));
            }
        }
    }

    // Port assignments on externs: the extern's declared type documents
    // the port's unit as well as a managed block's would (D52).
    for set in module.sets() {
        if let Some(e) = module.externs().find(|e| e.slug == set.target.slug) {
            module.resolve_value_for(
                &set.value,
                &e.block_type,
                &set.target.slug,
                &set.target.port,
            )?;
        }
    }

    // Test statements (D36): a referenced port on a managed block must
    // exist. Direction is deliberately unrestricted — the simulator
    // injects into inputs and overrides outputs alike, and `expect` reads
    // either side.
    for item in &module.items {
        let super::ast::Item::Test(t) = item else {
            continue;
        };
        for stmt in &t.body {
            let port = match stmt {
                super::ast::TestItem::Inject(s) => &s.target,
                super::ast::TestItem::Expect(e) => &e.port,
                _ => continue,
            };
            if module.blocks().any(|b| b.slug == port.slug) {
                known_port(module, &port.slug, &port.port)?;
            }
        }
    }

    // Wire endpoints on managed blocks — argument-list bindings and `<-`
    // statements alike: the port must exist and its direction must fit
    // (source = output; sink = input or param).
    for (from, to) in module.wire_pairs() {
        // The reverse half of an API link drawn through a minted mirror
        // (see `api_mirror_entry`). The sink's own direction is checked
        // below (managed) or by compile (extern).
        let reverse_api = to.port == "OutputAPI"
            && module.blocks().any(|b| {
                b.slug == from.slug
                    && api_mirror_entry(&b.block_type, &from.port, mirrored_type(module, b))
            });
        for (endpoint, want) in [(&from, PortDir::Output), (&to, PortDir::Input)] {
            let Some(block) = module.blocks().find(|b| b.slug == endpoint.slug) else {
                continue; // extern — checked against the base config later
            };
            if reverse_api && want == PortDir::Output {
                continue;
            }
            if attr_params(&block.block_type).contains(&endpoint.port.as_str()) {
                return Err(Error::Compile(format!(
                    "`{endpoint}` is an attribute parameter, not a connector — \
                     it takes a value (`{port}: \"…\"`), never a wire",
                    port = endpoint.port,
                )));
            }
            known_port(module, &block.slug, &endpoint.port)?;
            let specs = builtin(&block.block_type).expect("type validated above");
            let dir = specs
                .iter()
                .find(|s| s.key == endpoint.port)
                .map(|s| s.dir)
                .expect("port validated above");
            let ok = match want {
                // Api connectors are wire-bidirectional (see PortDir::Api).
                PortDir::Output => matches!(dir, PortDir::Output | PortDir::Api),
                // Wire sinks accept inputs, params, and Api connectors alike.
                PortDir::Input | PortDir::Param | PortDir::Api => dir != PortDir::Output,
            };
            if !ok {
                return Err(Error::Compile(format!(
                    "`{endpoint}` is an {} port and cannot be used as {}",
                    if dir == PortDir::Output {
                        "output"
                    } else {
                        "input"
                    },
                    if want == PortDir::Output {
                        "a wire source"
                    } else {
                        "a wire sink"
                    },
                )));
            }
        }
    }
    Ok(())
}

/// Error unless `key` is a port of the managed block `slug`.
fn known_port(module: &Module, slug: &str, key: &str) -> Result<()> {
    let block = module
        .blocks()
        .find(|b| b.slug == slug)
        .expect("caller resolved the slug to a block");
    let specs = builtin(&block.block_type).expect("caller validated the type");
    if specs.iter().any(|s| s.key == key) {
        return Ok(());
    }
    // Loxone Config 17 silently DELETES off-descriptor connectors (and
    // their wires) on save — verified via the Wine oracle with a grown `I3`
    // on `And`. Gates are fixed two-input; refusing here is what keeps
    // compiled logic from vanishing.
    let gate_hint = if matches!(block.block_type.as_str(), "And" | "Or")
        && key
            .strip_prefix('I')
            .is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
    {
        "; Loxone Config gates are fixed two-input (a grown I3 is \
         silently deleted on save) — cascade 2-input gates instead"
    } else {
        ""
    };
    // The canvas shows short labels (`Tr`, `Don`), not connector keys — a
    // name copied off the screen gets pointed at its key (D52).
    let labeled: Vec<&str> = specs
        .iter()
        .filter(|s| port_meta(&block.block_type, s.key).and_then(|m| m.label) == Some(key))
        .map(|s| s.key)
        .collect();
    let hint = if labeled.is_empty() {
        suggest(
            key,
            specs
                .iter()
                .map(|s| s.key)
                .chain(attr_params(&block.block_type).iter().copied()),
        )
    } else {
        format!(
            "; `{key}` is the label Loxone Config shows on the canvas — the port is `{}`",
            labeled.join("` or `")
        )
    };
    Err(Error::Compile(format!(
        "unknown port `{key}` on block `{slug}` (type `{}`); known ports: {}{hint}{gate_hint}",
        block.block_type,
        specs.iter().map(|s| s.key).collect::<Vec<_>>().join(", ")
    )))
}

/// A "; did you mean `…`?" hint when a candidate is within a small edit
/// distance of the input (case-insensitive), or `""` when nothing is close.
pub(crate) fn suggest<'a>(input: &str, candidates: impl IntoIterator<Item = &'a str>) -> String {
    let needle = input.to_ascii_lowercase();
    let cutoff = |candidate: &str| {
        if needle.len().min(candidate.len()) >= 6 {
            2
        } else {
            1
        }
    };
    candidates
        .into_iter()
        .filter_map(|c| {
            let d = edit_distance(&needle, &c.to_ascii_lowercase());
            (d <= cutoff(c)).then_some((d, c))
        })
        .min_by_key(|&(d, _)| d)
        .map(|(_, c)| format!("; did you mean `{c}`?"))
        .unwrap_or_default()
}

/// Levenshtein distance over bytes (inputs are ASCII identifiers).
fn edit_distance(a: &str, b: &str) -> usize {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0; b.len() + 1];
    for (i, &ca) in a.iter().enumerate() {
        cur[0] = i + 1;
        for (j, &cb) in b.iter().enumerate() {
            let subst = prev[j] + usize::from(ca != cb);
            cur[j + 1] = subst.min(prev[j + 1] + 1).min(cur[j] + 1);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edit_distance_basics() {
        assert_eq!(edit_distance("", ""), 0);
        assert_eq!(edit_distance("abc", "abc"), 0);
        assert_eq!(edit_distance("abc", "abd"), 1);
        assert_eq!(edit_distance("abc", ""), 3);
        assert_eq!(edit_distance("kitten", "sitting"), 3);
    }

    #[test]
    fn suggest_finds_close_names_only() {
        assert_eq!(
            suggest("GreaterEqal", BUILTIN_TYPES.iter().copied()),
            "; did you mean `GreaterEqual`?"
        );
        // Case-insensitive.
        assert_eq!(
            suggest("greaterequal", BUILTIN_TYPES.iter().copied()),
            "; did you mean `GreaterEqual`?"
        );
        // Nothing close: no hint.
        assert_eq!(suggest("Irrigation", BUILTIN_TYPES.iter().copied()), "");
        // Short names use the tight cutoff.
        assert_eq!(suggest("Q1", ["Q", "I1", "I2"]), "; did you mean `Q`?");
        assert_eq!(suggest("xyz", ["Q", "I1", "I2"]), "");
    }

    #[test]
    fn unknown_type_and_port_carry_suggestions() {
        let m = Module::parse("a = Monoflop(Tme: 5)\n").unwrap();
        let err = validate_ports(&m).unwrap_err().to_string();
        assert!(err.contains("did you mean `Time`?"), "{err}");

        let m = Module::parse("a = Monofop()\n").unwrap();
        let err = validate_ports(&m).unwrap_err().to_string();
        assert!(err.contains("did you mean `Monoflop`?"), "{err}");
    }

    #[test]
    fn wire_ports_and_directions_are_checked_statically() {
        // A binding's source must be an output port.
        let m = Module::parse("a = And()\nb = And(I2: a.I1)\n").unwrap();
        let err = validate_ports(&m).unwrap_err().to_string();
        assert!(err.contains("wire source"), "{err}");

        // A binding's own port is the sink: an output cannot take a wire.
        let m = Module::parse("a = And()\nb = And(Q: a.Q)\n").unwrap();
        let err = validate_ports(&m).unwrap_err().to_string();
        assert!(err.contains("wire sink"), "{err}");

        let m = Module::parse("a = And()\nb = And(I3: a.Q)\n").unwrap();
        let err = validate_ports(&m).unwrap_err().to_string();
        assert!(err.contains("unknown port `I3`"), "{err}");
        assert!(err.contains("cascade"), "{err}");
    }

    #[test]
    fn valid_module_passes() {
        let m = Module::parse("b = GreaterEqual(Input2: 28)\na = And(I1: b.Q)\n").unwrap();
        validate_ports(&m).unwrap();
    }

    #[test]
    fn attr_params_bind_as_values_but_never_as_wires() {
        let m = Module::parse("f = Formula(Formula: \"I1+I2\", Input1: 3)\n").unwrap();
        validate_ports(&m).unwrap();

        let m = Module::parse("a = And()\nf = Formula(Formula: a.Q)\n").unwrap();
        let err = validate_ports(&m).unwrap_err().to_string();
        assert!(err.contains("attribute parameter"), "{err}");

        // Typos are matched against attribute parameters too.
        let m = Module::parse("f = Formula(Formla: \"I1\")\n").unwrap();
        let err = validate_ports(&m).unwrap_err().to_string();
        assert!(err.contains("did you mean `Formula`?"), "{err}");
    }
}
