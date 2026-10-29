//! Semantic diff between two configs.
//!
//! ```sh
//! cargo run --example diff -- old.Loxone new.Loxone
//! ```

use lxir::LoxoneDoc;
use lxir::diff::diff;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let (Some(path_a), Some(path_b)) = (args.next(), args.next()) else {
        eprintln!("usage: diff <old.Loxone> <new.Loxone>");
        std::process::exit(2);
    };
    let a = LoxoneDoc::parse(&std::fs::read(&path_a)?)?;
    let b = LoxoneDoc::parse(&std::fs::read(&path_b)?)?;
    let d = diff(&a, &b);

    for o in &d.added {
        println!("+ {} {} {:?}", o.block_type, o.uuid, o.title);
    }
    for o in &d.removed {
        println!("- {} {} {:?}", o.block_type, o.uuid, o.title);
    }
    for r in &d.renamed {
        let tag = if r.locale_suspect { " [locale?]" } else { "" };
        println!(
            "~ {} {} {:?} -> {:?}{tag}",
            r.block_type, r.uuid, r.from, r.to
        );
    }
    for p in &d.param_changes {
        println!(
            "* {} {}.{}: {:?} -> {:?}",
            p.block_type, p.object_uuid, p.port_key, p.from, p.to
        );
    }
    for w in &d.wires_added {
        println!("+wire {} -> {}", w.from_port, w.to_port);
    }
    for w in &d.wires_removed {
        println!("-wire {} -> {}", w.from_port, w.to_port);
    }
    let locale_noise = d.renamed.iter().filter(|r| r.locale_suspect).count();
    eprintln!(
        "\n{} added, {} removed, {} renamed ({locale_noise} locale-suspect), \
         {} param changes, {}/{} wires added/removed",
        d.added.len(),
        d.removed.len(),
        d.renamed.len(),
        d.param_changes.len(),
        d.wires_added.len(),
        d.wires_removed.len()
    );
    Ok(())
}
