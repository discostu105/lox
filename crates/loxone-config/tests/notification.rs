//! Push notifications (D51): the minted `OutputRefLM` that sends a message
//! when its input turns on, and the `recipients` declaration that says who
//! gets it (`ACT=` on the `Notification`).
//!
//! The fixture adds to examples/configs/pool.Loxone in memory what Loxone
//! Config 17.2 writes for a notification (oracle session 15): the
//! `Notification` object, two users and a group, carol already a
//! recipient with the older `ACT="1"`. Every UUID is made up.

use loxone_config::diff::{diff, semantic_fingerprint, semantic_fingerprint_v4};
use loxone_config::logic::{CompileOptions, Module, compile};
use loxone_config::uuid::parse_serial;
use loxone_config::{Lockfile, LoxoneDoc};

const NOTIFICATION: &str = "60000001-0000-0010-ffff504f94112233";
const DANA: &str = "60000002-0000-0020-ffff504f94112233";
const CAROL: &str = "60000003-0000-0030-ffff504f94112233";
const FAMILY: &str = "60000004-0000-0040-ffff504f94112233";
const SWITCH: &str = "30000005-0000-0050-ffff504f94112233";

fn tree() -> String {
    format!(
        r#"		<C Type="Notification" V="175" U="{NOTIFICATION}" Title="Frost" WF="16384" Entries="10">
			<IoData Cr="30000000-0000-000b-ffff504f94112233"/>
		</C>
		<C Type="LoxCaption" V="175" U="60000000-0000-0001-ffff504f94112233" Title="Users and Permissions" CaptionType="20" SubType="20">
			<C Type="UserCaption" V="175" U="60000000-0000-0002-ffff504f94112233" Title="User" CaptionType="30" SubType="30">
				<C Type="User" V="175" U="{DANA}" Title="dana" UsR="129" LastEdit="562400000" NFCArr="" NFCNArr="" CodeArr="" iBtnArr="" EAIdArr="">
					<HP H="" Sc="-2" S="" Alg="1"/>
					<HP H="" Sc="-2" S="" Alg="1"/>
					<PH NUM="1">
						<PRM REF="{SWITCH}" ACT="3"/>
					</PH>
				</C>
				<C Type="User" V="175" U="{CAROL}" Title="carol" UsR="129" LastEdit="562400000" NFCArr="" NFCNArr="" CodeArr="" iBtnArr="" EAIdArr="">
					<HP H="" Sc="-2" S="" Alg="1"/>
					<HP H="" Sc="-2" S="" Alg="1"/>
					<PH NUM="1">
						<PRM REF="{NOTIFICATION}" ACT="1"/>
					</PH>
				</C>
			</C>
			<C Type="LoxCaption" V="175" U="60000000-0000-0003-ffff504f94112233" Title="Permission groups" CaptionType="13" SubType="13">
				<C Type="UserGroup" V="175" U="{FAMILY}" Title="Family" Rights="129" User="{DANA},{CAROL}" UserGroups="" LEd="562300000"/>
			</C>
		</C>
"#
    )
}

/// examples/configs/pool.Loxone with the notification and the users, CRLF
/// like the file.
fn fixture() -> String {
    let base = include_str!("../examples/configs/pool.Loxone");
    let anchor = "\t\t<C Type=\"VirtualInCaption\"";
    assert!(base.contains(anchor));
    base.replacen(
        anchor,
        &format!("{}{anchor}", tree().replace('\n', "\r\n")),
        1,
    )
}

fn parse(xml: &str) -> LoxoneDoc {
    LoxoneDoc::parse(xml.as_bytes()).unwrap()
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

const EXTERNS: &str = "extern frost = Notification(title: \"Frost\")\n\
                       extern cover = VirtualIn(title: \"Abdeckung offen\")\n";

fn compile_src(base: &str, src: &str, lock: &mut Lockfile) -> loxone_config::Result<String> {
    let m = Module::parse(&format!("{EXTERNS}\n{src}"))?;
    let out = compile(&parse(base), &m, lock, &opts())?;
    Ok(String::from_utf8(out.to_bytes()).unwrap())
}

fn compiled(src: &str) -> String {
    compile_src(&fixture(), src, &mut Lockfile::new()).unwrap()
}

fn error(src: &str) -> String {
    match Module::parse(&format!("{EXTERNS}\n{src}")) {
        Err(e) => e.to_string(),
        Ok(m) => match compile(&parse(&fixture()), &m, &mut Lockfile::new(), &opts()) {
            Err(e) => e.to_string(),
            Ok(_) => panic!("compile must fail: {src}"),
        },
    }
}

/// The serialized `<C Type="…">` element of `block_type`, up to its first
/// closing tag (the elements here hold no nested `<C>`).
fn element(xml: &str, block_type: &str) -> String {
    let start = xml
        .find(&format!("<C Type=\"{block_type}\""))
        .unwrap_or_else(|| panic!("no {block_type}"));
    let end = start + xml[start..].find("</C>").unwrap();
    xml[start..end + 4].to_string()
}

/// The serialized `<C>` element with `uuid` (no nested `<C>`).
fn element_of(xml: &str, uuid: &str) -> String {
    let at = xml.find(&format!("U=\"{uuid}\"")).unwrap();
    let open = xml[..at].rfind("<C ").unwrap();
    let close = at + xml[at..].find("</C>").unwrap();
    xml[open..close + 4].to_string()
}

fn attr(el: &str, name: &str) -> Option<String> {
    let pat = format!(" {name}=\"");
    let i = el.find(&pat)? + pat.len();
    Some(el[i..i + el[i..].find('"')?].to_string())
}

const MESSAGE: &str = "warn = OutputRefLM(\n\
                       \tmirrors: frost,\n\
                       \tAI: cover.Q,\n\
                       \tOn: \"Abdeckung offen\",\n\
                       \tOff: \"Abdeckung zu\",\n\
                       \tMinimumTime: 1h,\n\
                       )\n";

#[test]
fn output_ref_lm_is_minted_as_loxone_config_places_it() {
    let out = compiled(MESSAGE);
    let el = element(&out, "OutputRefLM");
    assert_eq!(attr(&el, "Ref").as_deref(), Some(NOTIFICATION));
    assert_eq!(attr(&el, "Title").as_deref(), Some("Frost"));
    assert_eq!(attr(&el, "Cl").as_deref(), Some("0,0,0"));
    assert_eq!(attr(&el, "Nio").as_deref(), Some("2"));
    // A freshly placed OutputRefLM has neither (oracle session 15).
    assert_eq!(attr(&el, "WF"), None, "{el}");
    assert_eq!(attr(&el, "LinkRefType"), None, "{el}");
    let keys: Vec<&str> = el
        .match_indices("<Co K=\"")
        .map(|(i, _)| {
            let k = &el[i + 7..];
            &k[..k.find('"').unwrap()]
        })
        .collect();
    assert_eq!(keys, ["AI", "AQ"]);
    assert!(el.contains("<In Input="), "AI is wired: {el}");
    // The message sits behind the connectors, durations in milliseconds.
    assert!(
        el.contains(&format!(
            "<LoggerMailer RefNotification=\"{NOTIFICATION}\" On=\"Abdeckung offen\" \
             Off=\"Abdeckung zu\" MinimumTime=\"3600000\" NL=\"1\"/>\r\n"
        )),
        "{el}"
    );
}

#[test]
fn output_ref_lm_defaults_follow_loxone_config() {
    // No Off text: no Off attribute; no MinimumTime: the GUI's 5 s.
    let out = compiled("warn = OutputRefLM(mirrors: frost, AI: cover.Q, On: \"offen\")\n");
    let el = element(&out, "OutputRefLM");
    assert!(
        el.contains(&format!(
            "<LoggerMailer RefNotification=\"{NOTIFICATION}\" On=\"offen\" \
             MinimumTime=\"5000\" NL=\"1\"/>"
        )),
        "{el}"
    );
    // Seconds as a number work too.
    let out = compiled("warn = OutputRefLM(mirrors: frost, AI: cover.Q, MinimumTime: 90)\n");
    assert_eq!(
        attr(&element(&out, "OutputRefLM"), "MinimumTime").as_deref(),
        Some("90000")
    );
}

#[test]
fn output_ref_lm_recompiles_to_the_same_bytes() {
    let mut lock = Lockfile::new();
    let once = compile_src(&fixture(), MESSAGE, &mut lock).unwrap();
    let twice = compile_src(&once, MESSAGE, &mut lock).unwrap();
    assert!(once == twice, "second compile changed the bytes");
}

#[test]
fn message_texts_are_part_of_the_semantics() {
    let mut lock = Lockfile::new();
    let a = compile_src(&fixture(), MESSAGE, &mut lock).unwrap();
    let b = compile_src(&a, &MESSAGE.replace("Abdeckung zu", "wieder zu"), &mut lock).unwrap();
    let d = diff(&parse(&a), &parse(&b));
    let changes: Vec<(&str, Option<&str>, Option<&str>)> = d
        .param_changes
        .iter()
        .map(|c| (c.port_key.as_str(), c.from.as_deref(), c.to.as_deref()))
        .collect();
    assert_eq!(
        changes,
        [("message Off", Some("Abdeckung zu"), Some("wieder zu"))]
    );
    // The v5 fingerprint sees the text, v4 (recorded before D51) does not.
    assert_ne!(
        semantic_fingerprint(&parse(&a)),
        semantic_fingerprint(&parse(&b))
    );
    assert_eq!(
        semantic_fingerprint_v4(&parse(&a)),
        semantic_fingerprint_v4(&parse(&b))
    );
}

#[test]
fn output_ref_lm_is_checked() {
    let e = error("warn = OutputRefLM(mirrors: cover, AI: cover.Q)\n");
    assert!(e.contains("mirrors a push notification"), "{e}");
    let e = error("warn = OutputRefLM(mirrors: frost, AI: cover.Q, MinimumTime: \"soon\")\n");
    assert!(e.contains("takes a duration"), "{e}");
    let e = error("x = And(mirrors: frost)\n");
    assert!(e.contains("InputRef/OutputRef/OutputRefLM only"), "{e}");
}

// ── recipients (D51) ────────────────────────────────────────────────────────

fn recipients(body: &str) -> String {
    format!("recipients frost\n{body}end\n")
}

#[test]
fn recipients_declarations_are_canonical() {
    let src = format!(
        "{EXTERNS}\n\
         recipients frost # Frostwarnung\n\
         \t# die Familie\n\
         \tuser \"dana\"\n\
         \tgroup \"Family\" # alle\n\
         end\n"
    );
    let m = Module::parse(&src).unwrap();
    assert_eq!(m.to_text(), src);
    let grants: Vec<String> = m
        .access_decls()
        .next()
        .unwrap()
        .grants()
        .map(|g| g.to_string())
        .collect();
    assert_eq!(grants, [r#"user "dana""#, r#"group "Family""#]);
    // `recipients` names a declaration only in front of a slug.
    let m = Module::parse("extern recipients = Switch(title: \"Freigabe\")\nrecipients.On = 1\n")
        .unwrap();
    assert_eq!(m.externs().next().unwrap().slug, "recipients");
}

#[test]
fn recipients_declarations_are_checked() {
    let e = error(&recipients("\tuser \"dana\": \"Always\"\n"));
    assert!(e.contains("a recipient takes no schedule"), "{e}");
    let e = error(&recipients("\tuser dana\n"));
    assert!(e.contains("expected `user \"<title>\"`"), "{e}");
    let e = error("recipients cover\nend\n");
    assert!(e.contains("declared as `VirtualIn`"), "{e}");
    let e = error(&format!("{}{}", recipients(""), recipients("")));
    assert!(e.contains("declared twice"), "{e}");
    let e = error(&recipients("\tuser \"dana\"\n\tuser \"dana\"\n"));
    assert!(e.contains("user \"dana\" is listed twice"), "{e}");
    let e = error(&recipients("\tuser \"dahna\"\n\tuser \"carol\"\n"));
    assert!(e.contains("no user titled \"dahna\""), "{e}");
    // An `access` body still needs its schedule.
    let e = error("extern door = NfcCodeTouch(title: \"x\")\naccess door\n\tuser \"dana\"\nend\n");
    assert!(e.contains("name the user and the access schedule"), "{e}");
}

#[test]
fn recipients_grant_appends_act_and_counts_it() {
    let base = fixture();
    let out = compiled(&recipients("\tuser \"dana\"\n\tuser \"carol\"\n"));
    // dana: appended behind the right she had, ACT="3" as Config 17.2
    // writes it, NUM counts both.
    assert!(
        element_of(&out, DANA).contains(&format!(
            "<PH NUM=\"2\">\r\n\t\t\t\t\t\t<PRM REF=\"{SWITCH}\" ACT=\"3\"/>\r\n\
             \t\t\t\t\t\t<PRM REF=\"{NOTIFICATION}\" ACT=\"3\"/>\r\n\t\t\t\t\t</PH>"
        )),
        "{}",
        element_of(&out, DANA)
    );
    // carol already gets it: her older ACT="1" stays.
    assert_eq!(element_of(&out, CAROL), element_of(&base, CAROL));
    // A group gets a list of its own.
    let out = compiled(&recipients("\tuser \"carol\"\n\tgroup \"Family\"\n"));
    assert!(
        element_of(&out, FAMILY).contains(&format!(
            "<PH NUM=\"1\">\r\n\t\t\t\t\t\t<PRM REF=\"{NOTIFICATION}\" ACT=\"3\"/>\r\n"
        )),
        "{}",
        element_of(&out, FAMILY)
    );
    // The users section of the diff shows the new recipient.
    let out = compiled(&recipients("\tuser \"dana\"\n\tuser \"carol\"\n"));
    let d = diff(&parse(&base), &parse(&out));
    assert!(d.logic_is_empty());
    let lines: Vec<String> = d.access.iter().map(|c| c.to_string()).collect();
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(
        lines[0].contains("dana") && lines[0].contains("Frost"),
        "{lines:?}"
    );
}

#[test]
fn recipients_matching_the_base_reproduce_its_bytes() {
    let base = fixture();
    let out = compiled(&recipients("\tuser \"carol\"\n"));
    assert!(out == base, "compiled bytes differ from the base");
}

#[test]
fn recipients_revoke_what_the_declaration_leaves_out() {
    let base = fixture();
    // carol's only right goes, and with it her list.
    let out = compiled(&recipients(""));
    let carol = element_of(&out, CAROL);
    assert!(!carol.contains("<PH"), "{carol}");
    assert!(!carol.contains(NOTIFICATION), "{carol}");
    // Granting and revoking again returns to the base.
    let mut lock = Lockfile::new();
    let granted = compile_src(
        &base,
        &recipients("\tuser \"dana\"\n\tuser \"carol\"\n"),
        &mut lock,
    )
    .unwrap();
    let back = compile_src(&granted, &recipients("\tuser \"carol\"\n"), &mut lock).unwrap();
    assert!(back == base, "revoking did not restore the base");
}
