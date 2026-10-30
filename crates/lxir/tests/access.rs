//! Users, groups and access rights: the model (`lxir::access`), the
//! users/access section of the semantic diff, `permissions.bin`
//! (`lxir::permissions`), and the IR's `access` declaration (D49).
//!
//! The fixture adds a synthetic *Users and Permissions* tree to
//! examples/configs/pool.Loxone in memory, shaped like Loxone Config writes
//! it: alice (an NFC tag, an access code, a password; access to the Code
//! Touch `Front door` under `Always`), bob (no credentials), the groups
//! `Residents` (both), `Nobody` and `Full access`, the access schedules
//! `Always` and `Workdays`, and a page with the Code Touch block. Every
//! hash, salt and UUID is made up.

use lxir::LoxoneDoc;
use lxir::access::{AccessModel, Change, PrincipalKind, Subject};
use lxir::diff::{
    diff, semantic_fingerprint, semantic_fingerprint_v1, semantic_fingerprint_v2,
    semantic_fingerprint_v3,
};

const ALICE: &str = "50000001-0000-0010-ffff504f94112233";
const BOB: &str = "50000002-0000-0020-ffff504f94112233";
const RESIDENTS: &str = "50000003-0000-0030-ffff504f94112233";
const FULL_ACCESS: &str = "50000005-0000-0050-ffff504f94112233";
const ALWAYS: &str = "50000006-0000-0060-ffff504f94112233";
const WORKDAYS: &str = "50000007-0000-0070-ffff504f94112233";
const FRONT_DOOR: &str = "50000009-0000-0090-ffff504f94112233";
const SWITCH: &str = "30000005-0000-0050-ffff504f94112233";

// Made-up credentials, long enough to look like the real ones.
const TAG_ID: &str = "0A1B2C3D4E5F60718293A4B5C6D7E8F90A1B2C3D4E5F60718293A4B5C6D7E8F9";
const TAG_ID_2: &str = "F9E8D7C6B5A4938271605F4E3D2C1B0AF9E8D7C6B5A4938271605F4E3D2C1B0A";
const CODE: &str = "00112233445566778899AABBCCDDEEFF";
const CODE_2: &str = "FFEEDDCCBBAA99887766554433221100";
const PW_HASH: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const PW_HASH_2: &str = "2222222222222222222222222222222222222222222222222222222222222222";
const PW_SALT: &str = "SyntheticSaltSyntheticSalt012345";

fn users_tree() -> String {
    format!(
        r#"		<C Type="LoxCaption" V="175" U="50000000-0000-0001-ffff504f94112233" Title="Users and Permissions" CaptionType="20" SubType="20">
			<C Type="UserCaption" V="175" U="50000000-0000-0002-ffff504f94112233" Title="User" CaptionType="30" SubType="30">
				<C Type="User" V="175" U="{ALICE}" Title="alice" UsR="-1" LastEdit="562500000" NFCArr="{TAG_ID}" NFCNArr="Key fob" CodeArr="{CODE}" iBtnArr="" EAIdArr="">
					<HP H="{PW_HASH}" Sc="2" S="{PW_SALT}" Alg="1"/>
					<HP H="" Sc="-2" S="" Alg="1"/>
					<PH NUM="2">
						<PRM REF="{FRONT_DOOR}" TIM="{ALWAYS}" ACT="3"/>
						<PRM REF="{SWITCH}" ACT="3"/>
					</PH>
				</C>
				<C Type="User" V="175" U="{BOB}" Title="bob" UsR="129" LastEdit="562400000" NFCArr="" NFCNArr="" CodeArr="" iBtnArr="" EAIdArr="">
					<HP H="" Sc="-2" S="" Alg="1"/>
					<HP H="" Sc="-2" S="" Alg="1"/>
				</C>
			</C>
			<C Type="LoxCaption" V="175" U="50000000-0000-0003-ffff504f94112233" Title="Permission groups" CaptionType="13" SubType="13">
				<C Type="UserGroup" V="175" U="{RESIDENTS}" Title="Residents" Rights="129" User="{ALICE},{BOB}" UserGroups="" LEd="562300000">
					<PH NUM="1">
						<PRM REF="{SWITCH}" ACT="3"/>
					</PH>
				</C>
				<C Type="UserGroup" V="175" U="50000004-0000-0040-ffff504f94112233" Title="Nobody" UgType="3" User="" UserGroups=""/>
				<C Type="UserGroup" V="175" U="{FULL_ACCESS}" Title="Full access" Rights="-1" UgType="4" User="{ALICE}" UserGroups=""/>
			</C>
			<C Type="LoxCaption" V="175" U="50000000-0000-0004-ffff504f94112233" Title="Access Schedules" CaptionType="11" SubType="11">
				<C Type="SwitchingTimer" V="175" U="{ALWAYS}" Title="Always" M="3" FIX="1"/>
				<C Type="SwitchingTimer" V="175" U="{WORKDAYS}" Title="Workdays" M="3"/>
			</C>
		</C>
		<C Type="Page" V="175" U="50000008-0000-0080-ffff504f94112233" Title="Eingang">
			<C Type="NfcCodeTouch" V="175" U="{FRONT_DOOR}" Title="Front door" Nio="2" SpCdUuid="">
				<Co K="Q1" U="50000009-0000-0091-00ffaaaaaaaa5009"/>
				<Co K="TQ" U="50000009-0000-0092-01ffaaaaaaaa5009"/>
				<IoData Visu="true" Cr="30000000-0000-000b-ffff504f94112233" Pr="30000000-0000-0009-ffff504f94112233"/>
			</C>
		</C>
"#
    )
}

/// examples/configs/pool.Loxone with the users tree, CRLF like the file.
fn fixture() -> String {
    let base = include_str!("../examples/configs/pool.Loxone");
    let anchor = "\t\t<C Type=\"VirtualInCaption\"";
    assert!(base.contains(anchor));
    let tree = users_tree().replace('\n', "\r\n");
    base.replacen(anchor, &format!("{tree}{anchor}"), 1)
}

fn parse(xml: &str) -> LoxoneDoc {
    LoxoneDoc::parse(xml.as_bytes()).unwrap()
}

/// `xml` with `from` replaced once — the change must apply.
fn edit(xml: &str, from: &str, to: &str) -> String {
    assert!(xml.contains(from), "fixture lacks {from:?}");
    xml.replacen(from, to, 1)
}

fn changes_between(a: &str, b: &str) -> Vec<lxir::access::AccessChange> {
    let d = diff(&parse(a), &parse(b));
    d.access
}

#[test]
fn model_reads_users_groups_schedules_and_rights() {
    let m = AccessModel::read(&parse(&fixture()));

    let titles: Vec<_> = m.users.iter().map(|u| u.title.as_deref()).collect();
    assert_eq!(titles, [Some("alice"), Some("bob")]);
    let alice = m.user(ALICE).unwrap();
    assert_eq!(alice.last_edit.as_deref(), Some("562500000"));
    assert_eq!(alice.rights.as_deref(), Some("-1"));
    assert_eq!(alice.nfc_tags.len(), 1);
    assert_eq!(alice.nfc_tags[0].name.as_deref(), Some("Key fob"));
    assert_eq!(alice.codes.len(), 1);
    assert!(alice.ibuttons.is_empty() && alice.external_ids.is_empty());
    assert!(alice.password.is_some());
    assert!(alice.visu_password.is_none(), "an empty H= is no password");
    assert!(alice.has_tags_or_codes());
    assert_eq!(alice.permissions.len(), 2);
    assert_eq!(alice.permissions[0].target, FRONT_DOOR);
    assert_eq!(alice.permissions[0].schedule.as_deref(), Some(ALWAYS));
    assert_eq!(alice.permissions[0].ui.as_deref(), Some("3"));
    assert_eq!(alice.permissions[1].schedule, None);
    let bob = m.user(BOB).unwrap();
    assert!(!bob.has_tags_or_codes());
    assert!(bob.password.is_none() && bob.permissions.is_empty());

    assert_eq!(m.groups.len(), 3);
    let residents = m.group(RESIDENTS).unwrap();
    assert_eq!(residents.members, [ALICE, BOB]);
    assert_eq!(residents.last_edit.as_deref(), Some("562300000"));
    assert_eq!(m.group(FULL_ACCESS).unwrap().kind.as_deref(), Some("4"));
    let alice_groups: Vec<_> = m
        .groups_of_transitive(ALICE)
        .iter()
        .map(|g| g.uuid.as_str())
        .collect();
    assert_eq!(alice_groups, [RESIDENTS, FULL_ACCESS]);

    let schedules: Vec<_> = m.schedules.iter().map(|s| s.title.as_deref()).collect();
    assert_eq!(schedules, [Some("Always"), Some("Workdays")]);
    let rights = m.access_rights();
    assert_eq!(rights.len(), 1);
    assert_eq!(rights[0].kind, PrincipalKind::User);
    assert_eq!(rights[0].principal, ALICE);
    assert_eq!(rights[0].target, FRONT_DOOR);
    assert_eq!(rights[0].schedule, ALWAYS);
}

#[test]
fn several_tags_split_on_semicolons() {
    let xml = edit(
        &fixture(),
        &format!(r#"NFCArr="{TAG_ID}" NFCNArr="Key fob""#),
        &format!(r#"NFCArr="{TAG_ID};{TAG_ID_2}" NFCNArr="Key fob;Phone""#),
    );
    let m = AccessModel::read(&parse(&xml));
    let names: Vec<_> = m
        .user(ALICE)
        .unwrap()
        .nfc_tags
        .iter()
        .map(|t| t.name.as_deref())
        .collect();
    assert_eq!(names, [Some("Key fob"), Some("Phone")]);
}

#[test]
fn the_same_document_has_no_access_changes() {
    let xml = fixture();
    let d = diff(&parse(&xml), &parse(&xml));
    assert!(d.access.is_empty());
    assert!(d.is_empty());
}

#[test]
fn users_and_groups_live_in_the_access_section() {
    let a = fixture();
    // carol is new, bob is gone, Residents is renamed.
    let carol = r#"				<C Type="User" V="175" U="5000000a-0000-00a0-ffff504f94112233" Title="carol" UsR="129" LastEdit="562600000" NFCArr="" NFCNArr="" CodeArr="" iBtnArr="" EAIdArr="">
					<HP H="" Sc="-2" S="" Alg="1"/>
					<HP H="" Sc="-2" S="" Alg="1"/>
				</C>
"#;
    let bob_start = a
        .find(&format!(r#"<C Type="User" V="175" U="{BOB}""#))
        .unwrap();
    let line_start = a[..bob_start].rfind('\n').unwrap() + 1;
    let bob_end = line_start + a[line_start..].find("\t\t\t\t</C>\r\n").unwrap() + 8;
    let mut b = a.clone();
    b.replace_range(line_start..bob_end, &carol.replace('\n', "\r\n"));
    let b = edit(&b, r#"Title="Residents""#, r#"Title="Housemates""#);

    let d = diff(&parse(&a), &parse(&b));

    assert!(d.logic_is_empty(), "users are not logic: {d:?}");
    assert!(!d.is_empty());
    assert!(d.added.is_empty() && d.removed.is_empty() && d.renamed.is_empty());
    let lines: Vec<String> = d.access.iter().map(|c| c.to_string()).collect();
    assert!(
        lines.contains(&r#"user "carol": added"#.into()),
        "{lines:?}"
    );
    assert!(
        lines.contains(&r#"user "bob": removed"#.into()),
        "{lines:?}"
    );
    assert!(
        lines.contains(
            &r#"group "Housemates": renamed Some("Residents") -> Some("Housemates")"#.into()
        ),
        "{lines:?}"
    );
    // bob left with his removal; Residents lost him as a member, which the
    // removal already says.
    assert!(!lines.iter().any(|l| l.contains("left group")), "{lines:?}");

    // `lxir diff --summary` lists the same lines and counts them.
    let summary = lxir::diff::summary(&parse(&a), &parse(&b), &d);
    assert!(
        summary.starts_with(&format!(
            "change {} users/access entries\n\nUsers and access:\n",
            lines.len()
        )),
        "{summary}"
    );
    assert!(summary.contains("\n- user \"carol\": added\n"), "{summary}");
}

#[test]
fn membership_changes_name_the_group() {
    let a = fixture();
    let b = edit(
        &a,
        &format!(r#"User="{ALICE},{BOB}""#),
        &format!(r#"User="{ALICE}""#),
    );
    let b = edit(
        &b,
        &format!(r#"UgType="4" User="{ALICE}""#),
        &format!(r#"UgType="4" User="{ALICE},{BOB}""#),
    );

    let changes = changes_between(&a, &b);

    let bob: Vec<_> = changes.iter().filter(|c| c.uuid == BOB).collect();
    assert_eq!(bob.len(), 2, "{changes:?}");
    assert!(matches!(&bob[0].change, Change::Joined { group } if group.uuid == FULL_ACCESS));
    assert!(matches!(&bob[1].change, Change::Left { group } if group.uuid == RESIDENTS));
    assert_eq!(bob[1].to_string(), r#"user "bob": left group "Residents""#);
}

#[test]
fn credentials_show_names_counts_and_fingerprints_never_secrets() {
    let a = fixture();
    // alice swaps her tag for a phone, gets a new code and a new password.
    let b = edit(
        &a,
        &format!(r#"NFCArr="{TAG_ID}" NFCNArr="Key fob" CodeArr="{CODE}""#),
        &format!(r#"NFCArr="{TAG_ID_2}" NFCNArr="Phone" CodeArr="{CODE_2}""#),
    );
    let b = edit(&b, PW_HASH, PW_HASH_2);
    let b = edit(&b, r#"LastEdit="562500000""#, r#"LastEdit="562522922""#);

    let d = diff(&parse(&a), &parse(&b));
    let lines: Vec<String> = d.access.iter().map(|c| c.to_string()).collect();

    assert!(
        d.access
            .iter()
            .all(|c| c.subject == Subject::User && c.uuid == ALICE)
    );
    assert!(
        lines
            .iter()
            .any(|l| l.starts_with(r#"user "alice": NFC tag "Phone" added ["#)),
        "{lines:?}"
    );
    assert!(
        lines
            .iter()
            .any(|l| l.starts_with(r#"user "alice": NFC tag "Key fob" removed ["#)),
        "{lines:?}"
    );
    assert!(lines.contains(&r#"user "alice": access codes changed (1)"#.into()));
    assert!(lines.contains(&r#"user "alice": password changed"#.into()));
    assert!(lines.contains(
        &r#"user "alice": last edit 562500000 (2026-10-29 10:00) -> 562522922 (2026-10-29 16:22)"#
            .into()
    ));
    // Whatever form the change takes, the stored values stay out of it.
    let json = serde_json::to_string(&d).unwrap();
    let debug = format!("{d:?}");
    let text = lines.join("\n");
    for secret in [TAG_ID, TAG_ID_2, CODE, CODE_2, PW_HASH, PW_HASH_2, PW_SALT] {
        for (form, shown) in [("json", &json), ("debug", &debug), ("text", &text)] {
            assert!(!shown.contains(secret), "{form} shows a secret");
            assert!(
                !shown.contains(&secret[..16]),
                "{form} shows part of a secret"
            );
        }
    }
}

#[test]
fn a_renamed_tag_keeps_its_fingerprint() {
    let a = fixture();
    let b = edit(&a, r#"NFCNArr="Key fob""#, r#"NFCNArr="Blue fob""#);

    let changes = changes_between(&a, &b);

    assert_eq!(changes.len(), 1, "{changes:?}");
    let Change::NfcTagRenamed {
        from,
        to,
        fingerprint,
    } = &changes[0].change
    else {
        panic!("{changes:?}");
    };
    assert_eq!(
        (from.as_deref(), to.as_deref()),
        (Some("Key fob"), Some("Blue fob"))
    );
    assert_eq!(fingerprint.len(), 8);
}

#[test]
fn passwords_set_and_removed() {
    let a = fixture();
    let b = edit(
        &a,
        &format!(r#"<HP H="{PW_HASH}" Sc="2" S="{PW_SALT}" Alg="1"/>"#),
        r#"<HP H="" Sc="-2" S="" Alg="1"/>"#,
    );
    let changes = changes_between(&a, &b);
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].to_string(), r#"user "alice": password removed"#);
    let back = changes_between(&b, &a);
    assert_eq!(back[0].to_string(), r#"user "alice": password set"#);
}

#[test]
fn access_rights_granted_revoked_and_rescheduled() {
    let a = fixture();
    // alice now only on workdays; bob gets access; the group's UI right on
    // the switch goes and one on the Code Touch comes.
    let b = edit(
        &a,
        &format!(r#"<PRM REF="{FRONT_DOOR}" TIM="{ALWAYS}" ACT="3"/>"#),
        &format!(r#"<PRM REF="{FRONT_DOOR}" TIM="{WORKDAYS}" ACT="3"/>"#),
    );
    let bob_hp = "<HP H=\"\" Sc=\"-2\" S=\"\" Alg=\"1\"/>\r\n\t\t\t\t\t<HP H=\"\" Sc=\"-2\" S=\"\" Alg=\"1\"/>\r\n\t\t\t\t</C>";
    let b = edit(
        &b,
        bob_hp,
        &bob_hp.replace(
            "\t\t\t\t</C>",
            &format!(
                "\t\t\t\t\t<PH NUM=\"1\">\r\n\t\t\t\t\t\t<PRM REF=\"{FRONT_DOOR}\" TIM=\"{ALWAYS}\"/>\r\n\t\t\t\t\t</PH>\r\n\t\t\t\t</C>"
            ),
        ),
    );
    let b = edit(
        &b,
        &format!("<PH NUM=\"1\">\r\n\t\t\t\t\t\t<PRM REF=\"{SWITCH}\" ACT=\"3\"/>"),
        &format!("<PH NUM=\"1\">\r\n\t\t\t\t\t\t<PRM REF=\"{FRONT_DOOR}\" ACT=\"3\"/>"),
    );

    let d = diff(&parse(&a), &parse(&b));
    let lines: Vec<String> = d.access.iter().map(|c| c.to_string()).collect();

    assert!(d.logic_is_empty());
    assert_eq!(
        lines,
        [
            r#"user "alice": access schedule on NfcCodeTouch "Front door": "Always" -> "Workdays""#,
            r#"user "bob": access to NfcCodeTouch "Front door" granted, schedule "Always""#,
            r#"group "Residents": UI rights 1 added ("Front door"), 1 removed ("Freigabe")"#,
        ]
    );
    let revoked = changes_between(&b, &a);
    assert!(
        revoked.iter().any(|c| c.to_string()
            == r#"user "bob": access to NfcCodeTouch "Front door" revoked (schedule "Always")"#),
        "{revoked:?}"
    );
}

#[test]
fn group_rights_type_and_last_edit() {
    let a = fixture();
    let b = edit(&a, r#"Rights="129""#, r#"Rights="161""#);
    let b = edit(&b, r#"LEd="562300000""#, r#"LEd="562300100""#);
    let lines: Vec<String> = changes_between(&a, &b)
        .iter()
        .map(|c| c.to_string())
        .collect();
    assert_eq!(
        lines,
        [
            r#"group "Residents": last edit 562300000 (2026-10-27 02:26) -> 562300100 (2026-10-27 02:28)"#,
            r#"group "Residents": rights 129 -> 161"#,
        ]
    );
}

#[test]
fn named_codes_report_their_code_and_validity() {
    let caption = format!(
        "\t\t\t<C Type=\"KeycodeCaption\" V=\"175\" U=\"5000000b-0000-00b0-ffff504f94112233\" Title=\"Access codes\">\r\n\
         \t\t\t\t<C Type=\"Keycode\" V=\"175\" U=\"5000000c-0000-00c0-ffff504f94112233\" Title=\"Guests\" Cde=\"{CODE}\" StartT=\"\" EndT=\"\"/>\r\n\
         \t\t\t</C>\r\n"
    );
    let anchor = "\t\t\t<C Type=\"LoxAIR\"";
    let a = edit(&fixture(), anchor, &format!("{caption}{anchor}"));
    let b = edit(
        &a,
        &format!(r#"Cde="{CODE}" StartT="" EndT="""#),
        &format!(r#"Cde="{CODE_2}" StartT="" EndT="562600000""#),
    );

    let d = diff(&parse(&a), &parse(&b));
    let lines: Vec<String> = d.access.iter().map(|c| c.to_string()).collect();

    assert_eq!(
        lines,
        [
            r#"access code "Guests": code changed"#,
            r#"access code "Guests": valid until none -> 562600000"#,
        ]
    );
    assert!(d.logic_is_empty());
    assert!(!format!("{d:?}{}", serde_json::to_string(&d).unwrap()).contains(CODE_2));
}

#[test]
fn json_names_the_change_and_its_subject() {
    let a = fixture();
    let b = edit(&a, r#"Title="bob""#, r#"Title="robert""#);
    let d = diff(&parse(&a), &parse(&b));
    let json = serde_json::to_value(&d).unwrap();
    assert_eq!(
        json["access"],
        serde_json::json!([{
            "subject": "user",
            "uuid": BOB,
            "title": "robert",
            "change": "renamed",
            "from": "bob",
            "to": "robert",
        }])
    );
}

/// Credentials, membership and rights stay out of the semantic
/// fingerprints: a lockfile recorded before the users section existed must
/// not read as drift. Users and groups themselves are objects there (UUID,
/// type, title), as they always were.
#[test]
fn fingerprints_ignore_credentials_membership_and_rights() {
    let a = fixture();
    let b = edit(&a, PW_HASH, PW_HASH_2);
    let b = edit(
        &b,
        &format!(r#"NFCArr="{TAG_ID}""#),
        &format!(r#"NFCArr="{TAG_ID_2}""#),
    );
    let b = edit(&b, &format!(r#"User="{ALICE},{BOB}""#), r#"User="""#);
    let b = edit(
        &b,
        &format!(r#"TIM="{ALWAYS}" ACT="3""#),
        &format!(r#"TIM="{WORKDAYS}" ACT="3""#),
    );
    let b = edit(&b, r#"LastEdit="562500000""#, r#"LastEdit="562522922""#);
    let (da, db) = (parse(&a), parse(&b));
    assert!(!diff(&da, &db).access.is_empty());
    for fingerprint in [
        semantic_fingerprint,
        semantic_fingerprint_v3,
        semantic_fingerprint_v2,
        semantic_fingerprint_v1,
    ] {
        assert_eq!(fingerprint(&da), fingerprint(&db));
    }
}

// ── permissions.bin ─────────────────────────────────────────────────────────

const RIGHT_GROUP: &str = "50000010-0000-0100-ffff504f94112233";
const TASKS: &str = "50000011-0000-0110-ffff504f94112233";

/// The fixture with what `permissions.bin` needs besides users: a task
/// list and a right group for the room "Garten".
fn fixture_with_right_group() -> String {
    let xml = edit(
        &fixture(),
        r#"Title="Garten"/>"#,
        &format!(r#"Title="Garten" RGR="{RIGHT_GROUP}"/>"#),
    );
    let xml = edit(
        &xml,
        "\t\t\t\t<C Type=\"UserGroup\" V=\"175\" U=\"50000004",
        &format!(
            "\t\t\t\t<C Type=\"RightGroup\" V=\"175\" U=\"{RIGHT_GROUP}\" Title=\"Garten\" GT=\"1\" MG=\"\"/>\r\n\
             \t\t\t\t<C Type=\"UserGroup\" V=\"175\" U=\"50000004"
        ),
    );
    edit(
        &xml,
        "\t\t<C Type=\"Page\" V=\"175\" U=\"50000008",
        &format!(
            "\t\t<C Type=\"TaskCaption\" V=\"175\" U=\"{TASKS}\" Title=\"Tasks\"/>\r\n\
             \t\t<C Type=\"Page\" V=\"175\" U=\"50000008"
        ),
    )
}

/// Global state `n` of the synthetic LoxAPP3.json.
fn global(n: u32) -> String {
    format!("60000000-0000-{n:04x}-ffff504f94112233")
}

/// A LoxAPP3.json for the fixture: the Code Touch in "Garten", the switch
/// without room or category, one automatic rule. With a BOM, as Loxone
/// writes it.
fn loxapp3() -> String {
    let keys = [
        "operatingMode",
        "sunrise",
        "sunset",
        "pastTasks",
        "plannedTasks",
        "notifications",
        "modifications",
        "favColorSequences",
        "favColors",
        "miniserverTime",
        "liveSearch",
        "userSettings",
        "userSettingsTs",
        "cloudservice",
        "trustVersion",
        "propsVersion",
        "hasInternet",
    ];
    let globals: Vec<String> = keys
        .iter()
        .zip(1..)
        .map(|(k, n)| format!(r#""{k}": "{}""#, global(n)))
        .collect();
    let (s1, s2, s3) = (global(0x101), global(0x102), global(0x103));
    let (ap, ap2) = (global(0x201), global(0x202));
    let controls = format!(
        r#""{SWITCH}": {{"type": "Switch", "states": {{"active": "{s1}"}}}},
           "U:{FRONT_DOOR}": {{"type": "NfcCodeTouch", "room": "x",
               "states": {{"history": "{s2}", "codeDate": ["{s3}", "{s2}"]}}}}"#
    );
    let autopilot =
        format!(r#""rule": {{"uuidAction": "{ap}", "states": {{"changed": "{ap2}"}}}}"#);
    format!(
        "\u{feff}{{\"lastModified\": \"2026-10-29 16:22:00\", \"globalStates\": {{{}}}, \
         \"modes\": {{}}, \"controls\": {{{controls}}}, \"autopilot\": {{{autopilot}}}}}",
        globals.join(", ")
    )
}

#[test]
fn permissions_bin_generated_from_program_and_loxapp3() {
    use lxir::LoxUuid;
    use lxir::permissions::{Group, encode, generate, parse as parse_bin};
    let u = |s: &str| LoxUuid::parse(s).unwrap();
    let g = |n: u32| u(&global(n));
    let doc = parse(&fixture_with_right_group());

    let groups = generate(&doc, &loxapp3()).unwrap();

    let nobody = || vec![vec![]];
    let group = |object, states, principals, fd| Group {
        object,
        states,
        principals,
        fd,
    };
    let expected = vec![
        group(g(1), vec![], nobody(), true),
        group(g(2), vec![], nobody(), true),
        group(g(3), vec![], nobody(), true),
        group(u(TASKS), vec![g(4), g(5)], nobody(), true),
        group(g(7), (6..=16).map(g).collect(), nobody(), true),
        group(g(17), vec![g(17)], nobody(), true),
        // Users and groups with a right on the switch; no room, no category.
        group(
            u(SWITCH),
            vec![g(0x101)],
            vec![vec![u(ALICE), u(RESIDENTS)]],
            true,
        ),
        // alice by her right, the room's right group; states deduplicated.
        group(
            u(FRONT_DOOR),
            vec![g(0x102), g(0x103)],
            vec![vec![u(ALICE), u(RIGHT_GROUP)]],
            false,
        ),
        // Automatic rules: users with UsR bit 0x40 (alice: -1; bob: 129).
        group(
            g(0x201),
            vec![g(0x201), g(0x202)],
            vec![vec![u(ALICE)]],
            false,
        ),
    ];
    assert_eq!(groups, expected);
    let bytes = encode(&groups);
    assert_eq!(parse_bin(&bytes).unwrap(), expected);
}

#[test]
fn permissions_assignments_follow_the_rights() {
    use lxir::permissions::assignments;
    let a = fixture_with_right_group();
    // bob gets access to the Code Touch.
    let b = edit(
        &a,
        "\t\t\t\t\t<HP H=\"\" Sc=\"-2\" S=\"\" Alg=\"1\"/>\r\n\t\t\t\t</C>",
        &format!(
            "\t\t\t\t\t<HP H=\"\" Sc=\"-2\" S=\"\" Alg=\"1\"/>\r\n\
             \t\t\t\t\t<PH NUM=\"1\">\r\n\
             \t\t\t\t\t\t<PRM REF=\"{FRONT_DOOR}\" TIM=\"{ALWAYS}\"/>\r\n\
             \t\t\t\t\t</PH>\r\n\t\t\t\t</C>"
        ),
    );
    let (before, after) = (assignments(&parse(&a)), assignments(&parse(&b)));
    assert_eq!(
        before[FRONT_DOOR],
        [vec![ALICE.to_string(), RIGHT_GROUP.to_string()]]
    );
    assert_eq!(
        after[FRONT_DOOR],
        [vec![
            ALICE.to_string(),
            BOB.to_string(),
            RIGHT_GROUP.to_string()
        ]]
    );
    // A new schedule is no new assignment.
    let c = edit(
        &a,
        &format!(r#"TIM="{ALWAYS}" ACT="3""#),
        &format!(r#"TIM="{WORKDAYS}" ACT="3""#),
    );
    assert_eq!(assignments(&parse(&c)), before);
}

// ── access declarations (D49) ───────────────────────────────────────────────

use lxir::Lockfile;
use lxir::ir::{CompileOptions, LintKind, Module, compile, lint_access};
use lxir::uuid::parse_serial;

const DOOR: &str =
    "extern front_door = NfcCodeTouch(uuid: \"50000009-0000-0090-ffff504f94112233\")\n";

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

/// `body` as the access declaration of the front door, compiled against
/// `base`.
fn compile_access(base: &str, body: &str) -> lxir::Result<String> {
    let m = Module::parse(&format!("{DOOR}\naccess front_door\n{body}end\n"))?;
    let out = compile(&parse(base), &m, &mut Lockfile::new(), &opts())?;
    Ok(String::from_utf8(out.to_bytes()).unwrap())
}

/// The serialized `<C>` element with `uuid`, up to its closing tag (or the
/// self-closing tag itself).
fn element_of(xml: &str, uuid: &str) -> String {
    let at = xml.find(&format!("U=\"{uuid}\"")).unwrap();
    let open = xml[..at].rfind("<C ").unwrap();
    let tag_end = at + xml[at..].find('>').unwrap();
    if xml[..tag_end].ends_with('/') {
        return xml[open..=tag_end].to_string();
    }
    // Nested `<C>` would need counting; users and groups hold none.
    let close = tag_end + xml[tag_end..].find("</C>").unwrap();
    xml[open..close + 4].to_string()
}

#[test]
fn access_declarations_are_canonical() {
    let src = format!(
        "{DOOR}extern back_door = NfcCodeTouch(title: \"Back door\")\n\
         \n\
         access front_door # main entrance\n\
         \t# residents first\n\
         \tuser \"alice\": \"Always\"\n\
         \tgroup \"Residents\": \"Workdays\" # weekdays only\n\
         end # door\n\
         \n\
         access back_door\n\
         end\n"
    );
    let m = Module::parse(&src).unwrap();
    assert_eq!(m.to_text(), src);
    let a = m.access_decls().next().unwrap();
    let grants: Vec<String> = a.grants().map(|g| g.to_string()).collect();
    assert_eq!(
        grants,
        [
            r#"user "alice": "Always""#,
            r#"group "Residents": "Workdays""#
        ]
    );
    // `access` names a declaration only in front of a slug.
    let m = Module::parse(
        "extern access = Switch(title: \"Freigabe\")\nlatch = Switch(On: access.Q)\naccess.On = 1\n",
    )
    .unwrap();
    assert_eq!(m.externs().next().unwrap().slug, "access");
}

#[test]
fn access_declarations_are_checked() {
    let err = |src: &str| Module::parse(src).unwrap_err().to_string();
    assert!(
        err(&format!("{DOOR}access front_door\n\tuser \"alice\"\nend\n"))
            .contains("name the user and the access schedule by their titles (`user \"<title>\": \"<schedule>\"`)")
    );
    assert!(err(&format!("{DOOR}access front_door\n")).contains("unclosed `access front_door`"));
    assert!(err("access\n").contains("expected `access <slug>`"));
    assert!(
        err("template t(x: Switch)\naccess front_door\nend\nend\n")
            .contains("not allowed inside a template body")
    );
    assert!(err("access front_door\nend\n").contains("undeclared slug `front_door`"));
    assert!(
        err("extern front_door = Switch(title: \"Freigabe\")\naccess front_door\nend\n")
            .contains("declared as `Switch`")
    );
    assert!(
        err(&format!(
            "{DOOR}access front_door\nend\naccess front_door\nend\n"
        ))
        .contains("declared twice")
    );
    assert!(
        err(&format!(
            "{DOOR}access front_door\n\tuser \"alice\": \"Always\"\n\tuser \"alice\": \"Workdays\"\nend\n"
        ))
        .contains("user \"alice\" is listed twice")
    );
}

/// A declaration that states what the config holds writes nothing.
#[test]
fn access_matching_the_base_reproduces_its_bytes() {
    let base = fixture();
    let out = compile_access(&base, "\tuser \"alice\": \"Always\"\n").unwrap();
    assert!(out == base, "compiled bytes differ from the base");
}

#[test]
fn access_grants_append_a_right_and_count_it() {
    let base = fixture();
    let out = compile_access(
        &base,
        "\tuser \"alice\": \"Always\"\n\tuser \"bob\": \"Workdays\"\n\tgroup \"Residents\": \"Always\"\n",
    )
    .unwrap();
    // bob had no list: it follows his passwords.
    assert_eq!(
        element_of(&out, BOB),
        format!(
            "<C Type=\"User\" V=\"175\" U=\"{BOB}\" Title=\"bob\" UsR=\"129\" LastEdit=\"562400000\" \
             NFCArr=\"\" NFCNArr=\"\" CodeArr=\"\" iBtnArr=\"\" EAIdArr=\"\">\r\n\
             \t\t\t\t\t<HP H=\"\" Sc=\"-2\" S=\"\" Alg=\"1\"/>\r\n\
             \t\t\t\t\t<HP H=\"\" Sc=\"-2\" S=\"\" Alg=\"1\"/>\r\n\
             \t\t\t\t\t<PH NUM=\"1\">\r\n\
             \t\t\t\t\t\t<PRM REF=\"{FRONT_DOOR}\" TIM=\"{WORKDAYS}\"/>\r\n\
             \t\t\t\t\t</PH>\r\n\
             \t\t\t\t</C>"
        )
    );
    // Residents: appended behind the right it had, NUM counts both.
    let residents = element_of(&out, RESIDENTS);
    assert!(
        residents.contains(&format!(
            "<PH NUM=\"2\">\r\n\t\t\t\t\t\t<PRM REF=\"{SWITCH}\" ACT=\"3\"/>\r\n\
             \t\t\t\t\t\t<PRM REF=\"{FRONT_DOOR}\" TIM=\"{ALWAYS}\"/>\r\n"
        )),
        "{residents}"
    );
    // Nothing else changed: not alice, not bob's LastEdit, not the diff's
    // logic view.
    assert_eq!(element_of(&out, ALICE), element_of(&base, ALICE));
    let d = diff(&parse(&base), &parse(&out));
    assert!(d.logic_is_empty());
    let lines: Vec<String> = d.access.iter().map(|c| c.to_string()).collect();
    assert_eq!(lines.len(), 2, "{lines:?}");
}

#[test]
fn access_reschedules_in_place_and_keeps_the_app_right() {
    let base = fixture();
    let out = compile_access(&base, "\tuser \"alice\": \"Workdays\"\n").unwrap();
    assert_eq!(
        out,
        edit(
            &base,
            &format!(r#"TIM="{ALWAYS}" ACT="3""#),
            &format!(r#"TIM="{WORKDAYS}" ACT="3""#)
        )
    );
    // A right to operate the block in the app (ACT only) gains TIM between
    // REF and ACT, the order Loxone Config writes.
    let base = edit(
        &base,
        &format!(
            "<PRM REF=\"{SWITCH}\" ACT=\"3\"/>\r\n\t\t\t\t\t</PH>\r\n\t\t\t\t</C>\r\n\t\t\t\t<C Type=\"UserGroup\" V=\"175\" U=\"50000004"
        ),
        &format!(
            "<PRM REF=\"{FRONT_DOOR}\" ACT=\"3\"/>\r\n\t\t\t\t\t</PH>\r\n\t\t\t\t</C>\r\n\t\t\t\t<C Type=\"UserGroup\" V=\"175\" U=\"50000004"
        ),
    );
    let out = compile_access(
        &base,
        "\tuser \"alice\": \"Always\"\n\tgroup \"Residents\": \"Workdays\"\n",
    )
    .unwrap();
    assert_eq!(
        out,
        edit(
            &base,
            &format!(r#"<PRM REF="{FRONT_DOOR}" ACT="3"/>"#),
            &format!(r#"<PRM REF="{FRONT_DOOR}" TIM="{WORKDAYS}" ACT="3"/>"#)
        )
    );
}

#[test]
fn access_revokes_what_the_declaration_leaves_out() {
    let base = fixture();
    // alice keeps the right to operate the block in the app.
    let out = compile_access(&base, "").unwrap();
    assert_eq!(
        out,
        edit(
            &base,
            &format!(r#"<PRM REF="{FRONT_DOOR}" TIM="{ALWAYS}" ACT="3"/>"#),
            &format!(r#"<PRM REF="{FRONT_DOOR}" ACT="3"/>"#)
        )
    );
    // A right that was only access goes, NUM follows; an emptied list goes
    // too, and a user left without children closes itself.
    let granted = compile_access(
        &base,
        "\tuser \"alice\": \"Always\"\n\tuser \"bob\": \"Workdays\"\n\tgroup \"Residents\": \"Always\"\n",
    )
    .unwrap();
    let revoked = compile_access(&granted, "\tuser \"alice\": \"Always\"\n").unwrap();
    assert_eq!(revoked, base);
    let no_passwords = edit(
        &granted,
        "\t\t\t\t\t<HP H=\"\" Sc=\"-2\" S=\"\" Alg=\"1\"/>\r\n\t\t\t\t\t<HP H=\"\" Sc=\"-2\" S=\"\" Alg=\"1\"/>\r\n\t\t\t\t\t<PH NUM=\"1\">",
        "\t\t\t\t\t<PH NUM=\"1\">",
    );
    let out = compile_access(&no_passwords, "\tuser \"alice\": \"Always\"\n").unwrap();
    assert!(
        element_of(&out, BOB).ends_with(r#"EAIdArr=""/>"#),
        "{}",
        element_of(&out, BOB)
    );
}

#[test]
fn access_titles_must_name_exactly_one_object() {
    let base = fixture();
    let err = |base: &str, body: &str| compile_access(base, body).unwrap_err().to_string();
    let e = err(&base, "\tuser \"alise\": \"Always\"\n");
    assert!(e.contains("no user titled \"alise\""), "{e}");
    assert!(e.contains("alice"), "suggests the near miss: {e}");
    let e = err(&base, "\tgroup \"alice\": \"Always\"\n");
    assert!(e.contains("no group titled \"alice\""), "{e}");
    let e = err(&base, "\tuser \"alice\": \"Weekends\"\n");
    assert!(e.contains("no access schedule titled \"Weekends\""), "{e}");
    let twice = edit(&base, r#"Title="bob""#, r#"Title="alice""#);
    let e = err(&twice, "\tuser \"alice\": \"Always\"\n");
    assert!(e.contains("2 users are titled \"alice\""), "{e}");
    assert!(e.contains(ALICE) && e.contains(BOB), "{e}");
    // Nothing is written when a title fails.
    let m = Module::parse(&format!(
        "{DOOR}\naccess front_door\n\tuser \"bob\": \"Always\"\n\tuser \"carol\": \"Always\"\nend\n"
    ))
    .unwrap();
    assert!(compile(&parse(&base), &m, &mut Lockfile::new(), &opts()).is_err());
}

#[test]
fn lint_finds_tags_that_open_nothing() {
    let base = fixture();
    // alice has access; bob gets a code but no right.
    let with_code = edit(
        &base,
        r#"Title="bob" UsR="129" LastEdit="562400000" NFCArr="" NFCNArr="" CodeArr="""#,
        &format!(
            r#"Title="bob" UsR="129" LastEdit="562400000" NFCArr="" NFCNArr="" CodeArr="{CODE_2}""#
        ),
    );
    let findings = lint_access(&parse(&with_code));
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert_eq!(findings[0].kind, LintKind::NoAccessRight);
    assert_eq!(findings[0].slug, "bob");
    assert!(findings[0].detail.contains("an access code"));
    // Access through a group counts.
    let via_group = compile_access(
        &with_code,
        "\tuser \"alice\": \"Always\"\n\tgroup \"Residents\": \"Always\"\n",
    )
    .unwrap();
    assert!(lint_access(&parse(&via_group)).is_empty());
    // Full access does not: it never includes access.
    let revoked = compile_access(&with_code, "").unwrap();
    let names: Vec<String> = lint_access(&parse(&revoked))
        .into_iter()
        .map(|f| f.slug)
        .collect();
    assert_eq!(names, ["alice", "bob"]);
}

#[test]
fn decompile_lifts_access_rights() {
    use lxir::ir::{DecompileOptions, decompile};
    let base = edit(
        &fixture(),
        &format!(
            "<PRM REF=\"{SWITCH}\" ACT=\"3\"/>\r\n\t\t\t\t\t</PH>\r\n\t\t\t\t</C>\r\n\t\t\t\t<C Type=\"UserGroup\" V=\"175\" U=\"50000004"
        ),
        &format!(
            "<PRM REF=\"{SWITCH}\" ACT=\"3\"/>\r\n\t\t\t\t\t\t<PRM REF=\"{FRONT_DOOR}\" TIM=\"{WORKDAYS}\"/>\r\n\t\t\t\t\t</PH>\r\n\t\t\t\t</C>\r\n\t\t\t\t<C Type=\"UserGroup\" V=\"175\" U=\"50000004"
        ),
    );
    let base = edit(&base, "<PH NUM=\"1\">", "<PH NUM=\"2\">");
    let (m, _) = decompile(&parse(&base), &DecompileOptions::default()).unwrap();
    let text = m.to_text();
    assert!(
        text.contains(
            "access front_door\n\tuser \"alice\": \"Always\"\n\tgroup \"Residents\": \"Workdays\"\nend\n"
        ),
        "{text}"
    );
    // The Code Touch is a managed block since the house-control batch.
    assert!(text.contains("front_door = NfcCodeTouch("), "{text}");

    // Adopted, the declaration compiles back to the same rights.
    let (m, mut lock, _) = lxir::ir::adopt(&parse(&base)).unwrap();
    assert_eq!(m.access_decls().count(), 1);
    let out = compile(&parse(&base), &m, &mut lock, &opts()).unwrap();
    assert!(diff(&parse(&base), &out).access.is_empty());

    // A title that names two users cannot be a reference.
    let twice = edit(&base, r#"Title="bob""#, r#"Title="alice""#);
    let (m, _) = decompile(&parse(&twice), &DecompileOptions::default()).unwrap();
    assert!(m.access_decls().next().is_none());
    assert!(
        m.to_text()
            .contains("# access front_door: not lifted — 2 users are titled \"alice\"\n")
    );
}
