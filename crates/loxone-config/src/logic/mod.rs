//! Lox Logic, the text language: parse, decompile, compile.
//!
//! One statement per line; `#` starts a comment. The main forms:
//!
//! ```text
//! let     <name> = <value>                     (named constant)
//! extern  <slug> = <Type>(iname: "<value>")    (an object of the base config)
//! <slug>  = <Type>(["Label",] <Port>: <value | slug.Port>, …)   (managed block)
//! <slug>  = <expr>                             (named signal)
//! <slug>.<Port> <- <slug.Port | expr>          (wire onto an extern port)
//! <slug>.<Port> = <value>                      (Def write on an extern port)
//! removed <slug>                               (delete a managed block)
//! moved   <old_slug> -> <new_slug>             (rename, keeping identity)
//! page    "<Title>"                            (place the following blocks there)
//! defaults <Type>(<Port>: <value>, …)          (values for every block of the type)
//! <slug>  = <template>(<param>: <arg>, …)      (template instance: its blocks
//!                                               are `<slug>.<block>`, its primary block `<slug>`)
//! ```
//!
//! Bodies closed by `end`: `template`, `test`, `tiles`, `access`,
//! `recipients`, `schedule`, `rule` and `moods`. The full grammar is in
//! `docs/language.md` of the crate's repository.
//!
//! In a block's argument list, the value decides the meaning. A literal or
//! constant binds the port's `Def=` parameter. A `slug.Port` reference wires
//! that source into the port. An expression desugars into gate blocks wired
//! into the port. Slugs are `[a-z][a-z0-9_]*`, unique in the project, and
//! references are always slugs. Prefer `iname:` over `title:` for built-in
//! objects: a config save can translate their titles into the writing
//! system's language. `uuid:` pins exactly.

mod access;
mod adopt;
mod adopt_hardware;
mod adopt_tree;
mod ast;
mod compile;
mod decompile;
mod defaults;
mod desugar;
mod hardware;
mod lint;
mod moods;
mod parser;
mod rename;
mod rule;
pub(crate) mod schedule;
mod simtest;
mod template;
mod tiles;
mod validate;

pub use crate::access::PrincipalKind;
pub use access::lint_access;
pub use adopt::{AdoptReport, AdoptedBlock, PageFragments, adopt, adopt_one, adopt_pages};
pub use ast::{
    ACCESS_BLOCK_TYPE, AccessClaim, AccessDecl, AccessGrant, AccessItem, ArgItem, Attr, AttrDecl,
    Binding, BindingKind, BlockDecl, ClockDecl, CmpOp, Day, DefaultsDecl, ExpectDecl, Expr,
    ExprWireDecl, ExternDecl, Interval, Item, LetDecl, MatchSpec, ModeName, ModeSpellings, Module,
    MoodLine, MoodSetting, MoodsClaim, MoodsDecl, MoodsItem, MovedDecl, Operand, PageDecl, PortRef,
    RULE_BLOCK_TYPE, RemovedDecl, RightKind, RuleDecl, RuleItem, RuleStmt, RuleStmtKind,
    ScheduleClaim, ScheduleDecl, ScheduleItem, ScheduleLine, SetDecl, SetKind, SignalDecl,
    TemplateDecl, TemplateParam, TestCmp, TestDecl, TestItem, TickDecl, TileDecl, TilesClaim,
    TilesDecl, TilesItem, Value, WireDecl, clock_seconds, clock_text,
};
pub use compile::{CompileOptions, compile, resolve_extern_uuids};
pub use decompile::{
    DecompileOptions, DecompileReport, DecompileScope, PageModule, decompile, decompile_pages,
    slugify,
};
pub use defaults::taken as defaults_taken;
pub use desugar::DesugarInfo;
pub use lint::{
    LintFinding, LintKind, finish_lint, fix_unit_suffixes, fix_unit_suffixes_in, lint_dead_outputs,
    lint_source,
};
pub use rename::{Rekey, RekeyKind, apply_rekeys, lock_rekeys, rename_slug, valid_slug};
pub use simtest::{
    LoxSpecs, PlannedExpect, PlannedStep, Signal, TestPlan, TestResult, TestRun, lox_specs,
    plan_tests, read_results, run_tests, trace_tests,
};
pub use tiles::{TileFunction, TileProgram, abbreviations, lint_tiles, program as tile_program};
pub use validate::validate_ports;
