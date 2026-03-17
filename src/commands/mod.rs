pub mod config_cmd;
pub mod control;
pub mod inspect;
pub mod system;

pub struct RunContext {
    pub json: bool,
    pub quiet: bool,
    pub csv: bool,
    pub dry_run: bool,
    pub no_header: bool,
    pub trace_id: Option<String>,
}
