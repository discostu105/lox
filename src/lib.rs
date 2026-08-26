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
//! - [`diff`] — semantic diff between two configs (objects, params, wires),
//!   with locale-rename noise classification.
//! - [`project`] — `lox.toml` project files: one directory, one deployment
//!   target, zero flags.
//!
//! Transport (FTP, LoxCC compression, push) is deliberately **out of scope**;
//! it lives in the `lox` / `lox-cli` CLIs, which are the intended consumers
//! of this crate.

pub mod autopilot;
pub mod connectors;
pub mod diff;
pub mod doc;
pub mod error;
pub mod ir;
pub mod lock;
pub mod observed_defaults;
pub mod project;
pub mod uuid;
pub mod xml;

pub use doc::LoxoneDoc;
pub use error::{Error, Result};
pub use lock::Lockfile;
pub use project::Project;
pub use uuid::LoxUuid;
pub use xml::XmlDocument;
