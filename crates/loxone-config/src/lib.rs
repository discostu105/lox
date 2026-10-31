//! # loxone-config — the Loxone Config program file
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
//! - [`logic`] — Lox Logic, the text language,: parse, decompile
//!   (config → Lox Logic view), compile (Lox Logic + lockfile → config).
//! - [`diff`] — semantic diff between two configs (objects, params, wires,
//!   users and access), with locale-rename noise classification.
//! - [`access`] — users, groups, credentials and access rights, read as a
//!   model; secrets only ever print as fingerprints. [`permissions`]
//!   reads and generates the `permissions.bin` that goes with them;
//!   [`catalogue`] carries a compile into the app's `LoxAPP3.json`.
//! - [`layout`] — page geometry: footprints, port rows, wires, and the
//!   placement of blocks that have no position yet; [`render`] draws a
//!   page as SVG with layout metrics.
//! - [`moods`] — LightController2 moods: the read model and the circuit
//!   value codec.
//! - [`sim`] — the logic simulator behind `loxl test`: block models
//!   written from Loxone's documentation, run cycle by cycle.
//! - [`project`] — `lox.toml` project files: one directory, one deployment
//!   target, zero flags.
//! - [`archive`] (feature `archive`, default) — Miniserver backup ZIPs and
//!   the LoxCC codec of the `sps0.LoxCC` inside them.
//!
//! - [`cli`] — the toolchain's command line (the `loxl` binary, `lox logic`).
//!
//! Transport (FTP, HTTP, WebSocket, push) is deliberately **out of scope**:
//! it lives in the `lox` CLI (same repository), which consumes this crate.

// `cli` was the `loxl` binary's main file and still names the crate as
// `loxone_config::…`, the way a dependent does.
extern crate self as loxone_config;

pub mod access;
#[cfg(feature = "archive")]
pub mod archive;
pub mod autopilot;
pub mod blockdoc;
pub mod catalogue;
pub mod cli;
pub mod connectors;
pub mod coverage;
pub mod diff;
pub mod doc;
pub mod error;
pub mod layout;
pub mod library;
pub mod lock;
pub mod logic;
pub mod lsp;
pub mod moods;
pub mod observed_defaults;
pub mod periphery;
pub mod permissions;
pub mod port_meta;
pub mod project;
pub mod render;
pub mod scan;
pub mod settings;
pub mod sim;
pub mod uuid;
pub mod xml;

pub use doc::LoxoneDoc;
pub use error::{Error, Result};
pub use lock::Lockfile;
pub use project::Project;
pub use uuid::LoxUuid;
pub use xml::XmlDocument;
