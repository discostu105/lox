//! Attribute claims on externs (D39): `x.title|room|category = …` over the
//! Air device in examples/configs/pool.Loxone — canonical form, refusals,
//! followers, and the withdraw → tombstone → retire lifecycle.

use lxir::ir::{Attr, CompileOptions, Module, compile};
use lxir::uuid::parse_serial;
use lxir::{Lockfile, LoxoneDoc};

const DEVICE: &str = "3000000e-0000-00e0-ffff504f94112233";
const ONLINE: &str = "3000000f-0000-00f0-ffff504f94112233";
const TEMP: &str = "30000010-0000-0100-ffff504f94112233";
const BATTERY: &str = "30000011-0000-0110-ffff504f94112233";
const POOL: &str = "30000000-0000-0003-ffff504f94112233";
const TECHNIKRAUM: &str = "30000000-0000-0004-ffff504f94112233";
const GARTEN: &str = "30000000-0000-0009-ffff504f94112233";

const EXTERNS: &str = "extern klima = LoxAIRDevice(uuid: \"3000000e-0000-00e0-ffff504f94112233\")\n\
                       extern temp = LoxAIRAsensor(uuid: \"30000010-0000-0100-ffff504f94112233\")\n";

fn base() -> LoxoneDoc {
    LoxoneDoc::parse(&std::fs::read("examples/configs/pool.Loxone").unwrap()).unwrap()
}

fn opts() -> CompileOptions {
    CompileOptions {
        machine: parse_serial("504F94112233").unwrap(),
        mint_time_unix: 1_767_225_600,
        page_title: Some("Pool".into()),
        allow_removals: false,
        accept_version: None,
    }
}

fn module(claims: &str) -> Module {
    Module::parse(&format!("{EXTERNS}\n{claims}")).unwrap()
}

/// `Pr=`/`Cr=` on the object's `<IoData>`.
fn io(doc: &LoxoneDoc, uuid: &str, slot: &str) -> Option<String> {
    let path = doc.objects().into_iter().find(|o| o.uuid == uuid)?.path;
    let io = doc
        .element_at(&path)?
        .child_elements()
        .find(|e| e.name == "IoData")?;
    io.attr(slot).map(str::to_string)
}

fn title(doc: &LoxoneDoc, uuid: &str) -> Option<String> {
    doc.objects()
        .into_iter()
        .find(|o| o.uuid == uuid)
        .and_then(|o| o.title)
}

#[test]
fn attribute_assignments_are_canonical_and_never_wired() {
    let src = "let raum = \"Garten\"\n\
               \n\
               extern klima = LoxAIRDevice(uuid: \"3000000e-0000-00e0-ffff504f94112233\")\n\
               \n\
               klima.title = \"Raumklima \\\"Süd\\\" & Garten\" # Anführungszeichen bleiben\n\
               klima.room = raum\n";
    let m = Module::parse(src).unwrap();
    assert_eq!(m.to_text(), src);
    let attrs: Vec<_> = m.attrs().collect();
    assert_eq!(attrs.len(), 2);
    assert_eq!(attrs[0].attr, Attr::Title);
    assert_eq!(attrs[1].attr, Attr::Room);

    // XML escaping is the writer's job: the title arrives decoded.
    let out = compile(&base(), &m, &mut Lockfile::new(), &opts()).unwrap();
    assert_eq!(
        title(&out, DEVICE).as_deref(),
        Some("Raumklima \"Süd\" & Garten")
    );

    let err = Module::parse("extern v = VirtualIn(iname: \"VI1\")\nv.room <- v.Q\n").unwrap_err();
    assert!(
        err.to_string().contains("object attribute, not a port"),
        "{err}"
    );
}

#[test]
fn attribute_misuse_is_refused() {
    let fails = |src: &str, needle: &str| {
        let err = match Module::parse(src) {
            Err(e) => e.to_string(),
            Ok(m) => compile(&base(), &m, &mut Lockfile::new(), &opts())
                .unwrap_err()
                .to_string(),
        };
        assert!(err.contains(needle), "`{needle}` not in: {err}");
    };

    // Managed blocks: the label is the title; room/category stay GUI-owned.
    fails("b = Not(\"Nicht\")\nb.title = \"x\"\n", "give it a label");
    fails("b = Not(\"Nicht\")\nb.room = \"Pool\"\n", "D19");
    fails("klima.room = \"Pool\"\n", "klima");
    // Rewriting what the extern is matched by would break the match.
    fails(
        "extern s = Switch(title: \"Störmeldung\")\ns.title = \"Alarm\"\n",
        "pin it by `uuid:`",
    );
    fails(
        "extern w = Switch(title: \"Freigabe\", room: \"Technikraum\")\nw.room = \"Pool\"\n",
        "matched by",
    );
    fails(
        &format!("{EXTERNS}klima.room = \"Pool\"\nklima.room = \"Garten\"\n"),
        "duplicate assignment",
    );
    fails(&format!("{EXTERNS}klima.title = 5s\n"), "not a unit value");
    fails(
        "template t(x: LoxAIRDevice)\n\tx.room = \"Pool\"\nend\n",
        "top-level statements, D39",
    );

    // Compile-time: rooms are named by title and must exist, once.
    fails(
        &format!("{EXTERNS}klima.room = \"Garden\"\n"),
        "did you mean `Garten`",
    );
    fails(
        &format!("{EXTERNS}temp.category = \"Licht\"\n"),
        "no category",
    );
    // An attribute the object does not carry is never added.
    fails(
        "extern vi = VirtualIn(iname: \"VI1\")\nvi.room = \"Pool\"\n",
        "carries no room binding",
    );
    fails(
        &format!("{EXTERNS}klima.category = \"Klima\"\n"),
        "set it once",
    );
    // Two slugs resolving to one object must not both claim it.
    fails(
        &format!(
            "{EXTERNS}extern geraet = LoxAIRDevice(title: \"Raumklima Technik\")\n\
             klima.room = \"Pool\"\ngeraet.room = \"Garten\"\n"
        ),
        "claim the room of the same object",
    );
}

#[test]
fn room_claim_takes_the_children_that_shared_the_device_room() {
    let base = base();
    let mut lock = Lockfile::new();
    let out = compile(
        &base,
        &module("klima.room = \"Garten\"\n"),
        &mut lock,
        &opts(),
    )
    .unwrap();
    for u in [DEVICE, ONLINE, TEMP] {
        assert_eq!(io(&out, u, "Pr").as_deref(), Some(GARTEN), "{u}");
    }
    assert_eq!(io(&out, BATTERY, "Pr").as_deref(), Some(POOL));
    let c = &lock.attr_claims[DEVICE][&Attr::Room];
    assert_eq!(
        (c.original.as_str(), c.written.as_str()),
        (TECHNIKRAUM, GARTEN)
    );
    assert_eq!(c.followers, [ONLINE, TEMP]);

    // Nothing but the four bindings changed.
    let changed = String::from_utf8(base.to_bytes())
        .unwrap()
        .lines()
        .zip(String::from_utf8(out.to_bytes()).unwrap().lines())
        .filter(|(a, b)| a != b)
        .count();
    assert_eq!(changed, 3, "device + two follower IoData lines");

    // Fixpoint against its own output.
    let again = compile(
        &out,
        &module("klima.room = \"Garten\"\n"),
        &mut lock,
        &opts(),
    )
    .unwrap();
    assert_eq!(out.to_bytes(), again.to_bytes());
}

#[test]
fn explicitly_claimed_child_is_not_a_follower() {
    let mut lock = Lockfile::new();
    let out = compile(
        &base(),
        &module("klima.room = \"Garten\"\ntemp.room = \"Pool\"\n"),
        &mut lock,
        &opts(),
    )
    .unwrap();
    assert_eq!(io(&out, TEMP, "Pr").as_deref(), Some(POOL));
    assert_eq!(io(&out, ONLINE, "Pr").as_deref(), Some(GARTEN));
    assert_eq!(lock.attr_claims[DEVICE][&Attr::Room].followers, [ONLINE]);
    assert_eq!(lock.attr_claims[TEMP][&Attr::Room].original, TECHNIKRAUM);
}

#[test]
fn withdrawn_claim_restores_and_its_tombstone_retires() {
    let base = base();
    let claims = module("klima.title = \"Raumklima Garten\"\nklima.room = \"Garten\"\n");
    let none = module("");
    let mut lock = Lockfile::new();
    let deployed = compile(&base, &claims, &mut lock, &opts()).unwrap();

    // Withdrawal against the deployed claim: everything back, tombstoned.
    let out1 = compile(&deployed, &none, &mut lock, &opts()).unwrap();
    let never = compile(&base, &none, &mut Lockfile::new(), &opts()).unwrap();
    assert_eq!(
        out1.to_bytes(),
        never.to_bytes(),
        "identical to never having claimed"
    );
    assert!(lock.attr_claims.is_empty());
    let tomb = &lock.removed_attrs[DEVICE];
    assert_eq!(tomb[&Attr::Title].written, "Raumklima Garten");
    assert_eq!(tomb[&Attr::Room].followers, [ONLINE, TEMP]);

    // The compile-to-push window: the base still carries the claim, the
    // tombstone keeps restoring — output and lock are fixpoints.
    let lock1 = lock.to_json();
    let out2 = compile(&deployed, &none, &mut lock, &opts()).unwrap();
    assert_eq!(out2.to_bytes(), out1.to_bytes());
    assert_eq!(lock.to_json(), lock1);

    // Pushed: the base shows the originals, the tombstones retire.
    compile(&out1, &none, &mut lock, &opts()).unwrap();
    assert!(lock.removed_attrs.is_empty());
}

#[test]
fn follower_moved_in_the_gui_stays_put() {
    let claims = module("klima.room = \"Garten\"\n");
    let mut lock = Lockfile::new();
    let deployed = compile(&base(), &claims, &mut lock, &opts()).unwrap();

    // Someone moves the online status to Pool in Loxone Config.
    let online_io = format!("Cr=\"30000000-0000-000b-ffff504f94112233\" Pr=\"{GARTEN}\"");
    let text = String::from_utf8(deployed.to_bytes()).unwrap();
    assert_eq!(text.matches(&online_io).count(), 1);
    let edited = LoxoneDoc::parse(
        text.replace(
            &online_io,
            &format!("Cr=\"30000000-0000-000b-ffff504f94112233\" Pr=\"{POOL}\""),
        )
        .as_bytes(),
    )
    .unwrap();

    // The recompile keeps the GUI's choice and drops it from the followers.
    let out = compile(&edited, &claims, &mut lock, &opts()).unwrap();
    assert_eq!(io(&out, ONLINE, "Pr").as_deref(), Some(POOL));
    assert_eq!(io(&out, TEMP, "Pr").as_deref(), Some(GARTEN));
    assert_eq!(lock.attr_claims[DEVICE][&Attr::Room].followers, [TEMP]);

    // Withdrawing the claim restores only what still carries our write.
    let back = compile(&out, &module(""), &mut lock, &opts()).unwrap();
    assert_eq!(io(&back, DEVICE, "Pr").as_deref(), Some(TECHNIKRAUM));
    assert_eq!(io(&back, TEMP, "Pr").as_deref(), Some(TECHNIKRAUM));
    assert_eq!(io(&back, ONLINE, "Pr").as_deref(), Some(POOL));
}
