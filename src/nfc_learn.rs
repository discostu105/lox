//! `lox access learn`: read an NFC tag's ID at a Code Touch. Starts the
//! Code Touch's learn mode over the WebSocket (`nfc/startlearn`), waits for
//! its `nfcLearnResult` state to report a tag, and always ends learn mode
//! again (`nfc/stoplearn`) — on success, error, timeout and cancel alike.

use anyhow::{Context, Result, bail};
use futures_util::future::BoxFuture;
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::Value;
use std::future::Future;
use std::time::Duration;

use crate::stream::{Incoming, Session, StateEvent};

/// The two directions of a subscribed session: commands out, state events
/// and command answers in. The WebSocket in production, a script in tests.
pub trait LearnIo: Send {
    fn send<'a>(&'a mut self, cmd: &'a str) -> BoxFuture<'a, Result<()>>;
    /// The next message; `None` when the Miniserver closed the session.
    fn next(&mut self) -> BoxFuture<'_, Result<Option<Incoming>>>;
}

impl LearnIo for Session {
    fn send<'a>(&'a mut self, cmd: &'a str) -> BoxFuture<'a, Result<()>> {
        Box::pin(Session::send(self, cmd))
    }
    fn next(&mut self) -> BoxFuture<'_, Result<Option<Incoming>>> {
        Box::pin(Session::next(self))
    }
}

/// How long the initial state dump may take to show the last learn result
/// before learn mode starts anyway.
const SNAPSHOT_WAIT: Duration = Duration::from_secs(5);

/// How long to wait for the answer to `nfc/stoplearn`.
const STOP_WAIT: Duration = Duration::from_secs(2);

/// The longest learn session lox asks for.
pub const MAX_TIMEOUT: Duration = Duration::from_secs(300);

/// A Code Touch that can learn: its UUID, name and `nfcLearnResult` state.
#[derive(Debug, Clone)]
pub struct Target {
    pub uuid: String,
    pub name: String,
    pub result_state: String,
}

impl Target {
    pub fn start_cmd(&self) -> String {
        format!("jdev/sps/io/{}/nfc/startlearn", self.uuid)
    }
    pub fn stop_cmd(&self) -> String {
        format!("jdev/sps/io/{}/nfc/stoplearn", self.uuid)
    }
}

/// The Code Touch a query means, with its learn-result state from the
/// structure file.
pub fn target(lox: &mut crate::client::LoxClient, query: &str) -> Result<Target> {
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
    let s = crate::json_val_str(&v[key])?;
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

/// The error an ID of zeros with a code in the last byte stands for.
pub fn learn_error(id: &str) -> Option<String> {
    let bytes: Vec<&str> = id.split_whitespace().collect();
    let (last, rest) = bytes.split_last()?;
    if rest.is_empty() || !rest.iter().all(|b| *b == "00") {
        return None;
    }
    Some(match last.to_ascii_uppercase().as_str() {
        "E8" => "the Code Touch could not read the tag (read error) — hold it still against \
                 the reader and try again"
            .to_string(),
        "EE" => "the tag refused authentication (authentication error) — a phone must be \
                 unlocked; other tags may be of a type the Code Touch does not support"
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
    crate::json_val_str(ll.get("Code").or_else(|| ll.get("code"))?)?
        .trim()
        .parse()
        .ok()
}

/// Why `nfc/startlearn` was refused.
fn start_refused(code: u16, touch: &str) -> anyhow::Error {
    match code {
        423 => anyhow::anyhow!(
            "{} cannot learn NFC tags right now (code 423): it is offline, not set up, or \
             battery powered (an NFC Code Touch Air cannot learn)",
            touch
        ),
        401 | 403 | 500 => anyhow::anyhow!(
            "the Miniserver refused to start learning at {} (code {}) — the user lox \
             connects with must be an admin with a visualization password; change it with \
             `lox setup set --user <admin>`",
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

/// Wait for a tag after `nfc/startlearn` was sent.
async fn wait_for_tag<I: LearnIo + ?Sized>(io: &mut I, touch: &Target) -> Result<LearnResult> {
    while let Some(msg) = io.next().await? {
        match msg {
            Incoming::Reply(text) => {
                if let Some(code) = reply_code(&text, "nfc/startlearn")
                    && code != 200
                {
                    return Err(start_refused(code, &touch.name));
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
    if let Err(e) = io.send(&touch.stop_cmd()).await {
        return warn(format!("{:#}", e));
    }
    let answer = async {
        while let Ok(Some(msg)) = io.next().await {
            if let Incoming::Reply(text) = msg
                && let Some(code) = reply_code(&text, "nfc/stoplearn")
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
    let before = snapshot(io, &touch.result_state).await?;
    let outcome = async {
        io.send(&touch.start_cmd()).await?;
        tokio::select! {
            r = wait_for_tag(io, touch) => r,
            _ = tokio::time::sleep(timeout) => {
                let mut msg = format!(
                    "no NFC tag was read at {} within {} s",
                    touch.name,
                    timeout.as_secs()
                );
                // the state may not change when the same tag is learned again
                if let Some(Ok(Some(last))) = before.as_deref().map(parse_learn_result) {
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
            would_send: vec![
                format!("WS {}", touch.start_cmd()),
                format!(
                    "(wait up to {} s for nfcLearnResult {})",
                    timeout.as_secs(),
                    touch.result_state
                ),
                format!("WS {}", touch.stop_cmd()),
            ],
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

    pub fn target() -> Target {
        Target {
            uuid: TOUCH.to_string(),
            name: "Code Touch Eingang".to_string(),
            result_state: RESULT.to_string(),
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
        assert_eq!(sent(&io), vec![t.start_cmd(), t.stop_cmd()]);
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
        assert_eq!(sent(&io), vec![t.start_cmd(), t.stop_cmd()]);
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
            assert_eq!(sent(&io).last(), Some(&t.stop_cmd()));
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
            assert_eq!(sent(&io), vec![t.start_cmd(), t.stop_cmd()]);
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
        assert_eq!(sent(&io), vec![t.start_cmd(), t.stop_cmd()]);
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
