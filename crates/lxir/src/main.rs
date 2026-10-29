//! The `lxir` binary: the toolchain's command line, see [`lxir::cli`].

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    lxir::cli::main_with("lxir", &args)
}
