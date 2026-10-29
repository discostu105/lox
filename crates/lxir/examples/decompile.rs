//! Decompile a config into an IR view: managed blocks, externs, wires.
//!
//! ```sh
//! cargo run --example decompile -- [config.Loxone]
//! ```

use lxir::LoxoneDoc;
use lxir::ir::{DecompileOptions, decompile};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "examples/out/haus-compiled.Loxone".into());
    let doc = LoxoneDoc::parse(&std::fs::read(&path)?)?;
    let (module, report) = decompile(&doc, &DecompileOptions::default())?;
    print!("{}", module.to_text());
    eprintln!(
        "\n# {path}: {} managed, {} externs across {} pages, {} raw objects untouched",
        report.managed, report.externs, report.pages, report.raw_objects
    );
    Ok(())
}
