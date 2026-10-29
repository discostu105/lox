//! # lxir — Loxone config model
//!
//! A standalone, dependency-light library for working with Loxone Miniserver
//! configuration documents (`.Loxone` XML) as *source code*:
//!
//! - [`xml`] — lossless concrete-syntax model; parse → write is byte-identical
//!   for real Miniserver output (verified against live configs).
//! - [`uuid`] — the anatomy of Loxone UUIDs (creation time, minting machine,
//!   connector index) and a deterministic minter.
//! - [`doc`] — semantic read layer: objects, ports, wires, counters.
//! - [`connectors`] — port direction knowledge: a small verified builtin
//!   table plus evidence-based inference from real configs.
//! - [`lock`] — the lockfile: persistent identity (slug → UUIDs), counters,
//!   layout. The piece that makes `compile` stable across runs.
//! - [`ir`] — the textual intermediate representation: parse, decompile
//!   (config → IR view), compile (IR + lockfile → config).
//! - [`diff`] — semantic diff between two configs (objects, params, wires,
//!   users and access), with locale-rename noise classification.
//! - [`access`] — users, groups, credentials and access rights, read as a
//!   model; secrets only ever print as fingerprints. [`permissions`]
//!   reads and generates the `permissions.bin` that goes with them.
//! - [`layout`] — page geometry: footprints, port rows, wires, and the
//!   placement of blocks that have no position yet; [`render`] draws a
//!   page as SVG with layout metrics.
//! - [`project`] — `lox.toml` project files: one directory, one deployment
//!   target, zero flags.
//!
//! - [`cli`] — the toolchain's command line (the `lxir` binary, `lox ir`).
//!
//! Transport (FTP, LoxCC compression, push) is deliberately **out of scope**:
//! it lives in the `lox` CLI (same repository), which consumes this crate.

// `cli` was the `lxir` binary's main file and still names the crate as
// `lxir::…`, the way a dependent does.
extern crate self as lxir;

pub mod access;
pub mod autopilot;
pub mod cli;
pub mod connectors;
pub mod diff;
pub mod doc;
pub mod error;
pub mod ir;
pub mod layout;
pub mod lock;
pub mod observed_defaults;
pub mod permissions;
pub mod project;
pub mod render;
pub mod uuid;
pub mod xml;

pub use doc::LoxoneDoc;
pub use error::{Error, Result};
pub use lock::Lockfile;
pub use project::Project;
pub use uuid::LoxUuid;
pub use xml::XmlDocument;
