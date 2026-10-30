//! End-to-end IR pipeline tests over the shipped example fixture:
//! determinism, lockfile identity, teardown/restore semantics, and the
//! decompile view of compiled output.

use lxir::ir::{
    ArgItem, Binding, BindingKind, CompileOptions, DecompileOptions, DecompileScope, Item, Module,
    PortRef, Value, adopt, adopt_one, adopt_pages, compile, decompile, decompile_pages,
};
use lxir::uuid::parse_serial;
use lxir::xml::{Attr, Element, Node};
use lxir::{Lockfile, LoxoneDoc};

const MINT_TIME: i64 = 1_767_225_600; // 2026-01-01T00:00:00Z

fn base() -> LoxoneDoc {
    LoxoneDoc::parse(&std::fs::read("examples/configs/haus.Loxone").unwrap()).unwrap()
}

fn module() -> Module {
    Module::parse(&std::fs::read_to_string("examples/ir/beschattung.lxir").unwrap()).unwrap()
}

fn opts() -> CompileOptions {
    CompileOptions {
        machine: parse_serial("504F94112233").unwrap(),
        mint_time_unix: MINT_TIME,
        page_title: Some("Beschattung".into()),
        allow_removals: false,
        accept_version: None,
        relayout: Vec::new(),
    }
}

#[test]
fn compile_is_deterministic_and_lock_pins_identity() {
    let base = base();
    let module = module();

    // Fresh lock, twice: identical bytes and identical locks.
    let mut lock_a = Lockfile::new();
    let out_a = compile(&base, &module, &mut lock_a, &opts()).unwrap();
    let mut lock_b = Lockfile::new();
    let out_b = compile(&base, &module, &mut lock_b, &opts()).unwrap();
    assert_eq!(out_a.to_bytes(), out_b.to_bytes());
    assert_eq!(lock_a.to_json(), lock_b.to_json());

    // Recompiling with the produced lock changes nothing.
    let out_c = compile(&base, &module, &mut lock_a.clone(), &opts()).unwrap();
    assert_eq!(out_a.to_bytes(), out_c.to_bytes());

    // Even with a different mint time: every UUID comes from the lock.
    let late = CompileOptions {
        mint_time_unix: MINT_TIME + 999_999,
        ..opts()
    };
    let out_d = compile(&base, &module, &mut lock_a.clone(), &late).unwrap();
    assert_eq!(out_a.to_bytes(), out_d.to_bytes());

    // Compiled output itself roundtrips byte-identically.
    let bytes = out_a.to_bytes();
    assert_eq!(LoxoneDoc::parse(&bytes).unwrap().to_bytes(), bytes);
}

#[test]
fn recompiling_own_output_converges() {
    // Simulate the real workflow: compile, upload, download the (identical)
    // config, compile again against it. The compiler must tear down exactly
    // its own edits and rebuild them — a fixpoint.
    let module = module();
    let mut lock = Lockfile::new();
    let first = compile(&base(), &module, &mut lock, &opts()).unwrap();
    let second = compile(&first, &module, &mut lock, &opts()).unwrap();
    assert_eq!(first.to_bytes(), second.to_bytes());
}

#[test]
fn recompiling_a_gui_saved_config_keeps_its_in_order() {
    // D45: Loxone Config sorts a connector's `<In>` children on save. Here
    // the GUI saved a wire of its own behind the compiler's wire into the
    // extern sink; the recompile tears the compiler's wire down and must
    // put it back in front — not append it — to reproduce the save.
    let module = module();
    let mut lock = Lockfile::new();
    let mut saved = compile(&base(), &module, &mut lock, &opts()).unwrap();
    let path = saved
        .objects()
        .iter()
        .find(|o| o.block_type == "AutoJalousie")
        .unwrap()
        .path
        .clone();
    let co = saved
        .element_at_mut(&path)
        .unwrap()
        .child_elements_mut()
        .find(|c| c.name == "Co" && c.attr("K") == Some("AutoShade"))
        .unwrap();
    assert_eq!(co.attr("Nc"), Some("1"), "the compiler's wire");
    let mut gui = Element::new("In");
    gui.set_attr("Input", "20000003-0000-0032-01ffaaaaaaaa0003"); // Sonnenschein.Qm
    co.push_child(gui);
    co.set_attr("Nc", "2");

    let out = compile(&saved, &module, &mut lock, &opts()).unwrap();
    assert_eq!(
        String::from_utf8_lossy(&out.to_bytes()),
        String::from_utf8_lossy(&saved.to_bytes())
    );
}

#[test]
fn recompiling_a_gui_saved_config_keeps_its_block_order() {
    // D47: Loxone Config writes objects, attributes and connectors in an
    // order of its own on save. Here the save drew the managed GreaterEqual
    // in front of the extern AutoJalousie, moved its `Cl=` behind `Type=`
    // and its `Input2` connector in front of `Input1`; the recompile tears
    // the block down and must rebuild it exactly as saved.
    let module = module();
    let mut lock = Lockfile::new();
    let mut saved = compile(&base(), &module, &mut lock, &opts()).unwrap();
    let path = saved
        .objects()
        .iter()
        .find(|o| o.block_type == "GreaterEqual")
        .unwrap()
        .path
        .clone();
    let (at, page_path) = path.split_last().unwrap();
    let page = saved.element_at_mut(page_path).unwrap();
    let mut block = match page.children.remove(*at) {
        Node::Element(e) => e,
        Node::Text(_) => unreachable!(),
    };
    let cl = block.attrs.iter().position(|a| a.name == "Cl").unwrap();
    let cl = block.attrs.remove(cl);
    block.attrs.insert(1, cl);
    let input2 = block
        .children
        .iter()
        .position(|n| matches!(n, Node::Element(e) if e.attr("K") == Some("Input2")))
        .unwrap();
    let input2 = block.children.remove(input2);
    block.children.insert(0, input2);
    let first = page
        .children
        .iter()
        .position(|n| matches!(n, Node::Element(e) if e.name == "C"))
        .unwrap();
    page.children.insert(first, Node::Element(block));

    let out = compile(&saved, &module, &mut lock, &opts()).unwrap();
    assert_eq!(
        String::from_utf8_lossy(&out.to_bytes()),
        String::from_utf8_lossy(&saved.to_bytes())
    );
}

#[test]
fn managed_blocks_carry_the_documents_block_version() {
    // D46: Loxone Config writes every object at its release's block
    // version on save (17.2: V="178"). Compile emits that version when the
    // builtin table is verified for it, so a save has nothing to restamp,
    // and keeps the first verified one (175) for any other.
    let saved_at = |v: &str| {
        let text = std::fs::read_to_string("examples/configs/haus.Loxone").unwrap();
        let text = text.replace("V=\"175\"", &format!("V=\"{v}\""));
        LoxoneDoc::parse(text.as_bytes()).unwrap()
    };
    let versions = |doc: &LoxoneDoc, types: &[&str]| -> Vec<String> {
        doc.objects()
            .iter()
            .filter(|o| types.contains(&o.block_type.as_str()))
            .map(|o| {
                doc.element_at(&o.path)
                    .unwrap()
                    .attr("V")
                    .unwrap()
                    .to_string()
            })
            .collect()
    };
    let managed = ["GreaterEqual", "And"];

    let mut lock = Lockfile::new();
    let out = compile(&saved_at("178"), &module(), &mut lock, &opts()).unwrap();
    assert_eq!(versions(&out, &managed), ["178", "178"]);
    let again = compile(&out, &module(), &mut lock, &opts()).unwrap();
    assert_eq!(out.to_bytes(), again.to_bytes());

    // Adopt accepts 178 blocks and rebuilds them as they are.
    let (adopted, adopted_lock, report) = adopt(&out).unwrap();
    assert!(
        report.refused.iter().all(|r| !r.contains("block version")),
        "{:?}",
        report.refused
    );
    let rebuilt = compile(&out, &adopted, &mut adopted_lock.clone(), &opts()).unwrap();
    assert_eq!(versions(&rebuilt, &managed), ["178", "178"]);

    // An unverified version: managed blocks stay at 175 for Loxone
    // Config to migrate on load.
    let out = compile(&saved_at("180"), &module(), &mut Lockfile::new(), &opts()).unwrap();
    assert_eq!(versions(&out, &managed), ["175", "175"]);
    assert_eq!(versions(&out, &["AutoJalousie"]), ["180"]);
}

#[test]
fn counters_advance_once_per_minted_object_and_never_decrease() {
    let base = base();
    assert_eq!(base.counters().next_obj, 200);
    let mut lock = Lockfile::new();
    let out = compile(&base, &module(), &mut lock, &opts()).unwrap();
    // Two managed blocks minted → NextObj 200 + 2.
    assert_eq!(out.counters().next_obj, 202);
    // Recompile: nothing new minted.
    let out2 = compile(&base, &module(), &mut lock, &opts()).unwrap();
    assert_eq!(out2.counters().next_obj, 202);
}

#[test]
fn vanished_slug_is_a_hard_error_until_removed_from_lock() {
    let base = base();
    let mut lock = Lockfile::new();
    compile(&base, &module(), &mut lock, &opts()).unwrap();

    let smaller = Module::parse(
        "extern jal_sued = AutoJalousie(title: \"Beschattung S\u{fc}d\")\n\
         temp_hoch = GreaterEqual()\n",
    )
    .unwrap();
    let err = compile(&base, &smaller, &mut lock, &opts()).unwrap_err();
    assert!(err.to_string().contains("beschatten"), "{err}");
    assert!(err.to_string().contains("allow_removals"), "{err}");

    // Forgetting via remove_object also satisfies the sync check.
    lock.remove_object("beschatten").unwrap();
    let out = compile(&base, &smaller, &mut lock, &opts()).unwrap();
    assert!(!out.objects().iter().any(|o| o.block_type == "And"));
}

#[test]
fn removing_set_restores_original_def_and_wires_tear_down() {
    let base = base();
    let mut lock = Lockfile::new();
    let with_edits = compile(&base, &module(), &mut lock, &opts()).unwrap();

    // TargetPos rewritten 100 → 70; two wires landed on the extern.
    let jal = |doc: &LoxoneDoc| {
        let objs = doc.objects();
        let o = objs
            .iter()
            .find(|o| o.block_type == "AutoJalousie")
            .unwrap();
        lxir::doc::ports(doc.element_at(&o.path).unwrap())
    };
    let target = |doc: &LoxoneDoc| {
        jal(doc)
            .iter()
            .find(|p| p.key == "TargetPos")
            .unwrap()
            .def
            .clone()
    };
    assert_eq!(target(&with_edits).as_deref(), Some("70"));
    let wired: usize = jal(&with_edits).iter().map(|p| p.inputs.len()).sum();
    assert_eq!(wired, 2);

    // Drop everything from source. Without allow_removals this must refuse;
    // with it, the compiler deletes its blocks and reverts its edits.
    let empty = Module::parse("").unwrap();
    assert!(compile(&with_edits, &empty, &mut lock.clone(), &opts()).is_err());
    let destroy = CompileOptions {
        allow_removals: true,
        ..opts()
    };
    let reverted = compile(&with_edits, &empty, &mut lock, &destroy).unwrap();
    assert_eq!(
        target(&reverted).as_deref(),
        Some("100"),
        "original Def restored"
    );
    let wired: usize = jal(&reverted).iter().map(|p| p.inputs.len()).sum();
    assert_eq!(wired, 0, "our extern wires removed");
    assert!(lock.set_originals.is_empty());
    assert!(lock.extern_wires.is_empty());
    assert!(lock.objects.is_empty(), "removed blocks dropped from lock");
    // Managed blocks are gone again; only the base objects remain.
    assert_eq!(reverted.objects().len(), base.objects().len());
}

#[test]
fn ambiguity_and_no_match_are_reported() {
    let base = base();
    let mut lock = Lockfile::new();
    let nomatch = Module::parse("extern x = VirtualIn(iname: \"VI99\")\n").unwrap();
    let err = compile(&base, &nomatch, &mut lock, &opts()).unwrap_err();
    assert!(err.to_string().contains("VI99"), "{err}");

    // Titles are not unique across VirtualIns? In the fixture they are —
    // match on a type instead where two objects share nothing: use a title
    // that exists twice by matching VirtualIn Qm... simplest ambiguous spec:
    // both And/Or absent → keep to NoMatch here; ambiguity is covered by
    // the unit-level resolver behavior below.
    let unknown_port = Module::parse(
        "extern jal = AutoJalousie(title: \"Beschattung S\u{fc}d\")\n\
         b = And()\n\
         jal.DoesNotExist <- b.Q\n",
    )
    .unwrap();
    let err = compile(&base, &unknown_port, &mut Lockfile::new(), &opts()).unwrap_err();
    assert!(err.to_string().contains("DoesNotExist"), "{err}");
    assert!(
        err.to_string().contains("InputTrigger"),
        "lists available ports: {err}"
    );
}

#[test]
fn unverified_block_type_is_refused() {
    let m = Module::parse("t = Sauna()\n").unwrap();
    let err = compile(&base(), &m, &mut Lockfile::new(), &opts()).unwrap_err();
    assert!(err.to_string().contains("builtin table"), "{err}");
}

#[test]
fn pool_showcase_compiles_end_to_end() {
    // The showcase module exercising the whole language: page, let,
    // extern (iname / title+room), a template instance, an expression
    // binding, a unit value, and an expression wire — committed under
    // examples/ and kept compiling.
    let base = LoxoneDoc::parse(&std::fs::read("examples/configs/pool.Loxone").unwrap()).unwrap();
    let src = std::fs::read_to_string("examples/ir/pool.lxir").unwrap();
    let module = Module::parse(&src).unwrap();
    assert_eq!(module.to_text(), src, "example stays canonical");

    let opts = CompileOptions {
        page_title: Some("Pool".into()),
        ..opts()
    };
    let mut lock = Lockfile::new();
    let out = compile(&base, &module, &mut lock, &opts).unwrap();

    // `room: "Technikraum"` picked the right one of two "Freigabe".
    assert_eq!(
        lock.externals["wp_freigabe"].uuid,
        "30000006-0000-0060-ffff504f94112233"
    );
    // Three declared blocks (the template's `frost_alarm`, the
    // Monoflop, the threshold trigger) plus seven desugared: one gate
    // from the argument expression, five from the three-way wire
    // expression, one Formula from the arithmetic binding (D35).
    assert_eq!(lock.objects.len(), 10);
    assert!(lock.objects.contains_key("frost_alarm"));
    assert!(lock.objects["nachlauf_inputtrigger__ge1"].expr_owned);
    assert!(lock.objects["wp_freigabe_on__and2"].expr_owned);
    assert!(lock.objects["pv_kw_input__f1"].expr_owned);
    // `Time: 30min` compiled exactly to seconds.
    let nachlauf = out
        .objects()
        .iter()
        .find(|o| o.uuid == lock.objects["nachlauf"].uuid)
        .map(|o| o.path.clone())
        .unwrap();
    let time = lxir::doc::ports(out.element_at(&nachlauf).unwrap())
        .into_iter()
        .find(|p| p.key == "Time")
        .unwrap();
    assert_eq!(time.def.as_deref(), Some("1800"));

    // The enable wire landed on the Technikraum switch's On port.
    let objs = out.objects();
    let wp = objs
        .iter()
        .find(|o| o.uuid == "30000006-0000-0060-ffff504f94112233")
        .unwrap();
    let ports = lxir::doc::ports(out.element_at(&wp.path).unwrap());
    let on = ports.iter().find(|p| p.key == "On").unwrap();
    assert_eq!(on.inputs.len(), 1);

    // Fixpoint: recompiling against its own output changes nothing.
    let again = compile(&out, &module, &mut lock, &opts).unwrap();
    assert_eq!(out.to_bytes(), again.to_bytes());
}

#[test]
fn sprachtour_compiles_end_to_end() {
    // The systematic language tour (examples/ir/sprachtour.lxir): every
    // construct the narrative showcase does not cover — uuid/category
    // matchers, fan-in, the `Formula:` attribute parameter, template
    // interpolation/override/forwarding, both mirror forms (D32/D33),
    // mirror routing (D34), a second page, and attribute claims on
    // externs (D39).
    let base = LoxoneDoc::parse(&std::fs::read("examples/configs/pool.Loxone").unwrap()).unwrap();
    let src = std::fs::read_to_string("examples/ir/sprachtour.lxir").unwrap();
    let module = Module::parse(&src).unwrap();
    assert_eq!(module.to_text(), src, "example stays canonical");

    let opts = CompileOptions {
        page_title: Some("Pool".into()),
        ..opts()
    };
    let mut lock = Lockfile::new();
    let out = compile(&base, &module, &mut lock, &opts).unwrap();

    // The matcher palette resolved: uuid pin, category vs room on the
    // two "Freigabe" switches, `mirrors:` on the GUI-drawn ref.
    assert_eq!(
        lock.externals["wassertemp"].uuid,
        "30000001-0000-0010-ffff504f94112233"
    );
    assert_eq!(
        lock.externals["filterpumpe"].uuid,
        "30000005-0000-0050-ffff504f94112233"
    );
    assert_eq!(
        lock.externals["waermepumpe"].uuid,
        "30000006-0000-0060-ffff504f94112233"
    );
    assert_eq!(
        lock.externals["netz_spiegel"].uuid,
        "3000000a-0000-00a0-ffff504f94112233"
    );

    // 11 declared blocks + 2 template-expanded + 5 desugared.
    assert_eq!(lock.objects.len(), 18);
    assert!(lock.objects["anforderung_i2__ge1"].expr_owned);
    assert!(lock.objects["filterpumpe_on__and1"].expr_owned);
    assert!(lock.objects["speicher_input__eq1"].expr_owned);

    let element = |slug: &str| {
        let uuid = &lock.objects[slug].uuid;
        let obj = out
            .objects()
            .iter()
            .find(|o| &o.uuid == uuid)
            .unwrap()
            .path
            .clone();
        out.element_at(&obj).unwrap().clone()
    };
    let def_of = |slug: &str, key: &str| {
        lxir::doc::ports(&element(slug))
            .into_iter()
            .find(|p| p.key == key)
            .unwrap()
            .def
    };

    // Unit values compiled exactly (D27): 45min/1.5h/250ms/80%.
    assert_eq!(def_of("takt", "TimeHigh").as_deref(), Some("2700"));
    assert_eq!(def_of("takt", "TimeLow").as_deref(), Some("5400"));
    assert_eq!(def_of("blitz", "TimeHigh").as_deref(), Some("0.25"));
    assert_eq!(def_of("weit_offen", "Input2").as_deref(), Some("80"));

    // Template machinery (D23): value-parameter override, port
    // forwarding, and title interpolation from a string `let`.
    assert_eq!(def_of("uebertemp_alarm", "On").as_deref(), Some("32"));
    assert_eq!(
        def_of("uebertemp_alarm", "PulseTime").as_deref(),
        Some("10")
    );
    assert_eq!(
        element("hitze_alarm").attr("Title"),
        Some("Außenbereich: über Grenze")
    );

    // The `Formula:` attribute parameter with its Valid companion.
    let formula = element("mitteltemp");
    assert_eq!(formula.attr("Formula"), Some("(I1+I2)/2"));
    assert_eq!(formula.attr("Valid"), Some("false"));

    // Fan-in: two wires on the same gate input.
    let anforderung = element("anforderung");
    let i1 = lxir::doc::ports(&anforderung)
        .into_iter()
        .find(|p| p.key == "I1")
        .unwrap();
    assert_eq!(i1.inputs.len(), 2);

    // D34: the wire naming `netzbezug.Q` routes through the base ref on
    // the consuming page — the fixture ref's AQ, not the VirtualIn.
    let netz_hoch = element("netz_hoch");
    let routed = lxir::doc::ports(&netz_hoch)
        .into_iter()
        .find(|p| p.key == "Input1")
        .unwrap();
    assert_eq!(routed.inputs, vec!["3000000a-0000-00a3-02ffaaaaaaaa000a"]);

    // D33: the minted mirror carries the target identity, and a wire
    // naming the ref itself stays literal.
    let spiegel = element("wasser_spiegel");
    assert_eq!(
        spiegel.attr("Ref"),
        Some("30000001-0000-0010-ffff504f94112233")
    );
    assert_eq!(spiegel.attr("LinkRefType"), Some("71"));
    let kalt = lxir::doc::ports(&element("kalt_technik"))
        .into_iter()
        .find(|p| p.key == "Input1")
        .unwrap();
    assert_eq!(
        kalt.inputs,
        vec![lock.objects["wasser_spiegel"].ports["AQ"].clone()]
    );

    // D28: the two page statements placed the blocks on their pages.
    assert_eq!(
        lock.objects["heizbedarf"].page_uuid.as_deref(),
        Some("30000004-0000-0040-ffff504f94112233")
    );
    assert_eq!(
        lock.objects["kalt_technik"].page_uuid.as_deref(),
        Some("30000008-0000-0080-ffff504f94112233")
    );

    // D39: the device took title and room; the children that shared its
    // room followed, the one bound elsewhere stayed; the category claim
    // touched only its own input.
    const DEVICE: &str = "3000000e-0000-00e0-ffff504f94112233";
    const ONLINE: &str = "3000000f-0000-00f0-ffff504f94112233";
    const TEMP: &str = "30000010-0000-0100-ffff504f94112233";
    const BATTERY: &str = "30000011-0000-0110-ffff504f94112233";
    const TECHNIKRAUM: &str = "30000000-0000-0004-ffff504f94112233";
    const GARTEN: &str = "30000000-0000-0009-ffff504f94112233";
    const KLIMA: &str = "30000000-0000-000a-ffff504f94112233";
    let io = |uuid: &str, slot: &str| -> Option<String> {
        let path = out.objects().into_iter().find(|o| o.uuid == uuid)?.path;
        let c = out.element_at(&path)?;
        let io = c.child_elements().find(|e| e.name == "IoData")?;
        io.attr(slot).map(str::to_string)
    };
    let title_of = |uuid: &str| {
        out.objects()
            .into_iter()
            .find(|o| o.uuid == uuid)
            .and_then(|o| o.title)
    };
    assert_eq!(title_of(DEVICE).as_deref(), Some("Raumklima Garten"));
    for u in [DEVICE, ONLINE, TEMP] {
        assert_eq!(io(u, "Pr").as_deref(), Some(GARTEN), "{u} in Garten");
    }
    assert_eq!(
        io(BATTERY, "Pr").as_deref(),
        Some("30000000-0000-0003-ffff504f94112233"),
        "bound elsewhere, stays in Pool"
    );
    assert_eq!(io(TEMP, "Cr").as_deref(), Some(KLIMA));
    assert_eq!(
        io(ONLINE, "Cr").as_deref(),
        Some("30000000-0000-000b-ffff504f94112233"),
        "category claim has no followers here"
    );
    let room = &lock.attr_claims[DEVICE][&lxir::ir::Attr::Room];
    assert_eq!(room.original, TECHNIKRAUM);
    assert_eq!(room.written, GARTEN);
    assert_eq!(room.followers, [ONLINE, TEMP]);

    // Fixpoint: recompiling against its own output changes nothing.
    let again = compile(&out, &module, &mut lock, &opts).unwrap();
    assert_eq!(out.to_bytes(), again.to_bytes());
}

#[test]
fn project_example_is_current() {
    // The lox.toml project under examples/project/: compiling inside the
    // directory (lock/out redirected to a scratch copy) changes nothing
    // in the committed lock and reproduces the committed output byte for
    // byte — the same invariants examples/ci.sh checks.
    let root = env!("CARGO_MANIFEST_DIR");
    let tmp = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("project-example");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let lock = tmp.join("lock.json");
    let out = tmp.join("out.Loxone");
    std::fs::copy(format!("{root}/examples/project/pool.lock.json"), &lock).unwrap();
    let status = std::process::Command::new(env!("CARGO_BIN_EXE_lxir"))
        .current_dir(format!("{root}/examples/project"))
        .arg("compile")
        .arg("--lock")
        .arg(&lock)
        .arg("--out")
        .arg(&out)
        .status()
        .unwrap();
    assert!(status.success());
    assert_eq!(
        std::fs::read(&lock).unwrap(),
        std::fs::read(format!("{root}/examples/project/pool.lock.json")).unwrap(),
        "committed lock is current"
    );
    assert_eq!(
        std::fs::read(&out).unwrap(),
        std::fs::read(format!("{root}/examples/project/out.Loxone")).unwrap(),
        "committed output reproduces byte-identically"
    );
}

#[test]
fn lifecycle_stages_move_and_remove() {
    // The staged walkthrough under examples/ir/lifecycle/ (narrated by
    // examples/lifecycle.rs): `moved` carries identity across a rename,
    // an unauthorized disappearance refuses, `removed` deletes and
    // tombstones (D31), and the tombstone retires once deployed.
    let base = LoxoneDoc::parse(&std::fs::read("examples/configs/haus.Loxone").unwrap()).unwrap();
    let stage = |n: u8| {
        let src =
            std::fs::read_to_string(format!("examples/ir/lifecycle/schritt{n}.lxir")).unwrap();
        let m = Module::parse(&src).unwrap();
        assert_eq!(m.to_text(), src, "stage {n} stays canonical");
        m
    };
    let opts = CompileOptions {
        page_title: Some("Beschattung".into()),
        ..opts()
    };
    let mut lock = Lockfile::new();
    let out1 = compile(&base, &stage(1), &mut lock, &opts).unwrap();
    let born = lock.objects["wetter_ok"].uuid.clone();

    let out2 = compile(&out1, &stage(2), &mut lock, &opts).unwrap();
    assert_eq!(
        lock.objects["wetter_sperre"].uuid, born,
        "identity survives the rename"
    );
    assert!(!lock.objects.contains_key("wetter_ok"));

    let unauthorized = Module::parse(
        &std::fs::read_to_string("examples/ir/lifecycle/schritt3.lxir")
            .unwrap()
            .replace("removed altlast\n", ""),
    )
    .unwrap();
    let err = compile(&out2, &unauthorized, &mut lock, &opts).unwrap_err();
    assert!(err.to_string().contains("removed altlast"), "{err}");

    let out3 = compile(&out2, &stage(3), &mut lock, &opts).unwrap();
    assert!(!lock.objects.contains_key("altlast"));
    assert_eq!(lock.removed.len(), 1);
    assert_eq!(out3.objects().len(), out2.objects().len() - 1);

    compile(&out3, &stage(3), &mut lock, &opts).unwrap();
    assert!(lock.removed.is_empty(), "tombstone retires once deployed");
}

#[test]
fn composite_extern_matching_narrows_by_room_and_category() {
    // Two identically-titled Switches in different rooms; the room lives
    // in `<IoData Pr=…>` pointing at a Place (docs/loxone-format.md).
    let base = LoxoneDoc::parse(
        "<ControlList Version=\"1\" NextObj=\"20\">\r\n\
         \t<C Type=\"Document\" U=\"00000001-0000-0000-ffff000000000001\">\r\n\
         \t\t<C Type=\"Place\" U=\"00000002-0000-0000-ffff000000000001\" Title=\"B\u{fc}ro\"/>\r\n\
         \t\t<C Type=\"Place\" U=\"00000003-0000-0000-ffff000000000001\" Title=\"K\u{fc}che\"/>\r\n\
         \t\t<C Type=\"Category\" U=\"00000004-0000-0000-ffff000000000001\" Title=\"Beleuchtung\"/>\r\n\
         \t\t<C Type=\"Page\" U=\"00000005-0000-0000-ffff000000000001\" Title=\"P\">\r\n\
         \t\t\t<C Type=\"Switch\" U=\"00000006-0000-0000-ffff000000000001\" Title=\"Deckenlicht\">\r\n\
         \t\t\t\t<Co K=\"Trigger\" U=\"00000006-0000-0001-01ff000000000001\"/>\r\n\
         \t\t\t\t<IoData Visu=\"true\" Cr=\"00000004-0000-0000-ffff000000000001\" Pr=\"00000002-0000-0000-ffff000000000001\"/>\r\n\
         \t\t\t</C>\r\n\
         \t\t\t<C Type=\"Switch\" U=\"00000007-0000-0000-ffff000000000001\" Title=\"Deckenlicht\">\r\n\
         \t\t\t\t<Co K=\"Trigger\" U=\"00000007-0000-0001-01ff000000000001\"/>\r\n\
         \t\t\t\t<IoData Visu=\"true\" Pr=\"00000003-0000-0000-ffff000000000001\"/>\r\n\
         \t\t\t</C>\r\n\
         \t\t</C>\r\n\
         \t</C>\r\n\
         </ControlList>\r\n"
            .as_bytes(),
    )
    .unwrap();

    // Title alone is ambiguous.
    let m = Module::parse("extern licht = Switch(title: \"Deckenlicht\")\n").unwrap();
    let err = compile(&base, &m, &mut Lockfile::new(), &opts()).unwrap_err();
    assert!(err.to_string().contains("2"), "{err}");

    // The room narrows it to one.
    let m = Module::parse("extern licht = Switch(title: \"Deckenlicht\", room: \"B\u{fc}ro\")\n")
        .unwrap();
    let mut lock = Lockfile::new();
    compile(&base, &m, &mut lock, &opts()).unwrap();
    assert_eq!(
        lock.externals["licht"].uuid,
        "00000006-0000-0000-ffff000000000001"
    );

    // So does the category (only one Switch carries Cr=).
    let m =
        Module::parse("extern licht = Switch(title: \"Deckenlicht\", category: \"Beleuchtung\")\n")
            .unwrap();
    let mut lock = Lockfile::new();
    compile(&base, &m, &mut lock, &opts()).unwrap();
    assert_eq!(
        lock.externals["licht"].uuid,
        "00000006-0000-0000-ffff000000000001"
    );

    // A wrong room is NoMatch, and the error shows the full spec.
    let m =
        Module::parse("extern licht = Switch(title: \"Deckenlicht\", room: \"Bad\")\n").unwrap();
    let err = compile(&base, &m, &mut Lockfile::new(), &opts()).unwrap_err();
    assert!(err.to_string().contains("room: \"Bad\""), "{err}");

    // uuid + room is a parse error; the composite form survives fmt.
    assert!(
        Module::parse("extern x = Switch(uuid: \"00000006-0000-0000-ffff000000000001\", room: \"B\u{fc}ro\")\n")
            .is_err()
    );
    let src = "extern licht = Switch(title: \"Deckenlicht\", room: \"B\u{fc}ro\")\n";
    let m = Module::parse(src).unwrap();
    assert_eq!(m.to_text(), src, "canonical form round-trips");
}

#[test]
fn extern_matching_narrows_by_device_serial() {
    // Two identical Tree devices, each with a "Taster 1" (D55): the
    // device's serial tells them apart; the title alone cannot.
    let sensor = |dev: u32, n: u32| {
        format!(
            "\t\t\t\t\t<C Type=\"TreeSensor\" U=\"0000001{dev}-0000-000{n}-ffff000000000001\" \
             Title=\"Taster {n}\">\r\n\
             \t\t\t\t\t\t<Co K=\"Q\" U=\"0000001{dev}-0000-000{n}-01ff000000000001\"/>\r\n\
             \t\t\t\t\t</C>\r\n"
        )
    };
    let device = |dev: u32, serial: &str| {
        format!(
            "\t\t\t\t<C Type=\"TreeDevice\" U=\"0000000{dev}-0000-0000-ffff000000000001\" \
             Title=\"Tasterschnittstelle\" Serial=\"{serial}\">\r\n{}{}\t\t\t\t</C>\r\n",
            sensor(dev, 1),
            sensor(dev, 2)
        )
    };
    let base = LoxoneDoc::parse(
        format!(
            "<ControlList Version=\"1\" NextObj=\"20\">\r\n\
             \t<C Type=\"Document\" U=\"00000001-0000-0000-ffff000000000001\">\r\n\
             \t\t<C Type=\"LoxLIVE\" U=\"00000002-0000-0000-ffff000000000001\" Serial=\"504F94000001\">\r\n\
             \t\t\t<C Type=\"LoxTree\" U=\"00000003-0000-0000-ffff000000000001\" Serial=\"B0000000\">\r\n\
             {}{}\
             \t\t\t</C>\r\n\
             \t\t</C>\r\n\
             \t\t<C Type=\"Page\" U=\"00000006-0000-0000-ffff000000000001\" Title=\"P\">\r\n\
             \t\t\t<C Type=\"And\" U=\"00000007-0000-0000-ffff000000000001\" Title=\"Beide\">\r\n\
             \t\t\t\t<Co K=\"I1\" U=\"00000007-0000-0001-01ff000000000001\">\r\n\
             \t\t\t\t\t<In Input=\"00000014-0000-0001-01ff000000000001\"/>\r\n\
             \t\t\t\t</Co>\r\n\
             \t\t\t\t<Co K=\"I2\" U=\"00000007-0000-0002-01ff000000000001\">\r\n\
             \t\t\t\t\t<In Input=\"00000015-0000-0001-01ff000000000001\"/>\r\n\
             \t\t\t\t</Co>\r\n\
             \t\t\t\t<Co K=\"Q\" U=\"00000007-0000-0003-01ff000000000001\"/>\r\n\
             \t\t\t</C>\r\n\
             \t\t</C>\r\n\
             \t</C>\r\n\
             </ControlList>\r\n",
            device(4, "B0A1B2C3"),
            device(5, "B0D4E5F6")
        )
        .as_bytes(),
    )
    .unwrap();
    assert_eq!(
        base.device_serial(&base.objects()[4].path),
        Some("B0A1B2C3"),
        "nearest enclosing Serial=, not the Tree branch or the Miniserver"
    );

    let resolve = |src: &str| {
        let m = Module::parse(src)?;
        let mut lock = Lockfile::new();
        compile(&base, &m, &mut lock, &opts())?;
        Ok::<_, Box<dyn std::error::Error>>(lock.externals["t"].uuid.clone())
    };
    let err = resolve("extern t = TreeSensor(title: \"Taster 1\")\n").unwrap_err();
    assert!(err.to_string().contains("2"), "{err}");
    assert_eq!(
        resolve("extern t = TreeSensor(title: \"Taster 1\", device: \"b0d4e5f6\")\n").unwrap(),
        "00000015-0000-0001-ffff000000000001",
        "serials compare case-insensitively"
    );
    let err =
        resolve("extern t = TreeSensor(title: \"Taster 1\", device: \"B0FFFFFF\")\n").unwrap_err();
    assert!(err.to_string().contains("device: \"B0FFFFFF\""), "{err}");
    assert!(
        Module::parse(
            "extern t = TreeSensor(uuid: \"00000014-0000-0001-ffff000000000001\", device: \"B0A1B2C3\")\n"
        )
        .is_err()
    );
    let src = "extern t = TreeSensor(title: \"Taster 1\", room: \"Flur\", device: \"B0A1B2C3\")\n";
    assert_eq!(Module::parse(src).unwrap().to_text(), src);

    // Decompile says the same: a repeated title gets its device, and the
    // lifted externs resolve to the objects they were lifted from.
    let (m, _) = decompile(&base, &DecompileOptions::default()).unwrap();
    let text = m.to_text();
    let externs: Vec<&str> = text.lines().filter(|l| l.starts_with("extern ")).collect();
    assert_eq!(
        externs,
        [
            "extern taster_1 = TreeSensor(title: \"Taster 1\", device: \"B0A1B2C3\")",
            "extern taster_1_2 = TreeSensor(title: \"Taster 1\", device: \"B0D4E5F6\")",
        ],
        "{text}"
    );
    let m = Module::parse(&(externs.join("\n") + "\n")).unwrap();
    let mut lock = Lockfile::new();
    compile(&base, &m, &mut lock, &opts()).unwrap();
    assert_eq!(
        lock.externals["taster_1_2"].uuid,
        "00000015-0000-0001-ffff000000000001"
    );
}

#[test]
fn config_version_pin_refuses_an_unqualified_release() {
    let base = base(); // ConfigVersion="17010727"
    let module = module();
    let mut lock = Lockfile::new();
    compile(&base, &module, &mut lock, &opts()).unwrap();
    assert_eq!(lock.target.config_version.as_deref(), Some("17010727"));

    // A new Loxone release rewrites the base's ConfigVersion.
    let text = String::from_utf8(base.to_bytes()).unwrap();
    let bumped = LoxoneDoc::parse(
        text.replace("ConfigVersion=\"17010727\"", "ConfigVersion=\"17020101\"")
            .as_bytes(),
    )
    .unwrap();
    let err = compile(&bumped, &module, &mut lock.clone(), &opts()).unwrap_err();
    assert!(err.to_string().contains("ConfigVersion"), "{err}");
    assert!(err.to_string().contains("--accept-version"), "{err}");

    // Acceptance must name the base's version exactly.
    let wrong = CompileOptions {
        accept_version: Some("17039999".into()),
        ..opts()
    };
    let err = compile(&bumped, &module, &mut lock.clone(), &wrong).unwrap_err();
    assert!(err.to_string().contains("does not match"), "{err}");

    // Exact acceptance compiles and re-pins; the next plain compile
    // against the new release is quiet again.
    let accept = CompileOptions {
        accept_version: Some("17020101".into()),
        ..opts()
    };
    compile(&bumped, &module, &mut lock, &accept).unwrap();
    assert_eq!(lock.target.config_version.as_deref(), Some("17020101"));
    compile(&bumped, &module, &mut lock, &opts()).unwrap();
}

#[test]
fn edited_uuid_matcher_repins_the_extern() {
    const VI1: &str = "20000001-0000-0010-ffff504f94112233"; // Außentemperatur
    const VI3: &str = "20000003-0000-0030-ffff504f94112233"; // Sonnenschein
    let base = base();
    let src_of = |matcher: &str| {
        let src = std::fs::read_to_string("examples/ir/beschattung.lxir").unwrap();
        Module::parse(&src.replace(
            "VirtualIn(iname: \"VI3\")",
            &format!("VirtualIn({matcher})"),
        ))
        .unwrap()
    };
    let mut lock = Lockfile::new();
    compile(
        &base,
        &src_of(&format!("uuid: \"{VI3}\"")),
        &mut lock,
        &opts(),
    )
    .unwrap();
    assert_eq!(lock.externals["sonne"].uuid, VI3);

    // Editing the value re-points the extern: the pin follows the source,
    // exactly as a fresh resolution would — same lock, same bytes.
    let edited = src_of(&format!("uuid: \"{VI1}\""));
    let mut repinned = lock.clone();
    let out = compile(&base, &edited, &mut repinned, &opts()).unwrap();
    let pin = &repinned.externals["sonne"];
    assert_eq!(pin.uuid, VI1);
    assert_eq!(pin.title_at_match.as_deref(), Some("Au\u{df}entemperatur"));
    let mut fresh = Lockfile::new();
    let out_fresh = compile(&base, &edited, &mut fresh, &opts()).unwrap();
    assert_eq!(out.to_bytes(), out_fresh.to_bytes());
    assert_eq!(repinned.to_json(), fresh.to_json());

    // A typo is a NoMatch, not a silent keep of the old pin.
    let typo = src_of("uuid: \"20000003-0000-0030-ffff504f94112299\"");
    let err = compile(&base, &typo, &mut lock.clone(), &opts()).unwrap_err();
    assert!(err.to_string().contains("sonne"), "{err}");

    // A matcher-*kind* change still has to confirm the pin (D32): an
    // iname pin converted to a uuid naming another object is refused.
    let mut by_iname = Lockfile::new();
    compile(&base, &src_of("iname: \"VI3\""), &mut by_iname, &opts()).unwrap();
    let err = compile(&base, &edited, &mut by_iname, &opts())
        .unwrap_err()
        .to_string();
    assert!(err.contains("does not match the locked object"), "{err}");
}

#[test]
fn duplicate_uuids_are_refused_in_base_and_output() {
    const VI1: &str = "20000001-0000-0010-ffff504f94112233";
    const VI3: &str = "20000003-0000-0030-ffff504f94112233";
    let base = base();
    assert!(base.duplicate_uuids().is_empty());
    let text = String::from_utf8(base.to_bytes()).unwrap();

    // `<SObj>` entries reuse their object's UUID by design.
    let sobj = LoxoneDoc::parse(
        text.replacen(
            "</ControlList>",
            &format!("<SObj T=\"0\" U=\"{VI1}\"/></ControlList>"),
            1,
        )
        .as_bytes(),
    )
    .unwrap();
    assert!(sobj.duplicate_uuids().is_empty());

    // Two objects sharing one identity: every entry point refuses.
    let twins = LoxoneDoc::parse(
        text.replacen(&format!("U=\"{VI1}\""), &format!("U=\"{VI3}\""), 1)
            .as_bytes(),
    )
    .unwrap();
    assert_eq!(twins.duplicate_uuids(), vec![VI3.to_string()]);
    let err = compile(&twins, &module(), &mut Lockfile::new(), &opts())
        .unwrap_err()
        .to_string();
    assert!(err.contains("duplicate UUID") && err.contains(VI3), "{err}");
    assert!(adopt(&twins).is_err());
    assert!(adopt_pages(&twins).is_err());
    let err = adopt_one(&twins, VI3, "x", &module(), &mut Lockfile::new())
        .unwrap_err()
        .to_string();
    assert!(err.contains("duplicate UUID"), "{err}");

    // A lock that maps a block port onto an existing connector's UUID
    // would put twins into the output — the self-check stops it.
    let mut lock = Lockfile::new();
    let out = compile(&base, &module(), &mut lock, &opts()).unwrap();
    let vi1_port = lxir::doc::ports(out.element_at(&out.index().by_uuid[VI1]).unwrap())
        .remove(0)
        .uuid;
    lock.objects
        .get_mut("beschatten")
        .unwrap()
        .ports
        .insert("I1".into(), vi1_port.clone());
    let err = compile(&base, &module(), &mut lock, &opts())
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("output would carry") && err.contains(&vi1_port),
        "{err}"
    );
}

/// Synthetic config for the incremental-adopt tests: two VirtualIns, a
/// managed And wired from the first — and slots for a GUI-drawn Or block
/// plus its wire into the And's I2.
fn incr_base(or_block: &str, and_i2: &str) -> LoxoneDoc {
    LoxoneDoc::parse(
        format!(
            "<ControlList Version=\"1\" NextObj=\"50\">\r\n\
             \t<C Type=\"Document\" U=\"00000001-0000-0000-ffff000000000001\" ConfigVersion=\"17010727\">\r\n\
             \t\t<C Type=\"VirtualInCaption\" U=\"00000002-0000-0000-ffff000000000001\">\r\n\
             \t\t\t<C Type=\"VirtualIn\" U=\"00000010-0000-0000-ffff000000000001\" Title=\"Sensor A\">\r\n\
             \t\t\t\t<Co K=\"Q\" U=\"00000010-0000-0001-01ff000000000001\"/>\r\n\
             \t\t\t</C>\r\n\
             \t\t\t<C Type=\"VirtualIn\" U=\"00000011-0000-0000-ffff000000000001\" Title=\"Sensor B\">\r\n\
             \t\t\t\t<Co K=\"Q\" U=\"00000011-0000-0001-01ff000000000001\"/>\r\n\
             \t\t\t</C>\r\n\
             \t\t</C>\r\n\
             \t\t<C Type=\"Page\" U=\"00000005-0000-0000-ffff000000000001\" Title=\"Regeln\">\r\n\
             \t\t\t<C Type=\"And\" V=\"175\" U=\"00000020-0000-0000-ffff000000000001\" Title=\"Beide\" \
             Px=\"100\" Py=\"100\" Px2=\"154\" Py2=\"136\" Nio=\"3\">\r\n\
             \t\t\t\t<Co K=\"I1\" U=\"00000020-0000-0001-01ff000000000001\" Nc=\"1\">\r\n\
             \t\t\t\t\t<In Input=\"00000010-0000-0001-01ff000000000001\"/>\r\n\
             \t\t\t\t</Co>\r\n\
             \t\t\t\t{and_i2}\r\n\
             \t\t\t\t<Co K=\"Q\" U=\"00000020-0000-0003-01ff000000000001\" Nc=\"0\"/>\r\n\
             \t\t\t</C>\r\n\
             {or_block}\
             \t\t</C>\r\n\
             \t</C>\r\n\
             </ControlList>\r\n"
        )
        .as_bytes(),
    )
    .unwrap()
}

const OR_UUID: &str = "00000030-0000-0000-ffff000000000001";
const INCR_I2_UNWIRED: &str = "<Co K=\"I2\" U=\"00000020-0000-0002-01ff000000000001\" Nc=\"0\"/>";
const INCR_I2_WIRED: &str = "<Co K=\"I2\" U=\"00000020-0000-0002-01ff000000000001\" Nc=\"1\">\
     <In Input=\"00000030-0000-0003-01ff000000000001\"/></Co>";
const INCR_OR_BLOCK: &str = "\t\t\t<C Type=\"Or\" V=\"175\" \
     U=\"00000030-0000-0000-ffff000000000001\" Title=\"Nachtlicht\" \
     Px=\"200\" Py=\"200\" Px2=\"254\" Py2=\"236\" Nio=\"3\">\
     <Co K=\"I1\" U=\"00000030-0000-0001-01ff000000000001\" Nc=\"1\">\
     <In Input=\"00000010-0000-0001-01ff000000000001\"/></Co>\
     <Co K=\"I2\" U=\"00000030-0000-0002-01ff000000000001\" Nc=\"1\">\
     <In Input=\"00000011-0000-0001-01ff000000000001\"/></Co>\
     <Co K=\"Q\" U=\"00000030-0000-0003-01ff000000000001\" Nc=\"0\"/></C>\r\n";

#[test]
fn incremental_adopt_claims_a_gui_drawn_block() {
    // Whole-adopt the starting state: one managed And, one extern.
    let base1 = incr_base("", INCR_I2_UNWIRED);
    let (mut module, lock0, report) = adopt(&base1).unwrap();
    assert_eq!(report.blocks, 1);
    assert!(lock0.objects.contains_key("beide"));
    assert!(lock0.externals.contains_key("sensor_a"));

    // Then someone draws an Or in Loxone Config: fed by both sensors,
    // feeding the managed And's I2.
    let base2 = incr_base(INCR_OR_BLOCK, INCR_I2_WIRED);

    // The wire into the managed sink is not in source yet: refused, and
    // the error names the exact line to add.
    let err = adopt_one(&base2, OR_UUID, "nachtlicht", &module, &mut lock0.clone()).unwrap_err();
    assert!(err.to_string().contains("I2: nachtlicht.Q"), "{err}");
    assert!(err.to_string().contains("`beide`"), "{err}");

    // The manual fix: declare the wire in the And's argument list.
    for item in &mut module.items {
        if let Item::Block(b) = item
            && b.slug == "beide"
        {
            b.args.push(ArgItem::Binding(Binding {
                port: "I2".into(),
                kind: BindingKind::Wire(PortRef {
                    slug: "nachtlicht".into(),
                    port: "Q".into(),
                }),
                comment: None,
            }));
        }
    }

    let mut lock = lock0.clone();
    let adopted = adopt_one(&base2, OR_UUID, "nachtlicht", &module, &mut lock).unwrap();
    assert_eq!(adopted.page_title, "Regeln");
    assert_eq!(adopted.new_externs, vec!["sensor_b".to_string()]);

    // Two items: the new extern and the block. Sensor A is referenced by
    // its existing slug, not re-declared; the outgoing wire lives in the
    // And's declaration, so no `<-` statement.
    let text = Module {
        items: adopted.items.clone(),
    }
    .to_text();
    assert_eq!(adopted.items.len(), 2, "{text}");
    assert!(
        text.contains("extern sensor_b = VirtualIn(title: \"Sensor B\")"),
        "{text}"
    );
    assert!(text.contains("I1: sensor_a.Q"), "{text}");
    assert!(text.contains("I2: sensor_b.Q"), "{text}");

    // The lock pins the block's existing identity and the new extern, and
    // the adopted-from config is the new drift baseline.
    assert_eq!(lock.objects["nachtlicht"].uuid, OR_UUID);
    assert_eq!(lock.objects["nachtlicht"].ports.len(), 3);
    assert!(lock.objects["nachtlicht"].layout.is_some());
    assert!(lock.externals.contains_key("sensor_b"));
    assert_eq!(
        lock.target.semantic_fingerprint.as_deref(),
        Some(&*lxir::diff::semantic_fingerprint(&base2))
    );

    // Appending the items yields a module whose rebuild is a semantic
    // no-op against the config — and stays deterministic.
    let mut merged = module.clone();
    merged.items.extend(adopted.items.clone());
    merged.validate().unwrap();
    let noop_opts = CompileOptions {
        page_title: None,
        ..opts()
    };
    let out = compile(&base2, &merged, &mut lock, &noop_opts).unwrap();
    let d = lxir::diff::diff(&base2, &out);
    assert!(d.is_empty(), "{d:#?}");
    let out2 = compile(&base2, &merged, &mut lock, &noop_opts).unwrap();
    assert_eq!(out.to_bytes(), out2.to_bytes());

    // Slug hygiene on the same scenario.
    let taken = adopt_one(&base2, OR_UUID, "beide", &module, &mut lock0.clone()).unwrap_err();
    assert!(taken.to_string().contains("already taken"), "{taken}");
    let bad = adopt_one(&base2, OR_UUID, "Nacht", &module, &mut lock0.clone()).unwrap_err();
    assert!(bad.to_string().contains("not a valid slug"), "{bad}");

    // An unqualified release pin refuses before anything else mutates.
    let mut pinned = lock0.clone();
    pinned.target.config_version = Some("17999999".into());
    let err = adopt_one(&base2, OR_UUID, "nachtlicht", &module, &mut pinned).unwrap_err();
    assert!(err.to_string().contains("--accept-version"), "{err}");
}

#[test]
fn incremental_adopt_refuses_claimed_or_unverified_identities() {
    let module = module();
    let mut lock = Lockfile::new();
    // The compiled output holds both the managed blocks and the externs.
    let base = compile(&base(), &module, &mut lock, &opts()).unwrap();

    let err = adopt_one(
        &base,
        "12345678-0000-0000-ffff000000000001",
        "x",
        &module,
        &mut lock.clone(),
    )
    .unwrap_err();
    assert!(err.to_string().contains("no object"), "{err}");

    let managed_uuid = lock.objects["beschatten"].uuid.clone();
    let err = adopt_one(&base, &managed_uuid, "x", &module, &mut lock.clone()).unwrap_err();
    assert!(err.to_string().contains("already managed"), "{err}");

    let extern_uuid = lock.externals["jal_sued"].uuid.clone();
    let err = adopt_one(&base, &extern_uuid, "x", &module, &mut lock.clone()).unwrap_err();
    assert!(err.to_string().contains("extern `jal_sued`"), "{err}");

    let vi = base
        .objects()
        .iter()
        .find(|o| o.block_type == "VirtualIn")
        .unwrap()
        .uuid
        .clone();
    let err = adopt_one(&base, &vi, "x", &module, &mut lock.clone()).unwrap_err();
    assert!(
        err.to_string().contains("reference it as an extern"),
        "{err}"
    );
}

const FASSADE_MODULE: &str = "\
extern aussentemp = VirtualIn(iname: \"VI1\")\n\
extern wind_alarm = VirtualIn(iname: \"VI2\")\n\
extern sonne = VirtualIn(iname: \"VI3\")\n\
extern jal_sued = AutoJalousie(title: \"Beschattung S\u{fc}d\")\n\
\n\
template fassade(jalousie: AutoJalousie, schwelle = 28, pos = 70)\n\
\thoch = GreaterEqual(Input1: aussentemp.Q, Input2: schwelle)\n\
\n\
\tbeschatten = And(I1: hoch.Q, I2: sonne.Q)\n\
\n\
\tjalousie.AutoShade <- beschatten.Q\n\
\tjalousie.Safety <- wind_alarm.Q\n\
\n\
\tjalousie.TargetPos = pos\n\
end\n\
\n\
sued = fassade(jalousie: jal_sued)\n";

#[test]
fn templates_expand_with_locked_identities() {
    let base = base();
    let module = Module::parse(FASSADE_MODULE).unwrap();

    // The canonical form keeps the template intact and is a fixpoint
    // (call arguments canonicalize to one per line, so the compact
    // fixture is not itself canonical).
    let text = module.to_text();
    assert_eq!(
        Module::parse(&text).unwrap(),
        module,
        "parse ∘ to_text = id"
    );
    assert_eq!(Module::parse(&text).unwrap().to_text(), text, "fixpoint");
    assert!(
        text.contains("template fassade(jalousie: AutoJalousie, schwelle = 28, pos = 70)"),
        "{text}"
    );
    assert!(text.contains("\tjalousie.TargetPos = pos\nend"), "{text}");

    // Compiling sees only the expansion: instance `sued` owns two blocks
    // under their expanded, lockfile-keyed names.
    let mut lock = Lockfile::new();
    let out = compile(&base, &module, &mut lock, &opts()).unwrap();
    assert_eq!(
        lock.objects.keys().collect::<Vec<_>>(),
        ["sued_beschatten", "sued_hoch"]
    );

    // The captured externs and the object parameter resolved: AutoShade
    // fed by the instance's And, Safety by the wind alarm, TargetPos 70.
    let jal = |doc: &LoxoneDoc| {
        let objs = doc.objects();
        let o = objs
            .iter()
            .find(|o| o.block_type == "AutoJalousie")
            .unwrap();
        lxir::doc::ports(doc.element_at(&o.path).unwrap())
    };
    let ports = jal(&out);
    let target = ports.iter().find(|p| p.key == "TargetPos").unwrap();
    assert_eq!(target.def.as_deref(), Some("70"));
    let wired: usize = ports.iter().map(|p| p.inputs.len()).sum();
    assert_eq!(wired, 2);

    // Fixpoint, and value overrides re-parameterize without re-minting.
    let again = compile(&out, &module, &mut lock, &opts()).unwrap();
    assert_eq!(out.to_bytes(), again.to_bytes());
    let tuned = Module::parse(&FASSADE_MODULE.replace(
        "sued = fassade(jalousie: jal_sued)",
        "sued = fassade(jalousie: jal_sued, schwelle: 30, pos: 55)",
    ))
    .unwrap();
    let next_obj = out.counters().next_obj;
    let tuned_out = compile(&base, &tuned, &mut lock, &opts()).unwrap();
    assert_eq!(tuned_out.counters().next_obj, next_obj, "nothing re-minted");
    let ports = jal(&tuned_out);
    assert_eq!(
        ports
            .iter()
            .find(|p| p.key == "TargetPos")
            .unwrap()
            .def
            .as_deref(),
        Some("55")
    );

    // Growing the template body mints only the addition; shrinking it is
    // the usual vanished-slug error, per instance.
    let grown = Module::parse(&FASSADE_MODULE.replace(
        "\tjalousie.TargetPos = pos\n",
        "\tjalousie.TargetPos = pos\n\twarnung = Not(I: hoch.Q)\n",
    ))
    .unwrap();
    let grown_out = compile(&base, &grown, &mut lock, &opts()).unwrap();
    assert!(lock.objects.contains_key("sued_warnung"));
    assert_eq!(grown_out.counters().next_obj, next_obj + 1);
    let err = compile(&base, &module, &mut lock.clone(), &opts()).unwrap_err();
    assert!(err.to_string().contains("sued_warnung"), "{err}");
}

#[test]
fn two_instances_get_disjoint_identities() {
    let src = "\
extern vi_a = VirtualIn(iname: \"VI1\")\n\
extern vi_b = VirtualIn(iname: \"VI2\")\n\
\n\
template melder(quelle: VirtualIn, schwelle = 1)\n\
\talarm = GreaterEqual(Input1: quelle.Q, Input2: schwelle)\n\
end\n\
\n\
a = melder(quelle: vi_a)\n\
b = melder(quelle: vi_b, schwelle: 5)\n";
    let module = Module::parse(src).unwrap();
    let mut lock = Lockfile::new();
    let out = compile(&base(), &module, &mut lock, &opts()).unwrap();
    assert_eq!(
        lock.objects.keys().collect::<Vec<_>>(),
        ["a_alarm", "b_alarm"]
    );
    assert_ne!(lock.objects["a_alarm"].uuid, lock.objects["b_alarm"].uuid);
    let fixpoint = compile(&out, &module, &mut lock, &opts()).unwrap();
    assert_eq!(out.to_bytes(), fixpoint.to_bytes());
}

#[test]
fn template_titles_interpolate_value_parameters() {
    let src = "\
extern vi_a = VirtualIn(iname: \"VI1\")\n\
extern vi_b = VirtualIn(iname: \"VI2\")\n\
\n\
let name_b = \"Melder Garage\"\n\
\n\
template melder(quelle: VirtualIn, titel = \"Melder\", schwelle = 1)\n\
\talarm = GreaterEqual(\"{titel} (scharf)\", Input1: quelle.Q, Input2: schwelle)\n\
end\n\
\n\
a = melder(quelle: vi_a, titel: \"Melder Keller\")\n\
b = melder(quelle: vi_b, titel: name_b)\n";
    let module = Module::parse(src).unwrap();
    let mut lock = Lockfile::new();
    let out = compile(&base(), &module, &mut lock, &opts()).unwrap();
    let mut titles: Vec<_> = out
        .objects()
        .iter()
        .filter(|o| o.block_type == "GreaterEqual")
        .map(|o| o.title.clone().unwrap())
        .collect();
    titles.sort();
    assert_eq!(titles, ["Melder Garage (scharf)", "Melder Keller (scharf)"]);

    // A brace-wrapped slug that names no value parameter is a typo guard.
    let bad = src.replace("{titel}", "{titl}");
    let err = compile(
        &base(),
        &Module::parse(&bad).unwrap(),
        &mut Lockfile::new(),
        &opts(),
    )
    .unwrap_err();
    assert!(err.to_string().contains("{titl}"), "{err}");

    // Braces around anything not slug-shaped pass through verbatim.
    let verbatim = src.replace("{titel} (scharf)", "{titel} {2x} {}");
    let out2 = compile(
        &base(),
        &Module::parse(&verbatim).unwrap(),
        &mut Lockfile::new(),
        &opts(),
    )
    .unwrap();
    assert!(
        out2.objects()
            .iter()
            .any(|o| o.title.as_deref() == Some("Melder Keller {2x} {}")),
        "{:?}",
        out2.objects()
            .iter()
            .map(|o| o.title.clone())
            .collect::<Vec<_>>()
    );
}

#[test]
fn instance_calls_forward_port_bindings_to_the_single_body_block() {
    let src = "\
extern vi_a = VirtualIn(iname: \"VI1\")\n\
extern vi_b = VirtualIn(iname: \"VI2\")\n\
\n\
template melder(schwelle = 1)\n\
\talarm = GreaterEqual(Input2: schwelle)\n\
end\n\
\n\
a = melder(schwelle: 3, Input1: vi_a.Q, Input1: vi_b.Q)\n";
    let module = Module::parse(src).unwrap();
    let mut lock = Lockfile::new();
    let out = compile(&base(), &module, &mut lock, &opts()).unwrap();
    assert!(lock.objects.contains_key("a_alarm"));
    let objs = out.objects();
    let o = objs
        .iter()
        .find(|o| o.block_type == "GreaterEqual")
        .unwrap();
    let ports = lxir::doc::ports(out.element_at(&o.path).unwrap());
    // Both repeated forwards landed as feeds; the value param still binds.
    assert_eq!(
        ports
            .iter()
            .find(|p| p.key == "Input1")
            .unwrap()
            .inputs
            .len(),
        2
    );
    assert_eq!(
        ports
            .iter()
            .find(|p| p.key == "Input2")
            .unwrap()
            .def
            .as_deref(),
        Some("3")
    );

    // A body with several blocks cannot take forwards (qualified form is
    // deferred): the unknown binding reports the constraint.
    let multi = src.replace(
        "\talarm = GreaterEqual(Input2: schwelle)\n",
        "\talarm = GreaterEqual(Input2: schwelle)\n\tinvers = Not(I: alarm.Q)\n",
    );
    let err = Module::parse(&multi).unwrap_err();
    assert!(err.to_string().contains("exactly one block"), "{err}");
}

#[test]
fn template_misuse_is_reported() {
    let fails = |src: &str, needle: &str| {
        let err = match Module::parse(src) {
            Err(e) => e.to_string(),
            Ok(m) => compile(&base(), &m, &mut Lockfile::new(), &opts())
                .unwrap_err()
                .to_string(),
        };
        assert!(err.contains(needle), "`{needle}` not in: {err}");
    };
    let tmpl = "template t(x: VirtualIn, n = 1)\n\ty = Not(I: x.Q)\nend\n";
    let vi = "extern vi = VirtualIn(iname: \"VI1\")\n";

    fails("a = nirgends(x: 1)\n", "unknown template `nirgends`");
    fails(&format!("{tmpl}a = t()\n"), "`x` must be given");
    // An unknown name on a single-block template forwards as a port
    // binding (D23) and surfaces as an unknown port downstream.
    fails(
        &format!("{vi}{tmpl}a = t(x: vi, oops: 2)\n"),
        "unknown port `oops`",
    );
    fails(
        &format!("{vi}{tmpl}a = t(x: vi, x: vi)\n"),
        "duplicate argument",
    );
    fails(
        &format!("{vi}{tmpl}a = t(x: 5)\n"),
        "takes an extern or block slug",
    );
    fails(&format!("{vi}{tmpl}a = t(x: vi, n: vi.Q)\n"), "not a port");
    fails(
        &format!("{vi}{tmpl}a = t(\"Label\", x: vi)\n"),
        "no label string",
    );
    fails(
        "template t(x: VirtualIn)\n\tlet n = 1\nend\n",
        "not allowed inside a template body",
    );
    fails(
        "template a(x: VirtualIn)\n\ty = Not(I: x.Q)\nend\n\
           template b(x: VirtualIn)\n\tz = a(x: x)\nend\n",
        "cannot instantiate another template",
    );
    fails("use fassade(jal)\n", "v0 syntax");
    fails(
        "template t(x: VirtualIn)\n\ty = Not(I: x.Q)\n",
        "missing `end`",
    );
    fails("end\n", "`end` without an open `template`");
    // The instance slug names no object; the message points at the
    // expanded names.
    fails(
        &format!("{vi}{tmpl}a = t(x: vi)\nb = Not(I: a.Q)\n"),
        "template instance",
    );
    // A wrong object-arg type is caught where the declaration is known.
    fails(
        &format!("extern j = AutoJalousie(title: \"X\")\n{tmpl}a = t(x: j)\n"),
        "expects a VirtualIn",
    );
}

#[test]
fn grown_gate_inputs_are_refused() {
    // Oracle-verified (docs/oracle-wine.md): Loxone Config 17 silently
    // deletes off-descriptor connectors on save. A grown `I3` must be a
    // compile error, never minted.
    let m = Module::parse(
        "extern t1 = VirtualIn(iname: \"VI1\")\n\
         extern t2 = VirtualIn(iname: \"VI2\")\n\
         extern t3 = VirtualIn(iname: \"VI3\")\n\
         any = Or(I1: t1.Q, I2: t2.Q, I3: t3.Q)\n",
    )
    .unwrap();
    let err = compile(&base(), &m, &mut Lockfile::new(), &opts()).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("unknown port `I3`"), "{msg}");
    assert!(msg.contains("cascade"), "{msg}");
}

#[test]
fn decompile_of_compiled_output_reflects_the_module() {
    let mut lock = Lockfile::new();
    let out = compile(&base(), &module(), &mut lock, &opts()).unwrap();
    let managed_only = DecompileOptions {
        scope: DecompileScope::ManagedOnly,
        ..Default::default()
    };
    let (m, report) = decompile(&out, &managed_only).unwrap();
    // The fixture's AutoJalousie is a managed type since its admission, so
    // the view lifts it as a block (adopt, with verification, would refuse
    // this partial old-generation instance — but views don't verify).
    assert_eq!(report.managed, 3);
    assert_eq!(report.externs, 3);
    assert_eq!(m.blocks().count(), 3);
    // Every wire lands in a block argument list — the AutoShade and Safety
    // sinks are on the lifted AutoJalousie now.
    assert_eq!(m.wire_pairs().len(), 5);
    assert_eq!(m.extern_wires().count(), 0);
    // Everything the compiler owns sits on the one page it compiled onto.
    assert_eq!(report.pages, 1);
    // The IR text parses back to the same module (canonical fixpoint).
    let text = m.to_text();
    assert_eq!(Module::parse(&text).unwrap(), m);
}

#[test]
fn full_view_decompile_shows_every_page_block_and_wire() {
    let mut lock = Lockfile::new();
    let out = compile(&base(), &module(), &mut lock, &opts()).unwrap();
    let (m, report) = decompile(&out, &DecompileOptions::default()).unwrap();
    // The AutoJalousie lifts as a managed block since its admission.
    assert_eq!(report.managed, 3);
    // The three VirtualIns (periphery).
    assert_eq!(report.externs, 3);
    // Only the VirtualInCaption container stays raw.
    assert_eq!(report.raw_objects, 1);
    assert_eq!(report.pages, 1);
    // Every wire sinks on a lifted block now — nothing needs `<-`.
    assert_eq!(m.extern_wires().count(), 0);
    assert_eq!(m.wire_pairs().len(), 5);
    // Output is grouped into page sections headed by real `page`
    // statements (D28); the periphery keeps its comment.
    assert!(
        m.items
            .iter()
            .any(|i| matches!(i, Item::Page(p) if p.title == "Beschattung"))
    );
    assert!(
        m.items
            .iter()
            .any(|i| matches!(i, Item::Comment(c) if c.contains("periphery")))
    );
    // The view is still canonical, parseable language text.
    let text = m.to_text();
    assert_eq!(Module::parse(&text).unwrap(), m);
    // D29: a label the slug already encodes is dropped in the read-only
    // full view ("Temp über 28" → temp_ueber_28, "Beschattung Süd" →
    // beschattung_sued) — no block needs one here.
    assert!(m.blocks().all(|b| b.title.is_none()));
    // …while the adoptable view keeps every label: rebuilding must write
    // the exact `Title=` back.
    let mo = DecompileOptions {
        scope: DecompileScope::ManagedOnly,
        ..Default::default()
    };
    let (m, report) = decompile(&out, &mo).unwrap();
    assert_eq!(report.ref_wires_folded, 0);
    assert_eq!(
        m.blocks()
            .filter_map(|b| b.title.as_deref())
            .collect::<Vec<_>>(),
        ["Beschattung Süd", "Temp über 28"]
    );
}

/// D29: `InputRef`/`OutputRef` plumbing folds out of the full view — the
/// ref extern says what it mirrors, a periphery object connected only by
/// plumbing stays out entirely, and the `<-` pile is sorted by sink.
#[test]
fn full_view_folds_ref_plumbing_and_sorts_extern_wires() {
    let mut lock = Lockfile::new();
    let mut out = compile(&base(), &module(), &mut lock, &opts()).unwrap();

    // A periphery VirtualIn nothing else references…
    let caption = out
        .objects()
        .iter()
        .find(|o| o.block_type == "VirtualInCaption")
        .unwrap()
        .path
        .clone();
    let mut regen = Element::new("C");
    regen.set_attr("Type", "VirtualIn");
    regen.set_attr("V", "175");
    regen.set_attr("U", "20000097-0000-0970-ffff504f94112233");
    regen.set_attr("Title", "Regensensor");
    regen.set_attr("IName", "VI9");
    let mut q = Element::new("Co");
    q.set_attr("K", "Q");
    q.set_attr("U", "20000097-0000-0971-00ffaaaaaaaa0097");
    regen.push_child(q);
    out.element_at_mut(&caption).unwrap().push_child(regen);

    // …page refs, the OutputRef first in document order. Its `Ref=`
    // names the managed And block, so its wire from the block is
    // plumbing (folded) and the extern says `mirrors: beschatten`
    // (D32). The InputRef mirrors unlifted periphery — its plumbing
    // folds too, but the matcher stays conventional with a `# mirrors`
    // note.
    let page = out
        .objects()
        .iter()
        .find(|o| o.block_type == "Page")
        .unwrap()
        .path
        .clone();
    let and_q = lock.objects["beschatten"].ports["Q"].clone();
    let mut zeta = Element::new("C");
    zeta.set_attr("Type", "OutputRef");
    zeta.set_attr("V", "175");
    zeta.set_attr("U", "20000095-0000-0950-ffff504f94112233");
    zeta.set_attr("Title", "Zeta Ref");
    zeta.set_attr("Ref", &lock.objects["beschatten"].uuid);
    let mut i = Element::new("Co");
    i.set_attr("K", "I");
    i.set_attr("U", "20000095-0000-0951-00ffaaaaaaaa0095");
    let mut inp = Element::new("In");
    inp.set_attr("Input", &and_q);
    i.push_child(inp);
    // A second, real source on the same connector: without it the ref
    // would be pure plumbing and fold out of the view entirely (D34).
    let mut inp = Element::new("In");
    inp.set_attr("Input", "20000001-0000-0011-00ffaaaaaaaa0001");
    i.push_child(inp);
    zeta.push_child(i);
    let mut alpha = Element::new("C");
    alpha.set_attr("Type", "InputRef");
    alpha.set_attr("V", "175");
    alpha.set_attr("U", "20000096-0000-0960-ffff504f94112233");
    alpha.set_attr("Title", "Alpha Ref");
    alpha.set_attr("Ref", "20000097-0000-0970-ffff504f94112233");
    let mut i = Element::new("Co");
    i.set_attr("K", "I");
    i.set_attr("U", "20000096-0000-0961-00ffaaaaaaaa0096");
    let mut inp = Element::new("In");
    inp.set_attr("Input", "20000097-0000-0971-00ffaaaaaaaa0097");
    i.push_child(inp);
    alpha.push_child(i);
    let mut ai = Element::new("Co");
    ai.set_attr("K", "AI");
    ai.set_attr("U", "20000096-0000-0962-01ffaaaaaaaa0096");
    let mut inp = Element::new("In");
    // The Außentemperatur VirtualIn's Q — a real source, not the mirror.
    inp.set_attr("Input", "20000001-0000-0011-00ffaaaaaaaa0001");
    ai.push_child(inp);
    alpha.push_child(ai);
    let page_el = out.element_at_mut(&page).unwrap();
    page_el.push_child(zeta);
    page_el.push_child(alpha);

    let (m, report) = decompile(&out, &DecompileOptions::default()).unwrap();
    assert_eq!(report.ref_wires_folded, 2);
    // The ref extern names what it mirrors instead of showing plumbing.
    let alpha = m.externs().find(|e| e.slug == "alpha_ref").unwrap();
    assert_eq!(
        alpha.comment.as_deref(),
        Some(" mirrors VirtualIn \"Regensensor\"")
    );
    // The ref of the managed block gets the semantic matcher, no note.
    let zeta = m.externs().find(|e| e.slug == "zeta_ref").unwrap();
    assert_eq!(
        zeta.match_spec,
        lxir::ir::MatchSpec::Mirrors("beschatten".into())
    );
    assert_eq!(zeta.comment, None);
    // The Regensensor's only connection was plumbing — it is not lifted.
    assert!(m.externs().all(|e| e.slug != "regensensor"));
    // Only the refs' real source wires survive the fold.
    let pile: Vec<String> = m.extern_wires().map(|w| w.to.to_string()).collect();
    assert_eq!(pile, ["alpha_ref.AI", "zeta_ref.I"]);
    // Still canonical, parseable text.
    let text = m.to_text();
    assert_eq!(Module::parse(&text).unwrap(), m);
}

/// D32: `mirrors: <slug>` matches an `InputRef`/`OutputRef` by its
/// `Ref=` attribute — the object it mirrors — instead of a uuid pin.
/// The declaring file's `page` statement narrows duplicate refs; a
/// matcher-kind upgrade re-verifies against the pin; and a pinned
/// `mirrors:` re-confirms every compile, so a source claim that stopped
/// being true is an error, not tolerated drift.
#[test]
fn mirrors_matcher_resolves_refs_by_their_target() {
    // Deploy once, then hand the page an InputRef whose `Ref=` names the
    // managed And block — the GUI's way of mirroring it elsewhere.
    let mut lock0 = Lockfile::new();
    let mut deployed = compile(&base(), &module(), &mut lock0, &opts()).unwrap();
    let and_uuid = lock0.objects["beschatten"].uuid.clone();
    let page = deployed
        .objects()
        .iter()
        .find(|o| o.block_type == "Page")
        .unwrap()
        .path
        .clone();
    let mut spiegel = Element::new("C");
    spiegel.set_attr("Type", "InputRef");
    spiegel.set_attr("V", "175");
    spiegel.set_attr("U", "200000a0-0000-0a00-ffff504f94112233");
    spiegel.set_attr("Title", "Beschatten Spiegel");
    spiegel.set_attr("Ref", &and_uuid);
    deployed.element_at_mut(&page).unwrap().push_child(spiegel);

    let src_of = |matcher: &str| {
        format!(
            "{}extern beschatten_ref = InputRef({matcher})\n",
            std::fs::read_to_string("examples/ir/beschattung.lxir").unwrap()
        )
    };

    // Fresh resolution finds the ref through its target.
    let mirrors = Module::parse(&src_of("mirrors: beschatten")).unwrap();
    let mut lock = lock0.clone();
    compile(&deployed, &mirrors, &mut lock, &opts()).unwrap();
    let pin = &lock.externals["beschatten_ref"];
    assert_eq!(pin.uuid, "200000a0-0000-0a00-ffff504f94112233");
    assert_eq!(pin.matched_by, "mirrors");
    assert_eq!(pin.title_at_match.as_deref(), Some("Beschatten Spiegel"));

    // Upgrading a uuid pin to `mirrors:` keeps the identity, records the
    // new kind, and changes nothing in the output.
    let pinned = Module::parse(&src_of("uuid: \"200000a0-0000-0a00-ffff504f94112233\"")).unwrap();
    let mut lock_a = lock0.clone();
    let out_a = compile(&deployed, &pinned, &mut lock_a, &opts()).unwrap();
    assert_eq!(lock_a.externals["beschatten_ref"].matched_by, "uuid");
    let mut lock_b = lock_a.clone();
    let out_b = compile(&deployed, &mirrors, &mut lock_b, &opts()).unwrap();
    assert_eq!(lock_b.externals["beschatten_ref"].matched_by, "mirrors");
    assert_eq!(
        lock_b.externals["beschatten_ref"].uuid,
        lock_a.externals["beschatten_ref"].uuid
    );
    assert_eq!(out_a.to_bytes(), out_b.to_bytes());

    // A pinned `mirrors:` whose claim stopped being true is refused —
    // here the source suddenly claims the ref mirrors `sonne`.
    let lying = Module::parse(&src_of("mirrors: sonne")).unwrap();
    let err = compile(&deployed, &lying, &mut lock_b.clone(), &opts())
        .unwrap_err()
        .to_string();
    assert!(err.contains("does not match the locked object"), "{err}");

    // A second ref of the same target on another page: ambiguous without
    // governance, resolved by the declaring file's `page` statement.
    let mut two_pages = deployed.clone();
    let doc_path = two_pages
        .objects()
        .iter()
        .find(|o| o.block_type == "Document")
        .unwrap()
        .path
        .clone();
    let mut werkstatt = Element::new("C");
    werkstatt.set_attr("Type", "Page");
    werkstatt.set_attr("V", "175");
    werkstatt.set_attr("U", "200000a1-0000-0a10-ffff504f94112233");
    werkstatt.set_attr("Title", "Werkstatt");
    let mut spiegel2 = Element::new("C");
    spiegel2.set_attr("Type", "InputRef");
    spiegel2.set_attr("V", "175");
    spiegel2.set_attr("U", "200000a2-0000-0a20-ffff504f94112233");
    spiegel2.set_attr("Title", "Beschatten Spiegel Werkstatt");
    spiegel2.set_attr("Ref", &and_uuid);
    werkstatt.push_child(spiegel2);
    two_pages
        .element_at_mut(&doc_path)
        .unwrap()
        .push_child(werkstatt);

    // Declared above the first `page` statement, nothing narrows the two
    // candidates — refused. (The fixture's own `page "Beschattung"`
    // governs everything appended below it, including `mirrors` above.)
    let ungoverned = Module::parse(&format!(
        "extern beschatten_ref = InputRef(mirrors: beschatten)\n\n{}",
        std::fs::read_to_string("examples/ir/beschattung.lxir").unwrap()
    ))
    .unwrap();
    let err = compile(&two_pages, &ungoverned, &mut lock0.clone(), &opts())
        .unwrap_err()
        .to_string();
    assert!(err.contains("beschatten_ref"), "{err}");
    let governed = Module::parse(&format!(
        "{}\npage \"Werkstatt\"\n\nextern beschatten_ref = InputRef(mirrors: beschatten)\n",
        std::fs::read_to_string("examples/ir/beschattung.lxir").unwrap()
    ))
    .unwrap();
    let mut lock_c = lock0.clone();
    compile(&two_pages, &governed, &mut lock_c, &opts()).unwrap();
    assert_eq!(
        lock_c.externals["beschatten_ref"].uuid,
        "200000a2-0000-0a20-ffff504f94112233"
    );

    // Unknown target and non-ref extern types are refused outright.
    let err = compile(
        &deployed,
        &Module::parse(&src_of("mirrors: nixda")).unwrap(),
        &mut lock0.clone(),
        &opts(),
    )
    .unwrap_err()
    .to_string();
    assert!(err.contains("neither a managed block"), "{err}");
    let err = compile(
        &deployed,
        &Module::parse(&format!(
            "{}extern kaputt = VirtualIn(mirrors: beschatten)\n",
            std::fs::read_to_string("examples/ir/beschattung.lxir").unwrap()
        ))
        .unwrap(),
        &mut lock0.clone(),
        &opts(),
    )
    .unwrap_err()
    .to_string();
    assert!(err.contains("InputRef/OutputRef/OutputRefLM only"), "{err}");
}

/// D30: the full view elides a parameter whose value equals the
/// corpus-observed GUI default (`--all-params` shows it, the report
/// counts it); the adoptable view keeps every parameter because
/// rebuilding writes the exact `Def=` back.
#[test]
fn full_view_elides_observed_default_params() {
    // The base AutoJalousie gains two Defs: `Dir` at the observed GUI
    // default (180) and `Width` off it (the observed default is 70).
    let mut doc = base();
    let jal = doc
        .objects()
        .iter()
        .find(|o| o.block_type == "AutoJalousie")
        .unwrap()
        .path
        .clone();
    for (key, uuid, def) in [
        ("Dir", "20000005-0000-0055-30ffaaaaaaaa0005", "180"),
        ("Width", "20000005-0000-0056-31ffaaaaaaaa0005", "50"),
    ] {
        let mut co = Element::new("Co");
        co.set_attr("K", key);
        co.set_attr("Def", def);
        co.set_attr("U", uuid);
        doc.element_at_mut(&jal).unwrap().push_child(co);
    }
    let params = |m: &Module| -> Vec<(String, String)> {
        m.blocks()
            .flat_map(|b| b.bindings())
            .filter_map(|x| match &x.kind {
                BindingKind::Param(v) => Some((x.port.clone(), v.to_string())),
                _ => None,
            })
            .collect()
    };

    let (m, report) = decompile(&doc, &DecompileOptions::default()).unwrap();
    assert_eq!(report.default_params_elided, 1);
    assert_eq!(
        params(&m),
        [
            ("TargetPos".into(), "100".into()),
            ("Width".into(), "50".into())
        ]
    );

    let all = DecompileOptions {
        all_params: true,
        ..Default::default()
    };
    let (m, report) = decompile(&doc, &all).unwrap();
    assert_eq!(report.default_params_elided, 0);
    assert!(params(&m).contains(&("Dir".into(), "180".into())));

    let mo = DecompileOptions {
        scope: DecompileScope::ManagedOnly,
        ..Default::default()
    };
    let (m, report) = decompile(&doc, &mo).unwrap();
    assert_eq!(report.default_params_elided, 0);
    assert!(params(&m).contains(&("Dir".into(), "180".into())));
}

#[test]
fn per_page_decompile_produces_self_contained_modules() {
    let mut lock = Lockfile::new();
    let out = compile(&base(), &module(), &mut lock, &opts()).unwrap();
    let (pages, report) = decompile_pages(&out, &DecompileOptions::default()).unwrap();
    assert_eq!(report.pages, 1);
    assert_eq!(pages.len(), 1);
    let p = &pages[0];
    assert_eq!(p.title, "Beschattung");
    assert_eq!(p.slug, "beschattung");
    // Self-contained: the periphery VirtualIns the page references are
    // declared as foreign externs with an origin note.
    let m = &p.module;
    // The module opens with its own placement (D28).
    assert!(matches!(&m.items[0], Item::Page(pg) if pg.title == "Beschattung"));
    assert_eq!(m.blocks().count(), 3);
    assert_eq!(m.externs().count(), 3);
    assert_eq!(
        m.externs()
            .filter(|e| e.comment.as_deref() == Some(" periphery"))
            .count(),
        3
    );
    assert_eq!(m.extern_wires().count(), 0);
    // Each page module is canonical, parseable language text.
    let text = m.to_text();
    assert_eq!(&Module::parse(&text).unwrap(), m);
}

/// D28: `page "Title"` statements govern placement of the blocks that
/// follow them — authoritatively, so editing the statement moves a block
/// already pinned elsewhere, and a title the base does not carry is an
/// error. Blocks above the first statement keep the options' page.
#[test]
fn page_statements_place_and_move_blocks() {
    // haus.Loxone with a second, empty page beside "Beschattung".
    fn base_with_page(title: &str) -> LoxoneDoc {
        let mut doc = base();
        let doc_path = doc
            .objects()
            .iter()
            .find(|o| o.block_type == "Document")
            .unwrap()
            .path
            .clone();
        let mut page = Element::new("C");
        page.set_attr("Type", "Page");
        page.set_attr("V", "175");
        page.set_attr("U", "20000098-0000-0980-ffff504f94112233");
        page.set_attr("Title", title);
        doc.element_at_mut(&doc_path).unwrap().push_child(page);
        doc
    }
    let page_of = |doc: &LoxoneDoc, lock: &Lockfile, slug: &str| {
        let objs = doc.objects();
        let block_uuid = &lock.objects[slug].uuid;
        let block = objs.iter().find(|o| &o.uuid == block_uuid).unwrap();
        let page = objs
            .iter()
            .filter(|o| o.block_type == "Page" && block.path.starts_with(&o.path))
            .max_by_key(|o| o.path.len())
            .unwrap();
        assert_eq!(
            lock.objects[slug].page_uuid.as_deref(),
            Some(&*page.uuid),
            "lock pin and document placement agree"
        );
        page.title.clone().unwrap()
    };

    let src = "extern wind = VirtualIn(iname: \"VI2\")\n\n\
               vor = Or(I1: wind.Q)\n\n\
               page \"Regeln\"\n\n\
               danach = Not(I: wind.Q)\n";
    let module = Module::parse(src).unwrap();
    let mut lock = Lockfile::new();
    let out = compile(&base_with_page("Regeln"), &module, &mut lock, &opts()).unwrap();
    assert_eq!(page_of(&out, &lock, "vor"), "Beschattung");
    assert_eq!(page_of(&out, &lock, "danach"), "Regeln");

    // Moving the statement's title re-pins the governed block in place.
    let moved = Module::parse(&src.replace("page \"Regeln\"", "page \"Beschattung\"")).unwrap();
    let out = compile(&base_with_page("Regeln"), &moved, &mut lock, &opts()).unwrap();
    assert_eq!(page_of(&out, &lock, "danach"), "Beschattung");

    // A title no page in the base carries is a compile error.
    let bad = Module::parse(&src.replace("page \"Regeln\"", "page \"Nirgendwo\"")).unwrap();
    let e = compile(&base_with_page("Regeln"), &bad, &mut lock.clone(), &opts()).unwrap_err();
    assert!(
        e.to_string()
            .contains("no <C Type=\"Page\"> titled `Nirgendwo`"),
        "{e}"
    );
}

/// A module exercising everything adoption must preserve: a Formula with
/// an attribute parameter, a comparator fed by it, and a wire onto an
/// extern port. Compiling it yields the "existing config" the adopt tests
/// then treat as GUI-authored.
const ADOPT_SRC: &str = "\
extern aussentemp = VirtualIn(iname: \"VI1\")
extern sonne = VirtualIn(iname: \"VI3\")
extern jal_sued = AutoJalousie(title: \"Beschattung S\u{fc}d\")

summe = Formula(
\t\"Summe\",
\tFormula: \"I1+I2\",
\tInput1: aussentemp.Q,
\tInput2: sonne.Q,
)
heiss = GreaterEqual(
\tInput1: summe.AQ,
\tInput2: 50,
)

jal_sued.AutoShade <- heiss.Q
";

fn adopted_config() -> LoxoneDoc {
    let m = Module::parse(ADOPT_SRC).unwrap();
    compile(&base(), &m, &mut Lockfile::new(), &opts()).unwrap()
}

#[test]
fn adopt_pages_fragments_merge_to_the_single_module() {
    // `adopt --out-dir` writes these fragments; concatenated in stem
    // order they must be exactly the single-module adoption, with the
    // identical lock — so both output shapes compile byte-identically.
    let existing = adopted_config();
    let (single, lock_a, _) = adopt(&existing).unwrap();
    let (fragments, lock_b, _) = adopt_pages(&existing).unwrap();
    assert_eq!(lock_a.to_json(), lock_b.to_json());
    let stems: Vec<&str> = fragments.iter().map(|(s, _)| s.as_str()).collect();
    assert_eq!(stems, ["_periphery", "beschattung"]);
    let merged = Module {
        items: fragments
            .iter()
            .flat_map(|(_, m)| m.items.iter().cloned())
            .collect(),
    };
    assert_eq!(merged, single);
    // Each fragment is parseable in fragment form (a lone fragment may
    // cross-reference sibling slugs, so full parse is not required).
    for (stem, m) in &fragments {
        let re =
            Module::parse_fragment(&m.to_text()).unwrap_or_else(|e| panic!("fragment {stem}: {e}"));
        assert_eq!(&re, m, "fragment {stem} round-trips");
    }
}

#[test]
fn adopt_then_compile_rebuilds_in_place() {
    let existing = adopted_config();
    let (module, mut lock, report) = adopt(&existing).unwrap();
    assert_eq!(report.blocks, 2);
    assert_eq!(report.pages, 1);

    // The fixture's partial old-generation AutoJalousie (4 of 49
    // connectors) refuses — its rebuild would mint the missing 45.
    assert_eq!(report.refused.len(), 1, "{:?}", report.refused);
    assert!(
        report.refused[0].contains("AutoJalousie"),
        "{:?}",
        report.refused
    );

    // The lock pins the blocks' existing identities.
    let objs = existing.objects();
    let formula = objs.iter().find(|o| o.block_type == "Formula").unwrap();
    assert_eq!(lock.objects["summe"].uuid, formula.uuid);
    let page = objs.iter().find(|o| o.block_type == "Page").unwrap();
    assert_eq!(
        lock.objects["summe"].page_uuid.as_deref(),
        Some(&*page.uuid)
    );

    // The lifted view carries the attribute parameter.
    assert!(module.to_text().contains("Formula: \"I1+I2\""));

    // Compiling the pair is a no-op: nothing minted, nothing moved —
    // byte-identical, since the "existing" config is our own output shape.
    let out = compile(&existing, &module, &mut lock, &opts()).unwrap();
    assert!(lxir::diff::diff(&existing, &out).is_empty());
    assert_eq!(out.counters().next_obj, existing.counters().next_obj);
    assert_eq!(out.to_bytes(), existing.to_bytes());
}

#[test]
fn fingerprint_is_recorded_and_survives_the_rebuild() {
    let existing = adopted_config();
    let (module, mut lock, _) = adopt(&existing).unwrap();
    let recorded = lock.target.semantic_fingerprint.clone().unwrap();
    assert_eq!(recorded, lxir::diff::semantic_fingerprint(&existing));

    // The rebuild is a semantic no-op, so compile re-records the same
    // baseline — `lxir drift` stays green across the whole cycle.
    let out = compile(&existing, &module, &mut lock, &opts()).unwrap();
    assert_eq!(
        lock.target.semantic_fingerprint.as_deref(),
        Some(&*recorded)
    );
    assert_eq!(lxir::diff::semantic_fingerprint(&out), recorded);

    // Any real edit moves it (here: a changed Def value).
    let changed_src = ADOPT_SRC.replace("Input2: 50", "Input2: 51");
    let changed = compile(
        &base(),
        &Module::parse(&changed_src).unwrap(),
        &mut Lockfile::new(),
        &opts(),
    )
    .unwrap();
    assert_ne!(lxir::diff::semantic_fingerprint(&changed), recorded);
}

#[test]
fn formula_attribute_parameter_compiles_and_diffs() {
    let existing = adopted_config();
    let objs = existing.objects();
    let f = objs.iter().find(|o| o.block_type == "Formula").unwrap();
    let el = existing.element_at(&f.path).unwrap();
    assert_eq!(el.attr("Formula"), Some("I1+I2"));
    assert_eq!(el.attr("Valid"), Some("false"));

    // A changed formula is a visible param change, not "semantically empty".
    let changed_src = ADOPT_SRC.replace("I1+I2", "I1*I2");
    let changed = compile(
        &base(),
        &Module::parse(&changed_src).unwrap(),
        &mut Lockfile::new(),
        &opts(),
    )
    .unwrap();
    let d = lxir::diff::diff(&existing, &changed);
    assert_eq!(d.param_changes.len(), 1, "{d:?}");
    assert_eq!(d.param_changes[0].port_key, "Formula");
    assert_eq!(d.param_changes[0].to.as_deref(), Some("I1*I2"));
}

#[test]
fn adopt_skips_blocks_the_rebuild_would_not_reproduce() {
    // An element attribute the rebuild does not emit would be lost: the
    // block is skipped with a reason, the rest of the config still adopts.
    let mut existing = adopted_config();
    let path = existing
        .objects()
        .iter()
        .find(|o| o.block_type == "Formula")
        .unwrap()
        .path
        .clone();
    existing.element_at_mut(&path).unwrap().set_attr("M", "3");
    let (module, lock, report) = adopt(&existing).unwrap();
    // One refusal for the M= attribute (plus the fixture's standing
    // partial-AutoJalousie refusal).
    let refused: Vec<&String> = report
        .refused
        .iter()
        .filter(|r| !r.contains("AutoJalousie"))
        .collect();
    assert_eq!(refused.len(), 1, "{:?}", report.refused);
    assert!(refused[0].contains("attribute `M="), "{}", refused[0]);
    assert!(refused[0].contains("Summe"), "{}", refused[0]);
    assert_eq!(report.blocks, 1, "the GreaterEqual still adopts");
    assert!(!lock.objects.contains_key("summe"));
    assert!(lock.objects.contains_key("heiss"));
    // The refused Formula stays visible as a pinned extern (heiss wires
    // from its AQ), and compiling the pair is still a no-op.
    assert!(module.externs().any(|e| e.slug == "summe"));
    assert!(lock.externals.contains_key("summe"));
    let out = compile(&existing, &module, &mut lock.clone(), &opts()).unwrap();
    assert!(lxir::diff::diff(&existing, &out).is_empty());
}

#[test]
fn adopt_carries_gui_owned_inv_connectors_verbatim() {
    // An `Inv=`-carrying connector is GUI-owned (D20): the block still
    // adopts, the connector's wire stays out of the source, and the
    // rebuild re-emits the whole <Co> verbatim — flag and wire included.
    let mut existing = adopted_config();
    let path = existing
        .objects()
        .iter()
        .find(|o| o.block_type == "GreaterEqual")
        .unwrap()
        .path
        .clone();
    existing
        .element_at_mut(&path)
        .unwrap()
        .child_elements_mut()
        .find(|c| c.name == "Co")
        .unwrap()
        .set_attr("Inv", "true");
    let (module, mut lock, report) = adopt(&existing).unwrap();
    // Only the fixture's standing partial-AutoJalousie refusal remains.
    assert_eq!(report.refused.len(), 1, "{:?}", report.refused);
    assert!(report.refused[0].contains("AutoJalousie"));
    assert!(lock.objects.contains_key("heiss"), "the block still adopts");
    // The inverted Input1 and its wire are GUI content now — absent from
    // the source (restating the wire would silently invert its meaning);
    // the untouched Input2 parameter is still lifted.
    let text = module.to_text();
    assert!(!text.contains("Input1: summe.AQ"), "{text}");
    assert!(text.contains("Input2: 50"), "{text}");
    // The rebuild is still a semantic no-op: the carried <Co> brings the
    // flag and the wire back verbatim.
    let out = compile(&existing, &module, &mut lock, &opts()).unwrap();
    assert!(lxir::diff::diff(&existing, &out).is_empty());
    let heiss = out
        .objects()
        .into_iter()
        .find(|o| o.block_type == "GreaterEqual")
        .unwrap();
    let co = out
        .element_at(&heiss.path)
        .unwrap()
        .child_elements()
        .find(|c| c.name == "Co")
        .unwrap();
    assert_eq!(co.attr("Inv"), Some("true"));
    assert_eq!(co.attr("Nc"), Some("1"), "the GUI wire survives");

    // Touching a GUI-owned connector from source is refused, both as a
    // wire sink and as a parameter binding.
    let wire_src = "\
extern sonne = VirtualIn(iname: \"VI3\")
extern heiss = GreaterEqual(title: \"heiss\")
heiss.Input1 <- sonne.Q
";
    let m = Module::parse(wire_src).unwrap();
    let err = compile(&existing, &m, &mut Lockfile::new(), &opts())
        .unwrap_err()
        .to_string();
    assert!(err.contains("Inv"), "{err}");
    assert!(err.contains("GUI-owned"), "{err}");
    let (module, mut lock, _) = adopt(&existing).unwrap();
    let mut text = module.to_text();
    text = text.replace("\tInput2: 50,", "\tInput1: 5,\n\tInput2: 50,");
    let m = Module::parse(&text).unwrap();
    let err = compile(&existing, &m, &mut lock, &opts())
        .unwrap_err()
        .to_string();
    assert!(err.contains("Inv"), "{err}");
    assert!(err.contains("cannot be set"), "{err}");
}

#[test]
fn adopt_carries_gui_owned_residue_verbatim() {
    // Dress the compiled Formula up like a real GUI-authored block:
    // custom color, LtE, a display attribute, and visualization children
    // — including the GUI's NON-self-closing <IoData></IoData> form,
    // which must survive as-is.
    let mut existing = adopted_config();
    let path = existing
        .objects()
        .iter()
        .find(|o| o.block_type == "Formula")
        .unwrap()
        .path
        .clone();
    let el = existing.element_at_mut(&path).unwrap();
    el.set_attr("Cl", "238,238,238");
    let wf = el.attrs.iter().position(|a| a.name == "WF").unwrap();
    el.attrs.insert(
        wf,
        Attr {
            name: "LtE".into(),
            value: "495646462".into(),
        },
    );
    el.set_attr("Tp", "2"); // display attrs trail the element
    let mut io = Element::new("IoData");
    io.set_attr("Pr", "room-uuid");
    io.self_closing = false;
    el.push_child(io);
    let mut display = Element::new("Display");
    display.set_attr("Unit", "<v.1>");
    el.push_child(display);

    // Adoption accepts the residue, and the rebuild reproduces the config
    // byte for byte — values, order, escaping, self-closing form. (The
    // fixture's standing partial-AutoJalousie refusal is not about us.)
    let (module, lock, report) = adopt(&existing).unwrap();
    assert!(
        report.refused.iter().all(|r| r.contains("AutoJalousie")),
        "{:?}",
        report.refused
    );
    let out = compile(&existing, &module, &mut lock.clone(), &opts()).unwrap();
    assert_eq!(
        String::from_utf8_lossy(&out.to_bytes()),
        String::from_utf8_lossy(&existing.to_bytes())
    );

    // Carried forward, not snapshotted: a later GUI edit to the residue
    // survives the next compile instead of being reverted.
    let mut edited = out;
    let path = edited
        .objects()
        .iter()
        .find(|o| o.block_type == "Formula")
        .unwrap()
        .path
        .clone();
    edited
        .element_at_mut(&path)
        .unwrap()
        .child_elements_mut()
        .find(|c| c.name == "Display")
        .unwrap()
        .set_attr("Unit", "°C");
    let out2 = compile(&edited, &module, &mut lock.clone(), &opts()).unwrap();
    assert_eq!(
        String::from_utf8_lossy(&out2.to_bytes()),
        String::from_utf8_lossy(&edited.to_bytes())
    );
}

#[test]
fn adopt_carries_wire_flags_verbatim() {
    // FLG= is Miniserver/app-created wire metadata the oracle probe showed
    // to be inert stored state: dress a managed block's incoming wire with
    // it and the round-trip must reproduce it byte for byte.
    let mut existing = adopted_config();
    let path = existing
        .objects()
        .iter()
        .find(|o| o.block_type == "GreaterEqual")
        .unwrap()
        .path
        .clone();
    existing
        .element_at_mut(&path)
        .unwrap()
        .child_elements_mut()
        .find(|c| c.name == "Co" && c.attr("Nc").is_some())
        .expect("the fixture wires into the GreaterEqual")
        .child_elements_mut()
        .find(|i| i.name == "In")
        .unwrap()
        .set_attr("FLG", "1");

    let (module, lock, report) = adopt(&existing).unwrap();
    assert!(
        report.refused.iter().all(|r| r.contains("AutoJalousie")),
        "{:?}",
        report.refused
    );
    let out = compile(&existing, &module, &mut lock.clone(), &opts()).unwrap();
    assert_eq!(
        String::from_utf8_lossy(&out.to_bytes()),
        String::from_utf8_lossy(&existing.to_bytes())
    );
}

#[test]
fn wire_direction_is_checked_on_managed_blocks() {
    let m = Module::parse("a = And()\nb = And(I2: a.I1)\n").unwrap();
    let err = compile(&base(), &m, &mut Lockfile::new(), &opts()).unwrap_err();
    assert!(err.to_string().contains("wire source"), "{err}");
}

#[test]
fn removed_statement_authorizes_scoped_removal() {
    let base = base();
    let mut lock = Lockfile::new();
    let full = compile(&base, &module(), &mut lock, &opts()).unwrap();

    // Drop `beschatten` from source with an explicit `removed` — no
    // allow_removals needed, and the removal is scoped to that one slug.
    let without = Module::parse(
        "extern aussentemp = VirtualIn(iname: \"VI1\")\n\
         temp_hoch = GreaterEqual(\n\
         \t\"Temp \u{fc}ber 28\",\n\
         \tInput1: aussentemp.Q,\n\
         \tInput2: 28,\n\
         )\n\
         removed beschatten\n",
    )
    .unwrap();
    let out = compile(&full, &without, &mut lock, &opts()).unwrap();
    assert!(!out.objects().iter().any(|o| o.block_type == "And"));
    assert!(!lock.objects.contains_key("beschatten"));
    assert!(lock.objects.contains_key("temp_hoch"), "others survive");

    // The `removed` is tolerated while its tombstone is pending (D31):
    // recompiling against the caught-up base is still a fixpoint, and the
    // sweep retires the tombstone.
    let again = compile(&out, &without, &mut lock, &opts()).unwrap();
    assert_eq!(out.to_bytes(), again.to_bytes());
    assert!(
        lock.removed.is_empty(),
        "tombstone retired by caught-up base"
    );

    // A `removed` authorizes exactly its slug — another vanished slug still
    // refuses, and the error suggests the statement.
    let mut lock2 = Lockfile::new();
    compile(&base, &module(), &mut lock2, &opts()).unwrap();
    let only_one = Module::parse("removed beschatten\n").unwrap();
    let err = compile(&base, &only_one, &mut lock2, &opts()).unwrap_err();
    assert!(err.to_string().contains("`temp_hoch`"), "{err}");
    assert!(err.to_string().contains("removed temp_hoch"), "{err}");
}

#[test]
fn removal_tombstone_bridges_the_compile_to_push_window() {
    // The r50 field scenario (D31): a removal is compiled but the output
    // has not been pushed yet, so the base still contains the old block.
    // Without tombstones a recompile from the updated lock passed the
    // orphaned object through unmanaged — a wrong artifact with the old
    // and new logic side by side.
    let base = base();
    let mut lock = Lockfile::new();
    let deployed = compile(&base, &module(), &mut lock, &opts()).unwrap();

    let without = Module::parse(
        "extern aussentemp = VirtualIn(iname: \"VI1\")\n\
         temp_hoch = GreaterEqual(\n\
         \t\"Temp \u{fc}ber 28\",\n\
         \tInput1: aussentemp.Q,\n\
         \tInput2: 28,\n\
         )\n\
         removed beschatten\n",
    )
    .unwrap();

    // Transition compile: block deleted, tombstone minted.
    let out1 = compile(&deployed, &without, &mut lock, &opts()).unwrap();
    assert!(!out1.objects().iter().any(|o| o.block_type == "And"));
    assert!(!lock.objects.contains_key("beschatten"));
    let (uuid, tomb) = lock.removed.iter().next().expect("tombstone minted");
    assert_eq!(tomb.slug, "beschatten");
    assert_eq!(tomb.block_type, "And");
    assert!(
        deployed.objects().iter().any(|o| &o.uuid == uuid),
        "tombstone remembers the deployed object's UUID"
    );
    // The withdrawn extern wires (AutoShade/Safety) and the withdrawn
    // TargetPos write tombstone too — without them, recompiles against the
    // old base would pass the wires through and keep our Def value.
    assert_eq!(lock.removed_wires.len(), 2, "withdrawn wires tombstoned");
    let retired = lock.removed_sets.values().next().expect("withdrawn set");
    assert_eq!(retired.written, "70");
    assert_eq!(retired.original.as_deref(), Some("100"));
    let lock1_json = lock.to_json();

    // The window: recompile against the OLD base from the updated lock.
    // The tombstone keeps deleting the object — output and lock are both
    // exact fixpoints, so the state is committable before the push.
    let out2 = compile(&deployed, &without, &mut lock, &opts()).unwrap();
    assert_eq!(out1.to_bytes(), out2.to_bytes(), "output fixpoint");
    assert_eq!(
        lock.to_json(),
        lock1_json,
        "lock fixpoint incl. fingerprint"
    );

    // The statement may also be dropped right after the transition — the
    // tombstone alone carries the intent through the window.
    let without_stmt = Module::parse(
        "extern aussentemp = VirtualIn(iname: \"VI1\")\n\
         temp_hoch = GreaterEqual(\n\
         \t\"Temp \u{fc}ber 28\",\n\
         \tInput1: aussentemp.Q,\n\
         \tInput2: 28,\n\
         )\n",
    )
    .unwrap();
    let out3 = compile(&deployed, &without_stmt, &mut lock.clone(), &opts()).unwrap();
    assert_eq!(out1.to_bytes(), out3.to_bytes());

    // Base caught up (the output was pushed): every tombstone retires.
    compile(&out1, &without_stmt, &mut lock, &opts()).unwrap();
    assert!(lock.removed.is_empty(), "object tombstone retired");
    assert!(lock.removed_wires.is_empty(), "wire tombstones retired");
    assert!(lock.removed_sets.is_empty(), "set tombstone retired");

    // A lingering `removed` after that is a hard error — the ratchet that
    // gets the transient statement cleaned up.
    let err = compile(&out1, &without, &mut lock, &opts()).unwrap_err();
    assert!(err.to_string().contains("removal is complete"), "{err}");

    // And a `removed` that never named anything errors immediately.
    let typo = Module::parse("removed nie_da\n").unwrap();
    let err = compile(&base, &typo, &mut Lockfile::new(), &opts()).unwrap_err();
    assert!(err.to_string().contains("nie_da"), "{err}");
}

#[test]
fn moved_statement_renames_identity() {
    let base = base();
    let mut lock = Lockfile::new();
    let first = compile(&base, &module(), &mut lock, &opts()).unwrap();
    let old = lock.objects["beschatten"].clone();

    let renamed_src = "\
extern aussentemp = VirtualIn(iname: \"VI1\")
extern wind_alarm = VirtualIn(iname: \"VI2\")
extern sonne = VirtualIn(iname: \"VI3\")
extern jal_sued = AutoJalousie(title: \"Beschattung S\u{fc}d\")

temp_hoch = GreaterEqual(
\t\"Temp \u{fc}ber 28\",
\tInput1: aussentemp.Q,
\tInput2: 28,
)
schatten_gate = And(
\tI1: temp_hoch.Q,
\tI2: sonne.Q,
)

jal_sued.AutoShade <- schatten_gate.Q
jal_sued.Safety <- wind_alarm.Q

jal_sued.TargetPos = 70

moved beschatten -> schatten_gate
";
    let renamed = Module::parse(renamed_src).unwrap();
    let second = compile(&first, &renamed, &mut lock, &opts()).unwrap();

    // Identity survived the rename: same object and port UUIDs under the
    // new slug; the only semantic change is the display title (which
    // defaults to the slug).
    assert!(!lock.objects.contains_key("beschatten"));
    let new = &lock.objects["schatten_gate"];
    assert_eq!(new.uuid, old.uuid);
    assert_eq!(new.ports, old.ports);
    let d = lxir::diff::diff(&first, &second);
    assert!(d.added.is_empty() && d.removed.is_empty(), "no re-mint");
    assert!(d.wires_added.is_empty() && d.wires_removed.is_empty());
    assert_eq!(d.renamed.len(), 1, "only the title changed");

    // Idempotent: the applied `moved` is a no-op on the next compile.
    let third = compile(&second, &renamed, &mut lock, &opts()).unwrap();
    assert_eq!(second.to_bytes(), third.to_bytes());

    // With no matching lock entry at all, `moved` is an error (typo guard).
    let err = compile(&base, &renamed, &mut Lockfile::new(), &opts()).unwrap_err();
    assert!(err.to_string().contains("neither slug"), "{err}");
}

#[test]
fn let_constants_resolve_to_defs() {
    let with_let = Module::parse(
        "let schwelle = 28\n\
         extern aussentemp = VirtualIn(iname: \"VI1\")\n\
         temp_hoch = GreaterEqual(Input1: aussentemp.Q, Input2: schwelle)\n\
         aussentemp.Qm = schwelle\n",
    )
    .unwrap();
    let literal = Module::parse(
        "extern aussentemp = VirtualIn(iname: \"VI1\")\n\
         temp_hoch = GreaterEqual(Input1: aussentemp.Q, Input2: 28)\n\
         aussentemp.Qm = 28\n",
    )
    .unwrap();
    let a = compile(&base(), &with_let, &mut Lockfile::new(), &opts()).unwrap();
    let b = compile(&base(), &literal, &mut Lockfile::new(), &opts()).unwrap();
    assert_eq!(
        a.to_bytes(),
        b.to_bytes(),
        "a `let` reference compiles exactly like its literal"
    );
}

#[test]
fn extern_port_errors_suggest_close_names() {
    let m = Module::parse(
        "extern jal = AutoJalousie(title: \"Beschattung S\u{fc}d\")\n\
         b = And()\n\
         jal.AutoShad <- b.Q\n",
    )
    .unwrap();
    let err = compile(&base(), &m, &mut Lockfile::new(), &opts()).unwrap_err();
    assert!(
        err.to_string().contains("did you mean `AutoShade`?"),
        "{err}"
    );
}

#[test]
fn module_fragments_merge_and_cross_reference() {
    // One file per page: the block in fragment B wires from an extern
    // declared in fragment A. Each fragment parses alone (no name
    // resolution), the merged module validates.
    let frag_a = "# page: A\nextern sonne = InputRef(title: \"Sonne\")\n";
    let frag_b = "# page: B\ngate = Not(\"Gate\", I: sonne.AQ)\n";
    assert!(
        Module::parse(frag_b).is_err(),
        "a lone fragment with a cross-file reference must not pass full parse"
    );
    let mut items = Module::parse_fragment(frag_a).unwrap().items;
    items.extend(Module::parse_fragment(frag_b).unwrap().items);
    let merged = Module { items };
    merged.validate().unwrap();
    assert_eq!(merged.externs().count(), 1);
    assert_eq!(merged.blocks().count(), 1);
}

#[test]
fn module_fragments_duplicate_slug_is_rejected() {
    let frag = "x = Not(\"X\", I: x.Q)\n";
    let mut items = Module::parse_fragment(frag).unwrap().items;
    items.extend(Module::parse_fragment(frag).unwrap().items);
    let err = Module { items }.validate().unwrap_err();
    assert!(err.to_string().contains("duplicate name `x`"), "{err}");
}

// ---------------------------------------------------------------------------
// Expression sugar (D24)

const EXPR_MODULE: &str = "\
extern aussentemp = VirtualIn(iname: \"VI1\")\n\
extern wind_alarm = VirtualIn(iname: \"VI2\")\n\
extern sonne = VirtualIn(iname: \"VI3\")\n\
extern jal_sued = AutoJalousie(title: \"Beschattung S\u{fc}d\")\n\
\n\
let schwelle = 28\n\
\n\
jal_sued.AutoShade <- sonne.Q and aussentemp.Q >= schwelle\n";

#[test]
fn expressions_desugar_with_locked_identities() {
    let base = base();
    let module = Module::parse(EXPR_MODULE).unwrap();

    // Canonical form keeps the expression as written (it is already
    // minimal-paren) and is a fixpoint.
    let text = module.to_text();
    assert_eq!(
        Module::parse(&text).unwrap(),
        module,
        "parse ∘ to_text = id"
    );
    assert_eq!(Module::parse(&text).unwrap().to_text(), text, "fixpoint");
    assert!(
        text.contains("jal_sued.AutoShade <- sonne.Q and aussentemp.Q >= schwelle"),
        "{text}"
    );

    // Desugaring emits one comparator and one gate, prefixed by the sink.
    let (plain, info) = module.expand().unwrap().desugar().unwrap();
    assert_eq!(info.expressions, 1);
    assert_eq!(
        info.synthetic.iter().collect::<Vec<_>>(),
        ["jal_sued_autoshade__and1", "jal_sued_autoshade__ge1"]
    );
    let ge = plain
        .blocks()
        .find(|b| b.slug == "jal_sued_autoshade__ge1")
        .unwrap();
    assert_eq!(ge.block_type, "GreaterEqual");
    assert_eq!(ge.title.as_deref(), Some("aussentemp.Q >= schwelle"));
    assert!(
        ge.params()
            .any(|(p, v)| p == "Input2" && v.to_string() == "schwelle"),
        "constant operand becomes a Def parameter"
    );

    // Compile: both synthetic blocks are locked and expression-owned.
    let mut lock = Lockfile::new();
    let out = compile(&base, &module, &mut lock, &opts()).unwrap();
    assert_eq!(
        lock.objects.keys().collect::<Vec<_>>(),
        ["jal_sued_autoshade__and1", "jal_sued_autoshade__ge1"]
    );
    assert!(lock.objects.values().all(|o| o.expr_owned));
    let ge_uuid = lock.objects["jal_sued_autoshade__ge1"].uuid.clone();
    let and_uuid = lock.objects["jal_sued_autoshade__and1"].uuid.clone();

    // Recompile: byte-identical, no re-mint.
    let out2 = compile(&base, &module, &mut lock, &opts()).unwrap();
    assert_eq!(out.to_bytes(), out2.to_bytes());

    // Growing the expression keeps the unchanged nodes' identities and
    // mints only the new gate — no `removed` statement anywhere.
    let grown =
        Module::parse(&EXPR_MODULE.replace(">= schwelle\n", ">= schwelle or wind_alarm.Q\n"))
            .unwrap();
    compile(&base, &grown, &mut lock, &opts()).unwrap();
    assert_eq!(lock.objects["jal_sued_autoshade__ge1"].uuid, ge_uuid);
    assert_eq!(lock.objects["jal_sued_autoshade__and1"].uuid, and_uuid);
    let or_uuid = lock.objects["jal_sued_autoshade__or1"].uuid.clone();
    // Cross-session mint collision guard: a block minted in a later
    // compile (same mint time) must not reuse any earlier (time, sequence)
    // pair — every locked UUID stays distinct.
    let all: Vec<&String> = lock
        .objects
        .values()
        .flat_map(|o| std::iter::once(&o.uuid).chain(o.ports.values()))
        .collect();
    let distinct: std::collections::BTreeSet<&&String> = all.iter().collect();
    assert_eq!(distinct.len(), all.len(), "duplicate minted UUIDs");

    // Shrinking to just the `or` auto-removes the orphaned comparator and
    // gate (they are expression-owned) while `__or1` keeps its identity.
    let shrunk = Module::parse(&EXPR_MODULE.replace(
        "sonne.Q and aussentemp.Q >= schwelle\n",
        "sonne.Q or wind_alarm.Q\n",
    ))
    .unwrap();
    compile(&base, &shrunk, &mut lock, &opts()).unwrap();
    assert_eq!(
        lock.objects.keys().collect::<Vec<_>>(),
        ["jal_sued_autoshade__or1"]
    );
    assert_eq!(lock.objects["jal_sued_autoshade__or1"].uuid, or_uuid);

    // Deleting the statement deletes everything expression-owned.
    let gone = Module::parse(&EXPR_MODULE.replace(
        "jal_sued.AutoShade <- sonne.Q and aussentemp.Q >= schwelle\n",
        "",
    ))
    .unwrap();
    let out_gone = compile(&base, &gone, &mut lock, &opts()).unwrap();
    assert!(lock.objects.is_empty());
    assert!(
        lxir::diff::diff(&base, &out_gone).is_empty(),
        "removing the expression restores the base"
    );
}

#[test]
fn expressions_expand_inside_template_bodies() {
    let base = base();
    let module = Module::parse(
        "extern aussentemp = VirtualIn(iname: \"VI1\")\n\
         extern sonne = VirtualIn(iname: \"VI3\")\n\
         extern jal_sued = AutoJalousie(title: \"Beschattung S\u{fc}d\")\n\
         template fassade(jalousie: AutoJalousie, schwelle = 28)\n\
         \tjalousie.AutoShade <- sonne.Q and aussentemp.Q >= schwelle\n\
         end\n\
         sued = fassade(jalousie: jal_sued, schwelle: 30)\n",
    )
    .unwrap();

    // The synthetic prefix uses the *expanded* sink — the passed extern.
    let mut lock = Lockfile::new();
    compile(&base, &module, &mut lock, &opts()).unwrap();
    assert_eq!(
        lock.objects.keys().collect::<Vec<_>>(),
        ["jal_sued_autoshade__and1", "jal_sued_autoshade__ge1"]
    );
    assert!(lock.objects.values().all(|o| o.expr_owned));
}

#[test]
fn expression_canonical_form_uses_minimal_parens() {
    let head = "extern a = VirtualIn(iname: \"VI1\")\n\
                extern b = VirtualIn(iname: \"VI2\")\n\
                extern c = VirtualIn(iname: \"VI3\")\n\
                extern jal = AutoJalousie(title: \"J\")\n";
    let canon = |rhs: &str| {
        let m = Module::parse(&format!("{head}jal.AutoShade <- {rhs}\n")).unwrap();
        let text = m.to_text();
        assert_eq!(Module::parse(&text).unwrap().to_text(), text, "fixpoint");
        text.lines()
            .find(|l| l.starts_with("jal.AutoShade <- "))
            .unwrap()
            .trim_start_matches("jal.AutoShade <- ")
            .to_string()
    };
    // Precedence needs no parens; grouping against it keeps them.
    assert_eq!(canon("a.Q and b.Q or c.Q"), "a.Q and b.Q or c.Q");
    assert_eq!(canon("(a.Q and b.Q) or c.Q"), "a.Q and b.Q or c.Q");
    assert_eq!(canon("a.Q and (b.Q or c.Q)"), "a.Q and (b.Q or c.Q)");
    assert_eq!(canon("(a.Q or b.Q) and c.Q"), "(a.Q or b.Q) and c.Q");
    assert_eq!(canon("a.Q or (b.Q or c.Q)"), "a.Q or (b.Q or c.Q)");
    // A comparison under `not` is always parenthesized for readability.
    assert_eq!(canon("not a.Q >= 5"), "not (a.Q >= 5)");
    assert_eq!(canon("not (a.Q >= 5)"), "not (a.Q >= 5)");
    assert_eq!(canon("not not a.Q"), "not not a.Q");
    assert_eq!(canon("not (a.Q and b.Q)"), "not (a.Q and b.Q)");
    assert_eq!(canon("a.Q < -5"), "a.Q < -5");
    // Arithmetic: `* /` bind tighter than `+ -`; both are left-
    // associative, grouping against precedence keeps its parens.
    assert_eq!(canon("a.Q + b.Q * c.Q"), "a.Q + b.Q * c.Q");
    assert_eq!(canon("(a.Q + b.Q) * c.Q"), "(a.Q + b.Q) * c.Q");
    assert_eq!(canon("a.Q * (b.Q + c.Q)"), "a.Q * (b.Q + c.Q)");
    assert_eq!(canon("a.Q - b.Q + c.Q"), "a.Q - b.Q + c.Q");
    assert_eq!(canon("a.Q - (b.Q + c.Q)"), "a.Q - (b.Q + c.Q)");
    assert_eq!(canon("a.Q / (b.Q * c.Q)"), "a.Q / (b.Q * c.Q)");
    assert_eq!(canon("(a.Q) + 1"), "a.Q + 1");
    assert_eq!(canon("a.Q * -2"), "a.Q * -2");
    assert_eq!(canon("a.Q-1"), "a.Q - 1");
}

#[test]
fn expression_misuse_is_reported() {
    let head = "extern a = VirtualIn(iname: \"VI1\")\n\
                extern b = VirtualIn(iname: \"VI2\")\n\
                extern jal = AutoJalousie(title: \"J\")\n\
                let schwelle = 28\n";
    let fails = |line: &str, needle: &str| {
        let err = Module::parse(&format!("{head}{line}\n"))
            .and_then(|m| m.expand())
            .and_then(|m| m.desugar().map(|_| ()))
            .unwrap_err();
        assert!(
            err.to_string().contains(needle),
            "`{line}`: expected {needle:?} in: {err}"
        );
    };

    // Parse-level shape errors.
    fails("jal.AutoShade <- a.Q and \"x\"", "strings have no place");
    fails("jal.AutoShade <- a.Q < b.Q < 5", "comparisons do not chain");
    fails(
        "jal.AutoShade <- (a.Q and b.Q) >= 5",
        "not parenthesized expressions",
    );
    fails("jal.AutoShade <- (a.Q and b.Q", "missing `)`");
    fails("jal.AutoShade <- a.Q b.Q", "unexpected trailing tokens");
    fails(
        "jal.AutoShade <- and a.Q",
        "expected an operand, found `and`",
    );
    fails("jal.AutoShade <- 5", "use `jal.AutoShade = 5`");
    fails("let and = 5", "reserved word");
    fails("or = And(I1: a.Q, I2: b.Q)", "reserved word");

    // Validation: references resolve before desugaring, on what the user
    // wrote.
    fails(
        "jal.AutoShade <- nix.Q and a.Q",
        "reference to undeclared slug `nix`",
    );
    fails(
        "jal.AutoShade <- a.Q and nixkonst >= 5",
        "undeclared constant `nixkonst`",
    );

    // Desugar-level semantic errors.
    fails("jal.AutoShade <- 5 and a.Q", "cannot drive a gate input");
    fails("jal.AutoShade <- schwelle >= 28", "compares two constants");
    fails(
        "jal_autoshade__and1 = And(I1: a.Q, I2: b.Q)\n\
         jal.AutoShade <- a.Q and b.Q",
        "claimed by the expression",
    );

    // A managed sink refuses `<-` exactly like plain wires do.
    let err = Module::parse(&format!(
        "{head}gate = And(I1: a.Q, I2: b.Q)\ngate.I1 <- a.Q and b.Q\n"
    ))
    .unwrap_err();
    assert!(err.to_string().contains("targets managed block"), "{err}");
}

// ---------------------------------------------------------------------------
// Unit-suffixed values (D27)

#[test]
fn unit_values_compile_byte_identically_to_their_base_literal() {
    let base = base();
    let with_units = Module::parse(
        "extern sonne = VirtualIn(iname: \"VI3\")\n\
         nachlauf = Monoflop(InputTrigger: sonne.Q, Time: 1.5h)\n",
    )
    .unwrap();
    let plain = Module::parse(
        "extern sonne = VirtualIn(iname: \"VI3\")\n\
         nachlauf = Monoflop(InputTrigger: sonne.Q, Time: 5400)\n",
    )
    .unwrap();
    let mut lock_a = Lockfile::new();
    let out_a = compile(&base, &with_units, &mut lock_a, &opts()).unwrap();
    let mut lock_b = Lockfile::new();
    let out_b = compile(&base, &plain, &mut lock_b, &opts()).unwrap();
    assert_eq!(
        out_a.to_bytes(),
        out_b.to_bytes(),
        "a unit value compiles byte-identically to its base-unit literal"
    );
}

// ---------------------------------------------------------------------------
// Expressions in argument bindings (D26)

#[test]
fn expression_bindings_desugar_like_wire_expressions() {
    let base = base();
    let src = "\
extern aussentemp = VirtualIn(iname: \"VI1\")\n\
extern wind_alarm = VirtualIn(iname: \"VI2\")\n\
extern sonne = VirtualIn(iname: \"VI3\")\n\
extern jal_sued = AutoJalousie(title: \"Beschattung S\u{fc}d\")\n\
\n\
let schwelle = 28\n\
\n\
gate = And(\n\
\tI1: sonne.Q and aussentemp.Q >= schwelle,\n\
\tI2: wind_alarm.Q,\n\
)\n\
\n\
jal_sued.AutoShade <- gate.Q\n";
    let module = Module::parse(src).unwrap();

    // Canonical form keeps the expression as written and is a fixpoint.
    let text = module.to_text();
    assert!(
        text.contains("\tI1: sonne.Q and aussentemp.Q >= schwelle,"),
        "{text}"
    );
    assert_eq!(
        Module::parse(&text).unwrap(),
        module,
        "parse ∘ to_text = id"
    );
    assert_eq!(Module::parse(&text).unwrap().to_text(), text, "fixpoint");

    // Desugaring prefixes with the managed sink and rewires the binding.
    let (plain, info) = module.expand().unwrap().desugar().unwrap();
    assert_eq!(info.expressions, 1);
    assert_eq!(
        info.synthetic.iter().collect::<Vec<_>>(),
        ["gate_i1__and1", "gate_i1__ge1"]
    );
    let gate = plain.blocks().find(|b| b.slug == "gate").unwrap();
    let wires: Vec<String> = gate
        .input_wires()
        .map(|(p, src)| format!("{p}: {src}"))
        .collect();
    assert_eq!(wires, ["I1: gate_i1__and1.Q", "I2: wind_alarm.Q"]);
    assert!(gate.expr_bindings().next().is_none());

    // Compile: synthetic blocks are locked and expression-owned; the
    // recompile is byte-identical (no re-mint).
    let mut lock = Lockfile::new();
    let out = compile(&base, &module, &mut lock, &opts()).unwrap();
    assert!(lock.objects["gate_i1__and1"].expr_owned);
    assert!(lock.objects["gate_i1__ge1"].expr_owned);
    assert!(!lock.objects["gate"].expr_owned);
    let out2 = compile(&base, &module, &mut lock, &opts()).unwrap();
    assert_eq!(out.to_bytes(), out2.to_bytes());

    // Dropping the expression from the binding auto-removes its blocks.
    let shrunk =
        Module::parse(&src.replace("I1: sonne.Q and aussentemp.Q >= schwelle,", "I1: sonne.Q,"))
            .unwrap();
    compile(&base, &shrunk, &mut lock, &opts()).unwrap();
    assert_eq!(lock.objects.keys().collect::<Vec<_>>(), ["gate"]);
}

#[test]
fn expression_bindings_expand_inside_template_bodies() {
    let base = base();
    let module = Module::parse(
        "extern aussentemp = VirtualIn(iname: \"VI1\")\n\
         extern sonne = VirtualIn(iname: \"VI3\")\n\
         extern jal_sued = AutoJalousie(title: \"Beschattung S\u{fc}d\")\n\
         template fassade(jalousie: AutoJalousie, schwelle = 28)\n\
         \tgate = Not(I: sonne.Q and aussentemp.Q >= schwelle)\n\
         \tjalousie.AutoShade <- gate.Q\n\
         end\n\
         sued = fassade(jalousie: jal_sued, schwelle: 30)\n",
    )
    .unwrap();

    // The synthetic prefix uses the *expanded* block slug, and the value
    // parameter substitutes into the expression.
    let (plain, info) = module.expand().unwrap().desugar().unwrap();
    assert_eq!(
        info.synthetic.iter().collect::<Vec<_>>(),
        ["sued_gate_i__and1", "sued_gate_i__ge1"]
    );
    let ge = plain
        .blocks()
        .find(|b| b.slug == "sued_gate_i__ge1")
        .unwrap();
    assert!(
        ge.params()
            .any(|(p, v)| p == "Input2" && v.to_string() == "30"),
        "template value parameter substitutes into the expression"
    );
    let mut lock = Lockfile::new();
    compile(&base, &module, &mut lock, &opts()).unwrap();
    assert!(lock.objects["sued_gate_i__and1"].expr_owned);
}

#[test]
fn expression_binding_misuse_is_reported() {
    let head = "extern a = VirtualIn(iname: \"VI1\")\n\
                extern b = VirtualIn(iname: \"VI2\")\n\
                let schwelle = 28\n";
    let fails = |args: &str, needle: &str| {
        let err = Module::parse(&format!("{head}gate = And({args})\n"))
            .and_then(|m| m.expand())
            .and_then(|m| m.desugar().map(|_| ()))
            .unwrap_err();
        assert!(
            err.to_string().contains(needle),
            "`{args}`: expected {needle:?} in: {err}"
        );
    };

    fails("I1: a.Q and \"x\"", "strings have no place");
    fails("I1: nix.Q and a.Q", "reference to undeclared slug `nix`");
    fails("I1: 5 and a.Q", "cannot drive a gate input");
    fails("I1: schwelle >= 28", "compares two constants");
    fails(
        "I1: a.Q and b.Q, I1: a.Q and b.Q",
        "duplicate expression `I1: a.Q and b.Q`",
    );

    // A parenthesized bare port or value canonicalizes to the plain form.
    let m = Module::parse(&format!("{head}gate = And(I1: (a.Q), I2: (28))\n")).unwrap();
    let gate = m.blocks().next().unwrap();
    assert!(
        matches!(
            &gate.args[0],
            ArgItem::Binding(Binding {
                kind: BindingKind::Wire(_),
                ..
            })
        ),
        "(a.Q) is a plain wire binding"
    );
    assert!(
        matches!(
            &gate.args[1],
            ArgItem::Binding(Binding {
                kind: BindingKind::Param(_),
                ..
            })
        ),
        "(28) is a plain parameter binding"
    );

    // A template's value parameter refuses an expression argument.
    let err = Module::parse(
        "extern a = VirtualIn(iname: \"VI1\")\n\
         template t(x: VirtualIn, n = 1)\n\
         \ty = Not(I: x.Q)\n\
         end\n\
         z = t(x: a, n: a.Q and a.Qm)\n",
    )
    .unwrap_err();
    assert!(
        err.to_string().contains("not a port or expression"),
        "{err}"
    );
}

// ---------------------------------------------------------------------------
// Arithmetic → one Formula block (D24 formula backend)

#[test]
fn arithmetic_desugars_to_one_formula_block() {
    let base = base();
    let src = "extern aussentemp = VirtualIn(iname: \"VI1\")\n\
               extern sonne = VirtualIn(iname: \"VI3\")\n\
               extern jal_sued = AutoJalousie(title: \"Beschattung S\u{fc}d\")\n\
               \n\
               jal_sued.AutoShade <- (aussentemp.Q + sonne.Q) * aussentemp.Q / 1000\n";
    let module = Module::parse(src).unwrap();

    // Canonical text keeps the necessary parens and is a fixpoint.
    let text = module.to_text();
    assert!(
        text.contains("jal_sued.AutoShade <- (aussentemp.Q + sonne.Q) * aussentemp.Q / 1000"),
        "{text}"
    );
    assert_eq!(
        Module::parse(&text).unwrap(),
        module,
        "parse ∘ to_text = id"
    );

    // ONE Formula block for the whole tree: distinct ports become
    // Input1/Input2 in first-appearance order, the repeated port reuses
    // its input, the constant is inlined into the compact formula text.
    let (plain, info) = module.expand().unwrap().desugar().unwrap();
    assert_eq!(info.expressions, 1);
    assert_eq!(
        info.synthetic.iter().collect::<Vec<_>>(),
        ["jal_sued_autoshade__f1"]
    );
    let f = plain
        .blocks()
        .find(|b| b.slug == "jal_sued_autoshade__f1")
        .unwrap();
    assert_eq!(f.block_type, "Formula");
    assert_eq!(
        f.title.as_deref(),
        Some("(aussentemp.Q + sonne.Q) * aussentemp.Q / 1000")
    );
    assert!(
        f.params()
            .any(|(p, v)| p == "Formula" && *v == Value::Str("(I1+I2)*I1/1000".into())),
        "formula text: {:?}",
        f.params().collect::<Vec<_>>()
    );
    let inputs: Vec<(String, String)> = f
        .input_wires()
        .map(|(port, src)| (port.to_string(), src.to_string()))
        .collect();
    assert_eq!(
        inputs,
        [
            ("Input1".to_string(), "aussentemp.Q".to_string()),
            ("Input2".to_string(), "sonne.Q".to_string()),
        ]
    );
    assert!(
        plain
            .wire_pairs()
            .iter()
            .any(|(src, sink)| src.to_string() == "jal_sued_autoshade__f1.AQ"
                && sink.to_string() == "jal_sued.AutoShade"),
        "the sink is fed from the Formula's AQ"
    );

    // Compile: locked and expression-owned like every synthetic block.
    let mut lock = Lockfile::new();
    let out = compile(&base, &module, &mut lock, &opts()).unwrap();
    assert_eq!(
        lock.objects.keys().collect::<Vec<_>>(),
        ["jal_sued_autoshade__f1"]
    );
    assert!(lock.objects.values().all(|o| o.expr_owned));
    let out2 = compile(&base, &module, &mut lock, &opts()).unwrap();
    assert_eq!(
        out.to_bytes(),
        out2.to_bytes(),
        "recompile is byte-identical"
    );
}

#[test]
fn arithmetic_inlines_let_constants_and_works_in_bindings() {
    let head = "extern a = VirtualIn(iname: \"VI1\")\n\
                extern b = VirtualIn(iname: \"VI2\")\n\
                extern jal = AutoJalousie(title: \"J\")\n\
                let faktor = 3\n";

    // A `let` number is inlined into the formula text; the title keeps
    // the name as written.
    let m = Module::parse(&format!("{head}jal.AutoShade <- (a.Q - b.Q) * faktor\n")).unwrap();
    let (plain, _) = m.expand().unwrap().desugar().unwrap();
    let f = plain
        .blocks()
        .find(|x| x.slug == "jal_autoshade__f1")
        .unwrap();
    assert_eq!(f.title.as_deref(), Some("(a.Q - b.Q) * faktor"));
    assert!(
        f.params()
            .any(|(p, v)| p == "Formula" && *v == Value::Str("(I1-I2)*3".into())),
        "{:?}",
        f.params().collect::<Vec<_>>()
    );

    // A negative constant is parenthesized in the formula text so the
    // `-` cannot read as binary.
    let m = Module::parse(&format!("{head}jal.AutoShade <- a.Q * -2\n")).unwrap();
    let (plain, _) = m.expand().unwrap().desugar().unwrap();
    let f = plain
        .blocks()
        .find(|x| x.slug == "jal_autoshade__f1")
        .unwrap();
    assert!(
        f.params()
            .any(|(p, v)| p == "Formula" && *v == Value::Str("I1*(-2)".into())),
        "{:?}",
        f.params().collect::<Vec<_>>()
    );

    // A whole argument binding may be arithmetic (D26): the Formula's AQ
    // is wired onto the bound input.
    let m = Module::parse(&format!("{head}gate = And(I1: a.Q - b.Q, I2: b.Q)\n")).unwrap();
    let (plain, info) = m.expand().unwrap().desugar().unwrap();
    assert_eq!(info.synthetic.iter().collect::<Vec<_>>(), ["gate_i1__f1"]);
    let gate = plain.blocks().find(|x| x.slug == "gate").unwrap();
    assert!(
        gate.input_wires()
            .any(|(port, src)| port == "I1" && src.to_string() == "gate_i1__f1.AQ"),
        "binding becomes a wire from the Formula's AQ"
    );
}

#[test]
fn arithmetic_misuse_is_reported() {
    let head = "extern a = VirtualIn(iname: \"VI1\")\n\
                extern b = VirtualIn(iname: \"VI2\")\n\
                extern jal = AutoJalousie(title: \"J\")\n";
    let fails = |line: &str, needle: &str| {
        let err = Module::parse(&format!("{head}{line}\n"))
            .and_then(|m| m.expand())
            .and_then(|m| m.desugar().map(|_| ()))
            .unwrap_err();
        assert!(
            err.to_string().contains(needle),
            "`{line}`: expected {needle:?} in: {err}"
        );
    };

    // Arithmetic stands alone (v1) — under gates and comparisons it is
    // deferred, with parse-time errors.
    fails("jal.AutoShade <- a.Q and b.Q + 1", "arithmetic under `and`");
    fails("jal.AutoShade <- b.Q + 1 or a.Q", "arithmetic under `or`");
    fails("jal.AutoShade <- not a.Q + 1", "arithmetic under `not`");
    fails("jal.AutoShade <- a.Q + 1 >= 5", "arithmetic under `>=`");
    fails("jal.AutoShade <- a.Q >= b.Q + 1", "arithmetic under `>=`");
    fails(
        "jal.AutoShade <- (a.Q and b.Q) + 1",
        "cannot take part in arithmetic",
    );
    fails(
        "jal.AutoShade <- a.Q * (b.Q >= 5)",
        "cannot take part in arithmetic",
    );

    // Operand shape errors.
    fails("jal.AutoShade <- 1 + 2", "computes on constants only");
    fails("jal.AutoShade <- a.Q + 40s", "carries a unit");
    fails("jal.AutoShade <- a.Q + \"x\"", "strings have no place");
    fails(
        "let s = \"x\"\njal.AutoShade <- a.Q * s",
        "formulas compute on plain numbers",
    );

    // A Formula block takes at most 4 inputs.
    fails(
        "jal.AutoShade <- a.Q + a.AQ + b.Q + b.AQ + jal.Q",
        "at most 4 inputs",
    );
}

#[test]
fn full_view_renders_autopilot_rules_as_comments() {
    // An `AutopilotRule` carries its whole rule as JSON in `APC=` —
    // the full view renders it above the extern, resolving control
    // UUIDs to titles; the adoptable view stays bare (D29 boundary).
    let apc = "{&quot;type&quot;:1,&quot;active&quot;:true,&quot;invalid&quot;:false,\
               &quot;name&quot;:&quot;Skimmer Ein&quot;,&quot;operator&quot;:3,\
               &quot;eventGroups&quot;:[{&quot;operator&quot;:1,&quot;events&quot;:[\
               {&quot;type&quot;:5,&quot;descID&quot;:272001,&quot;id&quot;:272,\
               &quot;value&quot;:540,&quot;operator&quot;:1,&quot;forceEdge&quot;:false}]}],\
               &quot;actions&quot;:[{&quot;type&quot;:0,&quot;descID&quot;:342101,\
               &quot;id&quot;:&quot;40000001-0000-0010-ffff504f94112233&quot;,\
               &quot;command&quot;:&quot;on&quot;}]}";
    let s = format!(
        "<ControlList Version=\"1\" NextObj=\"9\">\r\n\
         \t<C Type=\"Document\" U=\"40000000-0000-0001-ffff504f94112233\">\r\n\
         \t\t<C Type=\"Page\" U=\"40000000-0000-0002-ffff504f94112233\" Title=\"Automatik-Regeln\" SpNo=\"1\">\r\n\
         \t\t\t<C Type=\"Switch\" U=\"40000001-0000-0010-ffff504f94112233\" Title=\"Pumpe\" Nio=\"2\">\r\n\
         \t\t\t\t<Co K=\"On\" U=\"40000001-0000-0011-00ffaaaaaaaa0001\"/>\r\n\
         \t\t\t\t<Co K=\"Q\" U=\"40000001-0000-0012-01ffaaaaaaaa0001\"/>\r\n\
         \t\t\t</C>\r\n\
         \t\t\t<C Type=\"AutopilotRule\" U=\"40000002-0000-0020-ffff504f94112233\" Title=\"Skimmer Ein\" Nio=\"1\" APC=\"{apc}\" STA=\"1\">\r\n\
         \t\t\t\t<Co K=\"Q\" U=\"40000002-0000-0021-00ffaaaaaaaa0002\"/>\r\n\
         \t\t\t</C>\r\n\
         \t\t</C>\r\n\
         \t</C>\r\n\
         </ControlList>\r\n"
    );
    let doc = LoxoneDoc::parse(s.as_bytes()).unwrap();

    let (m, _) = decompile(&doc, &DecompileOptions::default()).unwrap();
    let text = m.to_text();
    assert!(
        text.contains("# when type 5, descID 272001, id 272: value 540"),
        "time event rendered raw:\n{text}"
    );
    assert!(
        text.contains("# then \"Pumpe\": on"),
        "action resolves the control title:\n{text}"
    );

    let mo = DecompileOptions {
        scope: DecompileScope::ManagedOnly,
        ..Default::default()
    };
    let (m, _) = decompile(&doc, &mo).unwrap();
    assert!(!m.to_text().contains("# when"), "adoptable view stays bare");
}

#[test]
fn unit_suffixes_follow_the_documented_port_unit() {
    // D52: `AutoShadeTime` is documented in minutes — `2h` is 120 there,
    // not 7200; a `let` carries its unit to the port that uses it; an
    // extern's port is checked against the extern's declared type.
    let src = "page \"Beschattung\"\n\
               let nachlauf = 90s\n\
               extern sonne = VirtualIn(iname: \"VI3\")\n\
               extern jal_sued = AutoJalousie(title: \"Beschattung Süd\")\n\
               puls = Monoflop(InputTrigger: sonne.Q, Time: 1.5h)\n\
               jal = AutoJalousie(AutoShadeTime: 2h, SRoff: nachlauf)\n\
               jal_sued.TargetPos = 70%\n";
    let module = Module::parse(src).unwrap();
    let mut lock = Lockfile::new();
    let out = compile(&base(), &module, &mut lock, &opts()).unwrap();
    let def_of = |uuid: &str, key: &str| {
        let obj = out
            .objects()
            .iter()
            .find(|o| o.uuid == uuid)
            .unwrap()
            .path
            .clone();
        lxir::doc::ports(out.element_at(&obj).unwrap())
            .into_iter()
            .find(|p| p.key == key)
            .unwrap()
            .def
    };
    let jal = lock.objects["jal"].uuid.clone();
    assert_eq!(def_of(&jal, "AutoShadeTime").as_deref(), Some("120"));
    assert_eq!(def_of(&jal, "SRoff").as_deref(), Some("1.5"));
    let puls = lock.objects["puls"].uuid.clone();
    assert_eq!(def_of(&puls, "Time").as_deref(), Some("5400"));
    let jal_sued = lock.externals["jal_sued"].uuid.clone();
    assert_eq!(def_of(&jal_sued, "TargetPos").as_deref(), Some("70"));

    // A suffix that cannot mean the port's unit is refused, by name.
    for (bad, needle) in [
        (
            "puls = Monoflop(InputTrigger: sonne.Q, Time: 70%)",
            "`puls.Time`: `70%` does not fit",
        ),
        (
            "jal = AutoJalousie(AutoShadeTime: 10s)",
            "`jal.AutoShadeTime`: `10s` is no exact",
        ),
        (
            "jal_sued.TargetPos = 5s",
            "`jal_sued.TargetPos`: `5s` does not fit",
        ),
    ] {
        let src = format!(
            "page \"Beschattung\"\n\
             extern sonne = VirtualIn(iname: \"VI3\")\n\
             extern jal_sued = AutoJalousie(title: \"Beschattung Süd\")\n{bad}\n"
        );
        let module = Module::parse(&src).unwrap();
        let err = compile(&base(), &module, &mut Lockfile::new(), &opts())
            .unwrap_err()
            .to_string();
        assert!(err.contains(needle), "{bad}: {err}");
    }
}

#[test]
fn canvas_labels_point_at_the_port_key() {
    let src = "extern sonne = VirtualIn(iname: \"VI3\")\n\
               puls = Monoflop(Tr: sonne.Q)\n";
    let module = Module::parse(src).unwrap();
    let err = compile(&base(), &module, &mut Lockfile::new(), &opts())
        .unwrap_err()
        .to_string();
    assert!(
        err.contains(
            "`Tr` is the label Loxone Config shows on the canvas — the port is `InputTrigger`"
        ),
        "{err}"
    );
}

#[test]
fn schedule_changes_are_semantic() {
    // lox#162: a schedule-only edit is a diff entry per day group, in the
    // source's terms, and moves the fingerprint; the v5 form (recorded
    // before schedules joined the projection) does not see it.
    let compiled = |schedule: &str| {
        let src = format!(
            "page \"Beschattung\"\nschedule abend\n{schedule}end\n\
             uhr = DayTimer(\"Abendlicht\")\nuhr.schedule = abend\n"
        );
        let module = Module::parse(&src).unwrap();
        compile(&base(), &module, &mut Lockfile::new(), &opts()).unwrap()
    };
    let a = compiled("\tdaily 00:00-06:00 activate, 21:00-24:00 activate\n");
    let b = compiled("\tdaily 00:00-06:00 activate, 22:00-24:00 activate\n");
    let d = lxir::diff::diff(&a, &b);
    assert!(d.added.is_empty() && d.removed.is_empty() && d.wires_added.is_empty());
    assert_eq!(d.param_changes.len(), 1, "{:?}", d.param_changes);
    let p = &d.param_changes[0];
    assert_eq!(p.port_key, "schedule daily");
    assert_eq!(
        p.from.as_deref(),
        Some("00:00-06:00 activate, 21:00-24:00 activate")
    );
    assert_eq!(
        p.to.as_deref(),
        Some("00:00-06:00 activate, 22:00-24:00 activate")
    );
    let summary = lxir::diff::summary(&a, &b, &d);
    assert!(
        summary.contains(
            "DayTimer \"Abendlicht\" schedule daily: 00:00-06:00 activate, \
             21:00-24:00 activate -> 00:00-06:00 activate, 22:00-24:00 activate"
        ),
        "{summary}"
    );
    use lxir::diff::{semantic_fingerprint, semantic_fingerprint_v5};
    assert_ne!(semantic_fingerprint(&a), semantic_fingerprint(&b));
    assert_eq!(semantic_fingerprint_v5(&a), semantic_fingerprint_v5(&b));
    assert!(lxir::diff::diff(&a, &a).is_empty());

    // Columns that change alike group under the schedule's day words; a
    // mode column one side lacks reads as none.
    let c = compiled("\tweekdays 00:00-06:00 activate, 21:00-24:00 activate\n\tholiday off\n");
    let d = lxir::diff::diff(&a, &c);
    let changes: Vec<(&str, Option<&str>, Option<&str>)> = d
        .param_changes
        .iter()
        .map(|p| (p.port_key.as_str(), p.from.as_deref(), p.to.as_deref()))
        .collect();
    assert_eq!(
        changes,
        [
            ("schedule holiday", None, Some("off")),
            (
                "schedule weekend",
                Some("00:00-06:00 activate, 21:00-24:00 activate"),
                Some("off")
            ),
        ]
    );
}

#[test]
fn schedules_write_modes_and_entries_onto_a_day_timer() {
    // D53: a schedule claim rewrites `Modes=`, `N=` and the `<Entry>`
    // children of the DayTimer — by mode index, then start.
    let src = "page \"Beschattung\"\n\
               schedule abend\n\
               \tweekend 08:00-23:00 activate\n\
               \tmon 06:00-07:30, 17:00-22:00\n\
               \tholiday off\n\
               end\n\
               uhr = DayTimer(\"Abendlicht\")\n\
               uhr.schedule = abend\n";
    let module = Module::parse(src).unwrap();
    assert_eq!(
        Module::parse(&module.to_text()).unwrap(),
        module,
        "fmt round-trips"
    );
    let mut lock = Lockfile::new();
    let out = compile(&base(), &module, &mut lock, &opts()).unwrap();
    let uuid = &lock.objects["uhr"].uuid;
    let path = out
        .objects()
        .iter()
        .find(|o| &o.uuid == uuid)
        .unwrap()
        .path
        .clone();
    let el = out.element_at(&path).unwrap();
    assert_eq!(
        el.attr("Modes"),
        Some(
            "00000000-0000-0001-1500000000000000,00000000-0000-0004-1500000000000000,\
             00000000-0000-0005-1500000000000000,00000000-0000-0006-1500000000000000,\
             00000000-0000-0007-1500000000000000,00000000-0000-0008-1500000000000000,\
             00000000-0000-0009-1500000000000000,00000000-0000-000a-1500000000000000"
        ),
        "every weekday has a column — unnamed ones are off"
    );
    assert_eq!(el.attr("N"), Some("4"));
    // `UserModes=` counts the modes beyond the weekdays, right before `N=`.
    let names: Vec<&str> = el.attrs.iter().map(|a| a.name.as_str()).collect();
    let n = names.iter().position(|&a| a == "N").unwrap();
    assert_eq!(names[n - 1], "UserModes");
    assert_eq!(el.attr("UserModes"), Some("1"));
    let entries: Vec<String> = el
        .child_elements()
        .filter(|e| e.name == "Entry")
        .map(|e| {
            e.attrs
                .iter()
                .map(|a| format!("{}={}", a.name, a.value))
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect();
    assert_eq!(
        entries,
        [
            "From=360 To=450 Ix=1 V=1",
            "From=1020 To=1320 Ix=1 V=1",
            "From=480 To=1380 Ix=6 NeedActivate=true V=1",
            "From=480 To=1380 Ix=7 NeedActivate=true V=1",
        ]
    );
    // Recompiling the output converges: the claim rewrites in place.
    let again = compile(&out, &module, &mut lock.clone(), &opts()).unwrap();
    assert_eq!(again.to_bytes(), out.to_bytes());

    // A digital timer takes no values; claims are checked by type.
    for (bad, needle) in [
        (
            "uhr.schedule = abend\nschedule abend\n\tmon 06:00-07:00 = 21\nend",
            "the timer is digital",
        ),
        (
            "p = Monoflop(\"x\")\np.schedule = abend\nschedule abend\n\tmon off\nend",
            "a schedule belongs to a `DayTimer`",
        ),
        (
            "uhr.schedule = abnd\nschedule abend\n\tmon off\nend",
            "did you mean `abend`?",
        ),
    ] {
        let src = format!("uhr = DayTimer(\"Abendlicht\")\n{bad}\n");
        let err = Module::parse(&src)
            .and_then(|m| compile(&base(), &m, &mut Lockfile::new(), &opts()))
            .unwrap_err()
            .to_string();
        assert!(err.contains(needle), "{bad}: {err}");
    }
}

#[test]
fn virtual_inputs_are_minted_into_their_folder() {
    // D54: a VirtualIn is an object of the "Virtuelle Eingänge" folder,
    // not a page block — appended there with the next free `VI<n>`, no
    // position, and wired across to the page like any extern source.
    let src = "page \"Beschattung\"\n\
               party = VirtualIn(\"Partymodus\")\n\
               soll = VirtualIn(\"Sollwert\", Analog: \"true\", MinVal: 5, MaxVal: 30, DefVal: 21)\n\
               licht = And(I1: party.Q)\n";
    let module = Module::parse(src).unwrap();
    let mut lock = Lockfile::new();
    let out = compile(&base(), &module, &mut lock, &opts()).unwrap();
    let objects = out.objects();
    let find = |slug: &str| {
        let uuid = &lock.objects[slug].uuid;
        objects.iter().find(|o| &o.uuid == uuid).unwrap()
    };
    let folder = objects
        .iter()
        .find(|o| o.block_type == "VirtualInCaption")
        .unwrap();
    for slug in ["party", "soll"] {
        assert_eq!(
            find(slug).path[..folder.path.len()],
            folder.path[..],
            "{slug}"
        );
        assert_eq!(lock.objects[slug].page_uuid, None);
        assert_eq!(lock.objects[slug].layout, None);
    }
    let el = |slug: &str| out.element_at(&find(slug).path).unwrap();
    let names = |slug: &str| {
        el(slug)
            .attrs
            .iter()
            .map(|a| a.name.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        names("party"),
        [
            "Type",
            "IName",
            "V",
            "U",
            "Title",
            "Cl",
            "Nio",
            "WF",
            "Analog",
            "MaxVal",
            "MinChange",
            "MinTime"
        ],
        "a save fills these in for a digital input"
    );
    assert_eq!(
        names("soll")[8..],
        ["Analog", "EnVal", "DefVal", "MinVal", "MaxVal"],
        "the order a save writes"
    );
    let kids = |slug: &str| {
        el(slug)
            .child_elements()
            .map(|c| c.name.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(kids("party"), ["Co", "Co", "IoData", "Display"]);
    assert_eq!(el("party").attr("IName"), Some("VI4"), "haus has VI1..VI3");
    assert_eq!(el("party").attr("Analog"), Some("false"));
    assert_eq!(el("soll").attr("IName"), Some("VI5"));
    assert_eq!(el("soll").attr("Analog"), Some("true"));
    assert_eq!(el("soll").attr("MaxVal"), Some("30"));
    // The page block reads the folder object's output directly (D34).
    let q = &lock.objects["party"].ports["Q"];
    let and = el("licht");
    let i1 = and
        .child_elements()
        .find(|c| c.attr("K") == Some("I1"))
        .unwrap();
    assert!(i1.child_elements().any(|i| i.attr("Input") == Some(q)));

    // Converges, and the designation is kept once minted — even when an
    // earlier VirtualIn takes the next number first.
    let again = compile(&out, &module, &mut lock.clone(), &opts()).unwrap();
    assert_eq!(again.to_bytes(), out.to_bytes());
    let first = Module::parse(&format!("{src}neu = VirtualIn(\"Neu\")\n")).unwrap();
    let out2 = compile(&out, &first, &mut lock, &opts()).unwrap();
    let iname = |slug: &str| {
        let uuid = &lock.objects[slug].uuid;
        let o = out2
            .objects()
            .into_iter()
            .find(|o| &o.uuid == uuid)
            .unwrap();
        out2.element_at(&o.path)
            .unwrap()
            .attr("IName")
            .unwrap()
            .to_string()
    };
    assert_eq!(
        [iname("party"), iname("soll"), iname("neu")],
        ["VI4", "VI5", "VI6"]
    );

    let bad = Module::parse("x = VirtualIn(Analog: \"ja\")\n").unwrap();
    let err = compile(&base(), &bad, &mut Lockfile::new(), &opts()).unwrap_err();
    assert!(err.to_string().contains("\"true\" or \"false\""), "{err}");
}

#[test]
fn decompile_names_repeated_uuids_instead_of_a_slug_clash() {
    // Two objects sharing one UUID (seen in lox-cli's eval fixture) used
    // to surface as `duplicate name` from the lifted module.
    let text = String::from_utf8(std::fs::read("examples/configs/haus.Loxone").unwrap())
        .unwrap()
        .replace(
            "U=\"20000002-0000-0020-ffff504f94112233\"",
            "U=\"20000001-0000-0010-ffff504f94112233\"",
        );
    let doc = LoxoneDoc::parse(text.as_bytes()).unwrap();
    let err = decompile(&doc, &DecompileOptions::default())
        .unwrap_err()
        .to_string();
    assert!(err.contains("duplicate UUID"), "{err}");
    assert!(err.contains("20000001-0000-0010-ffff504f94112233"), "{err}");
}

#[test]
fn diff_summary_is_a_commit_message_in_names() {
    let read = |p: &str| LoxoneDoc::parse(&std::fs::read(p).unwrap()).unwrap();
    let (base, out) = (
        read("examples/configs/haus.Loxone"),
        read("examples/out/haus-compiled.Loxone"),
    );
    let summary = |a: &LoxoneDoc, b: &LoxoneDoc| lxir::diff::summary(a, b, &lxir::diff::diff(a, b));
    assert_eq!(
        summary(&base, &out),
        "Beschattung: add 2 blocks, change 1 parameter, add 5 wires\n\
         \n\
         Added:\n\
         - GreaterEqual \"Temp über 28\" (page \"Beschattung\")\n\
         - And \"beschatten\" (page \"Beschattung\")\n\
         \n\
         Changed:\n\
         - AutoJalousie \"Beschattung Süd\" TargetPos: 100 -> 70\n\
         \n\
         Wires added:\n\
         - \"Temp über 28\".Q -> \"beschatten\".I1\n\
         - \"beschatten\".Q -> \"Beschattung Süd\".AutoShade\n\
         - \"Außentemperatur\".Q -> \"Temp über 28\".Input1\n\
         - \"Windalarm\".Q -> \"Beschattung Süd\".Safety\n\
         - \"Sonnenschein\".Q -> \"beschatten\".I2\n"
    );
    // Backwards, the removed side is named from the old document.
    let back = summary(&out, &base);
    assert!(
        back.starts_with("Beschattung: remove 2 blocks, change 1 parameter, remove 5 wires\n"),
        "{back}"
    );
    assert!(
        back.contains("Wires removed:\n- \"Temp über 28\".Q -> \"beschatten\".I1\n"),
        "{back}"
    );
    assert_eq!(summary(&base, &base), "no semantic change\n");
}

/// A page with one LightController2 of three circuits — a switch (AQ1),
/// an RGB (AQ2) and a tunable-white circuit (AQ3) — carrying the stock
/// moods of a Loxone Config save: "Viel Licht" (the motion mood) and
/// "Aus" (the off mood). Its `<LightscenesC>` is the house shape cut to
/// three outputs.
fn mood_base() -> LoxoneDoc {
    LoxoneDoc::parse(
        b"<ControlList Version=\"1\" NextObj=\"50\">\r\n\
          \t<C Type=\"Document\" U=\"00000001-0000-0000-ffff000000000001\" ConfigVersion=\"17010727\">\r\n\
          \t\t<C Type=\"Page\" U=\"00000005-0000-0000-ffff000000000001\" Title=\"Licht\">\r\n\
          \t\t\t<C Type=\"LightController2\" V=\"178\" U=\"00000040-0000-0000-ffff000000000001\" \
          Title=\"Licht Test\" Px=\"100\" Py=\"100\" Px2=\"154\" Py2=\"136\" Nio=\"4\">\r\n\
          \t\t\t\t<Co K=\"Sel1\" U=\"00000040-0000-0001-01ff000000000001\"/>\r\n\
          \t\t\t\t<Co K=\"AQ1\" U=\"00000040-0000-0002-01ff000000000001\"/>\r\n\
          \t\t\t\t<Co K=\"AQ2\" U=\"00000040-0000-0003-01ff000000000001\"/>\r\n\
          \t\t\t\t<Co K=\"AQ3\" U=\"00000040-0000-0004-01ff000000000001\"/>\r\n\
          \t\t\t\t<LightscenesC FC=\"0040FFFF,008080FF,00FFFF80,0040FF40\" Outputs=\"3\" Num=\"2\">\r\n\
          \t\t\t\t\t<LightsceneC Name=\"Viel Licht\" UUID=\"00000040-0000-0010-ffff000000000001\" \
          Move=\"true\" Outputs=\"3\" SID=\"777\" CID=\"22\" Q1=\"1\" Q2=\"1173841924\" Q3=\"1811619236\"/>\r\n\
          \t\t\t\t\t<LightsceneC Name=\"Aus\" UUID=\"00000040-0000-0011-ffff000000000001\" \
          ReadOnly=\"true\" Outputs=\"3\" SID=\"778\" CID=\"11\" Q1=\"0\" Q2=\"1073741824\" Q3=\"1610612736\"/>\r\n\
          \t\t\t\t</LightscenesC>\r\n\
          \t\t\t\t<LSConfig OpM=\"778\" ScMv=\"777\" ScAc=\"2\"/>\r\n\
          \t\t\t</C>\r\n\
          \t\t</C>\r\n\
          \t</C>\r\n\
          </ControlList>\r\n",
    )
    .unwrap()
}

fn mood_opts() -> CompileOptions {
    CompileOptions {
        page_title: Some("Licht".into()),
        ..opts()
    }
}

fn compile_moods(set: &str) -> lxir::Result<LoxoneDoc> {
    let src = format!(
        "extern licht = LightController2(title: \"Licht Test\")\n\
         moods abend\n{set}end\nlicht.moods = abend\n"
    );
    compile(
        &mood_base(),
        &Module::parse(&src)?,
        &mut Lockfile::new(),
        &mood_opts(),
    )
}

fn moods_of(doc: &LoxoneDoc) -> Vec<String> {
    let o = doc
        .objects()
        .into_iter()
        .find(|o| o.block_type == "LightController2")
        .unwrap();
    let scenes = doc.element_at(&o.path).unwrap();
    let scenes = scenes
        .child_elements()
        .find(|c| c.name == "LightscenesC")
        .unwrap();
    assert_eq!(
        scenes.attr("Num"),
        Some(scenes.child_elements().count().to_string().as_str())
    );
    scenes
        .child_elements()
        .map(|e| {
            e.attrs
                .iter()
                .filter(|a| a.name != "UUID" && a.name != "Outputs")
                .map(|a| format!("{}={}", a.name, a.value))
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect()
}

#[test]
fn moods_write_onto_a_light_controller() {
    // D58: a moods claim makes the controller's mood list the set's, plus
    // the off mood; values are encoded per circuit kind.
    let set = "\t\"Viel Licht\": all on, AQ2 rgb(100, 100, 100), AQ3 temp(100%, 6500K)\n\
               \t\"Lesen\": AQ1 on, AQ3 temp(80%, 2700K)\n\
               \t\"Kino\": AQ2 rgb(0, 0, 20)\n";
    let module = Module::parse(&format!("moods abend\n{set}end\n")).unwrap();
    assert_eq!(
        Module::parse(&module.to_text()).unwrap(),
        module,
        "fmt round-trips"
    );

    let out = compile_moods(set).unwrap();
    assert_eq!(
        moods_of(&out),
        [
            "Name=Viel Licht Move=true SID=777 CID=22 Q1=1 Q2=1173841924 Q3=1811619236",
            "Name=Lesen SID=1 CID=12 Q1=1 Q2=1073741824 Q3=1811415436",
            "Name=Kino SID=2 CID=13 Q1=0 Q2=1093741824 Q3=1610612736",
            "Name=Aus ReadOnly=true SID=778 CID=11 Q1=0 Q2=1073741824 Q3=1610612736",
        ],
        "kept moods keep their ids, new ones take the next free, the off mood stays last"
    );
    // New moods get a minted UUID.
    let xml = String::from_utf8(out.to_bytes()).unwrap();
    let lesen = &xml[xml.find("Name=\"Lesen\"").unwrap()..];
    assert!(
        lesen[..60].contains("-ffff504f94112233\""),
        "minted on this machine: {lesen:.80}"
    );

    // Recompiling the output converges.
    let again = compile(
        &out,
        &Module::parse(&format!(
            "extern licht = LightController2(title: \"Licht Test\")\n\
             moods abend\n{set}end\nlicht.moods = abend\n"
        ))
        .unwrap(),
        &mut Lockfile::new(),
        &mood_opts(),
    )
    .unwrap();
    assert_eq!(again.to_bytes(), out.to_bytes());

    // The stock moods as they are: nothing changes, byte for byte.
    let same =
        compile_moods("\t\"Viel Licht\": all on, AQ2 rgb(100, 100, 100), AQ3 temp(100%, 6500K)\n")
            .unwrap();
    let unclaimed = compile(
        &mood_base(),
        &Module::parse("extern licht = LightController2(title: \"Licht Test\")\n").unwrap(),
        &mut Lockfile::new(),
        &mood_opts(),
    )
    .unwrap();
    assert_eq!(same.to_bytes(), unclaimed.to_bytes());

    // Dropping a mood deletes it; renaming one is a drop plus a new mood.
    let fewer = compile(&out, &Module::parse(
        "extern licht = LightController2(title: \"Licht Test\")\n\
         moods abend\n\t\"Viel Licht\": all on\n\t\"Kino\": AQ2 rgb(0, 0, 20)\nend\nlicht.moods = abend\n",
    ).unwrap(), &mut Lockfile::new(), &mood_opts())
    .unwrap();
    let names: Vec<String> = moods_of(&fewer)
        .iter()
        .map(|m| m.split(" SID").next().unwrap().to_string())
        .collect();
    assert_eq!(
        names,
        [
            "Name=Viel Licht Move=true",
            "Name=Kino",
            "Name=Aus ReadOnly=true"
        ]
    );
}

#[test]
fn mood_changes_are_semantic() {
    let a = mood_base();
    let b = compile_moods(
        "\t\"Viel Licht\": all on, AQ2 rgb(100, 100, 100), AQ3 temp(100%, 6500K)\n\
         \t\"Lesen\": AQ1 on, AQ3 temp(80%, 2700K)\n",
    )
    .unwrap();
    let d = lxir::diff::diff(&a, &b);
    let changes: Vec<(&str, Option<&str>, Option<&str>)> = d
        .param_changes
        .iter()
        .map(|p| (p.port_key.as_str(), p.from.as_deref(), p.to.as_deref()))
        .collect();
    assert_eq!(
        changes,
        [
            ("mood \"Lesen\"", None, Some("AQ1 on, AQ3 temp(80%, 2700K)")),
            (
                "moods",
                Some("\"Viel Licht\", \"Aus\""),
                Some("\"Viel Licht\", \"Lesen\", \"Aus\"")
            ),
        ]
    );
    use lxir::diff::{semantic_fingerprint, semantic_fingerprint_v6};
    assert_ne!(semantic_fingerprint(&a), semantic_fingerprint(&b));
    assert_eq!(semantic_fingerprint_v6(&a), semantic_fingerprint_v6(&b));
}

#[test]
fn mood_misuse_is_reported() {
    for (set, needle) in [
        (
            "\t\"Viel Licht\": all on\n\t\"Lesen\": AQ2 on\n",
            "`on` does not fit",
        ),
        (
            "\t\"Viel Licht\": all on\n\t\"Lesen\": AQ1 rgb(1, 2, 3)\n",
            "does not fit",
        ),
        (
            "\t\"Viel Licht\": all on\n\t\"Aus\": all off\n",
            "built-in off mood",
        ),
        (
            "\t\"Lesen\": AQ1 on\n",
            "\"Viel Licht\" is a built-in mood (SID 777)",
        ),
        (
            "\t\"Viel Licht\": all on, AQ4 on\n",
            "the controller has 3 circuits",
        ),
        ("\t\"Viel Licht\": AQ1 on, AQ1 off\n", "AQ1"),
        (
            "\t\"Viel Licht\": all on\n\t\"Viel Licht\": all off\n",
            "Viel Licht",
        ),
    ] {
        let err = compile_moods(set).unwrap_err().to_string();
        assert!(err.contains(needle), "{set}: {err}");
    }
    for (src, needle) in [
        (
            "extern licht = LightController2(title: \"Licht Test\")\n\
             moods abend\n\t\"Viel Licht\": all on\nend\nlicht.moods = abnd\n",
            "did you mean `abend`?",
        ),
        (
            "extern licht = LightController2(title: \"Licht Test\")\n\
             moods abend\n\t\"Viel Licht\": all on\nend\n\
             licht.moods = abend\nlicht.moods = abend\n",
            "licht",
        ),
    ] {
        let err = Module::parse(src)
            .and_then(|m| compile(&mood_base(), &m, &mut Lockfile::new(), &mood_opts()))
            .unwrap_err()
            .to_string();
        assert!(err.contains(needle), "{src}: {err}");
    }
}

#[test]
fn mood_identities_are_lock_pinned_until_deployed() {
    // A mood the compiler mints keeps its UUID through every recompile
    // against the same base — the lock is a fixpoint — and the entry
    // retires once the base carries the mood.
    let src = "extern licht = LightController2(title: \"Licht Test\")\n\
               moods abend\n\t\"Viel Licht\": all on\n\t\"Lesen\": AQ1 on\nend\n\
               licht.moods = abend\n";
    let module = Module::parse(src).unwrap();
    let mut lock = Lockfile::new();
    let out = compile(&mood_base(), &module, &mut lock, &mood_opts()).unwrap();
    let pinned = &lock.moods["00000040-0000-0000-ffff000000000001"];
    assert_eq!(pinned.keys().collect::<Vec<_>>(), ["Lesen"]);
    let mut again = lock.clone();
    let out2 = compile(&mood_base(), &module, &mut again, &mood_opts()).unwrap();
    assert_eq!(out2.to_bytes(), out.to_bytes());
    assert_eq!(again, lock, "lock fixpoint");
    // Deployed: the base has "Lesen" — nothing left to pin.
    let mut after = lock.clone();
    compile(&out, &module, &mut after, &mood_opts()).unwrap();
    assert!(after.moods.is_empty(), "{:?}", after.moods);
}

#[test]
fn a_minted_controller_gets_the_stock_moods_first() {
    // Oracle session 21: Loxone Config's first save writes the stock
    // moods and their defaults into a bare LightController2. lxir writes
    // the same, so the claim applies straight away.
    let src = "neu = LightController2(\"Neu\")\n\
               moods abend\n\t\"Viel Licht\": all on\n\t\"Lesen\": AQ1 on, AQ2 40%\nend\n\
               neu.moods = abend\n";
    let module = Module::parse(src).unwrap();
    let mut lock = Lockfile::new();
    let out = compile(&mood_base(), &module, &mut lock, &mood_opts()).unwrap();
    let uuid = &lock.objects["neu"].uuid;
    let o = out.objects().into_iter().find(|o| &o.uuid == uuid).unwrap();
    let el = out.element_at(&o.path).unwrap();
    let children: Vec<&str> = el
        .child_elements()
        .filter(|c| c.name != "Co")
        .map(|c| c.name.as_str())
        .collect();
    assert_eq!(
        children,
        ["LightscenesC", "LSConfig", "IoData", "PSD", "HCL", "COHist"]
    );
    let lsconfig = el.child_elements().find(|c| c.name == "LSConfig").unwrap();
    assert_eq!(
        (
            lsconfig.attr("OpM"),
            lsconfig.attr("ScMv"),
            lsconfig.attr("ScAc")
        ),
        (Some("778"), Some("777"), Some("1"))
    );
    let fields = lxir::moods::mood_fields(&out, &o);
    assert_eq!(fields["moods"], "\"Viel Licht\", \"Lesen\", \"Aus\"");
    assert_eq!(fields["mood \"Viel Licht\""], "all on");
    assert_eq!(fields["mood \"Lesen\""], "AQ1 on, AQ2 40%");
    assert_eq!(fields["mood \"Aus\""], "all off");
    assert_eq!(
        lock.moods[uuid].keys().collect::<Vec<_>>(),
        ["Aus", "Lesen", "Viel Licht"]
    );
    let mut again = lock.clone();
    let out2 = compile(&mood_base(), &module, &mut again, &mood_opts()).unwrap();
    assert_eq!(out2.to_bytes(), out.to_bytes());
    assert_eq!(again, lock, "lock fixpoint");
}

#[test]
fn a_template_gives_every_room_its_moods() {
    // The loop Loxone Config lacks: one claim in a room template, the set
    // a value parameter with a default.
    let src = "moods abend\n\t\"Viel Licht\": all on\n\t\"Lesen\": AQ1 on\nend\n\
               moods kueche\n\t\"Viel Licht\": all on\n\t\"Kochen\": AQ1 50%\nend\n\
               template raum(stimmungen = \"abend\")\n\
               \tlc = LightController2(\"Licht\")\n\
               \tlc.moods = stimmungen\n\
               end\n\
               wohnen = raum()\n\
               kochen = raum(stimmungen: \"kueche\")\n";
    let module = Module::parse(src).unwrap();
    assert_eq!(
        Module::parse(&module.to_text()).unwrap(),
        module,
        "fmt round-trips"
    );
    let mut lock = Lockfile::new();
    let out = compile(&mood_base(), &module, &mut lock, &mood_opts()).unwrap();
    let order = |slug: &str| {
        let o = out
            .objects()
            .into_iter()
            .find(|o| o.uuid == lock.objects[slug].uuid)
            .unwrap();
        lxir::moods::mood_fields(&out, &o)["moods"].clone()
    };
    assert_eq!(order("wohnen_lc"), "\"Viel Licht\", \"Lesen\", \"Aus\"");
    assert_eq!(order("kochen_lc"), "\"Viel Licht\", \"Kochen\", \"Aus\"");
}

/// [`mood_base`] with a second controller, "Licht Zwei", that has the
/// same moods.
fn two_controller_mood_base() -> LoxoneDoc {
    let base = String::from_utf8(mood_base().to_bytes()).unwrap();
    let start = base.find("\t\t\t<C Type=\"LightController2\"").unwrap();
    let end = base[start..].find("\t\t\t</C>\r\n").unwrap() + start + "\t\t\t</C>\r\n".len();
    let twin = base[start..end]
        .replace("00000040-", "00000041-")
        .replace("Licht Test", "Licht Zwei")
        .replace("Py=\"100\"", "Py=\"300\"")
        .replace("Py2=\"136\"", "Py2=\"336\"");
    LoxoneDoc::parse(format!("{}{twin}{}", &base[..end], &base[end..]).as_bytes()).unwrap()
}

#[test]
fn existing_moods_lift_into_a_set_and_compile_back_unchanged() {
    // #170: decompile and adopt write a controller's own moods as a set
    // and a claim; adopted and compiled, the config is byte-identical.
    let set = "\t\"Viel Licht\": all on, AQ2 rgb(100, 100, 100), AQ3 temp(100%, 6500K)\n\
               \t\"Lesen\": AQ1 on, AQ3 temp(80%, 2700K)\n\
               \t\"Kino\": AQ2 rgb(0, 0, 20)\n\
               \t\"Roh\": AQ3 1700000000\n";
    let base = compile_moods(set).unwrap();
    let (view, _) = decompile(&base, &DecompileOptions::default()).unwrap();
    let text = view.to_text();
    // One circuit of three is on: no `all` to factor out.
    let lifted = set.replace("all on, AQ2", "AQ1 on, AQ2");
    assert!(
        text.contains(&format!("moods licht_test_moods\n{lifted}end\n")),
        "{text}"
    );
    assert!(
        text.contains("licht_test.moods = licht_test_moods\n"),
        "{text}"
    );

    let (module, mut lock, report) = adopt(&base).unwrap();
    assert_eq!(module.moods_claims().count(), 1, "{report:?}");
    let out = compile(&base, &module, &mut lock, &mood_opts()).unwrap();
    assert!(
        out.to_bytes() == base.to_bytes(),
        "adopt + compile is a no-op"
    );
    assert!(lock.moods.is_empty(), "nothing minted");
}

#[test]
fn only_controllers_with_their_own_moods_are_lifted() {
    // The stock "Viel Licht" (every circuit on) is what a first save
    // writes — nothing for source to own.
    let stock = String::from_utf8(mood_base().to_bytes())
        .unwrap()
        .replace("Q2=\"1173841924\" Q3=\"1811619236\"", "Q2=\"1\" Q3=\"1\"")
        .replace("Q2=\"1073741824\" Q3=\"1610612736\"", "Q2=\"0\" Q3=\"0\"");
    let stock = LoxoneDoc::parse(stock.as_bytes()).unwrap();
    let (view, _) = decompile(&stock, &DecompileOptions::default()).unwrap();
    assert_eq!(view.mood_sets().count(), 0, "{}", view.to_text());

    // Two controllers with the same moods share one set.
    let twins = two_controller_mood_base();
    let (view, _) = decompile(&twins, &DecompileOptions::default()).unwrap();
    let sets: Vec<_> = view.mood_sets().map(|s| s.name.as_str()).collect();
    assert_eq!(sets, ["licht_test_moods"]);
    let claims: Vec<String> = view.moods_claims().map(|c| c.to_string()).collect();
    assert_eq!(
        claims,
        [
            "licht_test.moods = licht_test_moods",
            "licht_zwei.moods = licht_test_moods"
        ]
    );
    let (pages, _) = decompile_pages(&twins, &DecompileOptions::default()).unwrap();
    assert_eq!(pages[0].module.mood_sets().count(), 1);

    // Moods source cannot write stay with Loxone Config, said in a comment.
    let dup = String::from_utf8(mood_base().to_bytes())
        .unwrap()
        .replace("Name=\"Aus\"", "Name=\"Viel Licht\"");
    let (view, _) = decompile(
        &LoxoneDoc::parse(dup.as_bytes()).unwrap(),
        &DecompileOptions::default(),
    )
    .unwrap();
    assert_eq!(view.moods_claims().count(), 0);
    assert!(
        view.to_text()
            .contains("# moods licht_test: not lifted — two moods named \"Viel Licht\""),
        "{}",
        view.to_text()
    );
}

#[test]
fn the_off_mood_keeps_its_place_in_the_list() {
    // Loxone Config may keep "Aus" between moods; a rewrite leaves it there.
    let base = String::from_utf8(mood_base().to_bytes()).unwrap().replace(
        "\t\t\t\t</LightscenesC>",
        "\t\t\t\t\t<LightsceneC Name=\"Kino\" UUID=\"00000040-0000-0012-ffff000000000001\" \
         Outputs=\"3\" SID=\"1\" CID=\"12\" Q1=\"1\" Q2=\"1073741824\" Q3=\"1610612736\"/>\r\n\
         \t\t\t\t</LightscenesC>",
    );
    let base = base.replace("Num=\"2\"", "Num=\"3\"");
    let src = "extern licht = LightController2(title: \"Licht Test\")\n\
               moods abend\n\t\"Kino\": AQ1 on\n\t\"Viel Licht\": all on\nend\n\
               licht.moods = abend\n";
    let out = compile(
        &LoxoneDoc::parse(base.as_bytes()).unwrap(),
        &Module::parse(src).unwrap(),
        &mut Lockfile::new(),
        &mood_opts(),
    )
    .unwrap();
    let names: Vec<String> = moods_of(&out)
        .iter()
        .map(|m| {
            m.split(" SID")
                .next()
                .unwrap()
                .replace(" Move=true", "")
                .replace(" ReadOnly=true", "")
        })
        .collect();
    assert_eq!(names, ["Name=Kino", "Name=Aus", "Name=Viel Licht"]);
}
