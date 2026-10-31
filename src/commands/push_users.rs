//! `lox config push`: the user changes an upload would discard.
//!
//! The Miniserver keeps changes to its user management (users, groups, NFC
//! tags, access codes — made in the app or with `lox user`) apart from the
//! program file until the program is saved again, and drops them when it
//! loads an uploaded program file. The push compares the live users with the
//! users of the program file it compiles against; the drift check compares
//! downloads, so it sees user changes saved into the program file (kept as
//! live), never the ones still pending.

use crate::loxone_xml::{AccessModel, parse_access_model};
use crate::usermgmt::User;
use anyhow::{Context, Result};
use serde::Serialize;
use std::collections::BTreeSet;
use std::fmt;

/// A user whose live state the program file does not hold.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct PendingUserChange {
    pub user: String,
    pub user_uuid: String,
    /// What differs, live against the program file
    pub changes: Vec<String>,
}

/// The push refuses: an upload would discard these user changes.
#[derive(Debug)]
pub(crate) struct UserChangesPending(pub Vec<PendingUserChange>);

impl fmt::Display for UserChangesPending {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let users: Vec<String> = self
            .0
            .iter()
            .map(|p| format!("{} ({})", p.user, p.changes.join("; ")))
            .collect();
        write!(
            f,
            "Users changed on the Miniserver since its program file was saved, and loading the \
             upload would delete that: {}. Load the program from the Miniserver in Loxone Config and \
             save it back, so the program file holds the changes (the push then compiles against \
             it and keeps them), or pass --discard-custom-changes to delete them.",
            users.join(", ")
        )
    }
}

impl std::error::Error for UserChangesPending {}

/// The hint when the live users cannot be read.
pub(crate) const UNREADABLE: &str = "Cannot read the Miniserver's users to check for user \
     changes, which loading the upload would delete. Fix that, or pass --discard-custom-changes \
     to push without the check";

/// The live users' changes the program file `xml` does not hold.
pub(crate) fn pending_in(xml: &[u8], live: &[User]) -> Result<Vec<PendingUserChange>> {
    let model = parse_access_model(xml).context("Cannot read the users of the live config")?;
    Ok(pending(&model, live))
}

/// Per user: existence, name, last edit, groups, NFC tags (count and names)
/// and the number of access codes.
pub(crate) fn pending(model: &AccessModel, live: &[User]) -> Vec<PendingUserChange> {
    let mut out = Vec::new();
    for user in live {
        // Trust users come from another Miniserver, not from this program file.
        if user.trust_member.is_some() {
            continue;
        }
        let changes = match model.user(&user.uuid) {
            None => vec!["created live, not in the program file".to_string()],
            Some(cu) => {
                let mut changes = Vec::new();
                if cu.name != user.name {
                    changes.push(format!("name: '{}' in the program file", cu.name));
                }
                if let (Some(file), Some(live)) = (cu.last_edit, user.last_edit)
                    && file != live
                {
                    changes.push(format!(
                        "last edit: {} live, {} in the program file",
                        loxone_client::time::lox_utc_timestamp_to_string(live),
                        loxone_client::time::lox_utc_timestamp_to_string(file)
                    ));
                }
                if let Some(groups) = group_change(model, user) {
                    changes.push(groups);
                }
                let live_names: Vec<&str> = user.nfc_tags.iter().map(|t| t.name.as_str()).collect();
                if live_names.len() != cu.nfc_tags
                    || !same_tag_names(&cu.nfc_names, cu.nfc_tags, &live_names)
                {
                    let file_names: Vec<&str> = cu.nfc_names.iter().map(String::as_str).collect();
                    changes.push(format!(
                        "NFC tags: {} live, {} in the program file",
                        tags(live_names.len(), &live_names),
                        tags(cu.nfc_tags, &file_names)
                    ));
                }
                if user.access_codes != cu.access_codes as i64 {
                    changes.push(format!(
                        "access codes: {} live, {} in the program file",
                        user.access_codes, cu.access_codes
                    ));
                }
                changes
            }
        };
        if !changes.is_empty() {
            out.push(PendingUserChange {
                user: user.name.clone(),
                user_uuid: user.uuid.clone(),
                changes,
            });
        }
    }
    for cu in &model.users {
        if !live.iter().any(|u| u.uuid == cu.uuid) {
            out.push(PendingUserChange {
                user: cu.name.clone(),
                user_uuid: cu.uuid.clone(),
                changes: vec!["deleted live, still in the program file".to_string()],
            });
        }
    }
    out
}

/// "groups: added to Family live, removed from Guests live" when the live
/// membership differs.
fn group_change(model: &AccessModel, user: &User) -> Option<String> {
    let live: BTreeSet<&str> = user.groups.iter().map(|g| g.uuid.as_str()).collect();
    let file: BTreeSet<&str> = model
        .groups_of(&user.uuid)
        .map(|g| g.uuid.as_str())
        .collect();
    if live == file {
        return None;
    }
    let mut parts: Vec<String> = user
        .groups
        .iter()
        .filter(|g| !file.contains(g.uuid.as_str()))
        .map(|g| format!("added to {} live", g.name))
        .collect();
    parts.extend(
        model
            .groups_of(&user.uuid)
            .filter(|g| !live.contains(g.uuid.as_str()))
            .map(|g| format!("removed from {} live", g.name)),
    );
    Some(format!("groups: {}", parts.join(", ")))
}

/// With one name per tag the names must match; when the program file's
/// names could not be split (the separator for several tags is
/// undocumented), each live name must appear in them.
fn same_tag_names(file: &[String], file_tags: usize, live: &[&str]) -> bool {
    if file.len() == file_tags {
        let mut a: Vec<&str> = file.iter().map(String::as_str).collect();
        let mut b = live.to_vec();
        a.sort_unstable();
        b.sort_unstable();
        a == b
    } else {
        let joined = file.join(" ");
        live.iter().all(|n| joined.contains(n))
    }
}

fn tags(count: usize, names: &[&str]) -> String {
    if names.is_empty() {
        count.to_string()
    } else {
        format!("{} ({})", count, names.join(", "))
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::usermgmt::{GroupRef, NfcTag};

    pub const ALICE: &str = "0f3e1a20-0001-1a2b-ffff112233445566";
    pub const BOB: &str = "0f3e1a20-0002-1a2b-ffff112233445566";
    pub const CAROL: &str = "0f3e1a20-0003-1a2b-ffff112233445566";
    pub const FAMILY: &str = "0f3e1a20-0010-1a2b-ffff112233445566";
    pub const GUESTS: &str = "0f3e1a20-0011-1a2b-ffff112233445566";

    /// Users and groups as a program file holds them: alice has one tag
    /// and is in Family, bob has an access code, carol nothing.
    pub fn users_xml() -> String {
        format!(
            r#"<C Type="UserCaption" Title="Users">
<C Type="User" U="{ALICE}" Title="alice" NFCArr="{hash}" NFCNArr="alice phone" CodeArr="" LastEdit="562000000"/>
<C Type="User" U="{BOB}" Title="bob" NFCArr="" NFCNArr="" CodeArr="{hash}" LastEdit="562000100"/>
<C Type="User" U="{CAROL}" Title="carol" LastEdit="562000200"/>
</C>
<C Type="UserGroupCaption" Title="Groups">
<C Type="UserGroup" U="{FAMILY}" Title="Family" User="{ALICE}" UserGroups=""/>
<C Type="UserGroup" U="{GUESTS}" Title="Guests" User="" UserGroups=""/>
</C>"#,
            hash = "ab".repeat(32)
        )
    }

    fn model() -> AccessModel {
        let xml = format!(
            r#"<?xml version="1.0"?><ControlList><C Type="Document" Title="Test">{}</C></ControlList>"#,
            users_xml()
        );
        parse_access_model(xml.as_bytes()).unwrap()
    }

    pub fn user(uuid: &str, name: &str, last_edit: i64) -> User {
        User {
            name: name.into(),
            uuid: uuid.into(),
            description: String::new(),
            state: 0,
            state_text: "enabled".into(),
            is_admin: false,
            last_edit: Some(last_edit),
            valid_from: None,
            valid_until: None,
            groups: Vec::new(),
            nfc_tags: Vec::new(),
            access_codes: 0,
            trust_member: None,
        }
    }

    /// The live users exactly as `users_xml` holds them.
    pub fn live_users() -> Vec<User> {
        let mut alice = user(ALICE, "alice", 562000000);
        alice.groups.push(GroupRef {
            name: "Family".into(),
            uuid: FAMILY.into(),
        });
        alice.nfc_tags.push(NfcTag {
            name: "alice phone".into(),
            id: "04 A1 B2 C3".into(),
        });
        let mut bob = user(BOB, "bob", 562000100);
        bob.access_codes = 1;
        vec![alice, bob, user(CAROL, "carol", 562000200)]
    }

    #[test]
    fn users_as_the_program_file_holds_them_are_no_change() {
        assert!(pending(&model(), &live_users()).is_empty());
    }

    #[test]
    fn a_tag_learned_live_is_a_pending_change() {
        let mut live = live_users();
        live[0].last_edit = Some(562000500);
        live[0].nfc_tags.push(NfcTag {
            name: "alice card".into(),
            id: "04 D4 E5 F6".into(),
        });

        let p = pending(&model(), &live);

        assert_eq!(p.len(), 1);
        assert_eq!(p[0].user, "alice");
        assert_eq!(p[0].user_uuid, ALICE);
        assert_eq!(p[0].changes.len(), 2, "{:?}", p[0].changes);
        assert!(
            p[0].changes[0].starts_with("last edit: "),
            "{:?}",
            p[0].changes
        );
        assert_eq!(
            p[0].changes[1],
            "NFC tags: 2 (alice phone, alice card) live, 1 (alice phone) in the program file"
        );
    }

    #[test]
    fn names_groups_and_codes_are_compared() {
        let mut live = live_users();
        live[0].nfc_tags[0].name = "alice watch".into();
        live[1].name = "robert".into();
        live[1].access_codes = 2;
        live[2].groups.push(GroupRef {
            name: "Guests".into(),
            uuid: GUESTS.into(),
        });
        live[0].groups.clear();

        let p = pending(&model(), &live);

        let by = |name: &str| &p.iter().find(|c| c.user == name).unwrap().changes;
        assert_eq!(
            by("alice"),
            &[
                "groups: removed from Family live",
                "NFC tags: 1 (alice watch) live, 1 (alice phone) in the program file"
            ]
        );
        assert_eq!(
            by("robert"),
            &[
                "name: 'bob' in the program file",
                "access codes: 2 live, 1 in the program file"
            ]
        );
        assert_eq!(by("carol"), &["groups: added to Guests live"]);
    }

    #[test]
    fn users_created_or_deleted_live_are_pending_changes() {
        let mut live = live_users();
        live.retain(|u| u.uuid != CAROL);
        live.push(user(
            "0f3e1a20-0004-1a2b-ffff112233445566",
            "dave",
            562000900,
        ));
        let mut trusted = user("0f3e1a20-0005-1a2b-ffff112233445566", "erin", 1);
        trusted.trust_member = Some("other Miniserver".into());
        live.push(trusted);

        let p = pending(&model(), &live);

        let summary: Vec<(&str, &str)> = p
            .iter()
            .map(|c| (c.user.as_str(), c.changes[0].as_str()))
            .collect();
        assert_eq!(
            summary,
            [
                ("dave", "created live, not in the program file"),
                ("carol", "deleted live, still in the program file")
            ]
        );
    }

    #[test]
    fn tag_names_the_file_cannot_split_are_matched_loosely() {
        let names = ["alice phone + card".to_string()];
        assert!(same_tag_names(&names, 2, &["alice phone", "card"]));
        assert!(!same_tag_names(&names, 2, &["alice phone", "watch"]));
        assert!(same_tag_names(
            &["b".to_string(), "a".to_string()],
            2,
            &["a", "b"]
        ));
    }

    #[test]
    fn the_refusal_names_users_changes_and_both_ways_out() {
        let e = UserChangesPending(vec![PendingUserChange {
            user: "alice".into(),
            user_uuid: ALICE.into(),
            changes: vec!["access codes: 1 live, 0 in the program file".into()],
        }])
        .to_string();
        assert!(
            e.contains("alice (access codes: 1 live, 0 in the program file)"),
            "{e}"
        );
        assert!(e.contains("Loxone Config") && e.contains("--discard-custom-changes"));
    }
}
