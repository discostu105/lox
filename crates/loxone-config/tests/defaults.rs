//! `defaults <Type>(Port: value, …)`: the values every managed block of a
//! type takes for the ports it does not bind. Compiles to the bytes of
//! writing them on each block; decompile and adopt factor them back out.

use loxone_config::logic::{
    CompileOptions, DecompileOptions, DecompileScope, Item, LintKind, Module, adopt, adopt_one,
    compile, decompile, lint_source,
};
use loxone_config::uuid::parse_serial;
use loxone_config::{Lockfile, LoxoneDoc};

fn base() -> LoxoneDoc {
    LoxoneDoc::parse(&std::fs::read("examples/configs/haus.Loxone").unwrap()).unwrap()
}

fn opts() -> CompileOptions {
    CompileOptions {
        machine: parse_serial("504F94112233").unwrap(),
        mint_time_unix: 1_767_225_600,
        page_title: Some("Beschattung".into()),
        allow_removals: false,
        accept_version: None,
        relayout: Vec::new(),
    }
}

const HEAD: &str = "\
extern wind_alarm = VirtualIn(iname: \"VI2\")\n\
extern sonne = VirtualIn(iname: \"VI3\")\n\
\n\
template nachlauf(quelle: any)\n\
\tm = Monoflop(InputTrigger: quelle.Q)\n\
end\n\
\n\
nl = nachlauf(quelle: sonne)\n";

/// Every value written on its block.
const EXPLICIT: &str = "\
a = Monoflop(InputTrigger: sonne.Q, Time: 2s)\n\
b = Monoflop(InputTrigger: wind_alarm.Q, Time: 2s)\n\
c = Monoflop(Reset: sonne.Q, Time: 2s)\n\
d = Monoflop(InputTrigger: sonne.Q, Time: 5s)\n";

/// The shared value in a `defaults`; `d` keeps its own, and the template
/// body block takes the default too.
const FACTORED: &str = "\
defaults Monoflop(Time: 2s)\n\
\n\
a = Monoflop(InputTrigger: sonne.Q)\n\
b = Monoflop(InputTrigger: wind_alarm.Q)\n\
c = Monoflop(Reset: sonne.Q)\n\
d = Monoflop(InputTrigger: sonne.Q, Time: 5s)\n";

fn module(body: &str) -> Module {
    Module::parse(&format!("{HEAD}{body}")).unwrap()
}

/// [`EXPLICIT`], the template body's block bound too.
fn explicit() -> Module {
    Module::parse(&format!("{HEAD}{EXPLICIT}").replace(
        "Monoflop(InputTrigger: quelle.Q)",
        "Monoflop(InputTrigger: quelle.Q, Time: 2s)",
    ))
    .unwrap()
}

fn compiled(m: &Module) -> (LoxoneDoc, Lockfile) {
    let mut lock = Lockfile::new();
    let out = compile(&base(), m, &mut lock, &opts()).unwrap();
    (out, lock)
}

#[test]
fn defaults_compile_like_the_values_on_each_block() {
    let explicit = explicit();
    let factored = module(FACTORED);

    // Canonical text: one argument per line, a fixpoint.
    let text = factored.to_text();
    assert!(
        text.contains("defaults Monoflop(\n\tTime: 2s,\n)\n"),
        "{text}"
    );
    assert_eq!(
        Module::parse(&text).unwrap(),
        factored,
        "parse ∘ to_text = id"
    );
    assert_eq!(Module::parse(&text).unwrap().to_text(), text, "fixpoint");

    // Expansion applies the statement and drops it.
    let flat = factored.expand().unwrap();
    assert!(!flat.items.iter().any(|i| matches!(i, Item::Defaults(_))));
    let time = |slug: &str| {
        flat.blocks()
            .find(|b| b.slug == slug)
            .unwrap()
            .params()
            .find(|(k, _)| *k == "Time")
            .map(|(_, v)| v.to_string())
    };
    assert_eq!(time("a").as_deref(), Some("2s"));
    assert_eq!(time("d").as_deref(), Some("5s"), "a block's own value wins");
    assert_eq!(time("nl_m").as_deref(), Some("2s"), "template bodies too");

    let (out_a, lock_a) = compiled(&explicit);
    let (out_b, lock_b) = compiled(&factored);
    assert!(out_a.to_bytes() == out_b.to_bytes(), "same config");
    assert_eq!(
        serde_json::to_string(&lock_a).unwrap(),
        serde_json::to_string(&lock_b).unwrap(),
        "same lock"
    );
}

#[test]
fn defaults_misuse_is_refused_with_the_fix() {
    let cases: &[(&str, &str)] = &[
        (
            "defaults Monoflop(\"Nachlauf\", Time: 2s)\n",
            "takes no label string",
        ),
        (
            "defaults Monoflop(InputTrigger: sonne.Q)\n",
            "is not a value",
        ),
        (
            "defaults Monoflop(InputTrigger: not sonne.Q)\n",
            "is not a value",
        ),
        (
            "defaults Monoflop(Time: 2s, Time: 3s)\n",
            "binds `Time` twice",
        ),
        ("defaults Monoflop()\n", "sets nothing"),
        ("defaults nachlauf(Time: 2s)\n", "takes a block type"),
        (
            "defaults ModbusASensor(device: modbus)\n",
            "says which object a block is",
        ),
        (
            "template t()\n\tdefaults Monoflop(Time: 2s)\nend\n",
            "`defaults` is a top-level statement",
        ),
        (
            "defaults Monoflop(\n\tTime: 2s,\n",
            "unclosed `(` in `defaults Monoflop(…`",
        ),
    ];
    for (src, want) in cases {
        let err = Module::parse(src).unwrap_err().to_string();
        assert!(err.contains(want), "{src:?}: {err}");
    }
    // Checked on the whole module: one statement per type, constants
    // declared.
    let err = Module::parse(&format!("{HEAD}defaults Monoflop(Reset: 1)\n{FACTORED}"))
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("two `defaults Monoflop(…)` statements"),
        "{err}"
    );
    let err = Module::parse(&format!("{HEAD}defaults Monoflop(Time: dauer)\n"))
        .unwrap_err()
        .to_string();
    assert!(err.contains("undeclared constant `dauer`"), "{err}");
    // A slug may still be called `defaults`.
    Module::parse("defaults = Monoflop()\n").unwrap();
}

#[test]
fn a_default_no_block_takes_is_linted() {
    let m = module(
        "defaults Monoflop(Time: 2s)\ndefaults PulseGen(TimeHigh: 1)\n\
         a = Monoflop(InputTrigger: sonne.Q, Time: 3s)\n",
    );
    let findings: Vec<String> = lint_source(&m)
        .unwrap()
        .into_iter()
        .filter(|f| f.kind == LintKind::UnusedDefault)
        .map(|f| f.detail)
        .collect();
    // `nl.m` takes `Time`, so only the PulseGen default is unused.
    assert_eq!(
        findings,
        vec!["`defaults PulseGen(…)`: no block of the type".to_string()]
    );
    let m = Module::parse(
        "extern sonne = VirtualIn(iname: \"VI3\")\ndefaults Monoflop(Time: 2s)\n\
         a = Monoflop(InputTrigger: sonne.Q, Time: 3s)\n",
    )
    .unwrap();
    let f = lint_source(&m).unwrap();
    assert!(
        f.iter().any(|f| f.kind == LintKind::UnusedDefault
            && f.detail
                .contains("every block of the type binds `Time` itself")),
        "{f:?}"
    );
}

#[test]
fn adopt_factors_shared_values_and_rebuilds_byte_identical() {
    let (out, _) = compiled(&explicit());
    let (adopted, lock, _) = adopt(&out).unwrap();
    let text = adopted.to_text();
    // Five Monoflops (the template's included) bind `Time`; four of them 2 s.
    assert!(
        text.contains("defaults Monoflop(\n\tTime: 2s,\n)\n"),
        "{text}"
    );
    assert!(text.contains("\tTime: 5s,\n"), "{text}");
    assert_eq!(text.matches("Time:").count(), 2, "{text}");
    let reparsed = Module::parse(&text).unwrap();
    let mut lock2 = lock.clone();
    let again = compile(&out, &reparsed, &mut lock2, &opts()).unwrap();
    assert!(again.to_bytes() == out.to_bytes(), "byte-identical rebuild");

    // The read-only view factors the same way.
    let (view, _) = decompile(
        &out,
        &DecompileOptions {
            scope: DecompileScope::ManagedOnly,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(
        view.to_text().contains("defaults Monoflop("),
        "{}",
        view.to_text()
    );
}

#[test]
fn adopting_one_block_drops_what_the_defaults_say() {
    let (out, mut lock) = compiled(&module(EXPLICIT));
    let uuid = lock.objects.remove("c").unwrap().uuid;
    let m = module(&FACTORED.replace("c = Monoflop(Reset: sonne.Q)\n", ""));
    let adopted = adopt_one(&out, &uuid, "c", &m, &mut lock).unwrap();
    let text = Module {
        items: adopted.items,
    }
    .to_text();
    assert!(
        text.contains("c = Monoflop(\n\tReset: sonne.Q,\n)\n"),
        "{text}"
    );
}

#[test]
fn decompile_spells_durations_in_units() {
    let m = Module::parse(
        "extern wind_alarm = VirtualIn(iname: \"VI2\")\n\
         extern sonne = VirtualIn(iname: \"VI3\")\n\
         extern jal_sued = AutoJalousie(title: \"Beschattung S\u{fc}d\")\n\
         a = OffDelay(InputTrigger: sonne.Q, Time: 600)\n\
         b = OnPulseDelay(InputTrigger: sonne.Q, Delay: 10800, Time: 0.5)\n\
         c = OffDelay(InputTrigger: wind_alarm.Q, Time: 58)\n\
         d = OffDelay(InputTrigger: wind_alarm.Q, Time: 0.02)\n\
         jal_sued.AutoShade <- a.Q\n",
    )
    .unwrap();
    let (out, _) = compiled(&m);
    let (adopted, lock, _) = adopt(&out).unwrap();
    let text = adopted.to_text();
    for want in [
        "\tTime: 10min,\n",
        "\tDelay: 3h,\n",
        "\tTime: 500ms,\n",
        "\tTime: 58s,\n",
        "\tTime: 20ms,\n",
    ] {
        assert!(text.contains(want), "{want:?} in {text}");
    }
    // The unit spellings compile back to the same bytes.
    let mut lock2 = lock.clone();
    let again = compile(&out, &Module::parse(&text).unwrap(), &mut lock2, &opts()).unwrap();
    assert!(again.to_bytes() == out.to_bytes(), "byte-identical rebuild");
}
