//! What the Miniserver has, read the way every front-end needs it: which
//! controls are sensors or energy meters, and a light controller's moods.
//!
//! Shared by the CLI commands, the MCP server and the TUI, so none of them
//! imports another's modules. Command output belongs in `commands/`; this
//! module returns data.

use anyhow::{Context, Result, bail};
use std::time::Duration;

use loxone_client::client::{Control, LoxClient};
use loxone_client::stream::{self, StateEvent};

/// Does a control belong to a sensor kind (`temperature`, `door-window`,
/// `motion`, `smoke`; anything else means all sensor types)?
/// Shared by `lox sensors` and the MCP `list_sensors` tool.
pub fn is_sensor_type(kind: &str, c: &Control) -> bool {
    let typ = c.typ.as_str();
    match kind {
        // InfoOnlyAnalog also carries CO2 (ppm), humidity (%), …: go by the unit
        "temperature" | "temp" => {
            typ == "InfoOnlyAnalog" && c.format.as_deref().is_none_or(|f| f.contains('°'))
        }
        "door-window" | "doorwindow" => typ == "InfoOnlyDigital",
        "motion" => matches!(typ, "PresenceDetector" | "MotionSensor"),
        "smoke" => typ == "SmokeAlarm",
        _ => matches!(
            typ,
            "InfoOnlyAnalog"
                | "InfoOnlyDigital"
                | "PresenceDetector"
                | "MotionSensor"
                | "SmokeAlarm"
                | "Meter"
        ),
    }
}

/// A control with its `/all` answer (empty when the read failed).
pub struct LiveControl {
    pub control: Control,
    pub all_xml: String,
}

/// The controls (in a room) that `keep` selects, each with its `/all` answer:
/// the live read behind `lox sensors`.
pub fn read_live(
    lox: &mut LoxClient,
    room: Option<&str>,
    keep: impl Fn(&Control) -> bool,
) -> Result<Vec<LiveControl>> {
    let controls = lox.list_controls(None, room)?;
    Ok(controls
        .into_iter()
        .filter(|c| keep(c))
        .map(|control| {
            let all_xml = lox.get_all(&control.uuid).unwrap_or_default();
            LiveControl { control, all_xml }
        })
        .collect())
}

/// Energy meters and managers (`lox energy`, MCP `list_sensors kind=energy`).
pub fn is_energy_type(typ: &str) -> bool {
    matches!(
        typ,
        "Meter" | "EnergyManager" | "EnergyMonitor" | "EnergyFlowMonitor"
    ) || typ.contains("Energy")
        || crate::energy::is_flow_monitor(typ)
}

/// Resolve a lighting controller and the UUID of its `moodList` text state.
///
/// Shared by `lox light moods` and the MCP `list_light_moods` tool.
pub fn resolve_mood_list(
    lox: &mut LoxClient,
    name_or_uuid: &str,
    room: Option<&str>,
) -> Result<(loxone_client::client::Control, String)> {
    let uuid = lox.resolve_with_room(name_or_uuid, room)?;
    let ctrl = lox.find_control(&uuid)?;
    if !matches!(ctrl.typ.as_str(), "LightControllerV2" | "LightController") {
        bail!(
            "'{}' is type '{}', not a LightController",
            ctrl.name,
            ctrl.typ
        );
    }
    let mood_list_uuid = lox
        .get_structure()?
        .get("controls")
        .and_then(|c| c.as_object())
        .and_then(|m| m.get(&uuid))
        .and_then(|c| c.get("states"))
        .and_then(|s| s.as_object())
        .and_then(|s| s.get("moodList"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "'{}' has no moodList state — is it a LightControllerV2?",
                ctrl.name
            )
        })?
        .to_string();
    Ok((ctrl, mood_list_uuid))
}

/// Wait (max 10 s) for the `moodList` text state in the WebSocket state dump.
pub async fn fetch_mood_list_json(
    conn: &loxone_client::connection::Connection,
    mood_list_uuid: &str,
) -> Result<String> {
    tokio::time::timeout(
        Duration::from_secs(10),
        fetch_mood_list_via_ws(conn, mood_list_uuid),
    )
    .await
    .map_err(|_| anyhow::anyhow!("Timeout waiting for moodList state (10s)"))?
}

/// Connect to the Miniserver WebSocket, subscribe to binary states,
/// and return the text content of the moodList state UUID on first receipt.
async fn fetch_mood_list_via_ws(
    conn: &loxone_client::connection::Connection,
    mood_list_uuid: &str,
) -> Result<String> {
    let target = mood_list_uuid.to_string();
    stream::stream_events_until(conn, |event| {
        if let StateEvent::TextState { uuid, text, .. } = event
            && *uuid == target
        {
            Some(text.clone())
        } else {
            None
        }
    })
    .await
    .context("moodList state not received in initial state dump")
}
