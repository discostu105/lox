//! Infer port directions from the evidence in a real config: which keys
//! occur per block type, at which connector index, wired as sink/source,
//! carrying Def values. Prints JSON — the seed data for growing the
//! verified connector table.
//!
//! ```sh
//! cargo run --example observe -- [config.Loxone]
//! ```

use lxir::LoxoneDoc;
use lxir::connectors::observe;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "examples/configs/haus.Loxone".into());
    let doc = LoxoneDoc::parse(&std::fs::read(&path)?)?;
    let obs = observe(&doc);
    println!("{}", serde_json::to_string_pretty(&obs)?);
    Ok(())
}
