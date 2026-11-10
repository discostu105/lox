//! The global flags every front-end reads.

pub struct RunContext {
    pub json: bool,
    pub quiet: bool,
    pub csv: bool,
    pub dry_run: bool,
    /// Fail instead of prompting (`--non-interactive`; `-o json` implies it)
    pub non_interactive: bool,
    pub no_header: bool,
    pub trace_id: Option<String>,
}
