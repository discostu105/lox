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
//! tiles   <name>                               (D41: Touch Pure Flex tile
//!     tile <i>(click|up|down: "<command>",      programs; `text:`, `to: <slug>`
//!              text: "…", to: <slug>, …)        apply to every command)
//! end
//! <slug>.tiles = <name>                        (write the set onto an extern
//!                                               TreeDevice)
//! access  <slug>                               (D49: the access rights on an
//!     user "<title>": "<schedule>"              extern NfcCodeTouch — every
//!     group "<title>": "<schedule>"             right on it; titles name the
//! end                                           base's users and schedules)
//! recipients <slug>                            (D51: who gets an extern
//!     user "<title>"                            Notification — `ACT=` on it;
//!     group "<title>"                           its messages come from
//! end                                           `OutputRefLM` blocks)
//! moods   <name>                               (D58: LightController2 moods;
//!     "<mood>": all on, AQ<n> <value>, …        values on|off|40%|rgb(r,g,b)|
//! end                                           temp(80%, 2700K))
//! <slug>.moods = <name>                        (write the set onto a
//!                                               LightController2; also in a
//!                                               template body)
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

mod access;
mod adopt;
mod adopt_tree;
mod ast;
mod compile;
mod decompile;
mod desugar;
mod lint;
mod moods;
mod parser;
mod rename;
mod rule;
mod schedule;
mod simtest;
mod template;
mod tiles;
mod validate;

pub use crate::access::PrincipalKind;
pub use access::lint_access;
pub use adopt::{AdoptReport, AdoptedBlock, PageFragments, adopt, adopt_one, adopt_pages};
pub use ast::{
    ACCESS_BLOCK_TYPE, AccessDecl, AccessGrant, AccessItem, ArgItem, Attr, AttrDecl, Binding,
    BindingKind, BlockDecl, ClockDecl, CmpOp, Day, ExpectDecl, Expr, ExprWireDecl, ExternDecl,
    Interval, Item, LetDecl, MatchSpec, Module, MoodLine, MoodSetting, MoodsClaim, MoodsDecl,
    MoodsItem, MovedDecl, Operand, PageDecl, PortRef, RULE_BLOCK_TYPE, RemovedDecl, RightKind,
    RuleDecl, RuleItem, RuleStmt, RuleStmtKind, ScheduleClaim, ScheduleDecl, ScheduleItem,
    ScheduleLine, SetDecl, TemplateDecl, TemplateParam, TestCmp, TestDecl, TestItem, TickDecl,
    TileDecl, TilesClaim, TilesDecl, TilesItem, Value, WireDecl,
};
pub use compile::{CompileOptions, compile, resolve_extern_uuids};
pub use decompile::{
    DecompileOptions, DecompileReport, DecompileScope, PageModule, decompile, decompile_pages,
    slugify,
};
pub use desugar::DesugarInfo;
pub use lint::{LintFinding, LintKind, lint_dead_outputs, lint_source};
pub use rename::{Rekey, RekeyKind, apply_rekeys, lock_rekeys, rename_slug, valid_slug};
pub use simtest::{
    LoxSpecs, PlannedExpect, PlannedStep, Signal, TestPlan, TestResult, TestRun, lox_specs,
    plan_tests, read_results, run_tests,
};
pub use tiles::{TileFunction, TileProgram, abbreviations, lint_tiles, program as tile_program};
pub use validate::validate_ports;
