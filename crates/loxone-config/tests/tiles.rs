//! Touch Pure Flex tiles (D41): `tiles <name>` … `end` sets claimed onto
//! extern `TreeDevice`s — canonical form, the rewrite of existing tiles
//! only, the API-link and abbreviation refusals, the broadcast and
//! dead-command lints, and the diff/fingerprint projection.
//!
//! The fixture adds synthetic Touch Pure Flex devices and shading blocks
//! to examples/configs/pool.Loxone in memory: `Küche` reaches Nord through
//! an `OutputRef` of its API connector and Süd and the light directly;
//! `Tür` reaches Nord and Süd; West is linked to nothing.

use loxone_config::Lockfile;
use loxone_config::LoxoneDoc;
use loxone_config::diff::{diff, semantic_fingerprint, semantic_fingerprint_v3};
use loxone_config::logic::{CompileOptions, LintKind, Module, compile, lint_tiles};
use loxone_config::uuid::parse_serial;

const KUECHE: &str = "40000001-0000-0010-ffff504f94112233";
const KUECHE_API: &str = "40000002-0000-0020-ffff504f94112233";
const MELDER: &str = "40000005-0000-0050-ffff504f94112233";
const NORD: &str = "40000010-0000-0100-ffff504f94112233";
const SUED: &str = "40000011-0000-0110-ffff504f94112233";

const TREE: &str = r#"			<C Type="LoxTree" V="175" U="40000000-0000-0000-ffff504f94112233" Title="Tree Extension">
				<C Type="TreeDevice" V="175" U="40000001-0000-0010-ffff504f94112233" Title="Touch Pure Flex Küche" cfgId="7" img="2">
					<C Type="ApiActor" V="175" U="40000002-0000-0020-ffff504f94112233" Title="API Connector">
						<Co K="I" U="40000002-0000-0021-00ffaaaaaaaa4002">
							<In Input="40000014-0000-0142-01ffaaaaaaaa4014"/>
						</Co>
					</C>
					<btns>
						<btn i="2" x="6" y="7" h="13" w="15">
							<f1 api="SET(Lico;Tg;pulse)" cT="=ECHO(Lico)"/>
							<f2 bT="1"/>
							<f3 bT="2"/>
						</btn>
						<btn ci="1" i="3" x="37" y="7" h="13" w="15">
							<f1 api="SET(As;Tg;pulse)" cT="=ECHO(As)"/>
							<f2 api="SET(As;Co;pulse)" cT="=ECHO(As)" bT="1"/>
							<f3 api="SET(As;Cc;pulse)" cT="=ECHO(As)" bT="2"/>
						</btn>
						<btn ci="2" i="4" x="68" y="7" h="13" w="15">
							<f1></f1>
							<f2 bT="1"/>
							<f3 bT="2"/>
						</btn>
						<btn ci="3" i="5" x="6" y="28" h="13" w="15">
							<f1 api="SET(Sw;Tg;pulse) " cT="=WAIT(500) &amp; GETOUTPUT(Sw;O;1:On;0:Off)"/>
							<f2 bT="1"/>
							<f3 bT="2"/>
						</btn>
					</btns>
				</C>
				<C Type="TreeDevice" V="175" U="40000003-0000-0030-ffff504f94112233" Title="Touch Pure Flex Tür" cfgId="7" img="2">
					<C Type="ApiActor" V="175" U="40000004-0000-0040-ffff504f94112233" Title="API Connector">
						<Co K="I" U="40000004-0000-0041-00ffaaaaaaaa4004"/>
					</C>
					<btns>
						<btn i="2" x="6" y="7" h="13" w="15">
							<f1 api="SET(As;Tg;pulse)"/>
							<f2 bT="1"/>
							<f3 bT="2"/>
						</btn>
						<btn ci="1" i="3" x="37" y="7" h="13" w="15">
							<f1 api="MENU(Lico;Mood)"/>
							<f2 bT="1"/>
							<f3 bT="2"/>
						</btn>
					</btns>
				</C>
				<C Type="TreeDevice" V="175" U="40000005-0000-0050-ffff504f94112233" Title="Präsenzmelder"/>
			</C>
"#;

const BLOCKS: &str = r#"			<C Type="AutoJalousie" V="175" U="40000010-0000-0100-ffff504f94112233" Title="Nord">
				<Co K="OutputAPI" U="40000010-0000-0101-00ffaaaaaaaa4010">
					<In Input="40000014-0000-0141-00ffaaaaaaaa4014" FLG="1"/>
					<In Input="40000004-0000-0041-00ffaaaaaaaa4004"/>
				</Co>
			</C>
			<C Type="AutoJalousie" V="175" U="40000011-0000-0110-ffff504f94112233" Title="Süd">
				<Co K="OutputAPI" U="40000011-0000-0111-00ffaaaaaaaa4011">
					<In Input="40000002-0000-0021-00ffaaaaaaaa4002"/>
					<In Input="40000004-0000-0041-00ffaaaaaaaa4004"/>
				</Co>
			</C>
			<C Type="AutoJalousie" V="175" U="40000012-0000-0120-ffff504f94112233" Title="West">
				<Co K="OutputAPI" U="40000012-0000-0121-00ffaaaaaaaa4012"/>
			</C>
			<C Type="LightController2" V="175" U="40000013-0000-0130-ffff504f94112233" Title="Licht">
				<Co K="OutputAPI" U="40000013-0000-0131-00ffaaaaaaaa4013">
					<In Input="40000002-0000-0021-00ffaaaaaaaa4002"/>
				</Co>
			</C>
			<C Type="OutputRef" V="175" U="40000014-0000-0140-ffff504f94112233" Title="API Connector" Ref="40000002-0000-0020-ffff504f94112233">
				<Co K="AI" U="40000014-0000-0141-00ffaaaaaaaa4014">
					<In Input="40000010-0000-0101-00ffaaaaaaaa4010"/>
				</Co>
				<Co K="AQ" U="40000014-0000-0142-01ffaaaaaaaa4014"/>
			</C>
"#;

fn base() -> LoxoneDoc {
    base_from(TREE, BLOCKS)
}

fn base_from(tree: &str, blocks: &str) -> LoxoneDoc {
    let pool = std::fs::read_to_string("examples/configs/pool.Loxone").unwrap();
    let crlf = |s: &str| s.replace('\n', "\r\n");
    let air = "\t\t\t<C Type=\"LoxAIR\"";
    let netz = "\t\t\t<C Type=\"InputRef\"";
    assert!(pool.contains(air) && pool.contains(netz));
    let text = pool
        .replacen(air, &format!("{}{air}", crlf(tree)), 1)
        .replacen(netz, &format!("{}{netz}", crlf(blocks)), 1);
    LoxoneDoc::parse(text.as_bytes()).unwrap()
}

fn opts() -> CompileOptions {
    CompileOptions {
        machine: parse_serial("504F94112233").unwrap(),
        mint_time_unix: 1_767_225_600,
        page_title: Some("Pool".into()),
        allow_removals: false,
        accept_version: None,
        relayout: Vec::new(),
    }
}

const EXTERNS: &str = "extern flex = TreeDevice(uuid: \"40000001-0000-0010-ffff504f94112233\")
extern jal_nord = AutoJalousie(uuid: \"40000010-0000-0100-ffff504f94112233\")
extern jal_sued = AutoJalousie(uuid: \"40000011-0000-0110-ffff504f94112233\")
extern jal_west = AutoJalousie(uuid: \"40000012-0000-0120-ffff504f94112233\")
extern licht = LightController2(uuid: \"40000013-0000-0130-ffff504f94112233\")
";

/// Canonical form, used by most tests.
const KUECHE_SET: &str = "tiles kueche # Küche
\t# Licht über die Schaltung, nicht über das Tile
\ttile 2() # aus
\ttile 3(
\t\tclick: \"SET(As;Tg;pulse)\",
\t\tup: \"SET(As;Co;pulse)\",
\t\tdown: \"SET(As;Cc;pulse)\",
\t\ttext: \"=ECHO(As)\",
\t\tto: jal_nord,
\t\tto: jal_sued,
\t\tup_to: jal_nord, # nur Nord fährt hoch
\t)
\ttile 4(click: \"SET(Lico;M+;pulse)&WAIT(500)&MENU(Lico;Mood)\", to: licht)
end

flex.tiles = kueche
";

fn module(rest: &str) -> Module {
    Module::parse(&format!("{EXTERNS}\n{rest}")).unwrap()
}

fn compile_err(rest: &str) -> String {
    let m = module(rest);
    compile(&base(), &m, &mut Lockfile::new(), &opts())
        .unwrap_err()
        .to_string()
}

fn parse_err(rest: &str) -> String {
    Module::parse(&format!("{EXTERNS}\n{rest}"))
        .unwrap_err()
        .to_string()
}

/// The serialized `<btn i="…">` element of a device.
fn btn(doc: &LoxoneDoc, device: &str, i: &str) -> String {
    let text = String::from_utf8(doc.to_bytes()).unwrap();
    let start = text.find(&format!("U=\"{device}\"")).unwrap();
    let from = start + text[start..].find(&format!(" i=\"{i}\"")).unwrap();
    let open = text[..from].rfind("<btn").unwrap();
    let close = from + text[from..].find("</btn>").unwrap();
    text[open..close + "</btn>".len()].to_string()
}

#[test]
fn tiles_sets_are_canonical() {
    let src = format!("{EXTERNS}\n{KUECHE_SET}");
    let m = Module::parse(&src).unwrap();
    assert_eq!(m.to_text(), src);
    // A short tile goes on one line, like any call.
    let short =
        format!("{EXTERNS}\ntiles t\n\ttile 4(click: \"SET(Lico;Tg;pulse)\", to: licht)\nend\n");
    assert_eq!(Module::parse(&short).unwrap().to_text(), short);
    let long = format!(
        "{EXTERNS}\ntiles t\n\ttile 4(\n\t\tclick: \"SET(Lico;Tg;pulse)\",\n\t\tto: licht,\n\t\tup: \"x\",\n\t\tdown: \"y\",\n\t)\nend\n"
    );
    assert_eq!(Module::parse(&long).unwrap().to_text(), long);
}

#[test]
fn compile_rewrites_only_the_listed_tiles() {
    let base = base();
    let m = module(KUECHE_SET);
    let mut lock = Lockfile::new();
    let out = compile(&base, &m, &mut lock, &opts()).unwrap();

    // `tile 2()` clears every function; bT stays.
    assert_eq!(
        btn(&out, KUECHE, "2"),
        "<btn i=\"2\" x=\"6\" y=\"7\" h=\"13\" w=\"15\">\r\n\
         \t\t\t\t\t\t\t<f1></f1>\r\n\
         \t\t\t\t\t\t\t<f2 bT=\"1\"/>\r\n\
         \t\t\t\t\t\t\t<f3 bT=\"2\"/>\r\n\
         \t\t\t\t\t\t</btn>"
    );
    // Tile-level `text:` and `to:` reach every command; `up_to:` narrows
    // the up arrow. Attribute order api, cT, bT, ctrls; layout untouched.
    let three = btn(&out, KUECHE, "3");
    assert!(three.starts_with("<btn ci=\"1\" i=\"3\" x=\"37\" y=\"7\" h=\"13\" w=\"15\">"));
    assert!(three.contains(&format!(
        "<f1 api=\"SET(As;Tg;pulse)\" cT=\"=ECHO(As)\" ctrls=\"{NORD},{SUED}\"/>"
    )));
    assert!(three.contains(&format!(
        "<f2 api=\"SET(As;Co;pulse)\" cT=\"=ECHO(As)\" bT=\"1\" ctrls=\"{NORD}\"/>"
    )));
    assert!(three.contains(&format!(
        "<f3 api=\"SET(As;Cc;pulse)\" cT=\"=ECHO(As)\" bT=\"2\" ctrls=\"{NORD},{SUED}\"/>"
    )));
    // A command without text; `&` escaped the way Loxone writes it.
    assert!(btn(&out, KUECHE, "4").contains(
        "<f1 api=\"SET(Lico;M+;pulse)&amp;WAIT(500)&amp;MENU(Lico;Mood)\" \
         ctrls=\"40000013-0000-0130-ffff504f94112233\"/>"
    ));
    // An unlisted tile is carried byte for byte.
    assert_eq!(btn(&out, KUECHE, "5"), btn(&base, KUECHE, "5"));

    // No lock state, and a second compile is a fixpoint.
    assert!(lock.attr_claims.is_empty());
    let again = compile(&out, &m, &mut lock, &opts()).unwrap();
    assert_eq!(again.to_bytes(), out.to_bytes());

    // Withdrawing the claim leaves the pushed programs (D40 semantics).
    let withdrawn = module("tiles kueche\nend\n");
    let after = compile(&out, &withdrawn, &mut lock, &opts()).unwrap();
    assert_eq!(btn(&after, KUECHE, "3"), three);
}

#[test]
fn diff_and_fingerprint_see_tile_programs() {
    let base = base();
    let out = compile(&base, &module(KUECHE_SET), &mut Lockfile::new(), &opts()).unwrap();
    let d = diff(&base, &out);
    let keys: Vec<&str> = d
        .param_changes
        .iter()
        .map(|p| p.port_key.as_str())
        .collect();
    assert_eq!(
        keys,
        [
            "tile 2 click",
            "tile 3 click",
            "tile 3 down",
            "tile 3 up",
            "tile 4 click"
        ]
    );
    let cleared = &d.param_changes[0];
    assert_eq!(cleared.object_uuid, KUECHE);
    assert_eq!(
        cleared.from.as_deref(),
        Some("api=\"SET(Lico;Tg;pulse)\" cT=\"=ECHO(Lico)\"")
    );
    assert_eq!(cleared.to, None);
    assert_ne!(semantic_fingerprint(&base), semantic_fingerprint(&out));
    // v3 (before D41) is blind to them — drift accepts it as in sync.
    assert_eq!(
        semantic_fingerprint_v3(&base),
        semantic_fingerprint_v3(&out)
    );
}

#[test]
fn targets_must_be_api_linked_and_of_the_addressed_type() {
    let err = compile_err(
        "tiles t\n\ttile 3(click: \"SET(As;Tg;pulse)\", to: jal_west)\nend\nflex.tiles = t\n",
    );
    assert!(
        err.contains("tile 3 click: `to: jal_west` is not API-linked to `flex`"),
        "{err}"
    );

    let err = compile_err(
        "tiles t\n\ttile 3(up: \"SET(As;Co;pulse)\", to: licht)\nend\nflex.tiles = t\n",
    );
    assert!(
        err.contains(
            "`to: licht` is a LightController2, but `SET(As;Co;pulse)` addresses AutoJalousie"
        ),
        "{err}"
    );
    // An abbreviation Lox Logic does not know is not judged.
    let m =
        module("tiles t\n\ttile 3(click: \"SET(Sw;Tg;pulse)\", to: licht)\nend\nflex.tiles = t\n");
    compile(&base(), &m, &mut Lockfile::new(), &opts()).unwrap();
}

#[test]
fn managed_and_template_targets_link_through_source_wires() {
    let api = format!("extern flex_api = ApiActor(uuid: \"{KUECHE_API}\")\n");
    let set = "tiles t\n\ttile 4(click: \"SET(As;Tg;pulse)\", to: ost)\nend\nflex.tiles = t\n";
    let template =
        "template shade(api: ApiActor)\n\tjal = AutoJalousie(\"Ost\", OutputAPI: api.I)\nend\n";

    let m = module(&format!(
        "{api}{template}\nost = shade(api: flex_api)\n\n{set}"
    ));
    let mut lock = Lockfile::new();
    let out = compile(&base(), &m, &mut lock, &opts()).unwrap();
    let ost = &lock.objects["ost_jal"].uuid;
    assert!(
        btn(&out, KUECHE, "4").contains(&format!("ctrls=\"{ost}\"")),
        "{}",
        btn(&out, KUECHE, "4")
    );

    // The same block without its API wire cannot receive the command.
    let unlinked = template.replace(", OutputAPI: api.I", "");
    let err = compile_err(&format!(
        "{api}{unlinked}\nost = shade(api: flex_api)\n\n{set}"
    ));
    assert!(err.contains("is not API-linked"), "{err}");

    // The expanded name of the block names its replacement.
    let err = parse_err(&format!(
        "{api}{template}\nost = shade(api: flex_api)\n\n{}",
        set.replace("to: ost)", "to: ost_jal)")
    ));
    assert!(err.contains("Write `ost` instead of `ost_jal`"), "{err}");
}

/// A second mirror of Küche's API connector on the page, distributing
/// into the actor like the first — D34 routing can no longer pick one.
const MIRROR2: &str = r#"			<C Type="OutputRef" V="175" U="40000015-0000-0150-ffff504f94112233" Title="API Connector" Ref="40000002-0000-0020-ffff504f94112233">
				<Co K="AI" U="40000015-0000-0151-00ffaaaaaaaa4015"/>
				<Co K="AQ" U="40000015-0000-0152-01ffaaaaaaaa4015"/>
			</C>
"#;

#[test]
fn new_api_links_go_through_an_explicit_output_ref() {
    let fed = "<In Input=\"40000014-0000-0142-01ffaaaaaaaa4014\"/>";
    let tree = TREE.replacen(
        fed,
        &format!("{fed}\n\t\t\t\t\t\t\t<In Input=\"40000015-0000-0152-01ffaaaaaaaa4015\"/>"),
        1,
    );
    let base = base_from(&tree, &format!("{BLOCKS}{MIRROR2}"));
    let run = |rest: &str| compile(&base, &module(rest), &mut Lockfile::new(), &opts());
    // The new block joins the mirrors on their page.
    let api = format!("extern flex_api = ApiActor(uuid: \"{KUECHE_API}\")\npage \"Technik\"\n");
    let set = "tiles t\n\ttile 4(click: \"SET(As;Tg;pulse)\", to: ost)\nend\nflex.tiles = t\n";

    // Naming the actor leaves the choice of mirror to D34, which refuses.
    let err = run(&format!(
        "{api}ost = AutoJalousie(\"Ost\", OutputAPI: flex_api.I)\n{set}"
    ))
    .unwrap_err()
    .to_string();
    assert!(err.contains("several mirrors of"), "{err}");

    // An explicit mirror carries the whole link: the forward feed into its
    // `AI`, its distribution into the actor, and the reverse wire on the
    // block's `OutputAPI` naming the mirror's `AI` — no `FLG=` minted.
    let link = "ost_api = OutputRef(mirrors: flex_api, AI: ost.OutputAPI)\n\
                flex_api.I <- ost_api.AQ\n\
                ost = AutoJalousie(\"Ost\", OutputAPI: ost_api.AI)\n";
    let mut lock = Lockfile::new();
    let out = compile(
        &base,
        &module(&format!("{api}{link}{set}")),
        &mut lock,
        &opts(),
    )
    .unwrap();
    let port = |slug: &str, key: &str| lock.objects[slug].ports[key].clone();
    let wires: Vec<(String, String)> = out
        .wires()
        .into_iter()
        .map(|w| (w.from_port, w.to_port))
        .collect();
    for wire in [
        (port("ost", "OutputAPI"), port("ost_api", "AI")),
        (port("ost_api", "AI"), port("ost", "OutputAPI")),
        (
            port("ost_api", "AQ"),
            "40000002-0000-0021-00ffaaaaaaaa4002".into(),
        ),
    ] {
        assert!(wires.contains(&wire), "{wire:?}");
    }
    let text = String::from_utf8(out.to_bytes()).unwrap();
    assert!(text.contains(&format!("<In Input=\"{}\"/>", port("ost_api", "AI"))));
    let ost = &lock.objects["ost"].uuid;
    assert!(btn(&out, KUECHE, "4").contains(&format!("ctrls=\"{ost}\"")));

    // The same through an extern sink and an existing (orphan) mirror.
    let orphan = "extern west_api = OutputRef(uuid: \"40000015-0000-0150-ffff504f94112233\")\n\
                  west_api.AI <- jal_west.OutputAPI\n\
                  jal_west.OutputAPI <- west_api.AI\n";
    let west = set.replace("to: ost", "to: jal_west");
    run(&format!("{orphan}{west}")).unwrap();
    // Without the mirror's distribution into the actor the link is half
    // drawn — the base's orphan never distributed, so the tile refuses.
    let undistributed = base_from(TREE, &format!("{BLOCKS}{MIRROR2}"));
    let err = compile(
        &undistributed,
        &module(&format!("{orphan}{west}")),
        &mut Lockfile::new(),
        &opts(),
    )
    .unwrap_err()
    .to_string();
    assert!(err.contains("`to: jal_west` is not API-linked"), "{err}");

    // Any block's `OutputAPI` that accepts wires reads it.
    run(&format!("{api}{link}licht.OutputAPI <- ost_api.AI\n{set}")).unwrap();

    // The exception stays narrow: only an API connector may read the
    // `AI` of a mirror, and only of a mirror of an `ApiActor`.
    let netz = "extern netz = InputRef(uuid: \"3000000a-0000-00a0-ffff504f94112233\")\n";
    for (rest, want) in [
        (
            format!("{api}{link}m = Monoflop(InputTrigger: ost_api.AI)\n"),
            "`ost_api.AI` is an input port and cannot be used as a wire source",
        ),
        (
            format!("{api}{link}{netz}netz.AI <- ost_api.AI\n"),
            "`ost_api.AI` is an input port and cannot be used as a wire source",
        ),
        (
            "nord_ref = OutputRef(mirrors: jal_nord)\njal_west.OutputAPI <- nord_ref.AI\n"
                .to_string(),
            "`nord_ref.AI` is an input port and cannot be used as a wire source",
        ),
    ] {
        let err = run(&rest).unwrap_err().to_string();
        assert!(err.contains(want), "{rest}: {err}");
    }
}

#[test]
fn layout_stays_with_loxone_config() {
    let err = compile_err("tiles t\n\ttile 9()\nend\nflex.tiles = t\n");
    assert!(err.contains("has no tile 9 (it has 2, 3, 4, 5)"), "{err}");
    let err = compile_err(&format!(
        "extern melder = TreeDevice(uuid: \"{MELDER}\")\ntiles t\n\ttile 2()\nend\nmelder.tiles = t\n"
    ));
    assert!(err.contains("no `<btns>`"), "{err}");
    let err = compile_err(&format!(
        "extern flex2 = TreeDevice(uuid: \"{KUECHE}\")\ntiles t\n\ttile 2()\nend\nflex.tiles = t\nflex2.tiles = t\n"
    ));
    assert!(err.contains("claim the tiles of the same device"), "{err}");
}

#[test]
fn statement_level_refusals() {
    for (src, want) in [
        (
            "tiles t\nend\njal_nord.tiles = t\n",
            "tiles belong to a Touch Pure Flex",
        ),
        (
            "x = Switch()\ntiles t\nend\nx.tiles = t\n",
            "is a managed block",
        ),
        (
            "flex.tiles = kuche\ntiles kueche\nend\n",
            "no `tiles kuche` set is declared",
        ),
        (
            "tiles t\nend\nflex.tiles = t\nflex.tiles = t\n",
            "duplicate assignment `flex.tiles",
        ),
        (
            "tiles t\n\ttile 2()\n\ttile 2()\nend\n",
            "tile 2 is programmed twice",
        ),
        (
            "tiles t\n\ttile 2(klick: \"x\")\nend\n",
            "unknown key `klick:`",
        ),
        (
            "tiles t\n\ttile 2(click: \"x\", up_to: jal_nord)\nend\n",
            "`up_to:` without `up:`",
        ),
        (
            "tiles t\n\ttile 2(text: \"x\")\nend\n",
            "`text:` without a command",
        ),
        (
            "tiles t\n\ttile 2(click: \"x\", to: \"jal_nord\")\nend\n",
            "`to:` takes a block slug",
        ),
        (
            "tiles t\n\ttile 2(click: jal_nord)\nend\n",
            "`click:` takes a quoted string",
        ),
        (
            "tiles t\n\ttile 2(click: \"x\", to: jal_nordd)\nend\n",
            "`to: jal_nordd` names an undeclared slug",
        ),
        (
            "tiles t\n\ttile 2(click: \"x\", to: t)\nend\n",
            "names a `tiles` set, not a block",
        ),
        ("tiles t\n\ttile 2(click: \"\")\nend\n", "is not a command"),
        (
            "tiles t\n\ttile 2(\"Label\")\nend\n",
            "takes no label string",
        ),
        ("tiles t\n\ttile 0()\nend\n", "positive whole number"),
        ("tile 2()\n", "only allowed inside a `tiles"),
        ("tiles t\n\ttile 2()\n", "unclosed `tiles t`"),
        (
            "tiles t\n\ttile 2(\nend\n",
            "expected `:` after argument name `end`",
        ),
        ("flex.tiles <- jal_nord.Q\n", "never a wire"),
        (
            "template x(d: TreeDevice)\n\ttiles t\nend\n",
            "declared at the top level, not inside a template body",
        ),
    ] {
        let err = parse_err(src);
        assert!(err.contains(want), "{src:?}: {err}");
    }
}

#[test]
fn lint_reports_broadcasts_and_dead_commands_on_every_device() {
    let base = base();
    let m = module("");
    let findings = lint_tiles(&m, &base);
    let lines: Vec<String> = findings
        .iter()
        .map(|f| format!("{} {}: {}", f.kind.label(), f.slug, f.detail))
        .collect();
    // Küche (pinned as `flex`): the As tile names no target and reaches
    // Nord (through the OutputRef) and Süd. Tür: the same broadcast, and a
    // Lico command no light controller of its own receives.
    assert_eq!(lines.len(), 5, "{lines:#?}");
    assert!(lines[0].starts_with(
        "tile-broadcast flex: tile 3 click: `SET(As;Tg;pulse)` names no target, so all 2 \
         API-linked AutoJalousie blocks act on it (AutoJalousie \"Nord\", AutoJalousie \"Süd\")"
    ));
    assert!(lines[3].starts_with("tile-broadcast Touch Pure Flex Tür: tile 2 click"));
    assert_eq!(findings[4].kind, LintKind::TileDeadCommand);
    assert!(
        lines[4]
            .contains("tile 3 click: `MENU(Lico;Mood)` addresses Lico, but no LightController2")
    );

    // Listing the targets settles Küche; Tür is reported until it is
    // programmed too.
    let out = compile(&base, &module(KUECHE_SET), &mut Lockfile::new(), &opts()).unwrap();
    let after = lint_tiles(&module(KUECHE_SET), &out);
    assert_eq!(after.len(), 2);
    assert!(after.iter().all(|f| f.slug == "Touch Pure Flex Tür"));
}

#[test]
fn dead_commands_of_one_type_are_one_finding_per_device() {
    // A second Lico function on Tür: both commands share the cause (no
    // light controller linked), so they read as one finding.
    let tree = TREE.replacen(
        "<f1 api=\"MENU(Lico;Mood)\"/>",
        "<f1 api=\"MENU(Lico;Mood)\"/>\n\t\t\t\t\t\t\t<f2 api=\"SET(Lico;M+;pulse)\" bT=\"1\"/>",
        1,
    );
    assert_ne!(tree, TREE);
    let findings = lint_tiles(&module(""), &base_from(&tree, BLOCKS));
    let dead: Vec<&str> = findings
        .iter()
        .filter(|f| f.kind == LintKind::TileDeadCommand)
        .map(|f| f.detail.as_str())
        .collect();
    assert_eq!(dead.len(), 1, "{dead:#?}");
    assert!(
        dead[0].starts_with(
            "2 commands address Lico, but no LightController2 is API-linked to the device, so \
             they reach nothing: tile 3 click: `MENU(Lico;Mood)`, tile 3 up: `SET(Lico;M+;pulse)`"
        ),
        "{}",
        dead[0]
    );
}
