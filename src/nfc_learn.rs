//! `lox access learn`: read an NFC tag's ID at a Code Touch. Starts the
//! Code Touch's learn mode over the WebSocket (`nfc/startlearn`), waits for
//! its `nfcLearnResult` state to report a tag, and always ends learn mode
//! again (`nfc/stoplearn`) — on success, error, timeout and cancel alike.
//! A secured Code Touch (`isSecured`) takes both only as secured commands,
//! each signed with a fresh key (see [`crate::visu`]).

use anyhow::{Context, Result, bail};
use futures_util::future::BoxFuture;
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::Value;
use std::future::Future;
use std::time::Duration;

use crate::visu::{self, VisuPassword};
use loxone_client::stream::{Incoming, Session, StateEvent};

/// The two directions of a subscribed session: commands out, state events
/// and command answers in. The WebSocket in production, a script in tests.
pub trait LearnIo: Send {
    fn send<'a>(&'a mut self, cmd: &'a str) -> BoxFuture<'a, Result<()>>;
    /// The next message; `None` when the Miniserver closed the session.
    fn next(&mut self) -> BoxFuture<'_, Result<Option<Incoming>>>;
}

impl LearnIo for Session {
    fn send<'a>(&'a mut self, cmd: &'a str) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move { Ok(Session::send(self, cmd).await?) })
    }
    fn next(&mut self) -> BoxFuture<'_, Result<Option<Incoming>>> {
        Box::pin(async move { Ok(Session::next(self).await?) })
    }
}

/// How long the initial state dump may take to show the last learn result
/// before learn mode starts anyway.
const SNAPSHOT_WAIT: Duration = Duration::from_secs(5);

/// How long to wait for the answer to `nfc/stoplearn`.
const STOP_WAIT: Duration = Duration::from_secs(2);

/// How long to wait for the key and salt for a secured command.
const SALT_WAIT: Duration = Duration::from_secs(5);

/// The longest learn session lox asks for.
pub const MAX_TIMEOUT: Duration = Duration::from_secs(300);

const START: &str = "nfc/startlearn";
const STOP: &str = "nfc/stoplearn";

/// A Code Touch that can learn: its UUID, name and `nfcLearnResult` state.
#[derive(Debug, Clone)]
pub struct Target {
    pub uuid: String,
    pub name: String,
    pub result_state: String,
    /// `isSecured`: learn mode only as a secured command
    pub secured: bool,
    /// The user lox connects with, whose visualization password signs
    /// secured commands
    pub user: String,
    /// That password; needed when `secured`
    pub visu_password: Option<VisuPassword>,
}

impl Target {
    /// `action` as a plain command.
    fn plain_cmd(&self, action: &str) -> String {
        format!("jdev/sps/io/{}/{}", self.uuid, action)
    }

    /// `action` as it goes out, for a dry run: secured commands with a
    /// placeholder for the hash.
    fn shown_cmds(&self, action: &str) -> Vec<String> {
        if self.secured {
            vec![
                format!("WS {}", visu::salt_cmd(&self.user)),
                format!("WS jdev/sps/ios/<hash>/{}/{}", self.uuid, action),
            ]
        } else {
            vec![format!("WS {}", self.plain_cmd(action))]
        }
    }

    /// The error when a secured Code Touch has no password to sign with.
    pub fn needs_password(&self) -> Option<anyhow::Error> {
        (self.secured && self.visu_password.is_none())
            .then(|| visu::missing(&self.name, &self.user))
    }
}

/// The Code Touch a query means, with its learn-result state from the
/// structure file.
pub fn target(lox: &mut loxone_client::client::LoxClient, query: &str) -> Result<Target> {
    let (uuid, name) = crate::usermgmt::find_code_touch(lox, query, &[])?;
    let ctrl = lox.get_control_json(&uuid)?;
    let result_state = ctrl["states"]["nfcLearnResult"]
        .as_str()
        .with_context(|| {
            format!(
                "{} has no nfcLearnResult state — learning NFC tags needs Miniserver \
                 firmware 10.2 or newer",
                name
            )
        })?
        .to_string();
    Ok(Target {
        uuid,
        name,
        result_state,
        secured: ctrl["isSecured"].as_bool().unwrap_or(false),
        user: lox.connection().user.clone(),
        visu_password: None,
    })
}

/// One tag in an `nfcLearnResult`, as the Miniserver reports it.
#[derive(Debug, Clone, PartialEq)]
pub struct LearnResult {
    pub id: String,
    pub name: Option<String>,
    pub user_uuid: Option<String>,
    pub device_uuid: Option<String>,
}

/// A field that is "0", 0, "" or missing when unset.
fn set_field(v: &Value, key: &str) -> Option<String> {
    let s = crate::util::json_val_str(&v[key])?;
    let s = s.trim();
    (!s.is_empty() && s != "0").then(|| s.to_string())
}

/// The tag in an `nfcLearnResult` text: an object, or (as the structure
/// file documents it) a list of them, of which the last counts. `None` for an
/// empty or cleared state.
pub fn parse_learn_result(text: &str) -> Result<Option<LearnResult>> {
    let text = text.trim();
    if text.is_empty() || text == "0" {
        return Ok(None);
    }
    let v: Value = serde_json::from_str(text).context("the learn result is not JSON")?;
    let tag = match &v {
        Value::Array(list) => match list.last() {
            Some(t) => t,
            None => return Ok(None),
        },
        Value::Object(_) => &v,
        _ => bail!("unexpected learn result: {}", text),
    };
    let Some(id) = set_field(tag, "id") else {
        return Ok(None);
    };
    Ok(Some(LearnResult {
        id,
        name: set_field(tag, "name"),
        user_uuid: set_field(tag, "userUuid"),
        device_uuid: set_field(tag, "deviceUuid"),
    }))
}

/// An `nfcLearnResult` entry without an ID: `Some(true)` when it names a
/// user, a tag name or a Code Touch. A tag that is already known opens the
/// door in learn mode instead of being learned, and the Miniserver then
/// reports such an entry. `None` for an entry with an ID, or no entry.
fn entry_without_id(text: &str) -> Option<bool> {
    let v: Value = serde_json::from_str(text.trim()).ok()?;
    let tag = match &v {
        Value::Array(list) => list.last()?,
        Value::Object(_) => &v,
        _ => return None,
    };
    if set_field(tag, "id").is_some() {
        return None;
    }
    Some(
        ["name", "userUuid", "deviceUuid"]
            .iter()
            .any(|k| set_field(tag, k).is_some()),
    )
}

/// What to say about a tag that was not learned because it is known.
fn known_tag_hint(touch: &str) -> String {
    format!(
        "the tag is probably paired already, so {} opened the door instead of reading it. \
         Only unpaired tags can be learned. `lox user show` lists a user's tags",
        touch
    )
}

/// The error an ID of zeros with a code in the last byte stands for.
pub fn learn_error(id: &str) -> Option<String> {
    let bytes: Vec<&str> = id.split_whitespace().collect();
    let (last, rest) = bytes.split_last()?;
    if rest.is_empty() || !rest.iter().all(|b| *b == "00") {
        return None;
    }
    Some(match last.to_ascii_uppercase().as_str() {
        "E8" => "the Code Touch could not read the tag (read error). Hold it still against \
                 the reader and try again"
            .to_string(),
        "EE" => "the tag refused authentication (authentication error). Unlock the phone, \
                 or use a tag type the Code Touch supports"
            .to_string(),
        "EF" | "00" => "the Code Touch's NFC reader failed to start (init error)".to_string(),
        code => format!("the Code Touch could not read the tag (error {})", code),
    })
}

/// The `Code` of a command answer that is about `cmd_suffix`.
fn reply_code(text: &str, cmd_suffix: &str) -> Option<u16> {
    let v: Value = serde_json::from_str(text.trim()).ok()?;
    let ll = v.get("LL")?;
    let control = ll.get("control").and_then(Value::as_str).unwrap_or("");
    if !control.ends_with(cmd_suffix) {
        return None;
    }
    loxone_client::system::code_of(&v)
}

/// Why `nfc/startlearn` was refused.
fn start_refused(code: u16, target: &Target) -> anyhow::Error {
    let touch = &target.name;
    match code {
        423 => anyhow::anyhow!(
            "{} cannot learn NFC tags right now (code 423): it is offline, not set up, or \
             battery powered (an NFC Code Touch Air cannot learn)",
            touch
        ),
        500 if target.secured => anyhow::anyhow!(
            "the Miniserver refused the secured learn command at {} (code 500): wrong \
             visualization password, or the user {} has none. Check LOX_VISU_PASSWORD or the \
             password file, or set a password for the user in Loxone Config or the app",
            touch,
            target.user
        ),
        401 | 403 | 500 => anyhow::anyhow!(
            "the Miniserver refused to start learning at {} (code {}). Connect as an admin \
             with a visualization password: `lox setup set --user <admin>`",
            touch,
            code
        ),
        _ => anyhow::anyhow!(
            "the Miniserver refused to start learning at {} (code {})",
            touch,
            code
        ),
    }
}

/// A learn session's outcome: the tag, and a problem ending learn mode.
#[derive(Debug)]
pub struct Learned {
    pub tag: LearnResult,
    pub stop_warning: Option<String>,
}

/// Wait for the initial state dump to show the result state's current value
/// (the last learned tag), so it is not taken for a new one. Returns it.
async fn snapshot<I: LearnIo + ?Sized>(io: &mut I, state: &str) -> Result<Option<String>> {
    let wait = async {
        while let Some(msg) = io.next().await? {
            let Incoming::States(events) = msg else {
                continue;
            };
            let mut text_dump = false;
            for e in events {
                if let StateEvent::TextState { uuid, text, .. } = e {
                    if uuid == state {
                        return Ok(Some(text));
                    }
                    text_dump = true;
                }
            }
            // the text states came in one table without this one
            if text_dump {
                return Ok(None);
            }
        }
        bail!("the Miniserver closed the connection before learning started")
    };
    match tokio::time::timeout(SNAPSHOT_WAIT, wait).await {
        Ok(r) => r,
        Err(_) => Ok(None),
    }
}

/// Send `action` to the Code Touch: plain, or for a secured one, a fresh
/// key and salt first and then the command signed with them.
async fn send_action<I: LearnIo + ?Sized>(io: &mut I, touch: &Target, action: &str) -> Result<()> {
    if !touch.secured {
        return io.send(&touch.plain_cmd(action)).await;
    }
    let Some(password) = &touch.visu_password else {
        return Err(visu::missing(&touch.name, &touch.user));
    };
    io.send(&visu::salt_cmd(&touch.user)).await?;
    let answer = async {
        while let Some(msg) = io.next().await? {
            if let Incoming::Reply(text) = msg
                && let Some(salt) = visu::parse_salt_reply(&text)
            {
                return salt;
            }
        }
        bail!("the Miniserver closed the connection")
    };
    let salt = tokio::time::timeout(SALT_WAIT, answer)
        .await
        .map_err(|_| {
            anyhow::anyhow!(
                "the Miniserver did not answer getvisusalt within {} s",
                SALT_WAIT.as_secs()
            )
        })??;
    let cmd = visu::secured_cmd(password, &salt, &touch.uuid, action)?;
    io.send(&cmd).await
}

/// Wait for a tag after `nfc/startlearn` was sent. `before` is the result
/// state's value before; `no_id` is set when an entry without an ID came.
async fn wait_for_tag<I: LearnIo + ?Sized>(
    io: &mut I,
    touch: &Target,
    before: Option<&str>,
    no_id: &mut bool,
) -> Result<LearnResult> {
    while let Some(msg) = io.next().await? {
        match msg {
            Incoming::Reply(text) => {
                if let Some(code) = reply_code(&text, START)
                    && code != 200
                {
                    return Err(start_refused(code, touch));
                }
            }
            Incoming::States(events) => {
                for e in events {
                    let StateEvent::TextState { uuid, text, .. } = e else {
                        continue;
                    };
                    if uuid != touch.result_state {
                        continue;
                    }
                    if let Some(known) = entry_without_id(&text) {
                        *no_id = true;
                        // a known tag; the old value again proves nothing
                        if known && before != Some(text.as_str()) {
                            bail!("no tag was learned: {}", known_tag_hint(&touch.name));
                        }
                        continue;
                    }
                    if let Some(tag) = parse_learn_result(&text)? {
                        if let Some(err) = learn_error(&tag.id) {
                            bail!("{}", err);
                        }
                        return Ok(tag);
                    }
                }
            }
        }
    }
    bail!("the Miniserver closed the connection while learning")
}

/// End learn mode; a problem doing so is returned as a warning.
async fn stop<I: LearnIo + ?Sized>(io: &mut I, touch: &Target) -> Option<String> {
    let warn = |e: String| {
        Some(format!(
            "could not end learn mode at {}: {} — it ends by itself after a while",
            touch.name, e
        ))
    };
    if let Err(e) = send_action(io, touch, STOP).await {
        return warn(format!("{:#}", e));
    }
    let answer = async {
        while let Ok(Some(msg)) = io.next().await {
            if let Incoming::Reply(text) = msg
                && let Some(code) = reply_code(&text, STOP)
            {
                return Some(code);
            }
        }
        None
    };
    match tokio::time::timeout(STOP_WAIT, answer).await {
        Ok(Some(code)) if code != 200 => warn(format!("the Miniserver answered code {}", code)),
        // 200, or no answer in time: the command went out
        _ => None,
    }
}

/// Start learn mode at `touch`, wait up to `timeout` for a tag, end learn
/// mode. `cancel` ends the wait early (Ctrl-C, a cancelled MCP request).
pub async fn learn<I, C>(
    io: &mut I,
    touch: &Target,
    timeout: Duration,
    cancel: C,
) -> Result<Learned>
where
    I: LearnIo + ?Sized,
    C: Future<Output = ()>,
{
    if let Some(e) = touch.needs_password() {
        return Err(e);
    }
    let before = snapshot(io, &touch.result_state).await?;
    let mut no_id = false;
    let outcome = async {
        send_action(io, touch, START).await?;
        tokio::select! {
            r = wait_for_tag(io, touch, before.as_deref(), &mut no_id) => r,
            _ = tokio::time::sleep(timeout) => {
                let mut msg = format!(
                    "no NFC tag was read at {} within {} s",
                    touch.name,
                    timeout.as_secs()
                );
                // the state may not change when the same tag is learned again
                if no_id {
                    msg.push_str(&format!(": {}", known_tag_hint(&touch.name)));
                } else if let Some(Ok(Some(last))) = before.as_deref().map(parse_learn_result) {
                    msg.push_str(&format!(
                        " (if it was the tag learned last, {}, the Miniserver may not \
                         report it again)",
                        last.id
                    ));
                }
                Err(anyhow::anyhow!(msg))
            }
            _ = cancel => Err(anyhow::anyhow!("learning cancelled")),
        }
    }
    .await;
    let stop_warning = stop(io, touch).await;
    match outcome {
        Ok(tag) => Ok(Learned { tag, stop_warning }),
        Err(e) => Err(match stop_warning {
            Some(w) => e.context(w),
            None => e,
        }),
    }
}

/// A tag read at a Code Touch, with what the Miniserver knows about it.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct LearnedTag {
    pub ok: bool,
    pub code_touch: String,
    pub code_touch_uuid: String,
    /// The tag ID as the Miniserver reports it (hex bytes)
    pub id: String,
    /// A user or Code Touch has this tag already
    pub known: bool,
    /// Known tag: its name
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Known tag: the user it is paired with
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_uuid: Option<String>,
    /// Known tag: the Code Touch it is linked to
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_uuid: Option<String>,
    /// Nothing was sent (dry run): the commands it would send
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub would_send: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

impl LearnedTag {
    /// The dry run of learning at `touch`.
    pub fn planned(touch: &Target, timeout: Duration) -> Self {
        Self {
            ok: true,
            code_touch: touch.name.clone(),
            code_touch_uuid: touch.uuid.clone(),
            id: String::new(),
            known: false,
            name: None,
            user: None,
            user_uuid: None,
            device_uuid: None,
            would_send: [
                touch.shown_cmds(START),
                vec![format!(
                    "(wait up to {} s for nfcLearnResult {})",
                    timeout.as_secs(),
                    touch.result_state
                )],
                touch.shown_cmds(STOP),
            ]
            .concat(),
            warnings: Vec::new(),
        }
    }

    /// A learned tag; `users` names the user it is paired with.
    pub fn new(touch: &Target, learned: Learned, users: &[crate::usermgmt::UserSummary]) -> Self {
        let t = learned.tag;
        let user = t.user_uuid.as_ref().and_then(|u| {
            users
                .iter()
                .find(|s| s.uuid.eq_ignore_ascii_case(u))
                .map(|s| s.name.clone())
        });
        Self {
            ok: true,
            code_touch: touch.name.clone(),
            code_touch_uuid: touch.uuid.clone(),
            known: t.name.is_some() || t.user_uuid.is_some() || t.device_uuid.is_some(),
            id: t.id,
            name: t.name,
            user,
            user_uuid: t.user_uuid,
            device_uuid: t.device_uuid,
            would_send: Vec::new(),
            warnings: learned.stop_warning.into_iter().collect(),
        }
    }

    /// One line: the ID and whether (and to whom) it is known.
    pub fn describe(&self) -> String {
        if !self.known {
            return format!("{} — a new tag, not paired with any user", self.id);
        }
        let mut parts = Vec::new();
        if let Some(n) = &self.name {
            parts.push(format!("'{}'", n));
        }
        match (&self.user, &self.user_uuid) {
            (Some(u), _) => parts.push(format!("of the user {}", u)),
            (None, Some(u)) => parts.push(format!("of the user {}", u)),
            _ => {}
        }
        if let Some(d) = &self.device_uuid {
            parts.push(format!("linked to the Code Touch {}", d));
        }
        format!("{} — known: {}", self.id, parts.join(", "))
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    pub const TOUCH: &str = "0f1e2d3c-000d-1111-ffff00000000000d";
    pub const RESULT: &str = "0f1e2d3c-000d-2222-ffff00000000000d";

    /// A scripted session, like the Miniserver: `dump` is queued from the
    /// start, `on_start` is queued behind it once `nfc/startlearn` is sent,
    /// `nfc/stoplearn` is answered with 200, and `sent` records the commands.
    /// An empty queue waits forever.
    #[derive(Default, Clone)]
    pub struct FakeIo {
        pub queue: Arc<Mutex<VecDeque<Incoming>>>,
        pub on_start: Arc<Mutex<Vec<Incoming>>>,
        pub sent: Arc<Mutex<Vec<String>>>,
    }

    impl FakeIo {
        pub fn new(dump: Vec<Incoming>, on_start: Vec<Incoming>) -> Self {
            Self {
                queue: Arc::new(Mutex::new(dump.into())),
                on_start: Arc::new(Mutex::new(on_start)),
                sent: Arc::default(),
            }
        }
    }

    impl LearnIo for FakeIo {
        fn send<'a>(&'a mut self, cmd: &'a str) -> BoxFuture<'a, Result<()>> {
            self.sent.lock().unwrap().push(cmd.to_string());
            if cmd.ends_with("nfc/startlearn") {
                let answers = std::mem::take(&mut *self.on_start.lock().unwrap());
                self.queue.lock().unwrap().extend(answers);
            }
            if cmd.ends_with("nfc/stoplearn") {
                self.queue.lock().unwrap().push_back(reply(cmd, "200"));
            }
            if cmd.contains("getvisusalt") {
                self.queue.lock().unwrap().push_back(salt_reply(cmd));
            }
            Box::pin(async { Ok(()) })
        }
        fn next(&mut self) -> BoxFuture<'_, Result<Option<Incoming>>> {
            let msg = self.queue.lock().unwrap().pop_front();
            Box::pin(async move {
                match msg {
                    Some(m) => Ok(Some(m)),
                    None => std::future::pending().await,
                }
            })
        }
    }

    pub fn text_state(uuid: &str, text: &str) -> Incoming {
        Incoming::States(vec![StateEvent::TextState {
            uuid: uuid.to_string(),
            icon_uuid: String::new(),
            text: text.to_string(),
        }])
    }

    pub fn reply(cmd: &str, code: &str) -> Incoming {
        Incoming::Reply(
            serde_json::json!({ "LL": { "control": cmd, "value": "", "Code": code } }).to_string(),
        )
    }

    /// A synthetic single-use key and salt.
    pub fn salt_reply(cmd: &str) -> Incoming {
        Incoming::Reply(
            serde_json::json!({ "LL": { "control": cmd, "Code": "200", "value": {
                "key": "0123456789abcdef0123456789abcdef",
                "salt": "73616c7479",
                "hashAlg": "SHA256",
            } } })
            .to_string(),
        )
    }

    pub fn target() -> Target {
        Target {
            uuid: TOUCH.to_string(),
            name: "Code Touch Eingang".to_string(),
            result_state: RESULT.to_string(),
            secured: false,
            user: "alice".to_string(),
            visu_password: None,
        }
    }

    const OLD: &str = r#"{"id":"0A 0B 0C 0D 0E 0F 10 11 EC","name":"old tag","userUuid":"1a2b3c4d-0001-0002-ffff000000000002","deviceUuid":0}"#;
    const NEW: &str = r#"{"id":"01 02 03 04 05 06 07 08 EC","name":0,"userUuid":0,"deviceUuid":0}"#;

    fn never() -> std::future::Pending<()> {
        std::future::pending()
    }

    fn sent(io: &FakeIo) -> Vec<String> {
        io.sent.lock().unwrap().clone()
    }

    #[tokio::test(start_paused = true)]
    async fn a_new_tag_after_the_start_is_returned_and_learn_mode_ends() {
        let t = target();
        // the dump: value states first, then the text states with the last tag
        let values = Incoming::States(vec![StateEvent::ValueState {
            uuid: "0f1e2d3c-000d-3333-ffff00000000000d".to_string(),
            value: 0.0,
        }]);
        let mut io = FakeIo::new(
            vec![values, text_state(RESULT, OLD)],
            vec![
                reply(&format!("jdev/sps/io/{}/nfc/startlearn", TOUCH), "200"),
                text_state(RESULT, ""),
                text_state(RESULT, NEW),
            ],
        );
        let learned = learn(&mut io, &t, Duration::from_secs(5), never())
            .await
            .unwrap();
        assert_eq!(learned.tag.id, "01 02 03 04 05 06 07 08 EC");
        assert_eq!(learned.tag.name, None);
        assert_eq!(learned.tag.user_uuid, None);
        assert!(learned.stop_warning.is_none());
        assert_eq!(sent(&io), vec![t.plain_cmd(START), t.plain_cmd(STOP)]);
    }

    #[tokio::test(start_paused = true)]
    async fn the_last_learned_tag_in_the_initial_dump_is_ignored() {
        let t = target();
        // the dump shows the old tag; nothing new comes: no result
        let mut io = FakeIo::new(vec![text_state(RESULT, OLD)], vec![]);
        let err = learn(&mut io, &t, Duration::from_millis(50), never())
            .await
            .unwrap_err();
        let msg = format!("{:#}", err);
        assert!(msg.contains("no NFC tag was read"), "{}", msg);
        assert!(msg.contains("0A 0B 0C"), "names the last tag: {}", msg);
        assert_eq!(sent(&io), vec![t.plain_cmd(START), t.plain_cmd(STOP)]);
    }

    #[tokio::test(start_paused = true)]
    async fn a_known_tag_carries_name_and_user() {
        let t = target();
        let mut io = FakeIo::new(
            vec![text_state("other-state", "x")],
            vec![text_state(RESULT, &format!("[{}]", OLD))],
        );
        let learned = learn(&mut io, &t, Duration::from_secs(5), never())
            .await
            .unwrap();
        assert_eq!(learned.tag.name.as_deref(), Some("old tag"));
        assert_eq!(
            learned.tag.user_uuid.as_deref(),
            Some("1a2b3c4d-0001-0002-ffff000000000002")
        );
        assert_eq!(learned.tag.device_uuid, None);
    }

    #[tokio::test(start_paused = true)]
    async fn read_errors_are_explained_and_learn_mode_still_ends() {
        let t = target();
        for (code, words) in [
            ("E8", "read error"),
            ("EE", "authentication error"),
            ("EF", "init error"),
        ] {
            let err_id = format!(r#"{{"id":"00 00 00 00 00 00 00 00 {}"}}"#, code);
            let mut io = FakeIo::new(vec![], vec![text_state(RESULT, &err_id)]);
            let err = learn(&mut io, &t, Duration::from_secs(5), never())
                .await
                .unwrap_err();
            assert!(format!("{:#}", err).contains(words), "{}: {:#}", code, err);
            assert_eq!(sent(&io).last(), Some(&t.plain_cmd(STOP)));
        }
    }

    #[tokio::test(start_paused = true)]
    async fn a_refused_start_says_why_and_still_sends_stop() {
        let t = target();
        for (code, words) in [("423", "offline"), ("500", "admin")] {
            let mut io = FakeIo::new(
                vec![],
                vec![reply(&format!("dev/sps/io/{}/nfc/startlearn", TOUCH), code)],
            );
            let err = learn(&mut io, &t, Duration::from_secs(5), never())
                .await
                .unwrap_err();
            assert!(format!("{:#}", err).contains(words), "{}: {:#}", code, err);
            assert_eq!(sent(&io), vec![t.plain_cmd(START), t.plain_cmd(STOP)]);
        }
    }

    #[tokio::test(start_paused = true)]
    async fn cancelling_still_sends_stop() {
        let t = target();
        let mut io = FakeIo::new(vec![], vec![]);
        let err = learn(&mut io, &t, Duration::from_secs(60), async {})
            .await
            .unwrap_err();
        assert!(format!("{:#}", err).contains("cancelled"));
        assert_eq!(sent(&io), vec![t.plain_cmd(START), t.plain_cmd(STOP)]);
    }

    fn secured() -> Target {
        Target {
            secured: true,
            visu_password: Some(crate::visu::tests::password("correct horse")),
            ..target()
        }
    }

    // the hash of the synthetic key, salt and password (see visu::tests)
    const HASH: &str = "14bcf905db0aba629be845ec52001f25b98c2f1a689cca7248f35bbc0d22a6e4";

    #[tokio::test(start_paused = true)]
    async fn a_secured_touch_gets_signed_commands_with_a_fresh_key_each() {
        let t = secured();
        let mut io = FakeIo::new(vec![], vec![text_state(RESULT, NEW)]);
        let learned = learn(&mut io, &t, Duration::from_secs(5), never())
            .await
            .unwrap();
        assert_eq!(learned.tag.id, "01 02 03 04 05 06 07 08 EC");
        assert!(learned.stop_warning.is_none());
        assert_eq!(
            sent(&io),
            vec![
                "jdev/sys/getvisusalt/alice".to_string(),
                format!("jdev/sps/ios/{}/{}/nfc/startlearn", HASH, TOUCH),
                "jdev/sys/getvisusalt/alice".to_string(),
                format!("jdev/sps/ios/{}/{}/nfc/stoplearn", HASH, TOUCH),
            ]
        );
        assert!(sent(&io).iter().all(|c| !c.contains("correct horse")));
    }

    #[tokio::test(start_paused = true)]
    async fn a_refused_secured_start_blames_the_visu_password() {
        let t = secured();
        let mut io = FakeIo::new(
            vec![],
            vec![reply(
                &format!("dev/sps/ios/{}/{}/nfc/startlearn", HASH, TOUCH),
                "500",
            )],
        );
        let err = learn(&mut io, &t, Duration::from_secs(5), never())
            .await
            .unwrap_err();
        let msg = format!("{:#}", err);
        assert!(msg.contains("wrong visualization password"), "{}", msg);
        assert!(msg.contains("alice"), "{}", msg);
    }

    #[tokio::test(start_paused = true)]
    async fn a_secured_touch_without_a_password_sends_nothing() {
        let t = Target {
            visu_password: None,
            ..secured()
        };
        let mut io = FakeIo::new(vec![], vec![]);
        let err = learn(&mut io, &t, Duration::from_secs(5), never())
            .await
            .unwrap_err();
        let msg = format!("{:#}", err);
        assert!(
            msg.contains("is secured") && msg.contains("LOX_VISU_PASSWORD"),
            "{}",
            msg
        );
        assert!(sent(&io).is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn a_known_tag_opens_the_door_instead_and_is_reported_at_once() {
        let t = target();
        let known = r#"{"id":"","name":"old tag","userUuid":"1a2b3c4d-0001-0002-ffff000000000002","deviceUuid":0}"#;
        let mut io = FakeIo::new(vec![], vec![text_state(RESULT, known)]);
        let err = learn(&mut io, &t, Duration::from_secs(60), never())
            .await
            .unwrap_err();
        let msg = format!("{:#}", err);
        assert!(msg.contains("paired already"), "{}", msg);
        assert!(!msg.contains("within"), "no timeout: {}", msg);
        assert_eq!(sent(&io).last(), Some(&t.plain_cmd(STOP)));
    }

    #[tokio::test(start_paused = true)]
    async fn an_entry_without_anything_is_named_at_the_timeout() {
        let t = target();
        let empty = r#"{"id":"","name":0,"userUuid":0,"deviceUuid":0}"#;
        let mut io = FakeIo::new(vec![], vec![text_state(RESULT, empty)]);
        let err = learn(&mut io, &t, Duration::from_secs(5), never())
            .await
            .unwrap_err();
        let msg = format!("{:#}", err);
        assert!(msg.contains("within 5 s"), "{}", msg);
        assert!(msg.contains("paired already"), "{}", msg);
    }

    #[tokio::test(start_paused = true)]
    async fn the_known_entry_from_before_the_start_is_not_taken_for_a_new_one() {
        let t = target();
        let known = r#"{"id":"","userUuid":"1a2b3c4d-0001-0002-ffff000000000002"}"#;
        let mut io = FakeIo::new(
            vec![text_state(RESULT, known)],
            vec![text_state(RESULT, known), text_state(RESULT, NEW)],
        );
        let learned = learn(&mut io, &t, Duration::from_secs(5), never())
            .await
            .unwrap();
        assert_eq!(learned.tag.id, "01 02 03 04 05 06 07 08 EC");
    }

    #[test]
    fn a_dry_run_of_a_secured_touch_shows_placeholders() {
        let tag = LearnedTag::planned(&secured(), Duration::from_secs(60));
        assert_eq!(tag.would_send[0], "WS jdev/sys/getvisusalt/alice");
        assert_eq!(
            tag.would_send[1],
            format!("WS jdev/sps/ios/<hash>/{}/nfc/startlearn", TOUCH)
        );
        assert!(tag.would_send[4].ends_with("nfc/stoplearn"));
        assert!(tag.would_send.iter().all(|c| !c.contains("correct horse")));
    }

    #[test]
    fn learn_results_parse_as_object_or_list() {
        assert_eq!(parse_learn_result("").unwrap(), None);
        assert_eq!(parse_learn_result("[]").unwrap(), None);
        let r = parse_learn_result(NEW).unwrap().unwrap();
        assert_eq!(r.id, "01 02 03 04 05 06 07 08 EC");
        assert!(r.name.is_none() && r.user_uuid.is_none() && r.device_uuid.is_none());
        let r = parse_learn_result(&format!("[{}]", OLD)).unwrap().unwrap();
        assert_eq!(r.name.as_deref(), Some("old tag"));
        assert!(parse_learn_result("{not json").is_err());
    }

    #[test]
    fn error_ids_are_recognised() {
        assert!(learn_error("00 00 00 00 00 00 00 00 E8").is_some());
        assert!(learn_error("00 00 00 00 00 00 00 00 00").is_some());
        assert!(learn_error("01 02 03 04 05 06 07 08 E8").is_none());
        assert!(learn_error("E8").is_none());
    }

    #[test]
    fn learned_tags_describe_themselves() {
        let users = vec![crate::usermgmt::UserSummary {
            name: "bob".into(),
            uuid: "1a2b3c4d-0001-0002-ffff000000000002".into(),
            state: 0,
            state_text: "enabled".into(),
            is_admin: false,
        }];
        let learned = Learned {
            tag: parse_learn_result(OLD).unwrap().unwrap(),
            stop_warning: None,
        };
        let tag = LearnedTag::new(&target(), learned, &users);
        assert!(tag.known);
        assert_eq!(tag.user.as_deref(), Some("bob"));
        assert!(
            tag.describe().contains("of the user bob"),
            "{}",
            tag.describe()
        );
        let learned = Learned {
            tag: parse_learn_result(NEW).unwrap().unwrap(),
            stop_warning: None,
        };
        let tag = LearnedTag::new(&target(), learned, &users);
        assert!(!tag.known);
        assert!(tag.describe().contains("new tag"));
    }
}
