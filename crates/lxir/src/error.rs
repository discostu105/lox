//! Crate-wide error type.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    /// Malformed XML in a `.Loxone` document.
    #[error("XML error at line {line}: {msg}")]
    Xml { line: usize, msg: String },

    /// The document is well-formed XML but violates `.Loxone` structure
    /// assumptions (missing root, missing Document element, …).
    #[error("invalid .Loxone document: {0}")]
    Structure(String),

    /// Malformed Loxone UUID.
    #[error("invalid Loxone UUID {value:?}: {msg}")]
    Uuid { value: String, msg: String },

    /// Syntax error in IR source text.
    #[error("IR parse error at line {line}: {msg}")]
    IrParse { line: usize, msg: String },

    /// Semantic error while compiling IR (unknown slug, unknown port, …).
    #[error("compile error: {0}")]
    Compile(String),

    /// An extern's match spec resolved to more than one object. Contains a
    /// human-readable candidate list so callers can surface a fix.
    #[error(
        "extern `{slug}`: {spec} is ambiguous — {count} candidates:\n{candidates}\nPin one with `uuid: \"…\"`."
    )]
    AmbiguousMatch {
        slug: String,
        spec: String,
        count: usize,
        candidates: String,
    },

    /// An extern's match spec resolved to no object at all.
    #[error("extern `{slug}`: no object matches {spec}")]
    NoMatch { slug: String, spec: String },

    /// Project file (`lox.toml`) problems: syntax outside the supported
    /// subset, unknown or missing keys.
    #[error("lox.toml: {0}")]
    Project(String),

    /// Lockfile violations (counter decrease, orphaned slug, …).
    #[error("lockfile error: {0}")]
    Lock(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
