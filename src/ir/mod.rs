//! The textual intermediate representation.
//!
//! v1 grammar (constructor style; one statement per line, `#` starts a
//! comment; only a block's `( … )` argument list spans lines):
//!
//! ```text
//! let     <name> = <value>                     (named constant)
//! extern  <slug> = <Type>(uuid|iname|title: "<value>"[, room|category: "<title>"])
//! <slug>  = <Type>(["Label",] <Port>: <value | slug.Port>, …)
//!                                              (managed block: params and
//!                                               input wires in one place)
//! <slug>.<Port> <- <slug>.<Port>               (wire onto an extern port)
//! <slug>.<Port> <- <expr>                      (expression sugar, D24:
//!                                               `or`/`and`/`not`/comparisons
//!                                               desugar into gate blocks)
//! <slug>.<Port> = <value>                      (Def write on an extern port)
//! removed <slug>                               (authorize deleting a managed block)
//! moved   <old_slug> -> <new_slug>             (rename keeping identity)
//! page    "<Title>"                            (D28: place the following blocks
//!                                               on that base-config page)
//! template <name>(<param>: <Type> | <param> = <default>, …)
//!     …blocks, wires, sets…                    (reusable body; D23)
//! end
//! <slug>  = <name>(<param>: <arg>, …)          (instantiate: lowercase callee
//!                                               = template, body slug `b`
//!                                               expands to `<slug>_b`)
//! ```
//!
//! In a block's argument list, the value decides the meaning: a literal or
//! constant binds the port's `Def=` parameter, a `slug.Port` reference
//! wires that source into the port, and an expression (D26) desugars into
//! gate blocks wired into the port. Slugs are `[a-z][a-z0-9_]*` and
//! project-unique. References are always slugs — never UUIDs, never
//! titles. `iname:` is preferred over `title:` for built-in objects
//! because display titles are locale-volatile (a config save can rename
//! all built-ins to the writing system's language); `uuid:` pins exactly.

mod adopt;
mod ast;
mod compile;
mod decompile;
mod desugar;
mod lint;
mod parser;
mod rename;
mod simtest;
mod template;
mod validate;

pub use adopt::{AdoptReport, AdoptedBlock, PageFragments, adopt, adopt_one, adopt_pages};
pub use ast::{
    ArgItem, Attr, AttrDecl, Binding, BindingKind, BlockDecl, ClockDecl, CmpOp, ExpectDecl, Expr,
    ExprWireDecl, ExternDecl, Item, LetDecl, MatchSpec, Module, MovedDecl, Operand, PageDecl,
    PortRef, RemovedDecl, SetDecl, TemplateDecl, TemplateParam, TestCmp, TestDecl, TestItem,
    TickDecl, Value, WireDecl,
};
pub use compile::{CompileOptions, compile};
pub use decompile::{
    DecompileOptions, DecompileReport, DecompileScope, PageModule, decompile, decompile_pages,
    slugify,
};
pub use desugar::DesugarInfo;
pub use lint::{LintFinding, LintKind, lint_dead_outputs, lint_source};
pub use rename::{Rekey, RekeyKind, apply_rekeys, lock_rekeys, rename_slug, valid_slug};
pub use simtest::{
    PlannedExpect, PlannedStep, TestPlan, TestResult, TestRun, plan_tests, read_results,
};
pub use validate::validate_ports;
