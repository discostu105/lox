//! Miniserver user management, read side: users (`getuserlist2`, `getuser`),
//! groups (`getgrouplist`), who is assigned to a control
//! (`getcontrolpermissions`) and the NFC Code Touch access log
//! (`/log/<uuid>.txt`). Pure parsers with typed results, thin calls on
//! [`LoxClient`], and the access check that joins live users with the access
//! rights of the program file.
//!
//! Every call needs a connection user with the right to manage users.

use anyhow::{Context, Result, bail};
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeSet;

use crate::client::{HttpStatusError, LoxClient, is_uuid, validate_uuid};
use crate::loxone_xml::{AccessModel, Permission};

// ── Types ─────────────────────────────────────────────────────────────────────

/// One entry of `getuserlist2`.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct UserSummary {
    pub name: String,
    pub uuid: String,
    /// 0 enabled, 1 disabled, 2 enabled until, 3 enabled from, 4 enabled within a timespan
    pub state: i64,
    /// `state` in words
    pub state_text: String,
    pub is_admin: bool,
}

/// A group a user belongs to.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct GroupRef {
    pub name: String,
    pub uuid: String,
}

/// An NFC tag paired with a user. The ID is the tag's serial as the
/// Miniserver reports it (hex bytes separated by spaces).
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct NfcTag {
    pub name: String,
    pub id: String,
}

/// A user as `getuser` reports it (personal fields such as e-mail left out).
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct User {
    pub name: String,
    pub uuid: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// 0 enabled, 1 disabled, 2 enabled until, 3 enabled from, 4 enabled within a timespan
    pub state: i64,
    /// `state` in words
    pub state_text: String,
    pub is_admin: bool,
    /// Loxone time (seconds since 2009-01-01) of the last change
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_edit: Option<i64>,
    /// Loxone time the user becomes valid (states 3 and 4)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub valid_from: Option<i64>,
    /// Loxone time the user expires (states 2 and 4)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub valid_until: Option<i64>,
    pub groups: Vec<GroupRef>,
    pub nfc_tags: Vec<NfcTag>,
    /// Number of access codes (the codes themselves are stored hashed)
    pub access_codes: i64,
    /// Trust member the user comes from, if any
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trust_member: Option<String>,
}

/// One entry of `getgrouplist`.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Group {
    pub name: String,
    pub uuid: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// 0 normal, 1 admin (deprecated), 2 all, 3 none, 4 full access
    #[serde(rename = "type")]
    pub typ: i64,
    /// Right bitmask
    pub user_rights: i64,
}

/// A user or group assigned to a control (`getcontrolpermissions`).
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Principal {
    pub name: String,
    pub uuid: String,
    /// User, UserGroup or RightGroup
    #[serde(rename = "type")]
    pub typ: String,
}

/// One line of a Code Touch access log, as the Miniserver writes it.
#[derive(Debug, Clone, PartialEq)]
pub struct AccessEvent {
    /// Unix time (UTC)
    pub ts: i64,
    /// Output that was opened (1-6, or up to 9999 with preselection), 0 for the
    /// first factor of two-factor authentication, -1 when access was denied
    pub output: i64,
    /// 0-11, see [`access_type_text`]
    pub typ: i64,
    pub user: Option<String>,
    /// Name of the code or NFC tag
    pub description: Option<String>,
    pub ext_medium: Option<String>,
    pub ext_authority: Option<String>,
}

/// An access log entry with its meaning spelled out.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct AccessEntry {
    /// Unix time (UTC)
    pub ts: i64,
    /// Local time, RFC 3339
    pub time: String,
    /// granted, denied or first-factor (2FA: the second factor is still due)
    pub result: String,
    /// Output opened (1-6 or up to 9999), 0 first factor, -1 denied
    pub output: i64,
    /// Raw type code (0-11)
    #[serde(rename = "type")]
    pub typ: i64,
    /// How the user identified: code, NFC tag, app, external …
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    /// Name of the code or NFC tag
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ext_medium: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ext_authority: Option<String>,
}

// ── Parsing ───────────────────────────────────────────────────────────────────

/// The Miniserver answered a user-management call with a code other than 200.
#[derive(Debug)]
pub struct LlCodeError {
    pub code: String,
    pub control: String,
}

impl std::fmt::Display for LlCodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "the Miniserver answered {} with code {}",
            self.control, self.code
        )
    }
}

impl std::error::Error for LlCodeError {}

/// The `value` of an `{"LL": {"control", "value", "Code"}}` answer, after
/// checking `Code` is 200. User-management calls put JSON into `value` as a
/// string; it is parsed when it is JSON, else returned as a string.
pub fn ll_value(body: &str) -> Result<Value> {
    let v: Value = serde_json::from_str(body.trim()).context("the answer is not JSON")?;
    let ll = v.get("LL").context("the answer has no LL envelope")?;
    let code = ll
        .get("Code")
        .or_else(|| ll.get("code"))
        .and_then(crate::json_val_str)
        .unwrap_or_default();
    if code != "200" {
        return Err(LlCodeError {
            code,
            control: ll
                .get("control")
                .and_then(Value::as_str)
                .unwrap_or("the request")
                .to_string(),
        }
        .into());
    }
    Ok(match ll.get("value") {
        Some(Value::String(s)) => {
            serde_json::from_str(s).unwrap_or_else(|_| Value::String(s.clone()))
        }
        Some(v) => v.clone(),
        None => Value::Null,
    })
}

/// A string field; numbers are accepted and printed.
fn text(v: &Value, key: &str) -> String {
    crate::json_val_str(&v[key]).unwrap_or_default()
}

/// An integer field; numeric strings are accepted.
fn int(v: &Value, key: &str) -> Option<i64> {
    match &v[key] {
        Value::Number(n) => n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// A boolean field; 1/0 and "true"/"1" are accepted.
fn flag(v: &Value, key: &str) -> bool {
    match &v[key] {
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_i64().is_some_and(|n| n != 0),
        Value::String(s) => matches!(s.as_str(), "true" | "1"),
        _ => false,
    }
}

fn array<'a>(v: &'a Value, what: &str) -> Result<&'a Vec<Value>> {
    v.as_array()
        .with_context(|| format!("unexpected {} answer: not a list", what))
}

pub fn state_text(state: i64) -> &'static str {
    match state {
        0 => "enabled",
        1 => "disabled",
        2 => "enabled until",
        3 => "enabled from",
        4 => "enabled in timespan",
        _ => "unknown",
    }
}

pub fn group_type_text(typ: i64) -> &'static str {
    match typ {
        0 => "normal",
        1 => "admin",
        2 => "all",
        3 => "none",
        4 => "full access",
        _ => "unknown",
    }
}

/// `getuserlist2`: the `value` of the LL envelope.
pub fn parse_user_list(value: &Value) -> Result<Vec<UserSummary>> {
    Ok(array(value, "getuserlist2")?
        .iter()
        .map(|u| {
            let state = int(u, "userState").unwrap_or(0);
            UserSummary {
                name: text(u, "name"),
                uuid: text(u, "uuid"),
                state,
                state_text: state_text(state).to_string(),
                is_admin: flag(u, "isAdmin"),
            }
        })
        .collect())
}

/// `getuser/<uuid>`: the `value` of the LL envelope.
pub fn parse_user(value: &Value) -> Result<User> {
    if !value.is_object() {
        bail!("unexpected getuser answer: not an object");
    }
    let state = int(value, "userState").unwrap_or(0);
    let groups = value["usergroups"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|g| GroupRef {
                    name: text(g, "name"),
                    uuid: text(g, "uuid"),
                })
                .collect()
        })
        .unwrap_or_default();
    let nfc_tags = value["nfcTags"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|t| NfcTag {
                    name: text(t, "name"),
                    id: text(t, "id"),
                })
                .collect()
        })
        .unwrap_or_default();
    let codes = int(value, "numKeyCodes")
        .or_else(|| value["keycodes"].as_array().map(|a| a.len() as i64))
        .unwrap_or(0);
    // the documentation spells it `lastedit`, firmware 16 sends `lastEdit`
    let last_edit = int(value, "lastEdit").or_else(|| int(value, "lastedit"));
    let trust = text(value, "trustMember");
    Ok(User {
        name: text(value, "name"),
        uuid: text(value, "uuid"),
        description: text(value, "desc"),
        state,
        state_text: state_text(state).to_string(),
        is_admin: flag(value, "isAdmin"),
        last_edit,
        valid_from: int(value, "validFrom").filter(|t| *t > 0),
        valid_until: int(value, "validUntil").filter(|t| *t > 0),
        groups,
        nfc_tags,
        access_codes: codes,
        trust_member: (!trust.is_empty()).then_some(trust),
    })
}

/// `getgrouplist`: the `value` of the LL envelope.
pub fn parse_group_list(value: &Value) -> Result<Vec<Group>> {
    Ok(array(value, "getgrouplist")?
        .iter()
        .map(|g| Group {
            name: text(g, "name"),
            uuid: text(g, "uuid"),
            description: text(g, "description"),
            typ: int(g, "type").unwrap_or(0),
            user_rights: int(g, "userRights").unwrap_or(0),
        })
        .collect())
}

/// `getcontrolpermissions/<uuid>`: the whole body. Firmware 16 answers with a
/// JSON list wrapped in braces, `{[…]}`, and no LL envelope; an LL envelope
/// or a plain list is accepted too.
pub fn parse_control_permissions(body: &str) -> Result<Vec<Principal>> {
    let body = body.trim();
    let list: Value = match serde_json::from_str::<Value>(body) {
        Ok(v) if v.get("LL").is_some() => match ll_value(body)? {
            Value::String(s) => unwrap_braced_list(&s)?,
            v => v,
        },
        Ok(v) => v,
        Err(_) => unwrap_braced_list(body)?,
    };
    Ok(array(&list, "getcontrolpermissions")?
        .iter()
        .map(|p| Principal {
            name: text(p, "name"),
            uuid: text(p, "uuid"),
            typ: text(p, "type"),
        })
        .collect())
}

/// `{[…]}` → the list inside.
fn unwrap_braced_list(s: &str) -> Result<Value> {
    let inner = s
        .trim()
        .strip_prefix('{')
        .and_then(|s| s.strip_suffix('}'))
        .map(str::trim)
        .filter(|s| s.starts_with('['))
        .context("unexpected getcontrolpermissions answer")?;
    serde_json::from_str(inner).context("unexpected getcontrolpermissions answer")
}

/// The Code Touch access log: JSON objects, each followed by `/`. Names may
/// contain `/` too, so the objects are read one by one rather than split.
/// Returns the events and the number of unreadable entries (a log being
/// written can end in a partial one).
pub fn parse_access_log(text: &str) -> (Vec<AccessEvent>, usize) {
    let mut events = Vec::new();
    let mut skipped = 0;
    let mut rest = text;
    loop {
        rest = rest.trim_start_matches(|c: char| c == '/' || c.is_whitespace());
        if rest.is_empty() {
            break;
        }
        let mut stream = serde_json::Deserializer::from_str(rest).into_iter::<Value>();
        match stream.next() {
            Some(Ok(v)) if v.is_object() && int(&v, "ts").is_some() => {
                let opt = |k: &str| v[k].as_str().map(str::to_string);
                events.push(AccessEvent {
                    ts: int(&v, "ts").unwrap_or(0),
                    output: int(&v, "output").unwrap_or(-1),
                    typ: int(&v, "type").unwrap_or(-1),
                    user: opt("user"),
                    description: opt("description"),
                    ext_medium: opt("extMedium"),
                    ext_authority: opt("extAuthority"),
                });
                rest = &rest[stream.byte_offset()..];
            }
            _ => {
                skipped += 1;
                // resume at the next entry
                match rest
                    .char_indices()
                    .skip(1)
                    .find(|(i, _)| rest[*i..].starts_with("/{"))
                {
                    Some((i, _)) => rest = &rest[i + 1..],
                    None => break,
                }
            }
        }
    }
    (events, skipped)
}

/// The access log's `type` in words.
pub fn access_type_text(typ: i64) -> &'static str {
    match typ {
        0 => "code",
        1 => "code (named)",
        2 => "code (several users)",
        3 => "NFC tag",
        4 => "NFC tag (named)",
        5 => "NFC tag (several users)",
        6 => "app",
        7 => "code",
        8 => "NFC tag",
        9 => "app",
        10 => "external",
        11 => "external",
        _ => "unknown",
    }
}

impl AccessEvent {
    /// granted, denied or first-factor.
    pub fn result(&self) -> &'static str {
        if self.output < 0 || matches!(self.typ, 7 | 8 | 9 | 11) {
            "denied"
        } else if self.output == 0 {
            "first-factor"
        } else {
            "granted"
        }
    }

    pub fn entry(&self) -> AccessEntry {
        use chrono::TimeZone;
        let time = chrono::Local
            .timestamp_opt(self.ts, 0)
            .single()
            .map(|t| t.to_rfc3339_opts(chrono::SecondsFormat::Secs, false))
            .unwrap_or_default();
        AccessEntry {
            ts: self.ts,
            time,
            result: self.result().to_string(),
            output: self.output,
            typ: self.typ,
            method: access_type_text(self.typ).to_string(),
            user: self.user.clone(),
            description: self.description.clone(),
            ext_medium: self.ext_medium.clone(),
            ext_authority: self.ext_authority.clone(),
        }
    }
}

/// A user by UUID, exact name (any case) or a unique part of the name.
pub fn find_user<'a>(users: &'a [UserSummary], query: &str) -> Result<&'a UserSummary> {
    if is_uuid(query) {
        return users
            .iter()
            .find(|u| u.uuid.eq_ignore_ascii_case(query))
            .with_context(|| format!("No user matching '{}' — see `lox user ls`", query));
    }
    let q = query.to_lowercase();
    if let Some(u) = users.iter().find(|u| u.name.to_lowercase() == q) {
        return Ok(u);
    }
    let matches: Vec<&UserSummary> = users
        .iter()
        .filter(|u| u.name.to_lowercase().contains(&q))
        .collect();
    match matches.as_slice() {
        [] => bail!("No user matching '{}' — see `lox user ls`", query),
        [u] => Ok(u),
        _ => bail!(
            "Ambiguous user '{}': {} — use the full name or the UUID",
            query,
            matches
                .iter()
                .map(|u| u.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

// ── Miniserver calls ──────────────────────────────────────────────────────────

/// Explain a refused user-management call and how to fix it.
fn needs_user_management(e: anyhow::Error) -> anyhow::Error {
    let refused = e
        .downcast_ref::<HttpStatusError>()
        .is_some_and(|h| matches!(h.status, 401 | 403))
        || e.downcast_ref::<LlCodeError>()
            .is_some_and(|c| matches!(c.code.as_str(), "401" | "403"));
    if refused {
        e.context(
            "the Miniserver refused to show its user management — the user lox connects \
             with needs the right to manage users (an admin); change it with \
             `lox setup set --user <admin>`",
        )
    } else {
        e
    }
}

fn ll_call(lox: &LoxClient, cmd: &str) -> Result<Value> {
    let body = lox
        .get_text(&format!("/jdev/sps/{}", cmd))
        .map_err(needs_user_management)?;
    ll_value(&body).map_err(needs_user_management)
}

pub fn user_list(lox: &LoxClient) -> Result<Vec<UserSummary>> {
    parse_user_list(&ll_call(lox, "getuserlist2")?)
}

pub fn user(lox: &LoxClient, uuid: &str) -> Result<User> {
    validate_uuid(uuid)?;
    parse_user(&ll_call(lox, &format!("getuser/{}", uuid))?)
}

pub fn group_list(lox: &LoxClient) -> Result<Vec<Group>> {
    parse_group_list(&ll_call(lox, "getgrouplist")?)
}

pub fn control_permissions(lox: &LoxClient, uuid: &str) -> Result<Vec<Principal>> {
    validate_uuid(uuid)?;
    let body = lox
        .get_text(&format!("/jdev/sps/getcontrolpermissions/{}", uuid))
        .map_err(needs_user_management)?;
    parse_control_permissions(&body).map_err(needs_user_management)
}

/// The raw access log of a Code Touch, `None` when it has none yet.
pub fn access_log(lox: &LoxClient, uuid: &str) -> Result<Option<String>> {
    validate_uuid(uuid)?;
    let body = match lox.get_text(&format!("/dev/fsget/log/{}.txt", uuid)) {
        Ok(b) => b,
        Err(e)
            if e.downcast_ref::<HttpStatusError>()
                .is_some_and(|h| h.status == 404) =>
        {
            return Ok(None);
        }
        Err(e) => return Err(needs_user_management(e)),
    };
    // the file server answers some errors with an XML body and status 200
    if body.trim_start().starts_with('<') && body.contains("<errorcode>") {
        return match crate::xml_attr(&body, "errorcode") {
            Some("404") => Ok(None),
            Some(code @ ("401" | "403")) => Err(needs_user_management(
                HttpStatusError {
                    status: code.parse().unwrap_or(403),
                    path: format!("log/{}.txt", uuid),
                }
                .into(),
            )),
            code => bail!(
                "the Miniserver answered error code {} for the access log",
                code.unwrap_or("?")
            ),
        };
    }
    Ok(Some(body))
}

/// The user one name or UUID means: the list, then that user's details.
pub fn resolve_user(lox: &LoxClient, query: &str) -> Result<User> {
    let users = user_list(lox)?;
    let summary = find_user(&users, query)?;
    user(lox, &summary.uuid)
}

// ── Access check ──────────────────────────────────────────────────────────────

/// Why a user may or may not open a Code Touch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Verdict {
    /// An access right (direct or through a group) with a schedule
    Granted,
    /// Neither the user nor one of their groups has an access right here
    NoAccessRight,
    /// The user has no NFC tag and no access code
    NoCredential,
    /// The user is disabled, expired or not yet valid
    UserDisabled,
    /// The only path is the full-access group, which is not verified to open Code Touches
    UnknownFullAccess,
}

/// An access right that applies to the user.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct AccessRight {
    /// "direct" or the name of the group that carries the right
    pub via: String,
    /// The schedule that limits the right, e.g. Always
    pub schedule: String,
}

/// The verdict for one user at one Code Touch.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct AccessCheck {
    pub code_touch: String,
    pub code_touch_uuid: String,
    pub verdict: Verdict,
    /// The verdict in a sentence
    pub reason: String,
    /// Access rights found in the program file (direct and through groups)
    pub rights: Vec<AccessRight>,
    /// Whether the Miniserver lists the user or one of their groups as assigned
    /// to this Code Touch (UI or access right, it does not tell which); absent
    /// when it could not be read
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assigned_live: Option<bool>,
    /// Signs that the program file and the live state disagree
    pub warnings: Vec<String>,
    /// Practical hints, e.g. that a phone must be unlocked
    pub hints: Vec<String>,
}

/// A user's access at Code Touches.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct AccessReport {
    pub user: String,
    pub user_uuid: String,
    pub user_state: String,
    pub nfc_tags: usize,
    pub access_codes: i64,
    /// Where the access rights come from, e.g. the backup's file name
    pub config: String,
    pub checks: Vec<AccessCheck>,
}

/// A Code Touch to check, with the users and groups the Miniserver lists as
/// assigned to it (`None` when that could not be read).
pub struct Touch {
    pub uuid: String,
    pub name: String,
    pub assigned: Option<Vec<Principal>>,
}

/// Loxone time now: seconds since 2009-01-01 local time.
pub fn lox_now() -> i64 {
    (chrono::Local::now() - crate::lox_epoch()).num_seconds()
}

/// Why the user cannot use any credential right now, if so.
fn disabled_reason(user: &User, now: i64) -> Option<String> {
    let fmt = |t: i64| crate::lox_timestamp_to_string(t);
    let expired = |until: Option<i64>| until.filter(|u| now > *u);
    let not_yet = |from: Option<i64>| from.filter(|f| now < *f);
    match user.state {
        1 => Some("the user is disabled".into()),
        2 => expired(user.valid_until).map(|u| format!("the user expired on {}", fmt(u))),
        3 => not_yet(user.valid_from).map(|f| format!("the user is only valid from {}", fmt(f))),
        4 => not_yet(user.valid_from)
            .map(|f| format!("the user is only valid from {}", fmt(f)))
            .or_else(|| {
                expired(user.valid_until).map(|u| format!("the user expired on {}", fmt(u)))
            }),
        _ => None,
    }
}

/// Join the live user with the access rights of the program file.
/// `groups` is the live group list (it tells which group is full access).
pub fn check_access(
    user: &User,
    groups: &[Group],
    model: &AccessModel,
    touch: &Touch,
    now: i64,
) -> AccessCheck {
    let mut warnings = Vec::new();
    let mut hints = Vec::new();

    // live group membership decides which group rights apply
    let group_ids: BTreeSet<&str> = user.groups.iter().map(|g| g.uuid.as_str()).collect();
    let config_user = model.user(&user.uuid);
    let mut rights = Vec::new();
    let mut never = Vec::new();
    let mut push = |via: String, schedule_uuid: &str| {
        let schedule = model.schedule(schedule_uuid);
        if schedule.never {
            never.push(format!("{} (schedule {})", via, schedule.title));
        } else {
            rights.push(AccessRight {
                via,
                schedule: schedule.title,
            });
        }
    };
    match config_user {
        Some(cu) => {
            for s in cu.access_schedules(&touch.uuid) {
                push("direct".into(), s);
            }
            if cu.last_edit.is_some() && user.last_edit.is_some() && cu.last_edit != user.last_edit
            {
                warnings.push(
                    "the user was changed after the program file was saved — rights edited \
                     since are not in it; save in Loxone Config or check again after the next save"
                        .into(),
                );
            }
        }
        None => warnings.push(
            "the user is not in the program file (created after it was saved?) — direct rights \
             cannot be checked"
                .into(),
        ),
    }
    for g in &model.groups {
        if group_ids.contains(g.uuid.as_str()) {
            for s in g.access_schedules(&touch.uuid) {
                push(format!("group {}", g.name), s);
            }
        }
    }
    let mut full_access = Vec::new();
    for g in &user.groups {
        let in_model = model.groups.iter().find(|m| m.uuid == g.uuid);
        let live_type = groups.iter().find(|l| l.uuid == g.uuid).map(|l| l.typ);
        if live_type == Some(4) || in_model.is_some_and(|m| m.ug_type == Some(4)) {
            full_access.push(g.name.clone());
        }
        if in_model.is_none() {
            warnings.push(format!(
                "group {} is not in the program file — its rights cannot be checked",
                g.name
            ));
        }
    }

    let assigned_live = touch.assigned.as_ref().map(|list| {
        list.iter()
            .any(|p| p.uuid == user.uuid || group_ids.contains(p.uuid.as_str()))
    });
    // any right on the block in the file, UI rights included: the live list
    // does not tell UI and access rights apart
    let on_block = |perms: &[Permission]| perms.iter().any(|p| p.target == touch.uuid);
    let in_file = config_user.is_some_and(|cu| on_block(&cu.permissions))
        || model
            .groups
            .iter()
            .any(|g| group_ids.contains(g.uuid.as_str()) && on_block(&g.permissions));
    match assigned_live {
        Some(false) if !rights.is_empty() => warnings.push(
            "the Miniserver does not list the user or their groups at this Code Touch — the right \
             may have been removed after the program file was saved"
                .into(),
        ),
        Some(true) if !in_file => warnings.push(
            "the Miniserver lists the user or one of their groups at this Code Touch, but the \
             program file has no right there for them — added after the file was saved?"
                .into(),
        ),
        _ => {}
    }

    let tags = user.nfc_tags.len();
    let (verdict, reason) = if let Some(why) = disabled_reason(user, now) {
        (Verdict::UserDisabled, why)
    } else if tags == 0 && user.access_codes == 0 {
        (
            Verdict::NoCredential,
            "the user has no NFC tag and no access code".into(),
        )
    } else if !rights.is_empty() {
        let paths: Vec<String> = rights
            .iter()
            .map(|r| format!("{} (schedule {})", r.via, r.schedule))
            .collect();
        (
            Verdict::Granted,
            format!("access right {}", paths.join(", ")),
        )
    } else if !full_access.is_empty() {
        (
            Verdict::UnknownFullAccess,
            format!(
                "no access right; the user is in the full-access group {} — not verified to open \
                 Code Touches, give the user an access right to be sure",
                full_access.join(", ")
            ),
        )
    } else if !never.is_empty() {
        (
            Verdict::NoAccessRight,
            format!("the only access right never applies: {}", never.join(", ")),
        )
    } else {
        (
            Verdict::NoAccessRight,
            "neither the user nor one of their groups has an access right here".into(),
        )
    };
    if verdict == Verdict::Granted && tags > 0 {
        hints.push("a phone used as NFC tag must be unlocked when held to the reader".into());
    }
    AccessCheck {
        code_touch: touch.name.clone(),
        code_touch_uuid: touch.uuid.clone(),
        verdict,
        reason,
        rights,
        assigned_live,
        warnings,
        hints,
    }
}

// ── Commands' building blocks (CLI and MCP) ───────────────────────────────────

/// An NFC Code Touch by UUID, alias, exact name (any case) or a unique part
/// of the name: the app's Code Touches plus `extra` ones (UUID, title) from
/// a program file. Returns (UUID, name).
pub fn find_code_touch(
    lox: &mut LoxClient,
    query: &str,
    extra: &[(String, String)],
) -> Result<(String, String)> {
    let mut all: Vec<(String, String)> = lox
        .list_controls(Some("NfcCodeTouch"), None)?
        .into_iter()
        .map(|c| (c.uuid, c.name))
        .collect();
    for (uuid, title) in extra {
        if !all.iter().any(|(u, _)| u == uuid) {
            all.push((uuid.clone(), title.clone()));
        }
    }
    let uuid = lox
        .cfg
        .aliases
        .get(query)
        .cloned()
        .or_else(|| is_uuid(query).then(|| query.to_string()));
    if let Some(uuid) = uuid {
        let name = all
            .iter()
            .find(|(u, _)| *u == uuid)
            .map(|(_, n)| n.clone())
            .unwrap_or_else(|| uuid.clone());
        return Ok((uuid, name));
    }
    let q = query.to_lowercase();
    if let Some(hit) = all.iter().find(|(_, n)| n.to_lowercase() == q) {
        return Ok(hit.clone());
    }
    let matches: Vec<&(String, String)> = all
        .iter()
        .filter(|(_, n)| n.to_lowercase().contains(&q))
        .collect();
    let names = |list: &[&(String, String)]| {
        list.iter()
            .map(|(_, n)| n.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    };
    match matches.as_slice() {
        [hit] => Ok((*hit).clone()),
        [] if all.is_empty() => bail!("No NFC Code Touch matching '{}': there is none", query),
        [] => bail!(
            "No NFC Code Touch matching '{}' — there are: {}",
            query,
            names(&all.iter().collect::<Vec<_>>())
        ),
        _ => bail!(
            "Ambiguous: '{}' matches the Code Touches {} — use the full name or the UUID",
            query,
            names(&matches)
        ),
    }
}

/// A Code Touch's access log, newest `limit` entries (0: all), oldest first.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct AccessLog {
    pub code_touch: String,
    pub code_touch_uuid: String,
    /// Entries shown
    pub count: usize,
    /// Entries in the log
    pub total: usize,
    pub entries: Vec<AccessEntry>,
    /// Unreadable entries that were skipped
    #[serde(default, skip_serializing_if = "is_zero")]
    pub skipped: usize,
}

fn is_zero(n: &usize) -> bool {
    *n == 0
}

pub fn read_access_log(lox: &mut LoxClient, code_touch: &str, limit: usize) -> Result<AccessLog> {
    let (uuid, name) = find_code_touch(lox, code_touch, &[])?;
    let (events, skipped) = match access_log(lox, &uuid)? {
        Some(text) => parse_access_log(&text),
        None => (Vec::new(), 0),
    };
    let total = events.len();
    let start = if limit == 0 {
        0
    } else {
        total.saturating_sub(limit)
    };
    let entries: Vec<AccessEntry> = events[start..].iter().map(AccessEvent::entry).collect();
    Ok(AccessLog {
        code_touch: name,
        code_touch_uuid: uuid,
        count: entries.len(),
        total,
        entries,
        skipped,
    })
}

/// Who is assigned to a control, live.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ControlPermissions {
    pub control: String,
    pub control_uuid: String,
    #[serde(rename = "type")]
    pub typ: String,
    pub count: usize,
    pub principals: Vec<Principal>,
}

pub fn read_control_permissions(lox: &mut LoxClient, control: &str) -> Result<ControlPermissions> {
    let uuid = lox.resolve(control)?;
    let ctrl = lox.find_control(&uuid)?;
    let principals = control_permissions(lox, &ctrl.uuid)?;
    Ok(ControlPermissions {
        control: ctrl.name,
        control_uuid: ctrl.uuid,
        typ: ctrl.typ,
        count: principals.len(),
        principals,
    })
}

/// Check a live user's access at one Code Touch, or at every Code Touch of
/// the program file, against `config` (the program file's XML) labeled `label`.
pub fn access_report(
    lox: &mut LoxClient,
    user: &User,
    code_touch: Option<&str>,
    config: &[u8],
    label: &str,
) -> Result<AccessReport> {
    let model = crate::loxone_xml::parse_access_model(config)
        .context("cannot read the users of the program file")?;
    let touches: Vec<(String, String)> = match code_touch {
        Some(q) => vec![find_code_touch(lox, q, &model.code_touches)?],
        None if model.code_touches.is_empty() => {
            bail!("the program file ({}) has no NFC Code Touch", label)
        }
        None => {
            let mut t = model.code_touches.clone();
            t.sort_by(|a, b| a.1.cmp(&b.1));
            t
        }
    };
    let groups = group_list(lox)?;
    let now = lox_now();
    let checks = touches
        .into_iter()
        .map(|(uuid, name)| {
            let assigned = control_permissions(lox, &uuid).ok();
            let touch = Touch {
                uuid,
                name,
                assigned,
            };
            check_access(user, &groups, &model, &touch, now)
        })
        .collect();
    Ok(AccessReport {
        user: user.name.clone(),
        user_uuid: user.uuid.clone(),
        user_state: user.state_text.clone(),
        nfc_tags: user.nfc_tags.len(),
        access_codes: user.access_codes,
        config: label.to_string(),
        checks,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loxone_xml::parse_access_model;

    const ALICE: &str = "1a2b3c4d-0001-0001-ffff000000000001";
    const BOB: &str = "1a2b3c4d-0001-0002-ffff000000000002";
    const CAROL: &str = "1a2b3c4d-0001-0003-ffff000000000003";
    const FAMILY: &str = "1a2b3c4d-0002-0001-ffff000000000001";
    const FULL: &str = "1a2b3c4d-0002-0002-ffff000000000002";
    const TOUCH: &str = "1a2b3c4d-0003-0001-ffff000000000001";
    const ALWAYS: &str = "1a2b3c4d-0004-0001-ffff000000000001";
    const NEVER: &str = "1a2b3c4d-0004-0002-ffff000000000002";

    fn envelope(control: &str, value: &Value) -> String {
        serde_json::json!({
            "LL": { "control": control, "value": value.to_string(), "Code": "200" }
        })
        .to_string()
    }

    #[test]
    fn ll_value_parses_the_json_string_and_checks_the_code() {
        let body = envelope(
            "dev/sps/getuserlist2",
            &serde_json::json!([{"name": "alice"}]),
        );
        assert_eq!(ll_value(&body).unwrap()[0]["name"], "alice");
        // numeric codes are fine too
        let ok = r#"{"LL":{"control":"x","value":"1","Code":200}}"#;
        assert_eq!(ll_value(ok).unwrap(), Value::from(1));
        let refused = r#"{"LL":{"control":"dev/sps/getuserlist2","value":"","Code":"403"}}"#;
        let e = ll_value(refused).unwrap_err();
        assert_eq!(e.downcast_ref::<LlCodeError>().unwrap().code, "403");
        assert!(ll_value("<html/>").is_err());
    }

    #[test]
    fn parses_the_user_list() {
        let v = serde_json::json!([
            {"name": "alice", "uniqueUserId": "", "uuid": ALICE, "isAdmin": true, "userState": 0, "representsControl": false},
            {"name": "bob", "uuid": BOB, "isAdmin": false, "userState": "1"},
        ]);
        let users = parse_user_list(&v).unwrap();
        assert_eq!(users.len(), 2);
        assert!(users[0].is_admin);
        assert_eq!(users[1].state, 1);
        assert_eq!(users[1].state_text, "disabled");
        assert!(parse_user_list(&serde_json::json!({})).is_err());
    }

    fn alice_json() -> Value {
        serde_json::json!({
            "name": "alice", "desc": "", "uuid": ALICE, "userid": "", "firstName": "",
            "lastEdit": 562000000, "userState": 0, "isAdmin": false, "changePassword": true,
            "masterAdmin": false, "userRights": 4294967295u64, "trustMember": "",
            "usergroups": [{"name": "Family", "uuid": FAMILY, "trustMember": ""}],
            "nfcNumTags": 1,
            "nfcTags": [{"name": "alice phone", "id": "01 23 45 67 89 AB CD EF EC"}],
            "numKeyCodes": 0, "keycodes": [], "customFields": ["", "", "", "", ""]
        })
    }

    #[test]
    fn parses_a_user() {
        let u = parse_user(&alice_json()).unwrap();
        assert_eq!(u.name, "alice");
        assert_eq!(u.last_edit, Some(562000000));
        assert_eq!(u.groups[0].name, "Family");
        assert_eq!(u.nfc_tags[0].id, "01 23 45 67 89 AB CD EF EC");
        assert_eq!(u.access_codes, 0);
        assert_eq!(u.trust_member, None);
        // the documented spelling and validity times
        let v = serde_json::json!({"name": "bob", "uuid": BOB, "lastedit": 5, "userState": 2,
            "validUntil": 100, "keycodes": [{}]});
        let u = parse_user(&v).unwrap();
        assert_eq!(u.last_edit, Some(5));
        assert_eq!(u.valid_until, Some(100));
        assert_eq!(u.access_codes, 1);
        assert!(u.groups.is_empty());
    }

    #[test]
    fn parses_the_group_list() {
        let v = serde_json::json!([
            {"name": "Family", "description": "", "uuid": FAMILY, "type": 0, "userRights": 129},
            {"name": "Full access", "description": "", "uuid": FULL, "type": 4, "userRights": -1},
        ]);
        let g = parse_group_list(&v).unwrap();
        assert_eq!(g[1].typ, 4);
        assert_eq!(group_type_text(g[1].typ), "full access");
    }

    #[test]
    fn parses_control_permissions_in_braces_or_an_envelope() {
        let list = format!(
            r#"[{{"name":"alice","uuid":"{ALICE}","type":"User"}},{{"name":"Family","uuid":"{FAMILY}","type":"UserGroup"}}]"#
        );
        // what firmware 16 sends: a list in braces, no envelope
        let p = parse_control_permissions(&format!("{{{}}}\n", list)).unwrap();
        assert_eq!(p.len(), 2);
        assert_eq!(p[1].typ, "UserGroup");
        let enveloped = serde_json::json!({"LL": {"control": "x", "value": format!("{{{}}}", list), "Code": "200"}});
        assert_eq!(
            parse_control_permissions(&enveloped.to_string())
                .unwrap()
                .len(),
            2
        );
        assert_eq!(parse_control_permissions(&list).unwrap().len(), 2);
        assert!(parse_control_permissions("{nonsense}").is_err());
    }

    #[test]
    fn parses_the_access_log() {
        let log = concat!(
            r#"{"ts":1790000000,"output":-1,"type":8}/"#,
            r#"{"ts":1790000004,"output":1,"type":3,"user":"alice","description":"alice/phone"}/"#,
            "\n",
            r#"{"ts":1790000010,"output":0,"type":0,"user":"bob"}/"#,
            r#"{"ts":1790000020,"output":-1,"type":11,"extMedium":"Card","extAuthority":"Server"}/"#,
            r#"{"ts":17900"#,
        );
        let (events, skipped) = parse_access_log(log);
        assert_eq!(events.len(), 4);
        assert_eq!(skipped, 1, "the partial last entry");
        assert_eq!(events[0].result(), "denied");
        assert_eq!(access_type_text(events[0].typ), "NFC tag");
        // a slash inside a name does not split the entry
        assert_eq!(events[1].description.as_deref(), Some("alice/phone"));
        assert_eq!(events[1].result(), "granted");
        assert_eq!(events[2].result(), "first-factor");
        assert_eq!(events[3].ext_medium.as_deref(), Some("Card"));
        let e = events[1].entry();
        assert_eq!(e.method, "NFC tag");
        assert!(e.time.starts_with("2026-"), "{}", e.time);
        assert_eq!(parse_access_log("").0.len(), 0);
        // garbage in the middle is skipped, the rest still read
        let (events, skipped) =
            parse_access_log(r#"{"ts":1,"output":1,"type":6}/oops/{"ts":2,"output":1,"type":6}/"#);
        assert_eq!((events.len(), skipped), (2, 1));
    }

    fn summaries() -> Vec<UserSummary> {
        parse_user_list(&serde_json::json!([
            {"name": "alice", "uuid": ALICE},
            {"name": "Alina", "uuid": BOB},
            {"name": "carol", "uuid": CAROL},
        ]))
        .unwrap()
    }

    #[test]
    fn finds_users_by_uuid_exact_name_or_unique_part() {
        let users = summaries();
        assert_eq!(find_user(&users, CAROL).unwrap().name, "carol");
        assert_eq!(find_user(&users, "ALICE").unwrap().name, "alice");
        assert_eq!(find_user(&users, "car").unwrap().name, "carol");
        let e = find_user(&users, "ali").unwrap_err().to_string();
        assert!(e.contains("Ambiguous") && e.contains("Alina"), "{}", e);
        assert!(find_user(&users, "dave").is_err());
    }

    fn model() -> AccessModel {
        let xml = format!(
            r#"<?xml version="1.0"?>
<ControlList Version="1">
<C Type="Document" Title="Test">
<C Type="UserCaption" Title="Users">
<C Type="User" U="{ALICE}" Title="alice" NFCArr="{tag}" NFCNArr="alice phone" LastEdit="562000000">
<PH NUM="1"><PRM REF="{TOUCH}" TIM="{ALWAYS}"/></PH>
</C>
<C Type="User" U="{BOB}" Title="bob" LastEdit="10"/>
<C Type="User" U="{CAROL}" Title="carol" CodeArr="{tag}">
<PH NUM="1"><PRM REF="{TOUCH}" TIM="{NEVER}"/></PH>
</C>
</C>
<C Type="UserGroupCaption" Title="Groups">
<C Type="UserGroup" U="{FAMILY}" Title="Family" User="{BOB}" UserGroups="">
<PH NUM="2"><PRM REF="{TOUCH}" TIM="{ALWAYS}"/><PRM REF="{ALWAYS}" ACT="3"/></PH>
</C>
<C Type="UserGroup" U="{FULL}" Title="Full access" Rights="-1" UgType="4" User="{ALICE},{CAROL}"/>
</C>
<C Type="SwitchingTimer" U="{ALWAYS}" Title="Always" FIX="1"/>
<C Type="SwitchingTimer" U="{NEVER}" Title="Never" FIX="2"/>
<C Type="NfcCodeTouch" U="{TOUCH}" Title="Front door"/>
</C>
</ControlList>"#,
            tag = "ab".repeat(32)
        );
        parse_access_model(xml.as_bytes()).unwrap()
    }

    fn touch(assigned: Option<Vec<Principal>>) -> Touch {
        Touch {
            uuid: TOUCH.into(),
            name: "Front door".into(),
            assigned,
        }
    }

    fn user(name: &str, uuid: &str, groups: &[(&str, &str)], tags: usize, codes: i64) -> User {
        User {
            name: name.into(),
            uuid: uuid.into(),
            description: String::new(),
            state: 0,
            state_text: "enabled".into(),
            is_admin: false,
            last_edit: None,
            valid_from: None,
            valid_until: None,
            groups: groups
                .iter()
                .map(|(n, u)| GroupRef {
                    name: n.to_string(),
                    uuid: u.to_string(),
                })
                .collect(),
            nfc_tags: (0..tags)
                .map(|i| NfcTag {
                    name: format!("tag {}", i),
                    id: "01 02".into(),
                })
                .collect(),
            access_codes: codes,
            trust_member: None,
        }
    }

    #[test]
    fn direct_right_grants_and_reminds_of_unlocked_phones() {
        let m = model();
        let mut alice = user("alice", ALICE, &[("Full access", FULL)], 1, 0);
        alice.last_edit = Some(562000000);
        let c = check_access(&alice, &[], &m, &touch(None), 0);
        assert_eq!(c.verdict, Verdict::Granted, "{}", c.reason);
        assert_eq!(c.rights[0].via, "direct");
        assert_eq!(c.rights[0].schedule, "Always");
        assert!(c.hints[0].contains("unlocked"));
        assert!(c.warnings.is_empty(), "{:?}", c.warnings);
    }

    #[test]
    fn group_right_grants_through_live_membership() {
        let m = model();
        let bob = user("bob", BOB, &[("Family", FAMILY)], 0, 1);
        let c = check_access(&bob, &[], &m, &touch(None), 0);
        assert_eq!(c.verdict, Verdict::Granted);
        assert_eq!(c.rights[0].via, "group Family");
        assert!(c.hints.is_empty(), "a code needs no phone hint");
        // not in the group live: the config membership does not count
        let bob = user("bob", BOB, &[], 0, 1);
        let c = check_access(&bob, &[], &m, &touch(None), 0);
        assert_eq!(c.verdict, Verdict::NoAccessRight);
    }

    #[test]
    fn full_access_alone_is_reported_as_unverified() {
        let m = model();
        let carol = user("carol", CAROL, &[("Full access", FULL)], 1, 0);
        let c = check_access(&carol, &[], &m, &touch(None), 0);
        assert_eq!(c.verdict, Verdict::UnknownFullAccess, "{}", c.reason);
        assert!(c.reason.contains("not verified"));
        // without the full-access group, the 'Never' right is no right
        let carol = user("carol", CAROL, &[], 1, 0);
        let listed = vec![Principal {
            name: "carol".into(),
            uuid: CAROL.into(),
            typ: "User".into(),
        }];
        let c = check_access(&carol, &[], &m, &touch(Some(listed)), 0);
        assert_eq!(c.verdict, Verdict::NoAccessRight);
        assert!(c.reason.contains("Never"), "{}", c.reason);
        // listed live, and the file has a right there (if a useless one)
        assert!(c.warnings.is_empty(), "{:?}", c.warnings);
    }

    #[test]
    fn disabled_and_credential_less_users_are_refused_first() {
        let m = model();
        let mut alice = user("alice", ALICE, &[], 1, 0);
        alice.state = 1;
        assert_eq!(
            check_access(&alice, &[], &m, &touch(None), 0).verdict,
            Verdict::UserDisabled
        );
        alice.state = 2;
        alice.valid_until = Some(100);
        let c = check_access(&alice, &[], &m, &touch(None), 200);
        assert_eq!(c.verdict, Verdict::UserDisabled);
        assert!(c.reason.contains("expired"));
        assert_eq!(
            check_access(&alice, &[], &m, &touch(None), 50).verdict,
            Verdict::Granted
        );
        let alice = user("alice", ALICE, &[], 0, 0);
        assert_eq!(
            check_access(&alice, &[], &m, &touch(None), 0).verdict,
            Verdict::NoCredential
        );
    }

    #[test]
    fn warns_when_the_program_file_and_the_live_state_disagree() {
        let m = model();
        // edited since the save, and unknown to the live permission list
        let mut alice = user("alice", ALICE, &[], 1, 0);
        alice.last_edit = Some(562000999);
        let c = check_access(&alice, &[], &m, &touch(Some(vec![])), 0);
        assert_eq!(c.assigned_live, Some(false));
        assert_eq!(c.warnings.len(), 2, "{:?}", c.warnings);
        // a user created after the save
        let dave = user("dave", "1a2b3c4d-0001-0004-ffff000000000004", &[], 1, 0);
        let assigned = vec![Principal {
            name: "dave".into(),
            uuid: "1a2b3c4d-0001-0004-ffff000000000004".into(),
            typ: "User".into(),
        }];
        let c = check_access(&dave, &[], &m, &touch(Some(assigned)), 0);
        assert_eq!(c.verdict, Verdict::NoAccessRight);
        assert!(
            c.warnings
                .iter()
                .any(|w| w.contains("not in the program file"))
        );
        assert!(c.warnings.iter().any(|w| w.contains("added after")));
    }
}
