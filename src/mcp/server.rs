//! The MCP server: tool definitions (rmcp macros), confirmation of risky
//! actions, and the bridge from async handlers to the blocking Miniserver client.

use anyhow::anyhow;
use rmcp::{
    ErrorData, RoleServer, ServerHandler,
    handler::server::{
        router::tool::ToolRouter,
        tool::{InputResponses, RequestState, ToolCallContext, schema_for_output},
        wrapper::Parameters,
    },
    model::{
        CacheScope, CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock,
        ElicitRequest, ElicitRequestParams, ElicitationSchema, Icon, Implementation, InputRequest,
        InputRequests, InputRequiredResult, ListToolsResult, PaginatedRequestParams,
        ProgressNotificationParam, ProtocolVersion, ResultType, ServerCapabilities, ServerConfig,
    },
    service::{ElicitationError, RequestContext},
    tool, tool_handler, tool_router,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use super::ServerOptions;
use super::ops::{self, ActionResult, ControlFilter, SceneRun, invalid, tool_error};
use crate::actions::{self, Action};
use crate::client::{Control, LoxClient};
use crate::config::Config;
use crate::nfc_learn::{self, LearnIo, LearnedTag};
use crate::usermgmt::{self, UserChange};

/// How long a `LoxClient` (and its in-memory structure) is reused before it is
/// rebuilt from config — picks up `lox cache refresh` and changed credentials.
const CLIENT_TTL: Duration = Duration::from_secs(15 * 60);

/// How long a pending confirmation (MRTR `requestState`) stays valid.
const CONFIRM_TTL: Duration = Duration::from_secs(10 * 60);

/// How long a legacy elicitation waits for the user.
const ELICIT_TIMEOUT: Duration = Duration::from_secs(5 * 60);

/// `tools/list` never changes while the server runs (SEP-2549 cache hint).
const TOOLS_TTL_MS: u64 = 60 * 60 * 1000;

const INSTRUCTIONS: &str = "\
Controls a Loxone smart home through its Miniserver.
Discover first: list_rooms, then list_controls (filter by room, type or name). \
Each control in list_controls names the tool that operates it.
Address controls by name: a case-insensitive substring match. If a name is ambiguous, \
pass `room` or write it as 'Name [Room]'. UUIDs work too.
Read live values with get_control or list_sensors. To explain why something happened \
(what drives a light, which sensors feed a block), use get_wiring. \
For door access at NFC Code Touches: access_check says why a user may or may not open, \
access_log shows who opened when. \
To give someone an NFC tag: learn_nfc_tag reads its ID at a Code Touch (tell the user \
first to have the tag or their unlocked phone ready), then assign_nfc_tag pairs it with \
the user. \
Every action tool accepts dry_run=true to preview the exact commands without sending them.
Doors, gates, the alarm, changes to users (NFC tags, access codes) and controls the \
user listed for confirmation are high-risk: the user is asked to confirm each one. \
If the user declines, do not retry.";

const ICON_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64"><rect width="64" height="64" rx="14" fill="#6dc04b"/><path d="M14 33 32 17l18 16v15a2 2 0 0 1-2 2H38V38H26v12H16a2 2 0 0 1-2-2z" fill="#fff"/></svg>"##;

// ── State ─────────────────────────────────────────────────────────────────────

struct State {
    /// Fixed config (tests); `None` means `Config::load()` on demand.
    cfg: Option<Config>,
    lox: Option<(LoxClient, Instant)>,
}

impl State {
    /// The Miniserver client, created lazily and rebuilt after `CLIENT_TTL`.
    fn client(&mut self) -> anyhow::Result<&mut LoxClient> {
        let stale = self
            .lox
            .as_ref()
            .is_none_or(|(_, at)| at.elapsed() > CLIENT_TTL);
        if stale {
            let cfg = match &self.cfg {
                Some(c) => c.clone(),
                None => Config::load()?,
            };
            self.lox = Some((LoxClient::new(cfg)?, Instant::now()));
        }
        Ok(&mut self.lox.as_mut().expect("client just created").0)
    }
}

/// A risky action waiting for the user's answer (MRTR round trip).
struct Pending {
    /// Binds the answer to the exact control and commands that were shown.
    fingerprint: String,
    created: Instant,
}

// ── Tool parameters ───────────────────────────────────────────────────────────

/// The control an action applies to.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct Target {
    /// Control name: case-insensitive substring, 'Name [Room]', alias, or UUID
    pub name: String,
    /// Room name (substring) to disambiguate the control
    #[serde(default)]
    pub room: Option<String>,
    /// Resolve the control and return the commands without sending them
    #[serde(default)]
    pub dry_run: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ControlQuery {
    /// Control name: case-insensitive substring, 'Name [Room]', alias, or UUID
    pub name: String,
    /// Room name (substring) to disambiguate the control
    #[serde(default)]
    pub room: Option<String>,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct ControlsFilter {
    /// Filter by control name (substring)
    #[serde(default)]
    pub name: Option<String>,
    /// Filter by room (substring)
    #[serde(default)]
    pub room: Option<String>,
    /// Filter by Loxone type, e.g. Jalousie, LightControllerV2, Switch
    #[serde(default, rename = "type")]
    pub typ: Option<String>,
    /// Filter by category (substring)
    #[serde(default)]
    pub category: Option<String>,
    /// Only favorites
    #[serde(default)]
    pub favorites_only: bool,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
#[schemars(inline)]
pub enum SensorKind {
    #[default]
    All,
    Temperature,
    DoorWindow,
    Motion,
    Smoke,
    Energy,
}

impl SensorKind {
    fn as_str(self) -> &'static str {
        match self {
            SensorKind::All => "all",
            SensorKind::Temperature => "temperature",
            SensorKind::DoorWindow => "door-window",
            SensorKind::Motion => "motion",
            SensorKind::Smoke => "smoke",
            SensorKind::Energy => "energy",
        }
    }
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct SensorQuery {
    /// all (default, every sensor except energy meters), temperature, door-window, motion, smoke, or energy
    #[serde(default)]
    pub kind: SensorKind,
    /// Filter by room (substring)
    #[serde(default)]
    pub room: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
#[schemars(inline)]
pub enum TraceDirection {
    Up,
    Down,
    Both,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct WiringQuery {
    /// Control or logic block name: case-insensitive substring, 'Name [Room]', alias, or UUID
    pub name: String,
    /// Room name (substring) to disambiguate
    #[serde(default)]
    pub room: Option<String>,
    /// Follow the wires transitively: up (to the sensors), down (to the actuators) or both. Omit for one hop.
    #[serde(default)]
    pub trace: Option<TraceDirection>,
    /// Hops to follow with `trace` (1–8, default 4)
    #[serde(default)]
    pub depth: Option<usize>,
    /// Also list parameters left at their default value
    #[serde(default)]
    pub all_params: bool,
    /// Download the current config from the Miniserver first (read-only FTP, a few seconds)
    #[serde(default)]
    pub download: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct UserQuery {
    /// User name (exact, or a unique part of it) or UUID
    pub name: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct AccessLogQuery {
    /// NFC Code Touch: name (or a unique part of it), alias or UUID
    pub code_touch: String,
    /// Number of newest entries to return (default 50, 0 = all)
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct AccessCheckQuery {
    /// User name (exact, or a unique part of it) or UUID
    pub user: String,
    /// NFC Code Touch: name, alias or UUID. Omit to check every Code Touch.
    #[serde(default)]
    pub code_touch: Option<String>,
    /// Read the access rights from the cached config instead of downloading the newest one from the Miniserver (read-only FTP, a few seconds). Faster, but may miss recent changes.
    #[serde(default)]
    pub cached: bool,
}

#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
#[schemars(inline)]
pub enum SwitchState {
    On,
    Off,
    Pulse,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SwitchParams {
    #[serde(flatten)]
    pub target: Target,
    /// on, off, or pulse (push-button)
    pub state: SwitchState,
}

#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
#[schemars(inline)]
pub enum BlindAction {
    Up,
    Down,
    Stop,
    Shade,
    Position,
    Slats,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct BlindParams {
    #[serde(flatten)]
    pub target: Target,
    /// up/down (full travel to the end position), stop, shade (automatic shading), position (needs value), slats (needs value)
    pub action: BlindAction,
    /// Required for position and slats (0-100, 100 = closed)
    #[serde(default)]
    #[schemars(range(min = 0, max = 100))]
    pub value: Option<f64>,
}

#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
#[schemars(inline)]
pub enum LightAction {
    Mood,
    Dim,
    Color,
}

/// A number or a string.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(untagged)]
#[schemars(inline)]
pub enum Scalar {
    Number(f64),
    Text(String),
}

impl Scalar {
    fn text(&self) -> String {
        match self {
            Scalar::Number(n) if n.fract() == 0.0 && n.abs() < 1e15 => format!("{}", *n as i64),
            Scalar::Number(n) => n.to_string(),
            Scalar::Text(s) => s.trim().to_string(),
        }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct LightParams {
    #[serde(flatten)]
    pub target: Target,
    /// mood (lighting controller), dim (dimmer, 0-100), or color (color picker)
    pub action: LightAction,
    /// mood: plus | minus | off | <mood id>; dim: 0-100; color: #RRGGBB | hsv(h,s,v) | temp(brightness,kelvin)
    pub value: Scalar,
}

#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
#[schemars(inline)]
pub enum ThermostatAction {
    Temp,
    Mode,
    Override,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ThermostatParams {
    #[serde(flatten)]
    pub target: Target,
    /// temp (comfort °C), mode (operating mode), or override (°C for `minutes`)
    pub action: ThermostatAction,
    /// °C for temp/override, mode name for mode
    pub value: Scalar,
    /// Override duration in minutes (default 60)
    #[serde(default)]
    #[schemars(range(min = 1))]
    pub minutes: Option<u64>,
}

#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
#[schemars(inline)]
pub enum GateAction {
    Open,
    Close,
    Stop,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GateParams {
    #[serde(flatten)]
    pub target: Target,
    /// open and close need the user's confirmation
    pub action: GateAction,
}

#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
#[schemars(inline)]
pub enum AlarmAction {
    Arm,
    ArmHome,
    Disarm,
    Quit,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct AlarmParams {
    #[serde(flatten)]
    pub target: Target,
    /// arm, arm-home and disarm need the user's confirmation; quit acknowledges an alarm
    pub action: AlarmAction,
    /// Arm without motion detectors
    #[serde(default)]
    pub no_motion: bool,
    /// Alarm PIN, if the alarm requires one
    #[serde(default)]
    pub code: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
#[schemars(inline)]
pub enum DoorAction {
    Lock,
    Unlock,
    Open,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DoorParams {
    #[serde(flatten)]
    pub target: Target,
    /// Every door action needs the user's confirmation
    pub action: DoorAction,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SceneParams {
    /// Scene ID from list_scenes
    pub scene: String,
    /// List the resolved steps without sending anything
    #[serde(default)]
    pub dry_run: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SendParams {
    #[serde(flatten)]
    pub target: Target,
    /// Raw Loxone command, e.g. 'on', 'FullUp', 'setComfortTemperature/21'
    pub command: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct LearnParams {
    /// NFC Code Touch: name (or a unique part of it), alias or UUID
    pub code_touch: String,
    /// Seconds to wait for a tag after the user confirms (default 60, at most 300)
    #[serde(default)]
    pub timeout_secs: Option<u64>,
    /// Resolve the Code Touch and return the commands without starting learn mode
    #[serde(default)]
    pub dry_run: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct AssignNfcParams {
    /// User name (exact, or a unique part of it) or UUID
    pub user: String,
    /// Tag ID as learn_nfc_tag returned it (hex bytes, spaces optional)
    pub id: String,
    /// Name for the tag, e.g. 'alice phone'
    pub name: String,
    /// Check the change and return the request without sending it
    #[serde(default)]
    pub dry_run: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RemoveNfcParams {
    /// User name (exact, or a unique part of it) or UUID
    pub user: String,
    /// The tag: its ID or its name
    pub tag: String,
    /// Check the change and return the request without sending it
    #[serde(default)]
    pub dry_run: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct AccessCodeParams {
    /// User name (exact, or a unique part of it) or UUID
    pub user: String,
    /// The new code: 2 to 8 digits. Never repeated in results or errors.
    pub code: String,
    /// Check the change and return the request (code masked) without sending it
    #[serde(default)]
    pub dry_run: bool,
}

/// The form the user fills in to confirm a high-risk action.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct Confirm {
    /// Check to carry out the action
    pub confirm: bool,
}

rmcp::elicit_safe!(Confirm);

// ── Server ────────────────────────────────────────────────────────────────────

/// The outcome of asking the user to confirm a risky action.
enum Confirmation {
    Confirmed,
    Declined,
    /// The client cannot ask the user (no elicitation support).
    Unsupported,
    /// MRTR: hand this back to the client; it retries with the answer.
    Ask(InputRequiredResult),
}

type ToolResponse = Result<CallToolResponse, ErrorData>;

fn respond<T: Serialize>(result: anyhow::Result<T>) -> ToolResponse {
    match result {
        Ok(v) => {
            let value = serde_json::to_value(v)
                .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
            Ok(CallToolResult::structured(value).into())
        }
        Err(e) => fail(e),
    }
}

/// rmcp reports arguments that don't fit the input schema (unknown enum value,
/// missing field) as a plain-text tool error with this prefix.
const ARGUMENT_ERROR_PREFIX: &str = "failed to deserialize parameters:";

/// The message of rmcp's argument error, if this response is one.
fn argument_error(response: &CallToolResponse) -> Option<String> {
    let CallToolResponse::Complete(result) = response else {
        return None;
    };
    if result.is_error != Some(true) {
        return None;
    }
    let text = &result.content.first()?.as_text()?.text;
    let detail = text.strip_prefix(ARGUMENT_ERROR_PREFIX)?.trim();
    Some(format!("Invalid arguments: {}", detail))
}

/// A tool error the model sees: the CLI's JSON error envelope as text.
fn fail(e: anyhow::Error) -> ToolResponse {
    let envelope = ops::error_envelope(&e);
    let text = serde_json::to_string_pretty(&envelope).unwrap_or_else(|_| envelope.to_string());
    Ok(CallToolResult::error(vec![ContentBlock::text(text)]).into())
}

fn modern(ctx: &RequestContext<RoleServer>) -> bool {
    ctx.protocol_version()
        .is_some_and(|v| v.as_str() >= ProtocolVersion::V_2026_07_28.as_str())
}

/// Opens the session an NFC learn runs on: the Miniserver's WebSocket, or a
/// script in tests.
pub type LearnConnector = Arc<
    dyn Fn(Config) -> futures_util::future::BoxFuture<'static, anyhow::Result<Box<dyn LearnIo>>>
        + Send
        + Sync,
>;

fn websocket_learn() -> LearnConnector {
    Arc::new(|cfg| {
        Box::pin(async move {
            let session = crate::stream::Session::open(&cfg, Duration::from_secs(20)).await?;
            Ok(Box::new(session) as Box<dyn LearnIo>)
        })
    })
}

#[derive(Clone)]
pub struct LoxMcp {
    opts: ServerOptions,
    state: Arc<Mutex<State>>,
    pending: Arc<Mutex<HashMap<String, Pending>>>,
    learn_io: LearnConnector,
    tool_router: ToolRouter<Self>,
}

/// Tools that change something; hidden with `--read-only`.
const ACTION_TOOLS: &[&str] = &[
    "switch",
    "blind",
    "light",
    "thermostat",
    "gate",
    "alarm",
    "door",
    "run_scene",
    "send_command",
    "learn_nfc_tag",
    "assign_nfc_tag",
    "remove_nfc_tag",
    "set_access_code",
];

impl LoxMcp {
    pub fn new(opts: ServerOptions) -> Self {
        Self::build(opts, None)
    }

    #[cfg(test)]
    pub fn with_config(opts: ServerOptions, cfg: Config) -> Self {
        Self::build(opts, Some(cfg))
    }

    /// Learn NFC tags on `connector`'s sessions instead of the WebSocket.
    #[cfg(test)]
    pub fn with_learn_io(mut self, connector: LearnConnector) -> Self {
        self.learn_io = connector;
        self
    }

    fn build(opts: ServerOptions, cfg: Option<Config>) -> Self {
        let mut tool_router = Self::tool_router();
        if opts.read_only {
            for t in ACTION_TOOLS {
                tool_router.remove_route(t);
            }
        } else if !opts.allow_raw {
            tool_router.remove_route("send_command");
        }
        Self {
            opts,
            state: Arc::new(Mutex::new(State { cfg, lox: None })),
            pending: Arc::new(Mutex::new(HashMap::new())),
            learn_io: websocket_learn(),
            tool_router,
        }
    }

    /// The tool definitions this server exposes, in `tools/list` order.
    pub fn tools(&self) -> Vec<rmcp::model::Tool> {
        self.tool_router.list_all()
    }

    /// Run blocking Miniserver work on tokio's blocking pool.
    async fn blocking<T, F>(&self, f: F) -> anyhow::Result<T>
    where
        F: FnOnce(&mut LoxClient) -> anyhow::Result<T> + Send + 'static,
        T: Send + 'static,
    {
        let state = self.state.clone();
        tokio::task::spawn_blocking(move || {
            let mut state = state.lock().unwrap_or_else(PoisonError::into_inner);
            f(state.client()?)
        })
        .await
        .map_err(|e| anyhow!("internal error: {}", e))?
    }

    /// Resolve, gate, confirm and send one action.
    async fn act(
        &self,
        target: Target,
        action: anyhow::Result<Action>,
        ctx: RequestContext<RoleServer>,
        request_state: Option<String>,
        input_responses: Option<rmcp::model::InputResponses>,
    ) -> ToolResponse {
        let action = match action {
            Ok(a) => a,
            Err(e) => return fail(invalid(format!("{:#}", e))),
        };
        let Target {
            name,
            room,
            dry_run,
        } = target;
        let dry_run = dry_run || self.opts.dry_run;
        let resolve_action = action.clone();
        let (ctrl, listed) = match self
            .blocking(move |lox| {
                let ctrl = ops::resolve_for_action(lox, &name, room.as_deref(), &resolve_action)?;
                let listed = ops::on_confirm_list(&lox.cfg, &ctrl);
                Ok((ctrl, listed))
            })
            .await
        {
            Ok(c) => c,
            Err(e) => return fail(e),
        };

        let mut out = ops::planned(&ctrl, &action);
        let needs_confirmation =
            (ops::needs_confirmation(&action, &ctrl) || listed) && !self.opts.allow_risky;
        if dry_run {
            out.dry_run = true;
            out.needs_confirmation = needs_confirmation;
            return respond(Ok(out));
        }
        if needs_confirmation {
            match self
                .confirm(&ctx, &ctrl, &action, request_state, input_responses)
                .await
            {
                Ok(Confirmation::Confirmed) => out.confirmed = true,
                Ok(Confirmation::Ask(input)) => return Ok(input.into()),
                Ok(Confirmation::Declined) => {
                    return fail(tool_error(
                        "declined_by_user",
                        format!(
                            "The user declined '{}' on '{}'. Do not retry unless they ask again.",
                            action.describe(),
                            ctrl.name
                        ),
                    ));
                }
                Ok(Confirmation::Unsupported) => {
                    return fail(tool_error(
                        "action_not_allowed",
                        format!(
                            "Refused: '{}' on '{}' needs the user's confirmation (doors, gates, alarm, \
                             and controls on the `confirm:` list in the lox config) and this MCP client cannot ask the user to confirm it. The user can run it \
                             themselves ({}) or restart the server with `lox mcp serve --allow-risky`.",
                            action.describe(),
                            ctrl.name,
                            out.cli
                        ),
                    ));
                }
                Err(e) => return fail(e),
            }
        }
        respond(
            self.blocking(move |lox| {
                ops::send(lox, &ctrl, &action, &mut out)?;
                Ok(out)
            })
            .await,
        )
    }

    /// Ask the user to confirm a risky action.
    ///
    /// Protocol 2026-07-28 uses a multi-round-trip request: the first call
    /// returns `input_required` with an elicitation, the client retries with the
    /// answer and our `requestState`. Older sessions get a regular server-to-client
    /// elicitation request. Either way the question goes to the human, not the model.
    async fn confirm(
        &self,
        ctx: &RequestContext<RoleServer>,
        ctrl: &Control,
        action: &Action,
        request_state: Option<String>,
        input_responses: Option<rmcp::model::InputResponses>,
    ) -> anyhow::Result<Confirmation> {
        let fingerprint = format!("{}|{}", ctrl.uuid, action.commands().join("|"));
        let message = format!(
            "{} — {}{}: {}?\n\nEquivalent command: {}",
            match action {
                Action::Door(_) => "Door lock",
                Action::Gate(_) => "Gate",
                Action::Alarm { .. } => "Alarm",
                _ => "High-risk action",
            },
            ctrl.name,
            ctrl.room
                .as_deref()
                .map(|r| format!(" ({})", r))
                .unwrap_or_default(),
            action.describe(),
            action.to_cli(&ctrl.name, ctrl.room.as_deref()),
        );
        self.ask(ctx, fingerprint, message, request_state, input_responses)
            .await
    }

    /// Ask the user to confirm `message`; `fingerprint` binds a multi-round-trip
    /// answer to exactly what was asked.
    async fn ask(
        &self,
        ctx: &RequestContext<RoleServer>,
        fingerprint: String,
        message: String,
        request_state: Option<String>,
        input_responses: Option<rmcp::model::InputResponses>,
    ) -> anyhow::Result<Confirmation> {
        if modern(ctx) {
            if let Some(state) = request_state {
                let pending = self
                    .pending
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .remove(&state);
                let Some(pending) = pending.filter(|p| p.created.elapsed() < CONFIRM_TTL) else {
                    return Err(tool_error(
                        "confirmation_expired",
                        "The confirmation expired or is unknown. Call the tool again.",
                    ));
                };
                if pending.fingerprint != fingerprint {
                    return Err(tool_error(
                        "confirmation_mismatch",
                        "The confirmation was given for a different action. Call the tool again.",
                    ));
                }
                let answer = input_responses
                    .as_ref()
                    .and_then(|r| r.get("confirm"))
                    .cloned()
                    .unwrap_or(Value::Null);
                let accepted = answer["action"] == "accept" && answer["content"]["confirm"] == true;
                return Ok(if accepted {
                    Confirmation::Confirmed
                } else {
                    Confirmation::Declined
                });
            }
            let can_elicit = ctx
                .client_capabilities()
                .is_some_and(|c| c.elicitation.is_some());
            if !can_elicit {
                return Ok(Confirmation::Unsupported);
            }
            let schema = ElicitationSchema::from_type::<Confirm>()
                .map_err(|e| anyhow!("confirmation schema: {}", e))?;
            let mut requests = InputRequests::new();
            requests.insert(
                "confirm".to_string(),
                InputRequest::Elicitation(ElicitRequest::new(
                    ElicitRequestParams::FormElicitationParams {
                        meta: None,
                        message,
                        requested_schema: schema,
                    },
                )),
            );
            let nonce = uuid::Uuid::new_v4().to_string();
            {
                let mut pending = self.pending.lock().unwrap_or_else(PoisonError::into_inner);
                pending.retain(|_, p| p.created.elapsed() < CONFIRM_TTL);
                pending.insert(
                    nonce.clone(),
                    Pending {
                        fingerprint,
                        created: Instant::now(),
                    },
                );
            }
            return Ok(Confirmation::Ask(InputRequiredResult::new(
                Some(requests),
                Some(nonce),
            )));
        }

        match ctx
            .peer
            .elicit_with_timeout::<Confirm>(message, Some(ELICIT_TIMEOUT))
            .await
        {
            Ok(Some(Confirm { confirm: true })) => Ok(Confirmation::Confirmed),
            Ok(_)
            | Err(ElicitationError::UserDeclined)
            | Err(ElicitationError::UserCancelled)
            | Err(ElicitationError::NoContent) => Ok(Confirmation::Declined),
            Err(ElicitationError::CapabilityNotSupported) => Ok(Confirmation::Unsupported),
            Err(e) => Err(anyhow!("confirmation failed: {}", e)),
        }
    }
}

impl LoxMcp {
    /// Confirm a user-management change or a learn session unless
    /// `--allow-risky`. `None`: go ahead; `Some`: the response to return
    /// (the question, a refusal).
    async fn confirm_change(
        &self,
        ctx: &RequestContext<RoleServer>,
        fingerprint: String,
        question: String,
        cli: &str,
        request_state: Option<String>,
        input_responses: Option<rmcp::model::InputResponses>,
    ) -> Option<ToolResponse> {
        if self.opts.allow_risky {
            return None;
        }
        let message = format!("{}\n\nEquivalent command: {}", question, cli);
        match self
            .ask(ctx, fingerprint, message, request_state, input_responses)
            .await
        {
            Ok(Confirmation::Confirmed) => None,
            Ok(Confirmation::Ask(input)) => Some(Ok(input.into())),
            Ok(Confirmation::Declined) => Some(fail(tool_error(
                "declined_by_user",
                format!(
                    "The user declined: {} Do not retry unless they ask again.",
                    question
                ),
            ))),
            Ok(Confirmation::Unsupported) => Some(fail(tool_error(
                "action_not_allowed",
                format!(
                    "Refused: changes to users and NFC learning need the user's confirmation, \
                     and this MCP client cannot ask the user to confirm. The user can run it \
                     themselves ({}) or restart the server with `lox mcp serve --allow-risky`.",
                    cli
                ),
            ))),
            Err(e) => Some(fail(e)),
        }
    }

    /// Plan, confirm and carry out a change to a user. `code` is the secret of
    /// set_access_code: it is bound to the confirmation by hash only.
    async fn change_user<F>(
        &self,
        plan: F,
        code: Option<String>,
        dry_run: bool,
        ctx: RequestContext<RoleServer>,
        request_state: Option<String>,
        input_responses: Option<rmcp::model::InputResponses>,
    ) -> ToolResponse
    where
        F: FnOnce(&mut LoxClient) -> anyhow::Result<UserChange> + Send + 'static,
    {
        let mut change = match self.blocking(plan).await {
            Ok(c) => c,
            Err(e) => return fail(e),
        };
        if dry_run || self.opts.dry_run {
            change.dry_run = true;
            return respond(Ok(change));
        }
        let mut fingerprint = change.requests.join("|");
        if let Some(code) = &code {
            use sha2::Digest;
            fingerprint.push('|');
            fingerprint.push_str(&hex::encode(sha2::Sha256::digest(code.as_bytes())));
        }
        if let Some(response) = self
            .confirm_change(
                &ctx,
                fingerprint,
                change.question(),
                &change.cli(),
                request_state,
                input_responses,
            )
            .await
        {
            return response;
        }
        change.confirmed = !self.opts.allow_risky;
        respond(
            self.blocking(move |lox| {
                usermgmt::apply(lox, &mut change, code.as_deref())?;
                Ok(change)
            })
            .await,
        )
    }
}

#[tool_router]
impl LoxMcp {
    // ── Read tools ────────────────────────────────────────────────────────────

    #[tool(
        title = "List rooms",
        description = "List all rooms of the Loxone installation with the number of controls in each.",
        output_schema = schema_for_output::<ops::RoomList>(),
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn list_rooms(&self) -> ToolResponse {
        respond(self.blocking(ops::list_rooms).await)
    }

    #[tool(
        title = "List controls",
        description = "List controls (lights, blinds, switches, sensors, …) with type, room, category and UUID. Filters are case-insensitive substrings and can be combined. Each control names the `tool` that operates it.",
        output_schema = schema_for_output::<ops::ControlList>(),
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn list_controls(&self, Parameters(f): Parameters<ControlsFilter>) -> ToolResponse {
        respond(
            self.blocking(move |lox| {
                ops::list_controls(
                    lox,
                    &ControlFilter {
                        name: f.name.as_deref(),
                        room: f.room.as_deref(),
                        typ: f.typ.as_deref(),
                        category: f.category.as_deref(),
                        favorites_only: f.favorites_only,
                    },
                )
            })
            .await,
        )
    }

    #[tool(
        title = "Get control state",
        description = "Read the live state of one control: main value, state attributes such as blind position, and named outputs.",
        output_schema = schema_for_output::<ops::ControlState>(),
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn get_control(&self, Parameters(q): Parameters<ControlQuery>) -> ToolResponse {
        respond(
            self.blocking(move |lox| ops::get_control(lox, &q.name, q.room.as_deref()))
                .await,
        )
    }

    #[tool(
        title = "List sensor readings",
        description = "Current readings of sensors: temperature, door/window contacts, motion/presence, smoke, or energy meters.",
        output_schema = schema_for_output::<ops::SensorList>(),
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn list_sensors(&self, Parameters(q): Parameters<SensorQuery>) -> ToolResponse {
        respond(
            self.blocking(move |lox| ops::list_sensors(lox, q.kind.as_str(), q.room.as_deref()))
                .await,
        )
    }

    #[tool(
        title = "List light moods",
        description = "List the moods (scenes) of a lighting controller with their numeric IDs, for use with the `light` tool (action=mood).",
        output_schema = schema_for_output::<ops::MoodList>(),
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn list_light_moods(&self, Parameters(q): Parameters<ControlQuery>) -> ToolResponse {
        let resolved = self
            .blocking(move |lox| {
                let (ctrl, mood_list) =
                    crate::commands::control::resolve_mood_list(lox, &q.name, q.room.as_deref())?;
                Ok((ctrl, mood_list, lox.cfg.clone()))
            })
            .await;
        let (ctrl, mood_list, cfg) = match resolved {
            Ok(r) => r,
            Err(e) => return fail(e),
        };
        let moods = crate::commands::control::fetch_mood_list_json(&cfg, &mood_list)
            .await
            .and_then(|json| actions::parse_mood_list(&json));
        respond(moods.map(|moods| {
            ops::MoodList {
                control: ops::ControlRef::from(&ctrl),
                moods: moods
                    .into_iter()
                    .map(|m| ops::Mood {
                        id: m.id,
                        name: m.name,
                    })
                    .collect(),
            }
        }))
    }

    #[tool(
        title = "List scenes",
        description = "List the user's lox scenes (named multi-step command sequences) for `run_scene`.",
        output_schema = schema_for_output::<ops::SceneList>(),
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn list_scenes(&self) -> ToolResponse {
        respond(self.blocking(|lox| ops::list_scenes(&lox.cfg)).await)
    }

    #[tool(
        title = "Miniserver status",
        description = "Miniserver firmware version, PLC state and memory usage.",
        output_schema = schema_for_output::<ops::SystemStatus>(),
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn system_status(&self) -> ToolResponse {
        respond(self.blocking(ops::system_status).await)
    }

    #[tool(
        title = "Get wiring",
        description = "Why does a control behave the way it does? Shows the Loxone Config logic around a control or logic block: what is wired to its inputs and outputs (sensors, other blocks, hardware), its non-default parameters, and with `trace` the whole path up to the sensors or down to the actuators. Read-only; reads the config program, not live values (use get_control for those).",
        output_schema = schema_for_output::<ops::Wiring>(),
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn get_wiring(&self, Parameters(q): Parameters<WiringQuery>) -> ToolResponse {
        let trace = q.trace.map(|t| match t {
            TraceDirection::Up => ops::Trace::Up,
            TraceDirection::Down => ops::Trace::Down,
            TraceDirection::Both => ops::Trace::Both,
        });
        respond(
            self.blocking(move |lox| {
                ops::get_wiring(
                    lox,
                    &q.name,
                    q.room.as_deref(),
                    trace,
                    q.depth.unwrap_or(4),
                    q.all_params,
                    q.download,
                )
            })
            .await,
        )
    }

    #[tool(
        title = "List users",
        description = "List the Miniserver's users with their state (enabled, disabled, enabled until/from …) and admin flag. Needs a connection user with the right to manage users.",
        output_schema = schema_for_output::<ops::UserList>(),
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn list_users(&self) -> ToolResponse {
        respond(self.blocking(ops::list_users).await)
    }

    #[tool(
        title = "Get user",
        description = "One user's details: state and validity, groups, NFC tags (name and ID) and the number of access codes (the codes themselves are stored hashed).",
        output_schema = schema_for_output::<crate::usermgmt::User>(),
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn get_user(&self, Parameters(q): Parameters<UserQuery>) -> ToolResponse {
        respond(self.blocking(move |lox| ops::get_user(lox, &q.name)).await)
    }

    #[tool(
        title = "Access log",
        description = "Access log of an NFC Code Touch: who opened when with which NFC tag, code or the app, and denied attempts. Returns the newest entries, oldest first, in local time.",
        output_schema = schema_for_output::<crate::usermgmt::AccessLog>(),
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn access_log(&self, Parameters(q): Parameters<AccessLogQuery>) -> ToolResponse {
        respond(
            self.blocking(move |lox| {
                crate::usermgmt::read_access_log(lox, &q.code_touch, q.limit.unwrap_or(50))
            })
            .await,
        )
    }

    #[tool(
        title = "Check door access",
        description = "Why may (or may not) a user open an NFC Code Touch? Joins the live user (state, groups, NFC tags, access codes) with the access rights in the Loxone Config program. Verdict per Code Touch: granted (with schedule, direct or via a group), no-access-right, no-credential, user-disabled, or unknown-full-access (only the full-access group, not verified to open doors). Warnings flag where the program file and the live state disagree.",
        output_schema = schema_for_output::<crate::usermgmt::AccessReport>(),
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn access_check(&self, Parameters(q): Parameters<AccessCheckQuery>) -> ToolResponse {
        respond(
            self.blocking(move |lox| {
                ops::access_check(lox, &q.user, q.code_touch.as_deref(), q.cached)
            })
            .await,
        )
    }

    // ── Action tools ──────────────────────────────────────────────────────────

    #[tool(
        name = "switch",
        title = "Switch on/off",
        description = "Turn a control on or off, or send a pulse (push-button). Works for switches, lighting controllers and most digital controls.",
        output_schema = schema_for_output::<ActionResult>(),
        annotations(read_only_hint = false, destructive_hint = false, open_world_hint = false)
    )]
    async fn switch(
        &self,
        Parameters(p): Parameters<SwitchParams>,
        ctx: RequestContext<RoleServer>,
        RequestState(state): RequestState,
        InputResponses(responses): InputResponses,
    ) -> ToolResponse {
        let action = match p.state {
            SwitchState::On => Action::On,
            SwitchState::Off => Action::Off,
            SwitchState::Pulse => Action::Pulse,
        };
        self.act(p.target, Ok(action), ctx, state, responses).await
    }

    #[tool(
        title = "Move blinds",
        description = "Move a blind/shade (Jalousie). 'up'/'down' travel all the way to the end position (FullUp/FullDown), 'stop' stops it. Position and slats are 0-100 where 100 is fully closed/down. 'shade' starts automatic shading.",
        output_schema = schema_for_output::<ActionResult>(),
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn blind(
        &self,
        Parameters(p): Parameters<BlindParams>,
        ctx: RequestContext<RoleServer>,
        RequestState(state): RequestState,
        InputResponses(responses): InputResponses,
    ) -> ToolResponse {
        let action = match (p.action, p.value) {
            (BlindAction::Up, _) => actions::parse_blind("up", None),
            (BlindAction::Down, _) => actions::parse_blind("down", None),
            (BlindAction::Stop, _) => actions::parse_blind("stop", None),
            (BlindAction::Shade, _) => actions::parse_blind("shade", None),
            (BlindAction::Position, v) => actions::parse_blind("pos", v),
            (BlindAction::Slats, Some(v)) => actions::parse_blind("shade", Some(v)),
            (BlindAction::Slats, None) => Err(anyhow!("slats requires a value 0-100")),
        };
        self.act(p.target, action, ctx, state, responses).await
    }

    #[tool(
        title = "Control lights",
        description = "Change a light: switch a lighting controller's mood (plus, minus, off, or a mood ID from list_light_moods), dim a dimmer (0-100), or set a color (#RRGGBB, hsv(h,s,v), or temp(brightness,kelvin)) on a color picker.",
        output_schema = schema_for_output::<ActionResult>(),
        annotations(read_only_hint = false, destructive_hint = false, open_world_hint = false)
    )]
    async fn light(
        &self,
        Parameters(p): Parameters<LightParams>,
        ctx: RequestContext<RoleServer>,
        RequestState(state): RequestState,
        InputResponses(responses): InputResponses,
    ) -> ToolResponse {
        let value = p.value.text();
        let action = match p.action {
            LightAction::Mood => actions::parse_mood(&value),
            LightAction::Dim => value
                .parse::<f64>()
                .map_err(|_| anyhow!("dim value must be a number 0-100, got '{}'", value))
                .and_then(actions::parse_dim),
            LightAction::Color => actions::parse_color(&value),
        };
        self.act(p.target, action, ctx, state, responses).await
    }

    #[tool(
        title = "Set room climate",
        description = "Set a room controller: comfort temperature (temp, °C), operating mode (mode: auto, auto-heat, auto-cool, manual, manual-heat, manual-cool), or a temporary override temperature (override, °C, for `minutes`, default 60).",
        output_schema = schema_for_output::<ActionResult>(),
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn thermostat(
        &self,
        Parameters(p): Parameters<ThermostatParams>,
        ctx: RequestContext<RoleServer>,
        RequestState(state): RequestState,
        InputResponses(responses): InputResponses,
    ) -> ToolResponse {
        let act = match p.action {
            ThermostatAction::Temp => "temp",
            ThermostatAction::Mode => "mode",
            ThermostatAction::Override => "override",
        };
        let value = p.value.text();
        let action = actions::parse_thermostat(act, Some(&value), p.minutes);
        self.act(p.target, action, ctx, state, responses).await
    }

    #[tool(
        title = "Operate gate",
        description = "Open, close or stop a gate or garage door. open and close are high-risk: the user is asked to confirm.",
        output_schema = schema_for_output::<ActionResult>(),
        annotations(read_only_hint = false, destructive_hint = true, open_world_hint = false)
    )]
    async fn gate(
        &self,
        Parameters(p): Parameters<GateParams>,
        ctx: RequestContext<RoleServer>,
        RequestState(state): RequestState,
        InputResponses(responses): InputResponses,
    ) -> ToolResponse {
        let act = match p.action {
            GateAction::Open => "open",
            GateAction::Close => "close",
            GateAction::Stop => "stop",
        };
        self.act(p.target, actions::parse_gate(act), ctx, state, responses)
            .await
    }

    #[tool(
        title = "Operate alarm",
        description = "Arm, arm in home mode, disarm, or acknowledge (quit) the burglar alarm. All but quit are high-risk: the user is asked to confirm.",
        output_schema = schema_for_output::<ActionResult>(),
        annotations(read_only_hint = false, destructive_hint = true, open_world_hint = false)
    )]
    async fn alarm(
        &self,
        Parameters(p): Parameters<AlarmParams>,
        ctx: RequestContext<RoleServer>,
        RequestState(state): RequestState,
        InputResponses(responses): InputResponses,
    ) -> ToolResponse {
        let act = match p.action {
            AlarmAction::Arm => "arm",
            AlarmAction::ArmHome => "arm-home",
            AlarmAction::Disarm => "disarm",
            AlarmAction::Quit => "quit",
        };
        let action = actions::parse_alarm(act, p.no_motion, p.code);
        self.act(p.target, action, ctx, state, responses).await
    }

    #[tool(
        title = "Operate door lock",
        description = "Lock, unlock or open a door lock. High-risk: the user is asked to confirm.",
        output_schema = schema_for_output::<ActionResult>(),
        annotations(read_only_hint = false, destructive_hint = true, open_world_hint = false)
    )]
    async fn door(
        &self,
        Parameters(p): Parameters<DoorParams>,
        ctx: RequestContext<RoleServer>,
        RequestState(state): RequestState,
        InputResponses(responses): InputResponses,
    ) -> ToolResponse {
        let act = match p.action {
            DoorAction::Lock => "lock",
            DoorAction::Unlock => "unlock",
            DoorAction::Open => "open",
        };
        self.act(p.target, actions::parse_door(act), ctx, state, responses)
            .await
    }

    #[tool(
        title = "Run scene",
        description = "Run one of the user's lox scenes (see list_scenes). Steps run in order with their configured delays; progress is reported per step and the run stops when the request is cancelled.",
        output_schema = schema_for_output::<SceneRun>(),
        annotations(read_only_hint = false, destructive_hint = false, open_world_hint = false)
    )]
    async fn run_scene(
        &self,
        Parameters(p): Parameters<SceneParams>,
        ctx: RequestContext<RoleServer>,
    ) -> ToolResponse {
        let dry_run = p.dry_run || self.opts.dry_run;
        let id = p.scene.clone();
        let scene = match self
            .blocking(move |lox| ops::load_scene(&lox.cfg, &id))
            .await
        {
            Ok(s) => s,
            Err(e) => return fail(e),
        };
        let total = scene.steps.len();
        let progress_token = ctx.meta.get_progress_token();
        let mut run = SceneRun {
            ok: true,
            scene: p.scene,
            name: scene.name.clone(),
            dry_run,
            cancelled: false,
            steps: Vec::with_capacity(total),
        };
        for (i, step) in scene.steps.into_iter().enumerate() {
            if ctx.ct.is_cancelled() {
                run.cancelled = true;
                break;
            }
            let delay = Duration::from_millis(step.delay_ms);
            let label = format!("{} → {}", step.control, step.cmd);
            let result = match self
                .blocking(move |lox| Ok(ops::run_step(lox, &step, dry_run)))
                .await
            {
                Ok(r) => r,
                Err(e) => return fail(e),
            };
            run.ok &= result.ok;
            run.steps.push(result);
            if let Some(token) = &progress_token {
                let _ = ctx
                    .peer
                    .notify_progress(
                        ProgressNotificationParam::new(token.clone(), (i + 1) as f64)
                            .with_total(total as f64)
                            .with_message(label),
                    )
                    .await;
            }
            if !dry_run && !delay.is_zero() && i + 1 < total {
                tokio::select! {
                    _ = tokio::time::sleep(delay) => {}
                    _ = ctx.ct.cancelled() => {
                        run.cancelled = true;
                        break;
                    }
                }
            }
        }
        if run.cancelled {
            run.ok = false;
        }
        respond(Ok(run))
    }

    #[tool(
        title = "Send raw command",
        description = "Send a raw Loxone command string to a control (/jdev/sps/io/{uuid}/{command}), e.g. 'on', 'FullUp', 'setComfortTemperature/21'. Prefer the typed tools.",
        output_schema = schema_for_output::<ActionResult>(),
        annotations(read_only_hint = false, destructive_hint = true, open_world_hint = false)
    )]
    async fn send_command(
        &self,
        Parameters(p): Parameters<SendParams>,
        ctx: RequestContext<RoleServer>,
        RequestState(state): RequestState,
        InputResponses(responses): InputResponses,
    ) -> ToolResponse {
        let action = if p.command.trim().is_empty() {
            Err(anyhow!("command must not be empty"))
        } else {
            Ok(Action::Raw(p.command))
        };
        self.act(p.target, action, ctx, state, responses).await
    }

    // ── User management ───────────────────────────────────────────────────────

    #[tool(
        title = "Learn NFC tag",
        description = "Read an NFC tag's ID at an NFC Code Touch (its learn mode). Tell the user BEFORE calling: have the NFC tag or the unlocked phone ready and hold it to the reader right after confirming — learn mode waits timeout_secs (default 60). Returns the ID and, for a known tag, its name and user; a tag that is already paired opens the door instead of being learned. Learn mode always ends again. Pair the tag with assign_nfc_tag. A secured Code Touch needs the visualization password in LOX_VISU_PASSWORD or the config's visu_password_file (never ask the user for it). High-risk: the user is asked to confirm.",
        output_schema = schema_for_output::<LearnedTag>(),
        annotations(read_only_hint = false, destructive_hint = true, open_world_hint = false)
    )]
    async fn learn_nfc_tag(
        &self,
        Parameters(p): Parameters<LearnParams>,
        ctx: RequestContext<RoleServer>,
        RequestState(state): RequestState,
        InputResponses(responses): InputResponses,
    ) -> ToolResponse {
        let secs = p.timeout_secs.unwrap_or(60);
        if secs == 0 || secs > nfc_learn::MAX_TIMEOUT.as_secs() {
            return fail(invalid("timeout_secs must be between 1 and 300"));
        }
        let timeout = Duration::from_secs(secs);
        let query = p.code_touch.clone();
        let (mut target, cfg) = match self
            .blocking(move |lox| Ok((nfc_learn::target(lox, &query)?, lox.cfg.clone())))
            .await
        {
            Ok(t) => t,
            Err(e) => return fail(e),
        };
        if p.dry_run || self.opts.dry_run {
            return respond(Ok(LearnedTag::planned(&target, timeout)));
        }
        // a secured Code Touch: the visualization password from the
        // environment or the config's file only, never from the client
        if target.secured {
            match crate::visu::from_settings(None, &cfg) {
                Ok(pw) => target.visu_password = pw,
                Err(e) => return fail(e),
            }
            if let Some(e) = target.needs_password() {
                return fail(e);
            }
        }
        let question = format!(
            "Learn an NFC tag at {}? After confirming, hold the NFC tag or the unlocked \
             phone to the reader within {} s.",
            target.name, secs
        );
        let cli = format!("lox access learn \"{}\"", target.name);
        if let Some(response) = self
            .confirm_change(
                &ctx,
                format!("learn|{}", target.uuid),
                question,
                &cli,
                state,
                responses,
            )
            .await
        {
            return response;
        }
        if let Some(token) = ctx.meta.get_progress_token() {
            let _ = ctx
                .peer
                .notify_progress(
                    ProgressNotificationParam::new(token, 0.0).with_message(format!(
                        "Hold the NFC tag or the unlocked phone to {} now",
                        target.name
                    )),
                )
                .await;
        }
        let learned = async {
            let mut io = (self.learn_io)(cfg).await?;
            nfc_learn::learn(io.as_mut(), &target, timeout, ctx.ct.cancelled()).await
        }
        .await;
        let learned = match learned {
            Ok(l) => l,
            Err(e) => return fail(e),
        };
        // names for the IDs; the tag is read even when the list is not
        let users = self
            .blocking(|lox| Ok(usermgmt::user_list(lox).unwrap_or_default()))
            .await
            .unwrap_or_default();
        respond(Ok(LearnedTag::new(&target, learned, &users)))
    }

    #[tool(
        title = "Assign NFC tag",
        description = "Pair an NFC tag (ID from learn_nfc_tag) with a user under a name. Refuses a tag the user has already. The change may live only in the Miniserver's custom changes until Loxone Config saves the program. High-risk: the user is asked to confirm.",
        output_schema = schema_for_output::<UserChange>(),
        annotations(read_only_hint = false, destructive_hint = true, open_world_hint = false)
    )]
    async fn assign_nfc_tag(
        &self,
        Parameters(p): Parameters<AssignNfcParams>,
        ctx: RequestContext<RoleServer>,
        RequestState(state): RequestState,
        InputResponses(responses): InputResponses,
    ) -> ToolResponse {
        let AssignNfcParams {
            user,
            id,
            name,
            dry_run,
        } = p;
        let plan = move |lox: &mut LoxClient| {
            let u = usermgmt::resolve_user(lox, &user)?;
            usermgmt::plan_add_nfc(&u, &id, &name)
        };
        self.change_user(plan, None, dry_run, ctx, state, responses)
            .await
    }

    #[tool(
        title = "Remove NFC tag",
        description = "Remove one of a user's NFC tags, by ID or name. The change may live only in the Miniserver's custom changes until Loxone Config saves the program. High-risk: the user is asked to confirm.",
        output_schema = schema_for_output::<UserChange>(),
        annotations(read_only_hint = false, destructive_hint = true, open_world_hint = false)
    )]
    async fn remove_nfc_tag(
        &self,
        Parameters(p): Parameters<RemoveNfcParams>,
        ctx: RequestContext<RoleServer>,
        RequestState(state): RequestState,
        InputResponses(responses): InputResponses,
    ) -> ToolResponse {
        let RemoveNfcParams { user, tag, dry_run } = p;
        let plan = move |lox: &mut LoxClient| {
            let u = usermgmt::resolve_user(lox, &user)?;
            usermgmt::plan_remove_nfc(&u, &tag)
        };
        self.change_user(plan, None, dry_run, ctx, state, responses)
            .await
    }

    #[tool(
        title = "Set access code",
        description = "Set a user's access code for NFC Code Touch keypads (2-8 digits), replacing the current one. The code never appears in results or errors. Fails with code_in_use when another user has it. The change may live only in the Miniserver's custom changes until Loxone Config saves the program. High-risk: the user is asked to confirm.",
        output_schema = schema_for_output::<UserChange>(),
        annotations(read_only_hint = false, destructive_hint = true, open_world_hint = false)
    )]
    async fn set_access_code(
        &self,
        Parameters(p): Parameters<AccessCodeParams>,
        ctx: RequestContext<RoleServer>,
        RequestState(state): RequestState,
        InputResponses(responses): InputResponses,
    ) -> ToolResponse {
        let AccessCodeParams {
            user,
            code,
            dry_run,
        } = p;
        let code = code.trim().to_string();
        if let Err(e) = usermgmt::validate_access_code(&code) {
            return fail(invalid(e));
        }
        let plan = move |lox: &mut LoxClient| {
            let u = usermgmt::resolve_user(lox, &user)?;
            Ok(usermgmt::plan_set_code(&u))
        };
        self.change_user(plan, Some(code), dry_run, ctx, state, responses)
            .await
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for LoxMcp {
    fn get_info(&self) -> ServerConfig {
        use base64::Engine;
        let icon = format!(
            "data:image/svg+xml;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(ICON_SVG)
        );
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(
                Implementation::new("lox", env!("CARGO_PKG_VERSION"))
                    .with_title("Loxone (lox)")
                    .with_description("Control and inspect a Loxone smart home Miniserver")
                    .with_icons(vec![
                        Icon::new(icon)
                            .with_mime_type("image/svg+xml")
                            .with_sizes(vec!["any".to_string()]),
                    ])
                    .with_website_url("https://github.com/discostu105/lox"),
            )
            .with_instructions(INSTRUCTIONS)
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> ToolResponse {
        let tcc = ToolCallContext::new(self, request, context);
        let response = self.tool_router.call(tcc).await?;
        // schema errors get the same envelope as every other tool error
        match argument_error(&response) {
            Some(message) => fail(invalid(message)),
            None => Ok(response),
        }
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        // SEP-2549: the list is fixed for the server's lifetime, so it may be cached.
        let hints = modern(&context);
        Ok(ListToolsResult {
            result_type: Some(ResultType::COMPLETE),
            tools: self.tool_router.list_all(),
            meta: None,
            next_cursor: None,
            ttl_ms: hints.then_some(TOOLS_TTL_MS),
            cache_scope: hints.then_some(CacheScope::Public),
        })
    }
}
