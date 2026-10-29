//! Compile an IR module against a base config, maintaining a lockfile.
//!
//! ```sh
//! cargo run --example compile -- [base.Loxone] [module.lxir] [lock.json] [out.Loxone]
//! ```
//!
//! Defaults compile the shipped example (`examples/configs/haus.Loxone` +
//! `examples/ir/beschattung.lxir`) into `examples/out/`. Running it twice
//! produces byte-identical output — the lockfile pins every UUID.

use lxir::ir::{CompileOptions, Module, compile};
use lxir::uuid::parse_serial;
use lxir::{Lockfile, LoxoneDoc};
use std::path::PathBuf;

/// Fixed mint time (2026-01-01T00:00:00Z) so the example is reproducible.
const MINT_TIME_UNIX: i64 = 1_767_225_600;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let base_path = PathBuf::from(
        args.next()
            .unwrap_or_else(|| "examples/configs/haus.Loxone".into()),
    );
    let module_path = PathBuf::from(
        args.next()
            .unwrap_or_else(|| "examples/ir/beschattung.lxir".into()),
    );
    let lock_path = PathBuf::from(
        args.next()
            .unwrap_or_else(|| "examples/out/beschattung.lock.json".into()),
    );
    let out_path = PathBuf::from(
        args.next()
            .unwrap_or_else(|| "examples/out/haus-compiled.Loxone".into()),
    );

    let base = LoxoneDoc::parse(&std::fs::read(&base_path)?)?;
    let module = Module::parse(&std::fs::read_to_string(&module_path)?)?;
    let mut lock = if lock_path.exists() {
        Lockfile::load(&lock_path)?
    } else {
        Lockfile::new()
    };

    let opts = CompileOptions {
        machine: parse_serial("504F94112233")?,
        mint_time_unix: MINT_TIME_UNIX,
        page_title: Some("Beschattung".into()),
        allow_removals: false,
        accept_version: None,
    };
    let out = compile(&base, &module, &mut lock, &opts)?;

    std::fs::create_dir_all(out_path.parent().unwrap())?;
    std::fs::write(&out_path, out.to_bytes())?;
    lock.save(&lock_path)?;

    println!(
        "compiled {} + {} -> {} ({} objects, NextObj {})",
        base_path.display(),
        module_path.display(),
        out_path.display(),
        out.objects().len(),
        out.counters().next_obj,
    );
    println!("lockfile: {}", lock_path.display());
    Ok(())
}
