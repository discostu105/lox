//! The MCP server: tool definitions (rmcp macros), confirmation of risky
//! actions, and the bridge from async handlers to the blocking Miniserver client.

use anyhow::anyhow;
use rmcp::{
    ErrorData, RoleServer, ServerHandler,
    handler::server::{
        router::tool::ToolRouter,
        tool::{InputResponses, RequestState, schema_for_output},
        wrapper::Parameters,
    },
    model::{
        CacheScope, CallToolResponse, CallToolResult, ContentBlock, ElicitRequest,
        ElicitRequestParams, ElicitationSchema, Icon, Implementation, InputRequest, InputRequests,
        InputRequiredResult, ListToolsResult, PaginatedRequestParams, ProgressNotificationParam,
        ProtocolVersion, ResultType, ServerCapabilities, ServerConfig,
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
Read live values with get_control or list_sensors. Every action tool accepts dry_run=true \
to preview the exact commands without sending them.
Doors, gates and the alarm are high-risk: the user is asked to confirm each one. \
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
    /// up, down, stop, shade (automatic shading), position (needs value), slats (needs value)
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

#[derive(Clone)]
pub struct LoxMcp {
    opts: ServerOptions,
    state: Arc<Mutex<State>>,
    pending: Arc<Mutex<HashMap<String, Pending>>>,
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
];

impl LoxMcp {
    pub fn new(opts: ServerOptions) -> Self {
        Self::build(opts, None)
    }

    #[cfg(test)]
    pub fn with_config(opts: ServerOptions, cfg: Config) -> Self {
        Self::build(opts, Some(cfg))
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
        let ctrl = match self
            .blocking(move |lox| {
                ops::resolve_for_action(lox, &name, room.as_deref(), &resolve_action)
            })
            .await
        {
            Ok(c) => c,
            Err(e) => return fail(e),
        };

        let mut out = ops::planned(&ctrl, &action);
        let needs_confirmation = ops::needs_confirmation(&action, &ctrl) && !self.opts.allow_risky;
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
                            "Refused: '{}' on '{}' is a high-risk action (doors, gates, alarm) and \
                             this MCP client cannot ask the user to confirm it. The user can run it \
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
    ) -> ToolResponse {
        let action = match p.state {
            SwitchState::On => Action::On,
            SwitchState::Off => Action::Off,
            SwitchState::Pulse => Action::Pulse,
        };
        self.act(p.target, Ok(action), ctx, None, None).await
    }

    #[tool(
        title = "Move blinds",
        description = "Move a blind/shade (Jalousie). Position and slats are 0-100 where 100 is fully closed/down. 'shade' starts automatic shading.",
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
        self.act(p.target, action, ctx, None, None).await
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
        self.act(p.target, action, ctx, None, None).await
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
    ) -> ToolResponse {
        let act = match p.action {
            ThermostatAction::Temp => "temp",
            ThermostatAction::Mode => "mode",
            ThermostatAction::Override => "override",
        };
        let value = p.value.text();
        let action = actions::parse_thermostat(act, Some(&value), p.minutes);
        self.act(p.target, action, ctx, None, None).await
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
    ) -> ToolResponse {
        let action = if p.command.trim().is_empty() {
            Err(anyhow!("command must not be empty"))
        } else {
            Ok(Action::Raw(p.command))
        };
        self.act(p.target, action, ctx, None, None).await
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
