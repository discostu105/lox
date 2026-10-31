//! The block lifecycle, narrated: rename with `moved`, delete with
//! `removed` — the two transient statements no permanent example module
//! can carry (they are done after one compile).
//!
//! ```sh
//! cargo run --example lifecycle
//! ```
//!
//! Plays the three staged modules under `examples/logic/lifecycle/` against
//! `examples/configs/haus.Loxone`, sharing one in-memory lockfile — like
//! one source file edited between compiles. Nothing is written to disk.

use loxone_config::logic::{CompileOptions, Module, compile};
use loxone_config::uuid::parse_serial;
use loxone_config::{Lockfile, LoxoneDoc};

/// Fixed mint time (2026-01-01T00:00:00Z) so the walkthrough is
/// reproducible.
const MINT_TIME_UNIX: i64 = 1_767_225_600;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let base = LoxoneDoc::parse(&std::fs::read("examples/configs/haus.Loxone")?)?;
    let stage =
        |n: u8| std::fs::read_to_string(format!("examples/logic/lifecycle/schritt{n}.loxl"));
    let opts = CompileOptions {
        machine: parse_serial("504F94112233")?,
        mint_time_unix: MINT_TIME_UNIX,
        page_title: Some("Beschattung".into()),
        allow_removals: false,
        accept_version: None,
        relayout: Vec::new(),
    };
    let mut lock = Lockfile::new();

    // Step 1: two blocks come to life; the lock pins their identity.
    let m1 = Module::parse(&stage(1)?)?;
    let out1 = compile(&base, &m1, &mut lock, &opts)?;
    let born = lock.objects["wetter_ok"].uuid.clone();
    println!("schritt1: {} managed blocks minted", lock.objects.len());
    println!("          wetter_ok = {born}");

    // Step 2: the declaration was renamed; `moved wetter_ok ->
    // wetter_sperre` carries the lock entry over, so object and port
    // UUIDs (and the GUI layout) survive.
    let m2 = Module::parse(&stage(2)?)?;
    let out2 = compile(&out1, &m2, &mut lock, &opts)?;
    assert_eq!(lock.objects["wetter_sperre"].uuid, born);
    assert!(!lock.objects.contains_key("wetter_ok"));
    println!("schritt2: renamed to wetter_sperre, same uuid — identity survived");

    // Interlude: dropping `altlast` from source WITHOUT authorization is
    // refused — the compiler never deletes silently.
    let unauthorized = stage(3)?.replace("removed altlast\n", "");
    let err = compile(&out2, &Module::parse(&unauthorized)?, &mut lock, &opts)
        .expect_err("a vanished block without `removed` must refuse");
    println!("ohne `removed`: {err}");

    // Step 3: `removed altlast` authorizes exactly this deletion. The
    // block is deleted and a tombstone keeps the intent (D31).
    let m3 = Module::parse(&stage(3)?)?;
    let out3 = compile(&out2, &m3, &mut lock, &opts)?;
    assert!(!lock.objects.contains_key("altlast"));
    assert_eq!(lock.removed.len(), 1);
    assert_eq!(out3.objects().len(), out2.objects().len() - 1);
    println!(
        "schritt3: altlast deleted, tombstone {:?} pending",
        lock.removed.values().next().unwrap().slug
    );

    // A recompile against the new output sees the deletion deployed and
    // retires the tombstone; the lingering `removed` line is tolerated
    // until then.
    compile(&out3, &m3, &mut lock, &opts)?;
    assert!(lock.removed.is_empty());
    println!("recompile: deletion deployed, tombstone retired");
    Ok(())
}
