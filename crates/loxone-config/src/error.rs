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

    /// Syntax error in Lox Logic source text.
    #[error("Lox Logic parse error at line {line}: {msg}")]
    Parse { line: usize, msg: String },

    /// Semantic error while compiling Lox Logic (unknown slug, unknown port, …).
    #[error("compile error: {0}")]
    Compile(String),

    /// An extern's match spec resolved to more than one object. Contains a
    /// human-readable candidate list so callers can surface a fix.
    #[error(
        "extern `{slug}`: {spec} matches {count} objects:\n{candidates}\nPin one with `uuid: \"…\"`."
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

    /// A backup ZIP or `sps0.LoxCC` that cannot be read or staged.
    #[error("{0}")]
    Archive(String),

    /// Several independent errors, in the order they were found. Each is
    /// reported on its own line.
    #[error("{}", join_lines(.0))]
    Many(Vec<Error>),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

/// The most errors `loxl check` and the editor show for one module.
/// Past this many, the rest are usually follow-ups of the first ones.
pub const MAX_ERRORS: usize = 50;

fn join_lines(errors: &[Error]) -> String {
    errors
        .iter()
        .map(|e| e.to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

impl Error {
    /// The errors this one stands for: the list of [`Error::Many`],
    /// else the error itself.
    pub fn into_list(self) -> Vec<Error> {
        match self {
            Error::Many(list) => list.into_iter().flat_map(Error::into_list).collect(),
            e => vec![e],
        }
    }

    /// `errors` as one result: `Ok` when empty, the error itself when
    /// there is one, else [`Error::Many`].
    pub fn collect(errors: Vec<Error>) -> Result<()> {
        let mut errors: Vec<Error> = errors.into_iter().flat_map(Error::into_list).collect();
        match errors.len() {
            0 => Ok(()),
            1 => Err(errors.pop().expect("one error")),
            _ => Err(Error::Many(errors)),
        }
    }
}

/// Collects the errors of independent checks, so one run reports them
/// all.
#[derive(Debug, Default)]
pub(crate) struct Errors(Vec<Error>);

impl Errors {
    /// Keep the error of `r`, if any; `r`'s value otherwise.
    pub(crate) fn check<T>(&mut self, r: Result<T>) -> Option<T> {
        match r {
            Ok(v) => Some(v),
            Err(e) => {
                self.0.push(e);
                None
            }
        }
    }

    pub(crate) fn push(&mut self, e: Error) {
        self.0.push(e);
    }

    /// `Ok` when nothing failed, else every error collected.
    pub(crate) fn finish(self) -> Result<()> {
        Error::collect(self.0)
    }
}
