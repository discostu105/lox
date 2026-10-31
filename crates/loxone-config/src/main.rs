//! The `loxl` binary: the toolchain's command line, see [`loxone_config::cli`].

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    loxone_config::cli::main_with("loxl", &args)
}
