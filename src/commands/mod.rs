pub mod access;
pub mod config_cmd;
pub mod config_insight;
pub mod config_push;
pub mod control;
pub mod ctx;
pub mod inspect;
pub mod push_users;
pub mod system;

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
