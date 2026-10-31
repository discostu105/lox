//! Users, groups and access rights: the *Users and Permissions* tree
//! of a `.Loxone` document, read as a model.
//!
//! Loxone Config, the app and the Miniserver's user management own all of
//! it. lxir reads it for the users/access section of the semantic diff
//! ([`changes`], carried in [`crate::diff::DocDiff::access`]). The format
//! and its evidence are in docs/loxone-format.md (*Users, groups and access
//! rights*):
//!
//! - a `User` carries its credentials as attributes (`NFCArr=`/`NFCNArr=`
//!   hashed tag IDs and their names, `CodeArr=`, `iBtnArr=`, `EAIdArr=`),
//!   its passwords as `<HP>` children and its rights as `<PH><PRM …/></PH>`;
//! - a `UserGroup` lists its members (`User=`, `UserGroups=`) and carries
//!   rights the same way;
//! - a `<PRM REF=… ACT=…/>` is a UI right on the object `REF=` names, a
//!   `<PRM REF=… TIM=…/>` an access right (*Zutritt*) under the access
//!   schedule `TIM=` names; one `PRM` may carry both;
//! - `Keycode`/`NFCID` objects are the named access codes and NFC tags of
//!   a Code Touch (`Cde=` hashed).
//!
//! Even a hash is not for printing: credentials are held as [`Secret`],
//! whose only printable form is a short fingerprint.

use crate::doc::{LoxoneDoc, ObjectSummary};
use crate::xml::Element;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// A stored credential: a password hash with its salt, a hashed NFC tag
/// ID or access code. It compares by value, and its only printable form
/// is [`Secret::fingerprint`]; `Debug` prints that too, so a model or a
/// diff can be dumped without leaking anything.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Secret(String);

impl Secret {
    fn new(value: String) -> Self {
        Secret(value)
    }

    /// Eight hex digits of a SHA-256 over the stored value: enough to tell
    /// two credentials apart in a diff, too short to confirm a guess.
    pub fn fingerprint(&self) -> String {
        let digest = crate::lock::sha256_hex(format!("lxir-secret\0{}", self.0).as_bytes());
        digest[..8].to_string()
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Secret({})", self.fingerprint())
    }
}

/// One `<PRM>` of a `<PH>` list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Permission {
    /// `REF=`: the object the right is on.
    pub target: String,
    /// `TIM=`: the access schedule (a `SwitchingTimer`) of an access right.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schedule: Option<String>,
    /// `ACT=`: the UI right (observed: `3`, rarely `1`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ui: Option<String>,
}

/// One NFC tag of a user: the name from `NFCNArr=` and the hashed ID from
/// `NFCArr=` at the same position. Either may be missing when the two
/// lists have different lengths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NfcTag {
    pub name: Option<String>,
    pub id: Option<Secret>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct User {
    pub uuid: String,
    pub title: Option<String>,
    /// Where the `<C>` element is (see [`ObjectSummary::path`]).
    pub path: Vec<usize>,
    /// `LastEdit=`: Loxone time (seconds since 2009-01-01) of the last edit,
    /// what the user management API reports as `lastEdit`.
    pub last_edit: Option<String>,
    /// `UsR=`: the user rights bit mask (`-1` = all).
    pub rights: Option<String>,
    pub nfc_tags: Vec<NfcTag>,
    /// `CodeArr=`: access codes.
    pub codes: Vec<Secret>,
    /// `iBtnArr=`: iButtons.
    pub ibuttons: Vec<Secret>,
    /// `EAIdArr=`: external access IDs.
    pub external_ids: Vec<Secret>,
    /// The password: the first `<HP>` (hash, salt, algorithm), or a legacy
    /// `Pwd=` attribute.
    pub password: Option<Secret>,
    /// The visualization password: the second `<HP>`, or a legacy `Pwd2=`.
    pub visu_password: Option<Secret>,
    pub permissions: Vec<Permission>,
}

impl User {
    /// Whether the user can identify at a Code Touch: an NFC tag or a code.
    pub fn has_tags_or_codes(&self) -> bool {
        !self.nfc_tags.is_empty() || !self.codes.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    pub uuid: String,
    pub title: Option<String>,
    pub path: Vec<usize>,
    /// `User=`: the member users.
    pub members: Vec<String>,
    /// `UserGroups=`: the member groups.
    pub subgroups: Vec<String>,
    /// `Rights=`: the group rights bit mask (`-1` = all).
    pub rights: Option<String>,
    /// `UgType=`: `3` the "nobody" group, `4` full access; absent on
    /// ordinary groups.
    pub kind: Option<String>,
    /// `LEd=`: Loxone time of the last edit.
    pub last_edit: Option<String>,
    pub permissions: Vec<Permission>,
}

/// A named access code (`Keycode`) or NFC tag (`NFCID`) of a Code Touch:
/// in its periphery folders or a child of the `NfcCodeTouch` block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamedCredential {
    pub uuid: String,
    pub title: Option<String>,
    pub path: Vec<usize>,
    /// `Keycode` or `NFCID`.
    pub block_type: String,
    /// `Cde=`: the hashed code or tag ID.
    pub secret: Option<Secret>,
    /// `StartT=`/`EndT=`: the validity window, as Loxone Config writes it.
    pub valid_from: Option<String>,
    pub valid_until: Option<String>,
}

/// An access schedule: a `SwitchingTimer` object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Schedule {
    pub uuid: String,
    pub title: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PrincipalKind {
    User,
    Group,
}

impl PrincipalKind {
    pub fn as_str(self) -> &'static str {
        match self {
            PrincipalKind::User => "user",
            PrincipalKind::Group => "group",
        }
    }
}

/// An access right as the model sees it, from whichever side holds it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AccessRight<'a> {
    pub kind: PrincipalKind,
    pub principal: &'a str,
    /// The object the right is on (a Code Touch block).
    pub target: &'a str,
    pub schedule: &'a str,
}

/// Users, groups, named credentials and access schedules of one document,
/// each list in document order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AccessModel {
    pub users: Vec<User>,
    pub groups: Vec<Group>,
    pub credentials: Vec<NamedCredential>,
    pub schedules: Vec<Schedule>,
}

impl AccessModel {
    pub fn read(doc: &LoxoneDoc) -> AccessModel {
        let mut m = AccessModel::default();
        for o in doc.objects() {
            let Some(el) = doc.element_at(&o.path) else {
                continue;
            };
            match o.block_type.as_str() {
                "User" => m.users.push(read_user(&o, el)),
                "UserGroup" => m.groups.push(Group {
                    uuid: o.uuid.clone(),
                    title: o.title.clone(),
                    path: o.path.clone(),
                    members: uuid_list(el, "User"),
                    subgroups: uuid_list(el, "UserGroups"),
                    rights: attr(el, "Rights"),
                    kind: attr(el, "UgType"),
                    last_edit: attr(el, "LEd"),
                    permissions: permissions(el),
                }),
                "Keycode" | "NFCID" => m.credentials.push(NamedCredential {
                    uuid: o.uuid.clone(),
                    title: o.title.clone(),
                    path: o.path.clone(),
                    block_type: o.block_type.clone(),
                    secret: attr(el, "Cde").filter(|v| !v.is_empty()).map(Secret::new),
                    valid_from: attr(el, "StartT").filter(|v| !v.is_empty()),
                    valid_until: attr(el, "EndT").filter(|v| !v.is_empty()),
                }),
                "SwitchingTimer" => m.schedules.push(Schedule {
                    uuid: o.uuid.clone(),
                    title: o.title.clone(),
                }),
                _ => {}
            }
        }
        m
    }

    pub fn user(&self, uuid: &str) -> Option<&User> {
        self.users.iter().find(|u| u.uuid == uuid)
    }

    pub fn group(&self, uuid: &str) -> Option<&Group> {
        self.groups.iter().find(|g| g.uuid == uuid)
    }

    /// The groups that list `member` (a user or a group) directly.
    pub fn groups_of<'a>(&'a self, member: &'a str) -> impl Iterator<Item = &'a Group> + 'a {
        self.groups.iter().filter(move |g| {
            g.members.iter().any(|m| m == member) || g.subgroups.iter().any(|m| m == member)
        })
    }

    /// The groups `user` belongs to directly or through nested groups, in
    /// document order.
    pub fn groups_of_transitive(&self, user: &str) -> Vec<&Group> {
        let mut found: BTreeSet<&str> = BTreeSet::new();
        let mut queue = vec![user];
        while let Some(member) = queue.pop() {
            for g in self.groups_of(member) {
                if found.insert(&g.uuid) {
                    queue.push(&g.uuid);
                }
            }
        }
        self.groups
            .iter()
            .filter(|g| found.contains(g.uuid.as_str()))
            .collect()
    }

    /// Every access right (`TIM=`) in the document: users first, then
    /// groups, each in document and `<PH>` order.
    pub fn access_rights(&self) -> Vec<AccessRight<'_>> {
        let users = self
            .users
            .iter()
            .map(|u| (PrincipalKind::User, u.uuid.as_str(), &u.permissions));
        let groups = self
            .groups
            .iter()
            .map(|g| (PrincipalKind::Group, g.uuid.as_str(), &g.permissions));
        users
            .chain(groups)
            .flat_map(|(kind, principal, perms)| {
                perms.iter().filter_map(move |p| {
                    Some(AccessRight {
                        kind,
                        principal,
                        target: &p.target,
                        schedule: p.schedule.as_deref()?,
                    })
                })
            })
            .collect()
    }
}

fn attr(el: &Element, name: &str) -> Option<String> {
    el.attr_decoded(name).map(|v| v.into_owned())
}

/// A credential list attribute. Entries are separated by `;` — the one
/// multi-tag user in the corpus has `NFCNArr="a;b"` and two 64-digit
/// hashes in `NFCArr=` joined the same way. Other separators are
/// unverified (docs/loxone-format.md), so a value without `;` reads as one
/// entry, whatever its length.
fn list(el: &Element, name: &str) -> Vec<String> {
    attr(el, name)
        .map(|v| {
            v.split(';')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// A comma-separated UUID list attribute (`User=`, `UserGroups=`).
fn uuid_list(el: &Element, name: &str) -> Vec<String> {
    attr(el, name)
        .map(|v| {
            v.split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// The `<PRM>` entries of the object's `<PH>` child.
fn permissions(el: &Element) -> Vec<Permission> {
    el.child_elements()
        .filter(|c| c.name == "PH")
        .flat_map(|ph| ph.child_elements().filter(|c| c.name == "PRM"))
        .filter_map(|prm| {
            Some(Permission {
                target: attr(prm, "REF")?,
                schedule: attr(prm, "TIM"),
                ui: attr(prm, "ACT"),
            })
        })
        .collect()
}

fn read_user(o: &ObjectSummary, el: &Element) -> User {
    let names = list(el, "NFCNArr");
    let ids = list(el, "NFCArr");
    let nfc_tags = (0..names.len().max(ids.len()))
        .map(|i| NfcTag {
            name: names.get(i).cloned(),
            id: ids.get(i).cloned().map(Secret::new),
        })
        .collect();
    let secrets = |name: &str| list(el, name).into_iter().map(Secret::new).collect();
    // `<HP H= S= Sc= Alg=>`: the hash, its salt and the algorithm; `Sc=` is
    // the strength score the app shows and no part of the secret. An empty
    // `H=` means no password.
    let hps: Vec<Option<Secret>> = el
        .child_elements()
        .filter(|c| c.name == "HP")
        .map(|hp| {
            let h = attr(hp, "H").unwrap_or_default();
            (!h.is_empty()).then(|| {
                let s = attr(hp, "S").unwrap_or_default();
                let alg = attr(hp, "Alg").unwrap_or_default();
                Secret::new(format!("{h}\0{s}\0{alg}"))
            })
        })
        .collect();
    let legacy = |name: &str| attr(el, name).filter(|v| !v.is_empty()).map(Secret::new);
    User {
        uuid: o.uuid.clone(),
        title: o.title.clone(),
        path: o.path.clone(),
        last_edit: attr(el, "LastEdit"),
        rights: attr(el, "UsR"),
        nfc_tags,
        codes: secrets("CodeArr"),
        ibuttons: secrets("iBtnArr"),
        external_ids: secrets("EAIdArr"),
        password: hps.first().cloned().flatten().or_else(|| legacy("Pwd")),
        visu_password: hps.get(1).cloned().flatten().or_else(|| legacy("Pwd2")),
        permissions: permissions(el),
    }
}

// ── The users/access section of the semantic diff ───────────────────────────

/// What an [`AccessChange`] is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Subject {
    User,
    Group,
    /// A named access code (`Keycode`) of a Code Touch.
    AccessCode,
    /// A named NFC tag (`NFCID`) of a Code Touch.
    NfcTag,
}

impl Subject {
    pub fn as_str(self) -> &'static str {
        match self {
            Subject::User => "user",
            Subject::Group => "group",
            Subject::AccessCode => "access code",
            Subject::NfcTag => "NFC tag",
        }
    }
}

/// An object a change refers to, with its type and title resolved in the
/// document that has it (the new one first).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ObjRef {
    pub uuid: String,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub block_type: Option<String>,
    pub title: Option<String>,
}

impl ObjRef {
    /// `"Title"`, or the UUID when the object has no title or is unknown.
    pub fn label(&self) -> String {
        match &self.title {
            Some(t) => format!("{t:?}"),
            None => self.uuid.clone(),
        }
    }

    /// The label with the type in front: `NfcCodeTouch "Front door"`.
    pub fn typed_label(&self) -> String {
        match &self.block_type {
            Some(t) => format!("{t} {}", self.label()),
            None => self.label(),
        }
    }
}

/// One change in the users/access section.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AccessChange {
    pub subject: Subject,
    pub uuid: String,
    /// The subject's title: in the new document, or the old one when the
    /// subject was removed.
    pub title: Option<String>,
    #[serde(flatten)]
    pub change: Change,
}

/// What changed. Credentials show as counts, names and fingerprints only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "change", rename_all = "kebab-case")]
pub enum Change {
    Added,
    Removed,
    Renamed {
        from: Option<String>,
        to: Option<String>,
    },
    /// Became a direct member of a group.
    Joined {
        group: ObjRef,
    },
    Left {
        group: ObjRef,
    },
    NfcTagAdded {
        name: Option<String>,
        fingerprint: Option<String>,
    },
    NfcTagRemoved {
        name: Option<String>,
        fingerprint: Option<String>,
    },
    /// The same tag ID under another name.
    NfcTagRenamed {
        from: Option<String>,
        to: Option<String>,
        fingerprint: String,
    },
    /// Access codes (`CodeArr=`), iButtons (`iBtnArr=`) or external IDs
    /// (`EAIdArr=`) differ: their counts before and after.
    Credentials {
        what: &'static str,
        from: usize,
        to: usize,
    },
    /// `what`: `password` or `visualization password`.
    Password {
        what: &'static str,
        from_set: bool,
        to_set: bool,
    },
    /// A named code's or tag's `Cde=` changed.
    SecretChanged,
    LastEdit {
        from: Option<String>,
        to: Option<String>,
    },
    /// `rights` (`UsR=`/`Rights=`), `group type` (`UgType=`), `valid from`
    /// / `valid until` (`StartT=`/`EndT=`).
    Property {
        name: &'static str,
        from: Option<String>,
        to: Option<String>,
    },
    AccessGranted {
        block: ObjRef,
        schedule: ObjRef,
    },
    AccessRevoked {
        block: ObjRef,
        schedule: ObjRef,
    },
    AccessSchedule {
        block: ObjRef,
        from: ObjRef,
        to: ObjRef,
    },
    /// UI rights (`ACT=`), summarized per principal: the objects that
    /// gained one, lost one, or whose value changed.
    UiRights {
        added: Vec<ObjRef>,
        removed: Vec<ObjRef>,
        changed: Vec<ObjRef>,
    },
}

impl AccessChange {
    /// `+` added, `-` removed, `~` changed.
    pub fn sign(&self) -> char {
        match self.change {
            Change::Added => '+',
            Change::Removed => '-',
            _ => '~',
        }
    }

    /// `"alice"`, or the UUID when untitled.
    pub fn label(&self) -> String {
        match &self.title {
            Some(t) => format!("{t:?}"),
            None => self.uuid.clone(),
        }
    }
}

impl Change {
    /// The change as text, without its subject: `NFC tag "Phone" added`.
    pub fn describe(&self) -> String {
        match self {
            Change::Added => "added".into(),
            Change::Removed => "removed".into(),
            Change::Renamed { from, to } => format!("renamed {from:?} -> {to:?}"),
            Change::Joined { group } => format!("joined group {}", group.label()),
            Change::Left { group } => format!("left group {}", group.label()),
            Change::NfcTagAdded { name, fingerprint } => {
                format!("NFC tag {} added{}", tag_name(name), fp(fingerprint))
            }
            Change::NfcTagRemoved { name, fingerprint } => {
                format!("NFC tag {} removed{}", tag_name(name), fp(fingerprint))
            }
            Change::NfcTagRenamed {
                from,
                to,
                fingerprint,
            } => format!(
                "NFC tag renamed {} -> {} [{fingerprint}]",
                tag_name(from),
                tag_name(to)
            ),
            Change::Credentials { what, from, to } => {
                if from == to {
                    format!("{what} changed ({to})")
                } else {
                    format!("{what} {from} -> {to}")
                }
            }
            Change::Password {
                what,
                from_set,
                to_set,
            } => match (from_set, to_set) {
                (false, true) => format!("{what} set"),
                (true, false) => format!("{what} removed"),
                _ => format!("{what} changed"),
            },
            Change::SecretChanged => "code changed".into(),
            Change::LastEdit { from, to } => {
                format!("last edit {} -> {}", loxone_time(from), loxone_time(to))
            }
            Change::Property { name, from, to } => format!(
                "{name} {} -> {}",
                from.as_deref().unwrap_or("none"),
                to.as_deref().unwrap_or("none")
            ),
            Change::AccessGranted { block, schedule } => format!(
                "access to {} granted, schedule {}",
                block.typed_label(),
                schedule.label()
            ),
            Change::AccessRevoked { block, schedule } => format!(
                "access to {} revoked (schedule {})",
                block.typed_label(),
                schedule.label()
            ),
            Change::AccessSchedule { block, from, to } => format!(
                "access schedule on {}: {} -> {}",
                block.typed_label(),
                from.label(),
                to.label()
            ),
            Change::UiRights {
                added,
                removed,
                changed,
            } => {
                let mut parts = Vec::new();
                for (what, list) in [("added", added), ("removed", removed), ("changed", changed)] {
                    if !list.is_empty() {
                        parts.push(format!("{} {what} ({})", list.len(), some_labels(list)));
                    }
                }
                format!("UI rights {}", parts.join(", "))
            }
        }
    }
}

impl fmt::Display for AccessChange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} {}: {}",
            self.subject.as_str(),
            self.label(),
            self.change.describe()
        )
    }
}

fn tag_name(name: &Option<String>) -> String {
    match name {
        Some(n) => format!("{n:?}"),
        None => "(unnamed)".into(),
    }
}

fn fp(fingerprint: &Option<String>) -> String {
    fingerprint
        .as_ref()
        .map(|f| format!(" [{f}]"))
        .unwrap_or_default()
}

/// Up to three labels, then how many more.
fn some_labels(list: &[ObjRef]) -> String {
    const SHOWN: usize = 3;
    let mut out: Vec<String> = list.iter().take(SHOWN).map(ObjRef::label).collect();
    if list.len() > SHOWN {
        out.push(format!("+{} more", list.len() - SHOWN));
    }
    out.join(", ")
}

/// A Loxone time (seconds since 2009-01-01 00:00, Miniserver local time)
/// with its calendar date: `562522922 (2026-10-29 16:22)`.
fn loxone_time(v: &Option<String>) -> String {
    let Some(v) = v else {
        return "none".into();
    };
    let Ok(secs) = v.parse::<i64>() else {
        return v.clone();
    };
    if secs <= 0 {
        return v.clone();
    }
    // Days since 1970-01-01 → civil date (Howard Hinnant's algorithm).
    let total = secs + crate::uuid::LOXONE_EPOCH_UNIX;
    let (days, rem) = (total.div_euclid(86_400), total.rem_euclid(86_400));
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{v} ({year:04}-{month:02}-{day:02} {:02}:{:02})",
        rem / 3600,
        rem % 3600 / 60
    )
}

/// Resolves object references against both documents, the new one first.
struct Titles<'a> {
    a: &'a BTreeMap<String, ObjectSummary>,
    b: &'a BTreeMap<String, ObjectSummary>,
}

impl Titles<'_> {
    fn obj(&self, uuid: &str) -> ObjRef {
        let o = self.b.get(uuid).or_else(|| self.a.get(uuid));
        ObjRef {
            uuid: uuid.to_string(),
            block_type: o.map(|o| o.block_type.clone()),
            title: o.and_then(|o| o.title.clone()),
        }
    }

    /// A schedule or group: the type is implied, so only the title.
    fn named(&self, uuid: &str) -> ObjRef {
        ObjRef {
            block_type: None,
            ..self.obj(uuid)
        }
    }
}

/// The users/access section between two documents: users and groups added,
/// removed or renamed, membership, credentials, last edit, rights, access
/// rights and UI rights; and the codes and validity of named credentials.
/// Their creation and removal stays in the object diff, where the rest of
/// the periphery is.
///
/// `objs_a`/`objs_b` are the documents' objects by UUID, for titles.
pub fn changes(
    a: &AccessModel,
    b: &AccessModel,
    objs_a: &BTreeMap<String, ObjectSummary>,
    objs_b: &BTreeMap<String, ObjectSummary>,
) -> Vec<AccessChange> {
    let titles = Titles {
        a: objs_a,
        b: objs_b,
    };
    let mut out = Vec::new();

    let users_a: BTreeMap<&str, &User> = a.users.iter().map(|u| (u.uuid.as_str(), u)).collect();
    for ub in &b.users {
        let push = |out: &mut Vec<AccessChange>, change| {
            out.push(AccessChange {
                subject: Subject::User,
                uuid: ub.uuid.clone(),
                title: ub.title.clone(),
                change,
            })
        };
        let ua = users_a.get(ub.uuid.as_str()).copied();
        match ua {
            None => push(&mut out, Change::Added),
            Some(ua) if ua.title != ub.title => push(
                &mut out,
                Change::Renamed {
                    from: ua.title.clone(),
                    to: ub.title.clone(),
                },
            ),
            Some(_) => {}
        }
        let mut found = membership(&titles, a, b, &ub.uuid, ua.is_none());
        found.extend(user_credentials(ua, ub));
        if let Some(ua) = ua {
            found.extend(passwords(ua, ub));
            found.extend(edits(&ua.last_edit, &ub.last_edit));
            found.extend(property("rights", &ua.rights, &ub.rights));
        }
        let none = Vec::new();
        found.extend(rights(
            &titles,
            ua.map_or(&none, |u| &u.permissions),
            &ub.permissions,
        ));
        for change in found {
            push(&mut out, change);
        }
    }
    for ua in &a.users {
        if b.user(&ua.uuid).is_none() {
            out.push(AccessChange {
                subject: Subject::User,
                uuid: ua.uuid.clone(),
                title: ua.title.clone(),
                change: Change::Removed,
            });
        }
    }

    let groups_a: BTreeMap<&str, &Group> = a.groups.iter().map(|g| (g.uuid.as_str(), g)).collect();
    for gb in &b.groups {
        let ga = groups_a.get(gb.uuid.as_str()).copied();
        let mut found = Vec::new();
        match ga {
            None => found.push(Change::Added),
            Some(ga) if ga.title != gb.title => found.push(Change::Renamed {
                from: ga.title.clone(),
                to: gb.title.clone(),
            }),
            Some(_) => {}
        }
        found.extend(membership(&titles, a, b, &gb.uuid, ga.is_none()));
        if let Some(ga) = ga {
            found.extend(edits(&ga.last_edit, &gb.last_edit));
            found.extend(property("rights", &ga.rights, &gb.rights));
            found.extend(property("group type", &ga.kind, &gb.kind));
        }
        let none = Vec::new();
        found.extend(rights(
            &titles,
            ga.map_or(&none, |g| &g.permissions),
            &gb.permissions,
        ));
        out.extend(found.into_iter().map(|change| AccessChange {
            subject: Subject::Group,
            uuid: gb.uuid.clone(),
            title: gb.title.clone(),
            change,
        }));
    }
    for ga in &a.groups {
        if b.group(&ga.uuid).is_none() {
            out.push(AccessChange {
                subject: Subject::Group,
                uuid: ga.uuid.clone(),
                title: ga.title.clone(),
                change: Change::Removed,
            });
        }
    }

    for cb in &b.credentials {
        let Some(ca) = a.credentials.iter().find(|c| c.uuid == cb.uuid) else {
            continue;
        };
        let subject = if cb.block_type == "Keycode" {
            Subject::AccessCode
        } else {
            Subject::NfcTag
        };
        let mut found = Vec::new();
        if ca.secret != cb.secret {
            found.push(Change::SecretChanged);
        }
        found.extend(property("valid from", &ca.valid_from, &cb.valid_from));
        found.extend(property("valid until", &ca.valid_until, &cb.valid_until));
        out.extend(found.into_iter().map(|change| AccessChange {
            subject,
            uuid: cb.uuid.clone(),
            title: cb.title.clone(),
            change,
        }));
    }
    out
}

/// Groups `member` joined or left. A new member only joins; a member that
/// is gone is reported removed, not as leaving each of its groups.
fn membership(
    titles: &Titles,
    a: &AccessModel,
    b: &AccessModel,
    member: &str,
    is_new: bool,
) -> Vec<Change> {
    let before: BTreeSet<&str> = if is_new {
        BTreeSet::new()
    } else {
        a.groups_of(member).map(|g| g.uuid.as_str()).collect()
    };
    let after: Vec<&str> = b.groups_of(member).map(|g| g.uuid.as_str()).collect();
    let mut out: Vec<Change> = after
        .iter()
        .filter(|g| !before.contains(*g))
        .map(|g| Change::Joined {
            group: titles.named(g),
        })
        .collect();
    out.extend(
        a.groups_of(member)
            .filter(|g| !is_new && !after.contains(&g.uuid.as_str()))
            .map(|g| Change::Left {
                group: titles.named(&g.uuid),
            }),
    );
    out
}

/// NFC tags (matched on their hashed ID), codes, iButtons, external IDs.
fn user_credentials(ua: Option<&User>, ub: &User) -> Vec<Change> {
    let mut out = Vec::new();
    let empty: Vec<NfcTag> = Vec::new();
    let tags_a = ua.map_or(&empty, |u| &u.nfc_tags);
    let fp = |t: &NfcTag| t.id.as_ref().map(Secret::fingerprint);
    for tb in &ub.nfc_tags {
        match tags_a.iter().find(|ta| ta.id == tb.id) {
            None => out.push(Change::NfcTagAdded {
                name: tb.name.clone(),
                fingerprint: fp(tb),
            }),
            Some(ta) if ta.name != tb.name => out.push(Change::NfcTagRenamed {
                from: ta.name.clone(),
                to: tb.name.clone(),
                fingerprint: fp(tb).unwrap_or_default(),
            }),
            Some(_) => {}
        }
    }
    for ta in tags_a {
        if !ub.nfc_tags.iter().any(|tb| tb.id == ta.id) {
            out.push(Change::NfcTagRemoved {
                name: ta.name.clone(),
                fingerprint: fp(ta),
            });
        }
    }
    let none: Vec<Secret> = Vec::new();
    for (what, from, to) in [
        ("access codes", ua.map_or(&none, |u| &u.codes), &ub.codes),
        ("iButtons", ua.map_or(&none, |u| &u.ibuttons), &ub.ibuttons),
        (
            "external IDs",
            ua.map_or(&none, |u| &u.external_ids),
            &ub.external_ids,
        ),
    ] {
        if from != to {
            out.push(Change::Credentials {
                what,
                from: from.len(),
                to: to.len(),
            });
        }
    }
    out
}

fn passwords(ua: &User, ub: &User) -> Vec<Change> {
    [
        ("password", &ua.password, &ub.password),
        (
            "visualization password",
            &ua.visu_password,
            &ub.visu_password,
        ),
    ]
    .into_iter()
    .filter(|(_, from, to)| from != to)
    .map(|(what, from, to)| Change::Password {
        what,
        from_set: from.is_some(),
        to_set: to.is_some(),
    })
    .collect()
}

fn edits(from: &Option<String>, to: &Option<String>) -> Option<Change> {
    (from != to).then(|| Change::LastEdit {
        from: from.clone(),
        to: to.clone(),
    })
}

fn property(name: &'static str, from: &Option<String>, to: &Option<String>) -> Option<Change> {
    (from != to).then(|| Change::Property {
        name,
        from: from.clone(),
        to: to.clone(),
    })
}

/// Access rights one by one, UI rights summarized into one change.
fn rights(titles: &Titles, pa: &[Permission], pb: &[Permission]) -> Vec<Change> {
    let mut out = Vec::new();
    let access = |ps: &[Permission]| -> Vec<(String, String)> {
        ps.iter()
            .filter_map(|p| Some((p.target.clone(), p.schedule.clone()?)))
            .collect()
    };
    let (acc_a, acc_b) = (access(pa), access(pb));
    let find = |list: &[(String, String)], target: &str| {
        list.iter()
            .find(|(t, _)| t == target)
            .map(|(_, s)| s.clone())
    };
    for (target, schedule) in &acc_b {
        match find(&acc_a, target) {
            None => out.push(Change::AccessGranted {
                block: titles.obj(target),
                schedule: titles.named(schedule),
            }),
            Some(old) if &old != schedule => out.push(Change::AccessSchedule {
                block: titles.obj(target),
                from: titles.named(&old),
                to: titles.named(schedule),
            }),
            Some(_) => {}
        }
    }
    for (target, schedule) in &acc_a {
        if find(&acc_b, target).is_none() {
            out.push(Change::AccessRevoked {
                block: titles.obj(target),
                schedule: titles.named(schedule),
            });
        }
    }

    let ui = |ps: &[Permission]| -> Vec<(String, String)> {
        ps.iter()
            .filter_map(|p| Some((p.target.clone(), p.ui.clone()?)))
            .collect()
    };
    let (ui_a, ui_b) = (ui(pa), ui(pb));
    let (mut added, mut removed, mut changed) = (Vec::new(), Vec::new(), Vec::new());
    for (target, value) in &ui_b {
        match find(&ui_a, target) {
            None => added.push(titles.obj(target)),
            Some(old) if &old != value => changed.push(titles.obj(target)),
            Some(_) => {}
        }
    }
    for (target, _) in &ui_a {
        if find(&ui_b, target).is_none() {
            removed.push(titles.obj(target));
        }
    }
    if !(added.is_empty() && removed.is_empty() && changed.is_empty()) {
        out.push(Change::UiRights {
            added,
            removed,
            changed,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secrets_print_their_fingerprint_only() {
        let s = Secret::new("0123456789ABCDEF".into());
        let shown = format!("{s:?}");
        assert!(!shown.contains("0123456789ABCDEF"), "{shown}");
        assert_eq!(s.fingerprint().len(), 8);
        assert_eq!(shown, format!("Secret({})", s.fingerprint()));
        assert_ne!(
            s.fingerprint(),
            Secret::new("0123456789ABCDEE".into()).fingerprint()
        );
    }

    #[test]
    fn loxone_time_has_its_date() {
        // 2009-01-01 00:00 + 1 day + 1 h 1 min.
        assert_eq!(
            loxone_time(&Some("90060".into())),
            "90060 (2009-01-02 01:01)"
        );
        assert_eq!(loxone_time(&Some("0".into())), "0");
        assert_eq!(loxone_time(&None), "none");
        assert_eq!(loxone_time(&Some("x".into())), "x");
    }
}
