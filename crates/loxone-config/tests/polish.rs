//! The v2 polish rules: contextual keywords, rule words that win their
//! place, `let` constants in `tick`, the shared time lexeme, scoped instance
//! names everywhere a name goes, errors that spell names like the source,
//! and the expression-prefix collision.

use loxone_config::logic::{
    CompileOptions, Item, Module, RuleItem, RuleStmtKind, TestItem, compile, plan_tests, run_tests,
};
use loxone_config::uuid::parse_serial;
use loxone_config::{Lockfile, LoxoneDoc};

const EXAMPLES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/examples");

fn base() -> LoxoneDoc {
    LoxoneDoc::parse(&std::fs::read(format!("{EXAMPLES}/configs/pool.Loxone")).unwrap()).unwrap()
}

fn opts() -> CompileOptions {
    CompileOptions {
        machine: parse_serial("504F94112233").unwrap(),
        mint_time_unix: 1_767_225_600,
        page_title: None,
        allow_removals: false,
        accept_version: None,
        relayout: Vec::new(),
    }
}

fn parse_err(src: &str) -> String {
    Module::parse_fragment(src).unwrap_err().to_string()
}

fn compile_err(src: &str) -> String {
    let m = Module::parse(src).unwrap();
    compile(&base(), &m, &mut Lockfile::new(), &opts())
        .unwrap_err()
        .to_string()
}

#[test]
fn section_kinds_are_names_outside_their_section_head() {
    let src = "extern vi = VirtualIn(iname: \"VI1\")\n\n\
               tiles = Not(I: vi.Q)\n\
               schedule = Not(I: tiles.Q)\n\
               moods = Not(I: schedule.Q)\n\
               access = Not(I: moods.Q)\n\
               recipients = Not(I: access.Q)\n\
               defaults = Not(I: recipients.Q)\n\
               rule = Not(I: defaults.Q)\n\n\
               moods.room = \"Pool\"\n";
    let m = Module::parse(src).unwrap();
    assert_eq!(m.to_text(), src);
    assert_eq!(m.blocks().count(), 7);
    compile(&base(), &m, &mut Lockfile::new(), &opts()).unwrap();

    // The truly reserved words stay refused as names.
    for word in ["tile", "tick", "page", "on", "and"] {
        let e = parse_err(&format!("{word} = Not()\n"));
        assert!(e.contains("reserved word"), "{word}: {e}");
        // The same error inside a template body, not "not allowed in a body".
        let e = parse_err(&format!("template t()\n\t{word} = Not()\nend\n"));
        assert!(e.contains("reserved word"), "{word} in a body: {e}");
    }
    for word in ["test", "let", "extern"] {
        let e = parse_err(&format!("template t()\n\t{word} = Not()\nend\n"));
        assert!(e.contains("reserved word"), "{word} in a body: {e}");
    }
}

#[test]
fn rule_words_win_their_place() {
    let rule = |line: &str| format!("rule r\n\t{line}\nend\n");
    for (line, want) in [
        (
            "then wait on",
            "A block named `wait` cannot be a rule target",
        ),
        (
            "then mode on",
            "a block named `mode` cannot be a rule target",
        ),
        (
            "then notify on",
            "a block named `notify` cannot be a rule target",
        ),
        (
            "then mail pulse",
            "a block named `mail` cannot be a rule target",
        ),
        (
            "then message off",
            "a block named `message` cannot be a rule target",
        ),
    ] {
        let e = parse_err(&rule(line));
        assert!(e.contains(want), "{line}: {e}");
        assert!(e.contains("loxl rename"), "{line}: {e}");
    }
    // A state condition carries a dot, so a block named like a condition
    // word is no problem there.
    let m = Module::parse_fragment(&rule("when mode.Q == 1")).unwrap();
    let Item::Rule(r) = &m.items[0] else {
        panic!("a rule")
    };
    assert!(
        r.body.iter().any(|i| matches!(
            i,
            RuleItem::Stmt(s) if matches!(&s.kind, RuleStmtKind::State { target, .. } if target == "mode")
        )),
        "{r:?}"
    );
}

#[test]
fn a_rule_time_is_a_clock_time() {
    let rule = |line: &str| format!("rule r\n\t{line}\nend\n");
    // The same lexeme as `At:` and `clock`: one spelling.
    for (written, canonical) in [("7:30", "07:30"), ("07:30", "07:30"), ("19:00:00", "19:00")] {
        let m = Module::parse_fragment(&rule(&format!("when time {written}"))).unwrap();
        assert_eq!(m.to_text(), rule(&format!("when time {canonical}")));
    }
    let e = parse_err(&rule("when time 07:30:15"));
    assert!(e.contains("whole minute"), "{e}");
    let e = parse_err(&rule("when time 25:00"));
    assert!(e.contains("not a time of day"), "{e}");
    let e = parse_err(&rule("when time \"07:30\""));
    assert!(
        e.contains("Write `when time 07:30` instead of `when time \"07:30\"`"),
        "{e}"
    );
}

#[test]
fn tick_and_dt_take_let_constants() {
    let src = "let warten = 5min\n\
               let schritte = 3\n\
               let takt = 500ms\n\n\
               extern vi = VirtualIn(iname: \"VI1\")\n\n\
               nach = OffDelay(InputTrigger: vi.Q, Time: 2s)\n\n\
               test \"konstanten\"\n\
               \tvi.Q = 1\n\
               \ttick schritte\n\
               \texpect nach.Q == 1\n\
               \tvi.Q = 0\n\
               \ttick warten dt takt\n\
               \texpect nach.Q == 0\n\
               end\n";
    let m = Module::parse(src).unwrap();
    assert_eq!(m.to_text(), src, "fmt keeps the names");
    let Item::Test(t) = m.items.last().unwrap() else {
        panic!("a test")
    };
    let lets = |name: &str| m.lets().find(|l| l.name == name).map(|l| &l.value);
    let counts: Vec<u64> = t
        .body
        .iter()
        .filter_map(|i| match i {
            TestItem::Tick(t) => t.count(lets).unwrap(),
            _ => None,
        })
        .collect();
    assert_eq!(counts, [3, 600]);

    let mut lock = Lockfile::new();
    let compiled = compile(&base(), &m, &mut lock, &opts()).unwrap();
    let (flat, _) = m.expand().unwrap().desugar().unwrap();
    let plan = plan_tests(&flat, &lock, &compiled, None).unwrap();
    let results = run_tests(&plan, &compiled);
    assert!(
        results[0].pass,
        "{:?} {:?}",
        results[0].failures, results[0].error
    );

    // A constant that is no count or duration, or does not divide: errors.
    for (lets, tick, want) in [
        ("let x = on", "tick x", "a plain tick count or a duration"),
        ("let x = 1.5", "tick x", "positive whole number"),
        (
            "let x = 3s",
            "tick x dt 2s",
            "not a whole number of `dt 2s` ticks",
        ),
        ("let x = 3s", "tick nix", "undeclared constant `nix`"),
    ] {
        let src = format!(
            "{lets}\nextern vi = VirtualIn(iname: \"VI1\")\n\
             test \"t\"\n\tvi.Q = 1\n\t{tick}\n\texpect vi.Q == 1\nend\n"
        );
        let e = Module::parse(&src)
            .and_then(|m| m.validate())
            .unwrap_err()
            .to_string();
        assert!(e.contains(want), "{tick} with {lets}: {e}");
    }
}

const PAAR: &str = "extern vi = VirtualIn(iname: \"VI1\")\n\n\
                    template paar(x: VirtualIn) -> ein\n\
                    \tein = Not(I: x.Q)\n\
                    \taus = Not(I: ein.Q)\n\
                    end\n\n\
                    a = paar(x: vi)\n";

#[test]
fn scoped_names_work_in_bulk_statements_and_mirrors() {
    let src = format!(
        "{PAAR}\n\
         ref_aus = InputRef(mirrors: a.aus, AI: a.aus.Q)\n\
         ref_ein = InputRef(mirrors: a, AI: a.Q)\n\n\
         (a.aus, a).room = \"Pool\"\n"
    );
    let m = Module::parse(&src).unwrap();
    assert_eq!(m.to_text(), src);
    let flat = m.expand().unwrap();
    let mirrors: Vec<String> = flat
        .blocks()
        .filter(|b| b.block_type == "InputRef")
        .flat_map(|b| b.params().map(|(_, v)| v.to_string()).collect::<Vec<_>>())
        .collect();
    assert_eq!(mirrors, ["a_aus", "a_ein"]);
    let titled: Vec<&str> = flat
        .items
        .iter()
        .filter_map(|i| match i {
            Item::Attr(a) => Some(a.slug.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(titled, ["a_aus", "a_ein"]);

    // The expanded spelling is refused there as everywhere.
    let e = Module::parse(&format!("{PAAR}\n(a_aus, vi).title = \"x\"\n"))
        .and_then(|m| m.expand())
        .unwrap_err()
        .to_string();
    assert!(e.contains("Write `a.aus` instead of `a_aus`"), "{e}");
}

#[test]
fn errors_spell_instance_blocks_like_the_source() {
    // An unknown port on a body block, found after expansion.
    let e = compile_err(&format!("{PAAR}\nb = Not(I: a.aus.Nope)\n"));
    assert!(e.contains("`a.aus`"), "{e}");
    assert!(!e.contains("a_aus"), "{e}");
    let e = compile_err(&format!("{PAAR}\nb = Not(I: a.Nope)\n"));
    assert!(e.contains("block `a`"), "{e}");
    assert!(!e.contains("a_ein"), "{e}");
}

#[test]
fn a_signal_named_like_an_expression_sink_is_refused() {
    let e = compile_err(
        "extern vi = VirtualIn(iname: \"VI1\")\n\
         a = And(I1: vi.Q < 3 and vi.Q > 1, I2: vi.Q)\n\
         a_i1 = vi.Q < 5 and vi.Q > 2\n",
    );
    assert!(
        e.contains(
            "the expression on `a.I1` and the named signal `a_i1` would both name their \
             blocks `a_i1__<op><n>`"
        ),
        "{e}"
    );
    assert!(e.contains("Rename one of them"), "{e}");
}
