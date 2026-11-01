//! Named signals (D81): `slug = <expr>` desugars like an inline
//! expression, the top block named `slug` (hand-owned), its sub-blocks
//! `slug__<op><n>` (expression-owned).

use loxone_config::logic::{CompileOptions, Module, compile, plan_tests, run_tests};
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
extern aussentemp = VirtualIn(iname: \"VI1\")\n\
extern wind_alarm = VirtualIn(iname: \"VI2\")\n\
extern sonne = VirtualIn(iname: \"VI3\")\n\
extern jal_sued = AutoJalousie(title: \"Beschattung S\u{fc}d\")\n\
\n\
let schwelle = 28\n\
\n";

fn module(body: &str) -> Module {
    Module::parse(&format!("{HEAD}{body}")).unwrap()
}

const SIGNAL: &str = "\
beschatten = sonne.Q and aussentemp.Q >= schwelle  # Sonne und warm\n\
\n\
jal_sued.AutoShade <- beschatten.Q\n";

#[test]
fn a_named_signal_desugars_with_its_own_slug_on_the_top_block() {
    let m = module(SIGNAL);

    // Canonical text: the statement as written, a fixpoint.
    let text = m.to_text();
    assert!(
        text.contains("beschatten = sonne.Q and aussentemp.Q >= schwelle # Sonne und warm\n"),
        "{text}"
    );
    assert_eq!(Module::parse(&text).unwrap(), m, "parse ∘ to_text = id");
    assert_eq!(Module::parse(&text).unwrap().to_text(), text, "fixpoint");

    // The top block is `beschatten`, the sub-blocks are `beschatten__…`;
    // only those are synthetic.
    let (plain, info) = m.expand().unwrap().desugar().unwrap();
    assert_eq!(info.expressions, 1);
    assert_eq!(
        info.synthetic.iter().collect::<Vec<_>>(),
        ["beschatten__ge1"]
    );
    assert_eq!(info.signals.iter().collect::<Vec<_>>(), ["beschatten"]);
    let top = plain.blocks().find(|b| b.slug == "beschatten").unwrap();
    assert_eq!(top.block_type, "And");
    assert_eq!(
        top.title.as_deref(),
        Some("sonne.Q and aussentemp.Q >= schwelle"),
        "the label is the expression text, as inline"
    );
    let ge = plain
        .blocks()
        .find(|b| b.slug == "beschatten__ge1")
        .unwrap();
    assert_eq!(ge.block_type, "GreaterEqual");

    // Locked: the top block hand-owned, the comparator expression-owned.
    let base = base();
    let mut lock = Lockfile::new();
    let out = compile(&base, &m, &mut lock, &opts()).unwrap();
    assert_eq!(
        lock.objects.keys().collect::<Vec<_>>(),
        ["beschatten", "beschatten__ge1"]
    );
    assert!(!lock.objects["beschatten"].expr_owned);
    assert!(lock.objects["beschatten__ge1"].expr_owned);
    let out2 = compile(&base, &m, &mut lock, &opts()).unwrap();
    assert_eq!(out.to_bytes(), out2.to_bytes(), "deterministic, no re-mint");
}

#[test]
fn a_named_signal_compiles_like_the_inline_expression() {
    // Same blocks, same wires — only the slugs (and so the minted UUIDs'
    // order) differ. Compare the decompiled shape: types and labels.
    let base = base();
    let inline = module("jal_sued.AutoShade <- sonne.Q and aussentemp.Q >= schwelle\n");
    let named = module(SIGNAL);
    let shape = |m: &Module| {
        let (plain, _) = m.expand().unwrap().desugar().unwrap();
        let mut v: Vec<(String, Option<String>)> = plain
            .blocks()
            .map(|b| (b.block_type.clone(), b.title.clone()))
            .collect();
        v.sort();
        v
    };
    assert_eq!(shape(&inline), shape(&named));
    let mut lock = Lockfile::new();
    let a = compile(&base, &inline, &mut lock, &opts()).unwrap();
    let mut lock = Lockfile::new();
    let b = compile(&base, &named, &mut lock, &opts()).unwrap();
    assert_eq!(a.objects().len(), b.objects().len());
}

#[test]
fn signal_identity_follows_the_lifecycle_rules() {
    let base = base();
    let mut lock = Lockfile::new();
    compile(&base, &module(SIGNAL), &mut lock, &opts()).unwrap();
    let top = lock.objects["beschatten"].uuid.clone();
    let ge = lock.objects["beschatten__ge1"].uuid.clone();

    // Growing under the top operator keeps every identity and mints
    // only the new gate.
    let grown = SIGNAL.replace(
        "aussentemp.Q >= schwelle ",
        "(aussentemp.Q >= schwelle or wind_alarm.Q) ",
    );
    compile(&base, &module(&grown), &mut lock, &opts()).unwrap();
    assert_eq!(lock.objects["beschatten"].uuid, top);
    assert_eq!(lock.objects["beschatten__ge1"].uuid, ge);
    assert!(lock.objects["beschatten__or1"].expr_owned);

    // A different top operator is a different block type: refused with
    // the way out, like a hand-written block's type change.
    let mut probe = lock.clone();
    let err = compile(
        &base,
        &module(&SIGNAL.replace("sonne.Q and", "sonne.Q or")),
        &mut probe,
        &opts(),
    )
    .unwrap_err()
    .to_string();
    assert!(
        err.contains("signal `beschatten` changed its top operator"),
        "{err}"
    );
    assert!(err.contains("`removed beschatten`"), "{err}");

    // `moved` renames the signal with its sub-blocks.
    let renamed = format!(
        "{}moved beschatten -> schatten\n",
        grown.replace("beschatten", "schatten")
    );
    compile(&base, &module(&renamed), &mut lock, &opts()).unwrap();
    assert_eq!(lock.objects["schatten"].uuid, top);
    assert_eq!(lock.objects["schatten__ge1"].uuid, ge);
    assert!(lock.objects["schatten__ge1"].expr_owned);
    assert!(!lock.objects.contains_key("beschatten__ge1"));

    // The statement gone: the hand-owned top block needs `removed`, the
    // expression-owned ones go along without one.
    let gone = "jal_sued.AutoShade <- sonne.Q\n";
    let mut probe = lock.clone();
    let err = compile(&base, &module(gone), &mut probe, &opts())
        .unwrap_err()
        .to_string();
    assert!(err.contains("schatten"), "{err}");
    compile(
        &base,
        &module(&format!("{gone}removed schatten\n")),
        &mut lock,
        &opts(),
    )
    .unwrap();
    assert!(lock.objects.is_empty(), "{:?}", lock.objects.keys());
}

#[test]
fn an_arithmetic_signal_is_a_formula_its_aq_feeds_a_comparison() {
    // Arithmetic under a comparison is deferred inline; a named signal
    // spells the two steps.
    let m = module(
        "temp_f = aussentemp.Q * 9 / 5 + 32\n\
         heiss = temp_f.AQ >= 90\n\
         \n\
         jal_sued.AutoShade <- heiss.Q\n",
    );
    let (plain, info) = m.expand().unwrap().desugar().unwrap();
    assert!(info.synthetic.is_empty(), "{:?}", info.synthetic);
    let f = plain.blocks().find(|b| b.slug == "temp_f").unwrap();
    assert_eq!(f.block_type, "Formula");
    let heiss = plain.blocks().find(|b| b.slug == "heiss").unwrap();
    assert_eq!(heiss.block_type, "GreaterEqual");
    let mut lock = Lockfile::new();
    compile(&base(), &m, &mut lock, &opts()).unwrap();
    assert_eq!(lock.objects.keys().collect::<Vec<_>>(), ["heiss", "temp_f"]);

    // A Formula's output is `AQ`: `.Q` is refused by port validation.
    let wrong = module(
        "temp_f = aussentemp.Q * 2\n\
         \n\
         jal_sued.AutoShade <- temp_f.Q\n",
    );
    let err = compile(&base(), &wrong, &mut Lockfile::new(), &opts())
        .unwrap_err()
        .to_string();
    assert!(err.contains("temp_f"), "{err}");
}

#[test]
fn named_signals_expand_per_template_instance() {
    let m = module(
        "template fassade(jalousie: AutoJalousie, grenze = 28)\n\
         \twarm = sonne.Q and aussentemp.Q >= grenze\n\
         \tjalousie.AutoShade <- warm.Q\n\
         end\n\
         \n\
         sued = fassade(jalousie: jal_sued, grenze: 30)\n",
    );
    // The canonical text keeps the body's signal.
    let text = m.to_text();
    assert!(
        text.contains("\twarm = sonne.Q and aussentemp.Q >= grenze\n"),
        "{text}"
    );
    assert_eq!(Module::parse(&text).unwrap(), m);

    let mut lock = Lockfile::new();
    compile(&base(), &m, &mut lock, &opts()).unwrap();
    assert_eq!(
        lock.objects.keys().collect::<Vec<_>>(),
        ["sued_warm", "sued_warm__ge1"]
    );
    assert!(!lock.objects["sued_warm"].expr_owned);
    assert!(lock.objects["sued_warm__ge1"].expr_owned);
}

#[test]
fn named_signals_are_referenced_like_blocks_in_tests() {
    let m = module(&format!(
        "{SIGNAL}\n\
         test \"Sonne und Hitze\"\n\
         \tsonne.Q = on\n\
         \taussentemp.Q = 30\n\
         \ttick 2\n\
         \texpect beschatten.Q == on\n\
         \taussentemp.Q = 20\n\
         \ttick 2\n\
         \texpect beschatten.Q == off\n\
         end\n"
    ));
    let mut lock = Lockfile::new();
    let compiled = compile(&base(), &m, &mut lock, &opts()).unwrap();
    let (flat, _) = m.expand().unwrap().desugar().unwrap();
    let plan = plan_tests(&flat, &lock, &compiled, None).unwrap();
    let results = run_tests(&plan, &compiled);
    assert_eq!(results.len(), 1);
    assert!(
        results[0].pass,
        "{:?} {:?}",
        results[0].failures, results[0].error
    );
}

#[test]
fn named_signal_misuse_is_reported() {
    let fails = |body: &str, needle: &str| {
        let err = Module::parse(&format!("{HEAD}{body}\n"))
            .and_then(|m| {
                m.validate()?;
                let m = m.expand()?;
                m.validate()?;
                m.desugar().map(|_| ())
            })
            .unwrap_err()
            .to_string();
        assert!(
            err.contains(needle),
            "`{body}`: expected {needle:?} in: {err}"
        );
    };
    // An alias is not a block; a value is a constant.
    fails("warm = sonne.Q", "an alias is not a block");
    fails("warm = (sonne.Q)", "an alias is not a block");
    fails("warm = 5", "`let warm = 5`");
    fails("warm = on", "`let warm = on`");
    fails("warm = schwelle", "`let warm = schwelle`");
    // Its inputs are its expression.
    fails(
        "warm = sonne.Q and wind_alarm.Q\nwarm.I1 <- sonne.Q",
        "targets named signal `warm`",
    );
    fails(
        "warm = sonne.Q and wind_alarm.Q\nwarm.I1 = 1",
        "targets named signal `warm`",
    );
    fails(
        "warm = sonne.Q and wind_alarm.Q\nwarm.room = \"x\"",
        "takes only a `.title`",
    );
    fails(
        "warm = sonne.Q and wind_alarm.Q\nwarm.title = \"x\"\nwarm.title = \"y\"",
        "duplicate assignment `warm.title = …`",
    );
    // One namespace with every other name.
    fails(
        "warm = sonne.Q and wind_alarm.Q\nwarm = Or(I1: sonne.Q)",
        "duplicate name `warm`",
    );
    fails(
        "schwelle = sonne.Q and wind_alarm.Q",
        "duplicate name `schwelle`",
    );
    // Its sub-block namespace is its own.
    fails(
        "warm__ge1 = Or(I1: sonne.Q)\nwarm = sonne.Q and aussentemp.Q >= 5",
        "claimed by the expression",
    );
    fails("warm = nix.Q and sonne.Q", "undeclared slug `nix`");
    // Nothing forwards onto a template's only signal.
    fails(
        "template t()\n\twarm = sonne.Q and wind_alarm.Q\nend\nx = t(I1: sonne.Q)",
        "named signal `warm`",
    );
}

#[test]
fn a_signal_title_labels_its_top_block() {
    let m = module(
        "let titel = \"Beschatten\"\n\
         beschatten = sonne.Q and aussentemp.Q >= schwelle\n\
         \n\
         beschatten.title = titel\n\
         warm = aussentemp.Q >= 30\n\
         warm.title = \"Warm\"  # eigener Titel\n",
    );
    // fmt keeps each title right below its signal.
    let text = m.to_text();
    assert!(
        text.contains(
            "beschatten = sonne.Q and aussentemp.Q >= schwelle\nbeschatten.title = titel\n\
             warm = aussentemp.Q >= 30\nwarm.title = \"Warm\" # eigener Titel\n"
        ),
        "{text}"
    );
    assert_eq!(Module::parse(&text).unwrap().to_text(), text, "fixpoint");

    let (plain, _) = m.expand().unwrap().desugar().unwrap();
    let title = |slug: &str| {
        plain
            .blocks()
            .find(|b| b.slug == slug)
            .and_then(|b| b.title.clone())
    };
    assert_eq!(title("beschatten").as_deref(), Some("Beschatten"));
    assert_eq!(title("warm").as_deref(), Some("Warm"));
    // The sub-blocks keep their expression text.
    assert_eq!(
        title("beschatten__ge1").as_deref(),
        Some("aussentemp.Q >= schwelle")
    );
    assert_eq!(plain.attrs().count(), 0, "the statements are consumed");

    // Only the label changes: the lock keys stay the same.
    let mut a = Lockfile::new();
    compile(&base(), &module(SIGNAL), &mut a, &opts()).unwrap();
    let mut b = Lockfile::new();
    let titled = module(&format!("{SIGNAL}beschatten.title = \"Beschatten\"\n"));
    let out = compile(&base(), &titled, &mut b, &opts()).unwrap();
    assert_eq!(
        a.objects.keys().collect::<Vec<_>>(),
        b.objects.keys().collect::<Vec<_>>()
    );
    let uuid = &b.objects["beschatten"].uuid;
    let top = out.objects().into_iter().find(|o| &o.uuid == uuid).unwrap();
    assert_eq!(top.title.as_deref(), Some("Beschatten"));
}

#[test]
fn a_template_body_signal_takes_a_title() {
    let m = module(
        "template fassade(jalousie: AutoJalousie, name = \"Süd\")\n\
         \twarm = sonne.Q and aussentemp.Q >= 28\n\
         \twarm.title = \"Warm {name}\"\n\
         \tjalousie.AutoShade <- warm.Q\n\
         end\n\
         \n\
         sued = fassade(jalousie: jal_sued)\n",
    );
    let (plain, _) = m.expand().unwrap().desugar().unwrap();
    let top = plain.blocks().find(|b| b.slug == "sued_warm").unwrap();
    assert_eq!(top.title.as_deref(), Some("Warm Süd"));
    // Only a body signal takes one.
    let err = Module::parse("template t()\n\tx = Or()\n\tx.title = \"X\"\nend\n")
        .unwrap_err()
        .to_string();
    assert!(err.contains("named signals and their titles"), "{err}");
}

#[test]
fn renaming_a_signal_rekeys_its_sub_blocks() {
    use loxone_config::logic::{RekeyKind, lock_rekeys, rename_slug};
    let old = module(SIGNAL);
    let mut new = old.clone();
    rename_slug(&mut new, "beschatten", "schatten");
    let text = new.to_text();
    assert!(text.contains("schatten = sonne.Q and"), "{text}");
    assert!(text.contains("jal_sued.AutoShade <- schatten.Q"), "{text}");
    let rekeys = lock_rekeys(&old, &new).unwrap();
    assert_eq!(
        rekeys,
        [
            (
                "beschatten__ge1".to_string(),
                "schatten__ge1".to_string(),
                RekeyKind::Object
            ),
            (
                "beschatten".to_string(),
                "schatten".to_string(),
                RekeyKind::Object
            ),
        ]
    );
}

#[test]
fn adopt_lifts_expressions_and_signals_back() {
    use loxone_config::logic::{DecompileOptions, adopt, decompile};
    let src = format!(
        "{SIGNAL}beschatten.title = \"Beschatten\"\n\
         warm = aussentemp.Q >= 30\n\
         jal_sued.Safety <- not wind_alarm.Q\n"
    );
    let out = compile(&base(), &module(&src), &mut Lockfile::new(), &opts()).unwrap();

    let (view, _) = decompile(&out, &DecompileOptions::default()).unwrap();
    let text = view.to_text();
    // The names come from the config: the extern's from its title, an
    // unlabelled signal's from its expression text.
    for line in [
        "let schwelle = 28\n",
        "AutoShade: beschatten.Q, Safety: not wind_alarm.Q,",
        "beschatten = sonnenschein.Q and aussentemp.Q >= schwelle\n\
         beschatten.title = \"Beschatten\"\n",
        "aussentemp_q_30 = aussentemp.Q >= 30\n",
    ] {
        assert!(text.contains(line), "{line:?} missing in\n{text}");
    }
    assert!(!text.contains("And("), "{text}");

    // The adoption compiles back to the same file, the sub-blocks
    // expression-owned.
    let (adopted, lock, _) = adopt(&out).unwrap();
    assert!(lock.objects["beschatten__ge1"].expr_owned);
    assert!(!lock.objects["beschatten"].expr_owned);
    let again = compile(&out, &adopted, &mut lock.clone(), &opts()).unwrap();
    assert_eq!(out.to_bytes(), again.to_bytes());
}
