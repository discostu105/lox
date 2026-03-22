//! Loxone WebSocket binary protocol parser and real-time state streaming.
//!
//! Connects to the Miniserver via WebSocket, subscribes to binary status updates,
//! and yields structured state-change events. Used by `lox stream` and `lox otel`.

use anyhow::{Result, bail};
use futures_util::{SinkExt, StreamExt};
use hmac::{Hmac, Mac};
use rand::RngCore;
use rsa::{Pkcs1v15Encrypt, RsaPublicKey, pkcs8::DecodePublicKey};
use serde_json::Value;
use sha1::Sha1 as Sha1Digest;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::time::Duration;
use tokio_tungstenite::tungstenite::Message;

use crate::config::Config;
use crate::ws::LoxWsClient;

type HmacSha256 = Hmac<Sha256>;

// ── Binary message types ────────────────────────────────────────────────────

const MSG_TEXT: u8 = 0x00;
const MSG_BINARY_FILE: u8 = 0x01;
const MSG_VALUE_STATES: u8 = 0x02;
const MSG_TEXT_STATES: u8 = 0x03;
const MSG_DAYTIMER_STATES: u8 = 0x04;
const MSG_OUT_OF_SERVICE: u8 = 0x05;
const MSG_KEEPALIVE: u8 = 0x06;
const MSG_WEATHER_STATES: u8 = 0x07;

// ── State events ────────────────────────────────────────────────────────────

/// A single state-change event from the Miniserver WebSocket stream.
#[derive(Debug, Clone)]
#[allow(dead_code)] // Fields used by otel exporter
pub enum StateEvent {
    /// Numeric state update: UUID → f64 value
    ValueState { uuid: String, value: f64 },
    /// Text state update: UUID → string value
    TextState {
        uuid: String,
        icon_uuid: String,
        text: String,
    },
    /// Daytimer schedule entry
    DaytimerState {
        uuid: String,
        default_value: f64,
        entries: Vec<DaytimerEntry>,
    },
    /// Weather forecast data
    WeatherState {
        uuid: String,
        last_update: u32,
        entries: Vec<WeatherEntry>,
    },
    /// Keepalive heartbeat
    Keepalive,
    /// Miniserver going offline (firmware update, etc.)
    OutOfService,
}

#[derive(Debug, Clone)]
#[allow(dead_code)] // Fields used by otel exporter
pub struct DaytimerEntry {
    pub mode: i32,
    pub from: i32,
    pub to: i32,
    pub need_activate: i32,
    pub value: f64,
}

#[derive(Debug, Clone)]
#[allow(dead_code)] // Fields used by otel exporter
pub struct WeatherEntry {
    pub timestamp: i32,
    pub weather_type: i32,
    pub wind_direction: i32,
    pub solar_radiation: i32,
    pub relative_humidity: i32,
    pub temperature: f64,
    pub perceived_temperature: f64,
    pub dew_point: f64,
    pub precipitation: f64,
    pub wind_speed: f64,
    pub barometric_pressure: f64,
}

// ── State UUID mapping ──────────────────────────────────────────────────────

/// Metadata for a state UUID, linking it back to its parent control.
#[derive(Debug, Clone)]
#[allow(dead_code)] // Fields used by otel exporter
pub struct StateUuidInfo {
    pub control_name: String,
    pub control_uuid: String,
    pub state_name: String,
    pub control_type: String,
    pub room: Option<String>,
    pub category: Option<String>,
    pub unit: Option<String>,
}

/// Build a map from state UUID → control metadata by walking the structure cache.
///
/// Each control in `LoxApp3.json` has a `states` object like:
/// ```json
/// { "active": "state-uuid-1", "value": "state-uuid-2" }
/// ```
/// This function creates a reverse map so we can look up which control owns a state UUID.
pub fn build_state_uuid_map(structure: &Value) -> HashMap<String, StateUuidInfo> {
    let mut map = HashMap::new();

    // Build room/category name lookups
    let rooms = build_name_map(structure, "rooms");
    let cats = build_name_map(structure, "cats");

    // Walk controls and their states
    if let Some(ctrl_map) = structure.get("controls").and_then(|c| c.as_object()) {
        for (ctrl_uuid, ctrl) in ctrl_map {
            let name = str_field(ctrl, "name");
            let typ = str_field(ctrl, "type");
            let room = ctrl
                .get("room")
                .and_then(|r| r.as_str())
                .and_then(|r| rooms.get(r))
                .cloned();
            let cat = ctrl
                .get("cat")
                .and_then(|c| c.as_str())
                .and_then(|c| cats.get(c))
                .cloned();

            let unit = extract_unit(ctrl);

            // Control's own states
            add_states_from(
                &mut map,
                ctrl,
                ctrl_uuid,
                &name,
                &typ,
                room.as_deref(),
                cat.as_deref(),
                unit.as_deref(),
            );

            // Sub-control states
            if let Some(subs) = ctrl.get("subControls").and_then(|s| s.as_object()) {
                for (sub_uuid, sub) in subs {
                    let sub_name = format!("{}/{}", name, str_field(sub, "name"));
                    let sub_type = str_field(sub, "type");
                    let sub_unit = extract_unit(sub).or_else(|| unit.clone());
                    add_states_from(
                        &mut map,
                        sub,
                        sub_uuid,
                        &sub_name,
                        &sub_type,
                        room.as_deref(),
                        cat.as_deref(),
                        sub_unit.as_deref(),
                    );
                }
            }
        }
    }

    // Global states (operatingMode, sunrise, etc.)
    if let Some(globals) = structure.get("globalStates").and_then(|g| g.as_object()) {
        for (state_name, uuid_val) in globals {
            if let Some(uuid) = uuid_val.as_str() {
                map.insert(
                    uuid.to_string(),
                    StateUuidInfo {
                        control_name: "Global".to_string(),
                        control_uuid: String::new(),
                        state_name: state_name.clone(),
                        control_type: "GlobalState".to_string(),
                        room: None,
                        category: None,
                        unit: None,
                    },
                );
            }
        }
    }

    // Autopilot rule states
    if let Some(auto_map) = structure.get("autopilot").and_then(|a| a.as_object()) {
        for (rule_uuid, rule) in auto_map {
            let rule_name = str_field(rule, "name");
            if let Some(states) = rule.get("states").and_then(|s| s.as_object()) {
                for (state_name, uuid_val) in states {
                    if let Some(uuid) = uuid_val.as_str() {
                        map.insert(
                            uuid.to_string(),
                            StateUuidInfo {
                                control_name: rule_name.clone(),
                                control_uuid: rule_uuid.clone(),
                                state_name: state_name.clone(),
                                control_type: "AutopilotRule".to_string(),
                                room: None,
                                category: None,
                                unit: None,
                            },
                        );
                    }
                }
            }
        }
    }

    map
}

#[allow(clippy::too_many_arguments)]
fn add_states_from(
    map: &mut HashMap<String, StateUuidInfo>,
    ctrl: &Value,
    ctrl_uuid: &str,
    name: &str,
    typ: &str,
    room: Option<&str>,
    cat: Option<&str>,
    unit: Option<&str>,
) {
    if let Some(states) = ctrl.get("states").and_then(|s| s.as_object()) {
        for (state_name, uuid_val) in states {
            if let Some(uuid) = uuid_val.as_str() {
                map.insert(
                    uuid.to_string(),
                    StateUuidInfo {
                        control_name: name.to_string(),
                        control_uuid: ctrl_uuid.to_string(),
                        state_name: state_name.clone(),
                        control_type: typ.to_string(),
                        room: room.map(|r| r.to_string()),
                        category: cat.map(|c| c.to_string()),
                        unit: unit.map(|u| u.to_string()),
                    },
                );
            }
        }
    }
}

fn build_name_map(structure: &Value, key: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    if let Some(obj) = structure.get(key).and_then(|v| v.as_object()) {
        for (uuid, entry) in obj {
            if let Some(name) = entry.get("name").and_then(|n| n.as_str()) {
                map.insert(uuid.clone(), name.to_string());
            }
        }
    }
    map
}

/// Extract the unit suffix from a Loxone printf-style format string.
/// e.g. "%.1f°C" → "°C", "%.0fppm" → "ppm", "%.3fkW" → "kW"
/// Returns the raw suffix without normalization — the Loxone format
/// is the most accurate source and conversion could lose information
/// (e.g. "kW" for instantaneous power vs "kWh" for energy totals).
fn extract_unit(ctrl: &Value) -> Option<String> {
    let details = ctrl.get("details")?;
    for key in &["format", "actualFormat"] {
        if let Some(fmt) = details.get(*key).and_then(|v| v.as_str()) {
            // Find the last format specifier (e.g. %d, %f, %s) and take what's after it
            if let Some(pos) = fmt.rfind(|c: char| "dfs".contains(c)) {
                let suffix = fmt[pos + 1..].trim();
                if !suffix.is_empty() {
                    return Some(suffix.to_string());
                }
            }
        }
    }
    None
}

fn str_field(val: &Value, key: &str) -> String {
    val.get(key)
        .and_then(|v| v.as_str())
        .unwrap_or("?")
        .to_string()
}

// ── Binary protocol parsing ─────────────────────────────────────────────────

/// Loxone binary message header (8 bytes).
#[derive(Debug)]
struct BinHeader {
    msg_type: u8,
    #[allow(dead_code)]
    estimated: bool,
    payload_len: u32,
}

fn parse_header(data: &[u8]) -> Result<BinHeader> {
    if data.len() < 8 {
        bail!("Binary header too short: {} bytes", data.len());
    }
    if data[0] != 0x03 {
        bail!("Invalid binary header magic: 0x{:02x}", data[0]);
    }
    let msg_type = data[1];
    let estimated = data[2] & 0x01 != 0;
    let payload_len = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    Ok(BinHeader {
        msg_type,
        estimated,
        payload_len,
    })
}

/// Parse a 16-byte Loxone UUID from binary format.
///
/// Format: `{u32_le}-{u16_le}-{u16_le}-{8×u8_hex}`
pub fn parse_uuid(data: &[u8]) -> Result<String> {
    if data.len() < 16 {
        bail!("UUID too short: {} bytes", data.len());
    }
    let d1 = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
    let d2 = u16::from_le_bytes([data[4], data[5]]);
    let d3 = u16::from_le_bytes([data[6], data[7]]);
    Ok(format!(
        "{:08x}-{:04x}-{:04x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        d1, d2, d3, data[8], data[9], data[10], data[11], data[12], data[13], data[14], data[15],
    ))
}

/// Parse Value-State event table: repeated 24-byte records (UUID + f64).
fn parse_value_states(data: &[u8]) -> Result<Vec<StateEvent>> {
    let mut events = Vec::new();
    let mut offset = 0;
    while offset + 24 <= data.len() {
        let uuid = parse_uuid(&data[offset..offset + 16])?;
        let value = f64::from_le_bytes([
            data[offset + 16],
            data[offset + 17],
            data[offset + 18],
            data[offset + 19],
            data[offset + 20],
            data[offset + 21],
            data[offset + 22],
            data[offset + 23],
        ]);
        events.push(StateEvent::ValueState { uuid, value });
        offset += 24;
    }
    Ok(events)
}

/// Parse Text-State event table: variable-length records.
fn parse_text_states(data: &[u8]) -> Result<Vec<StateEvent>> {
    let mut events = Vec::new();
    let mut offset = 0;
    while offset + 36 <= data.len() {
        // 16 bytes UUID + 16 bytes icon UUID + 4 bytes text length
        let uuid = parse_uuid(&data[offset..offset + 16])?;
        let icon_uuid = parse_uuid(&data[offset + 16..offset + 32])?;
        let text_len = u32::from_le_bytes([
            data[offset + 32],
            data[offset + 33],
            data[offset + 34],
            data[offset + 35],
        ]) as usize;
        offset += 36;
        if offset + text_len > data.len() {
            break;
        }
        let text = String::from_utf8_lossy(&data[offset..offset + text_len]).to_string();
        // Round up to next multiple of 4
        let padded_len = (text_len + 3) & !3;
        offset += padded_len;
        events.push(StateEvent::TextState {
            uuid,
            icon_uuid,
            text,
        });
    }
    Ok(events)
}

/// Parse Daytimer-State event table.
fn parse_daytimer_states(data: &[u8]) -> Result<Vec<StateEvent>> {
    let mut events = Vec::new();
    let mut offset = 0;
    while offset + 28 <= data.len() {
        let uuid = parse_uuid(&data[offset..offset + 16])?;
        let default_value =
            f64::from_le_bytes(data[offset + 16..offset + 24].try_into().unwrap_or([0; 8]));
        let nr_entries =
            i32::from_le_bytes(data[offset + 24..offset + 28].try_into().unwrap_or([0; 4]))
                as usize;
        offset += 28;
        let mut entries = Vec::new();
        for _ in 0..nr_entries {
            if offset + 24 > data.len() {
                break;
            }
            let mode = i32::from_le_bytes(data[offset..offset + 4].try_into().unwrap_or([0; 4]));
            let from =
                i32::from_le_bytes(data[offset + 4..offset + 8].try_into().unwrap_or([0; 4]));
            let to = i32::from_le_bytes(data[offset + 8..offset + 12].try_into().unwrap_or([0; 4]));
            let need_activate =
                i32::from_le_bytes(data[offset + 12..offset + 16].try_into().unwrap_or([0; 4]));
            let value =
                f64::from_le_bytes(data[offset + 16..offset + 24].try_into().unwrap_or([0; 8]));
            entries.push(DaytimerEntry {
                mode,
                from,
                to,
                need_activate,
                value,
            });
            offset += 24;
        }
        events.push(StateEvent::DaytimerState {
            uuid,
            default_value,
            entries,
        });
    }
    Ok(events)
}

/// Parse Weather-State event table.
fn parse_weather_states(data: &[u8]) -> Result<Vec<StateEvent>> {
    let mut events = Vec::new();
    let mut offset = 0;
    while offset + 24 <= data.len() {
        let uuid = parse_uuid(&data[offset..offset + 16])?;
        let last_update =
            u32::from_le_bytes(data[offset + 16..offset + 20].try_into().unwrap_or([0; 4]));
        let nr_entries =
            i32::from_le_bytes(data[offset + 20..offset + 24].try_into().unwrap_or([0; 4]))
                as usize;
        offset += 24;
        let mut entries = Vec::new();
        // Each weather entry: 4×i32 + 7×f64 = 16 + 56 = 72 bytes
        // But official spec: 4×i32 (16 bytes) + 6×f64 (48 bytes) = 68 bytes? Let's check:
        // timestamp(i32) + weatherType(i32) + windDirection(i32) + solarRadiation(i32)
        // + relativeHumidity(i32) = 5×i32 = 20 bytes
        // + temperature(f64) + perceivedTemp(f64) + dewPoint(f64) + precipitation(f64)
        //   + windSpeed(f64) + barometricPressure(f64) = 6×f64 = 48 bytes
        // Total = 68 bytes per entry
        for _ in 0..nr_entries {
            if offset + 68 > data.len() {
                break;
            }
            let timestamp =
                i32::from_le_bytes(data[offset..offset + 4].try_into().unwrap_or([0; 4]));
            let weather_type =
                i32::from_le_bytes(data[offset + 4..offset + 8].try_into().unwrap_or([0; 4]));
            let wind_direction =
                i32::from_le_bytes(data[offset + 8..offset + 12].try_into().unwrap_or([0; 4]));
            let solar_radiation =
                i32::from_le_bytes(data[offset + 12..offset + 16].try_into().unwrap_or([0; 4]));
            let relative_humidity =
                i32::from_le_bytes(data[offset + 16..offset + 20].try_into().unwrap_or([0; 4]));
            let temperature =
                f64::from_le_bytes(data[offset + 20..offset + 28].try_into().unwrap_or([0; 8]));
            let perceived_temperature =
                f64::from_le_bytes(data[offset + 28..offset + 36].try_into().unwrap_or([0; 8]));
            let dew_point =
                f64::from_le_bytes(data[offset + 36..offset + 44].try_into().unwrap_or([0; 8]));
            let precipitation =
                f64::from_le_bytes(data[offset + 44..offset + 52].try_into().unwrap_or([0; 8]));
            let wind_speed =
                f64::from_le_bytes(data[offset + 52..offset + 60].try_into().unwrap_or([0; 8]));
            let barometric_pressure =
                f64::from_le_bytes(data[offset + 60..offset + 68].try_into().unwrap_or([0; 8]));
            entries.push(WeatherEntry {
                timestamp,
                weather_type,
                wind_direction,
                solar_radiation,
                relative_humidity,
                temperature,
                perceived_temperature,
                dew_point,
                precipitation,
                wind_speed,
                barometric_pressure,
            });
            offset += 68;
        }
        events.push(StateEvent::WeatherState {
            uuid,
            last_update,
            entries,
        });
    }
    Ok(events)
}

/// Parse any binary message payload based on the header message type.
pub fn parse_binary_payload(msg_type: u8, data: &[u8]) -> Result<Vec<StateEvent>> {
    match msg_type {
        MSG_VALUE_STATES => parse_value_states(data),
        MSG_TEXT_STATES => parse_text_states(data),
        MSG_DAYTIMER_STATES => parse_daytimer_states(data),
        MSG_WEATHER_STATES => parse_weather_states(data),
        MSG_KEEPALIVE => Ok(vec![StateEvent::Keepalive]),
        MSG_OUT_OF_SERVICE => Ok(vec![StateEvent::OutOfService]),
        MSG_TEXT | MSG_BINARY_FILE => Ok(vec![]), // text responses / binary files — skip
        other => {
            eprintln!("Unknown binary message type: 0x{:02x}", other);
            Ok(vec![])
        }
    }
}

// ── WebSocket streaming ─────────────────────────────────────────────────────

/// Connect to the Miniserver WebSocket, subscribe to state updates,
/// and call `handler` for each batch of state events.
///
/// This function runs until the connection is closed or an error occurs.
/// Authenticate on the WebSocket using RSA key exchange + AES encryption.
/// This is required before any commands (like enablebinstatusupdate) are accepted.
///
/// The entire auth flow happens on the same WebSocket connection:
/// 1. Fetch RSA public key via WS
/// 2. Key exchange (send RSA-encrypted AES session key)
/// 3. Request getkey2 via WS (one-time key is session-specific)
/// 4. Compute HMAC and send encrypted authenticate command
pub async fn ws_authenticate_pub(
    ws: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    cfg: &Config,
) -> Result<()> {
    ws_authenticate(ws, cfg).await
}

async fn ws_authenticate(
    ws: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    cfg: &Config,
) -> Result<()> {
    // 1. Fetch RSA public key via HTTP (WS doesn't support this command)
    let cfg2 = cfg.clone();
    let pub_key_pem: String = tokio::task::spawn_blocking(move || -> Result<String> {
        let client = reqwest::blocking::Client::builder()
            .user_agent(crate::client::USER_AGENT)
            .danger_accept_invalid_certs(true)
            .build()?;
        let resp: serde_json::Value = client
            .get(format!("{}/jdev/sys/getPublicKey", cfg2.host))
            .basic_auth(&cfg2.user, Some(&cfg2.pass))
            .send()?
            .json()?;
        Ok(resp
            .pointer("/LL/value")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("no public key"))?
            .to_string())
    })
    .await??;

    // Parse RSA public key (Loxone mis-labels as CERTIFICATE)
    let pub_key: RsaPublicKey = {
        let b64: String = pub_key_pem
            .replace("-----BEGIN CERTIFICATE-----", "")
            .replace("-----END CERTIFICATE-----", "")
            .replace("-----BEGIN PUBLIC KEY-----", "")
            .replace("-----END PUBLIC KEY-----", "")
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        let mut pem = String::from("-----BEGIN PUBLIC KEY-----\n");
        for chunk in b64.as_bytes().chunks(64) {
            pem.push_str(std::str::from_utf8(chunk).unwrap_or(""));
            pem.push('\n');
        }
        pem.push_str("-----END PUBLIC KEY-----");
        RsaPublicKey::from_public_key_pem(&pem).map_err(|e| anyhow::anyhow!("RSA parse: {}", e))?
    };

    // 2. Generate AES-256 key + IV
    let mut aes_key = [0u8; 32];
    let mut aes_iv = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut aes_key);
    rand::thread_rng().fill_bytes(&mut aes_iv);
    let key_info = format!("{}:{}", hex::encode(aes_key), hex::encode(aes_iv));

    // 3. RSA-encrypt key info and do key exchange
    let encrypted_b64 = {
        let enc = pub_key.encrypt(
            &mut rand::thread_rng(),
            Pkcs1v15Encrypt,
            key_info.as_bytes(),
        )?;
        base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &enc)
    };
    ws.send(Message::Text(format!(
        "jdev/sys/keyexchange/{}",
        encrypted_b64
    )))
    .await?;
    ws_expect_code_200(ws, "keyexchange").await?;

    // 4. Get one-time HMAC key over WS
    ws.send(Message::Text(format!("jdev/sys/getkey2/{}", cfg.user)))
        .await?;
    let key2_json = ws_read_text_value(ws, "getkey2").await?;
    let key2_val: Value = serde_json::from_str(&key2_json).unwrap_or_default();

    let key_hex = key2_val.get("key").and_then(|v| v.as_str()).unwrap_or("");
    let salt_hex = key2_val.get("salt").and_then(|v| v.as_str()).unwrap_or("");
    let hash_alg = key2_val
        .get("hashAlg")
        .and_then(|v| v.as_str())
        .unwrap_or("SHA1");

    let key_b = hex::decode(key_hex)?;

    // 5. Compute HMAC signature per lxcommunicator protocol:
    //    pwHash = hashAlg(password + ":" + salt_hex).toUpperCase()
    //    sig = HMAC-hashAlg(hex_decode(key), "user:" + pwHash)
    //    Note: salt is used as raw hex string (NOT hex-decoded)
    let pw_hash = if hash_alg == "SHA256" {
        format!(
            "{:X}",
            Sha256::digest(format!("{}:{}", cfg.pass, salt_hex).as_bytes())
        )
    } else {
        let mut h1 = Sha1Digest::new();
        h1.update(format!("{}:{}", cfg.pass, salt_hex).as_bytes());
        format!("{:X}", h1.finalize())
    };
    let mut mac = HmacSha256::new_from_slice(&key_b)?;
    mac.update(format!("{}:{}", cfg.user, pw_hash).as_bytes());
    let sig = hex::encode(mac.finalize().into_bytes());

    // 6. Send gettoken command
    let client_uuid = uuid::Uuid::new_v4().to_string();
    let cmd = format!(
        "jdev/sys/gettoken/{}/{}/4/{}/lox-cli",
        sig, cfg.user, client_uuid
    );
    ws.send(Message::Text(cmd)).await?;
    ws_expect_code_200(ws, "gettoken").await?;

    Ok(())
}

/// Read a text response from the WS, returning the "value" field as a string.
/// Skips binary messages. If the value is an object, returns it as JSON.
async fn ws_read_text_value(
    ws: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    label: &str,
) -> Result<String> {
    for _ in 0..10 {
        match tokio::time::timeout(Duration::from_secs(5), ws.next()).await {
            Ok(Some(Ok(Message::Text(t)))) => {
                let v: Value = serde_json::from_str(&t).unwrap_or_default();
                let code = v
                    .pointer("/LL/Code")
                    .or_else(|| v.pointer("/LL/code"))
                    .and_then(|c| {
                        c.as_str()
                            .map(|s| s.to_string())
                            .or_else(|| c.as_i64().map(|n| n.to_string()))
                    })
                    .unwrap_or_else(|| "0".to_string());
                if code == "200" {
                    let val = v.pointer("/LL/value").unwrap_or(&Value::Null);
                    return if val.is_string() {
                        Ok(val.as_str().unwrap().to_string())
                    } else {
                        Ok(val.to_string())
                    };
                }
                if code != "0" {
                    bail!("{} failed ({}): {}", label, code, t);
                }
            }
            Ok(Some(Ok(Message::Binary(_)))) => continue,
            Ok(Some(Err(e))) => bail!("WS error during {}: {}", label, e),
            _ => bail!("WS timeout during {}", label),
        }
    }
    bail!("{}: no response after 10 messages", label)
}

/// Read WS messages until we get a text response with code 200.
async fn ws_expect_code_200(
    ws: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    label: &str,
) -> Result<()> {
    for _ in 0..10 {
        match tokio::time::timeout(Duration::from_secs(5), ws.next()).await {
            Ok(Some(Ok(Message::Text(t)))) => {
                let v: Value = serde_json::from_str(&t).unwrap_or_default();
                let code = v
                    .pointer("/LL/Code")
                    .or_else(|| v.pointer("/LL/code"))
                    .and_then(|c| {
                        c.as_str()
                            .map(|s| s.to_string())
                            .or_else(|| c.as_i64().map(|n| n.to_string()))
                    })
                    .unwrap_or_else(|| "0".to_string());
                if code == "200" {
                    return Ok(());
                }
                if code != "0" {
                    bail!("{} failed ({}): {}", label, code, t);
                }
            }
            Ok(Some(Ok(Message::Binary(_)))) => continue,
            Ok(Some(Err(e))) => bail!("WS error during {}: {}", label, e),
            _ => bail!("WS timeout during {}", label),
        }
    }
    bail!("{}: no 200 response after 10 messages", label)
}

pub async fn stream_events<F>(cfg: &Config, mut handler: F) -> Result<()>
where
    F: FnMut(Vec<StateEvent>) -> Result<()>,
{
    let ws_client = LoxWsClient::new(cfg.clone());
    let (mut ws_stream, _resp) = ws_client.connect_raw().await?;

    // Authenticate on the WebSocket.
    ws_authenticate(&mut ws_stream, cfg).await?;

    // Subscribe to binary status updates
    ws_stream
        .send(Message::Text("jdev/sps/enablebinstatusupdate".to_string()))
        .await?;

    // The Loxone protocol sends messages in pairs:
    // 1. A binary header (8 bytes) telling us the type and size of the next payload
    // 2. The payload (binary or text)
    let mut pending_header: Option<BinHeader> = None;

    while let Some(msg) = ws_stream.next().await {
        let msg = msg?;
        match msg {
            Message::Binary(data) => {
                if let Some(header) = pending_header.take() {
                    // This is the payload for the previous header
                    let events = parse_binary_payload(header.msg_type, &data)?;
                    if !events.is_empty() {
                        handler(events)?;
                    }
                } else if data.len() >= 8 && data[0] == 0x03 {
                    // This is a binary header
                    let header = parse_header(&data)?;
                    if header.estimated {
                        // Estimated header — wait for the exact header that follows
                        continue;
                    }
                    if header.payload_len == 0 {
                        // No payload — handle inline (keepalive, out-of-service)
                        let events = parse_binary_payload(header.msg_type, &[])?;
                        if !events.is_empty() {
                            handler(events)?;
                        }
                    } else if data.len() > 8 {
                        // Header + payload in the same frame
                        let events = parse_binary_payload(header.msg_type, &data[8..])?;
                        if !events.is_empty() {
                            handler(events)?;
                        }
                    } else {
                        pending_header = Some(header);
                    }
                }
            }
            Message::Text(_) => {
                // Text responses (command ACKs, etc.) — skip for streaming
            }
            Message::Ping(data) => {
                ws_stream.send(Message::Pong(data)).await?;
            }
            Message::Close(_) => {
                break;
            }
            _ => {}
        }
    }

    Ok(())
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_uuid() {
        // Example: 0f8e1234-036e-e9ad-ffff-ed57184a04d2
        // But Loxone format is: {u32_le}-{u16_le}-{u16_le}-{8×u8}
        //
        // For u32_le 0x0f8e1234 stored as bytes [0x34, 0x12, 0x8e, 0x0f]
        // For u16_le 0x036e stored as [0x6e, 0x03]
        // For u16_le 0xe9ad stored as [0xad, 0xe9]
        // Last 8 bytes are raw: [0xff, 0xff, 0xed, 0x57, 0x18, 0x4a, 0x04, 0xd2]
        let data: [u8; 16] = [
            0x34, 0x12, 0x8e, 0x0f, // u32_le → 0x0f8e1234
            0x6e, 0x03, // u16_le → 0x036e
            0xad, 0xe9, // u16_le → 0xe9ad
            0xff, 0xff, 0xed, 0x57, 0x18, 0x4a, 0x04, 0xd2, // raw bytes
        ];
        let uuid = parse_uuid(&data).unwrap();
        assert_eq!(uuid, "0f8e1234-036e-e9ad-ffffed57184a04d2");
    }

    #[test]
    fn test_parse_uuid_too_short() {
        assert!(parse_uuid(&[0u8; 15]).is_err());
    }

    #[test]
    fn test_parse_value_states() {
        // 2 entries: UUID + f64
        let mut data = Vec::new();
        // Entry 1: all zeros UUID, value = 42.0
        data.extend_from_slice(&[0u8; 16]); // UUID
        data.extend_from_slice(&42.0f64.to_le_bytes());
        // Entry 2: all ones UUID, value = -1.5
        data.extend_from_slice(&[0x01u8; 16]); // UUID
        data.extend_from_slice(&(-1.5f64).to_le_bytes());

        let events = parse_value_states(&data).unwrap();
        assert_eq!(events.len(), 2);
        match &events[0] {
            StateEvent::ValueState { value, .. } => assert_eq!(*value, 42.0),
            _ => panic!("Expected ValueState"),
        }
        match &events[1] {
            StateEvent::ValueState { value, .. } => assert_eq!(*value, -1.5),
            _ => panic!("Expected ValueState"),
        }
    }

    #[test]
    fn test_parse_value_states_truncated() {
        // 23 bytes — not enough for a full 24-byte entry
        let data = [0u8; 23];
        let events = parse_value_states(&data).unwrap();
        assert_eq!(events.len(), 0);
    }

    #[test]
    fn test_parse_text_states() {
        let mut data = Vec::new();
        // UUID (16 bytes)
        data.extend_from_slice(&[0xAAu8; 16]);
        // Icon UUID (16 bytes)
        data.extend_from_slice(&[0xBBu8; 16]);
        // Text length: 5
        data.extend_from_slice(&5u32.to_le_bytes());
        // Text: "Hello" (5 bytes) + 3 padding bytes to next multiple of 4
        data.extend_from_slice(b"Hello\x00\x00\x00");

        let events = parse_text_states(&data).unwrap();
        assert_eq!(events.len(), 1);
        match &events[0] {
            StateEvent::TextState { text, .. } => assert_eq!(text, "Hello"),
            _ => panic!("Expected TextState"),
        }
    }

    #[test]
    fn test_parse_text_states_exact_alignment() {
        let mut data = Vec::new();
        data.extend_from_slice(&[0u8; 16]); // UUID
        data.extend_from_slice(&[0u8; 16]); // Icon UUID
        data.extend_from_slice(&4u32.to_le_bytes()); // text length = 4 (already aligned)
        data.extend_from_slice(b"test");

        let events = parse_text_states(&data).unwrap();
        assert_eq!(events.len(), 1);
        match &events[0] {
            StateEvent::TextState { text, .. } => assert_eq!(text, "test"),
            _ => panic!("Expected TextState"),
        }
    }

    #[test]
    fn test_parse_header() {
        let data: [u8; 8] = [0x03, 0x02, 0x00, 0x00, 0x30, 0x00, 0x00, 0x00];
        let header = parse_header(&data).unwrap();
        assert_eq!(header.msg_type, MSG_VALUE_STATES);
        assert!(!header.estimated);
        assert_eq!(header.payload_len, 48); // 0x30 = 48 = 2 value-state entries
    }

    #[test]
    fn test_parse_header_estimated() {
        let data: [u8; 8] = [0x03, 0x02, 0x01, 0x00, 0xFF, 0x00, 0x00, 0x00];
        let header = parse_header(&data).unwrap();
        assert!(header.estimated);
    }

    #[test]
    fn test_parse_header_invalid_magic() {
        let data: [u8; 8] = [0x00, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
        assert!(parse_header(&data).is_err());
    }

    #[test]
    fn test_parse_daytimer_states() {
        let mut data = Vec::new();
        // UUID (16 bytes)
        data.extend_from_slice(&[0x11u8; 16]);
        // default_value: 22.0
        data.extend_from_slice(&22.0f64.to_le_bytes());
        // nrEntries: 1
        data.extend_from_slice(&1i32.to_le_bytes());
        // Entry: mode=1, from=480, to=1320, needActivate=0, value=23.5
        data.extend_from_slice(&1i32.to_le_bytes());
        data.extend_from_slice(&480i32.to_le_bytes());
        data.extend_from_slice(&1320i32.to_le_bytes());
        data.extend_from_slice(&0i32.to_le_bytes());
        data.extend_from_slice(&23.5f64.to_le_bytes());

        let events = parse_daytimer_states(&data).unwrap();
        assert_eq!(events.len(), 1);
        match &events[0] {
            StateEvent::DaytimerState {
                default_value,
                entries,
                ..
            } => {
                assert_eq!(*default_value, 22.0);
                assert_eq!(entries.len(), 1);
                assert_eq!(entries[0].mode, 1);
                assert_eq!(entries[0].from, 480);
                assert_eq!(entries[0].to, 1320);
                assert_eq!(entries[0].value, 23.5);
            }
            _ => panic!("Expected DaytimerState"),
        }
    }

    #[test]
    fn test_parse_weather_states() {
        let mut data = Vec::new();
        // UUID (16 bytes)
        data.extend_from_slice(&[0x22u8; 16]);
        // lastUpdate: 1000
        data.extend_from_slice(&1000u32.to_le_bytes());
        // nrEntries: 1
        data.extend_from_slice(&1i32.to_le_bytes());
        // Entry: timestamp=500, weatherType=2, windDir=180, solar=3, humidity=65
        data.extend_from_slice(&500i32.to_le_bytes());
        data.extend_from_slice(&2i32.to_le_bytes());
        data.extend_from_slice(&180i32.to_le_bytes());
        data.extend_from_slice(&3i32.to_le_bytes());
        data.extend_from_slice(&65i32.to_le_bytes());
        // temp=21.5, perceivedTemp=20.0, dewPoint=15.0, precip=0.0, wind=3.5, pressure=1013.25
        data.extend_from_slice(&21.5f64.to_le_bytes());
        data.extend_from_slice(&20.0f64.to_le_bytes());
        data.extend_from_slice(&15.0f64.to_le_bytes());
        data.extend_from_slice(&0.0f64.to_le_bytes());
        data.extend_from_slice(&3.5f64.to_le_bytes());
        data.extend_from_slice(&1013.25f64.to_le_bytes());

        let events = parse_weather_states(&data).unwrap();
        assert_eq!(events.len(), 1);
        match &events[0] {
            StateEvent::WeatherState {
                last_update,
                entries,
                ..
            } => {
                assert_eq!(*last_update, 1000);
                assert_eq!(entries.len(), 1);
                assert_eq!(entries[0].temperature, 21.5);
                assert_eq!(entries[0].wind_direction, 180);
                assert_eq!(entries[0].barometric_pressure, 1013.25);
            }
            _ => panic!("Expected WeatherState"),
        }
    }

    #[test]
    fn test_build_state_uuid_map() {
        let structure = serde_json::json!({
            "rooms": { "r1": { "name": "Kitchen" } },
            "cats": { "c1": { "name": "Lighting" } },
            "controls": {
                "ctrl-uuid-1": {
                    "name": "Kitchen Light",
                    "type": "LightControllerV2",
                    "room": "r1",
                    "cat": "c1",
                    "details": { "format": "%.1f°C" },
                    "states": {
                        "active": "state-active-uuid",
                        "value": "state-value-uuid"
                    },
                    "subControls": {
                        "sub-uuid-1": {
                            "name": "Dimmer",
                            "type": "Dimmer",
                            "states": {
                                "position": "state-dimmer-pos"
                            }
                        }
                    }
                }
            },
            "globalStates": {
                "operatingMode": "global-op-mode-uuid"
            },
            "autopilot": {
                "auto-uuid-1": {
                    "name": "Evening Mode",
                    "states": {
                        "changed": "auto-changed-uuid"
                    }
                }
            }
        });

        let map = build_state_uuid_map(&structure);

        // Control states
        let active = map.get("state-active-uuid").unwrap();
        assert_eq!(active.control_name, "Kitchen Light");
        assert_eq!(active.state_name, "active");
        assert_eq!(active.control_type, "LightControllerV2");
        assert_eq!(active.room.as_deref(), Some("Kitchen"));
        assert_eq!(active.category.as_deref(), Some("Lighting"));
        assert_eq!(active.unit.as_deref(), Some("°C"));

        // Sub-control states (inherit parent unit)
        let dimmer = map.get("state-dimmer-pos").unwrap();
        assert_eq!(dimmer.control_name, "Kitchen Light/Dimmer");
        assert_eq!(dimmer.state_name, "position");
        assert_eq!(dimmer.unit.as_deref(), Some("°C"));

        // Global states (no unit)
        let global = map.get("global-op-mode-uuid").unwrap();
        assert_eq!(global.control_name, "Global");
        assert_eq!(global.state_name, "operatingMode");
        assert_eq!(global.control_type, "GlobalState");

        // Autopilot states
        let auto = map.get("auto-changed-uuid").unwrap();
        assert_eq!(auto.control_name, "Evening Mode");
        assert_eq!(auto.control_type, "AutopilotRule");
    }

    #[test]
    fn test_build_state_uuid_map_empty_structure() {
        let structure = serde_json::json!({
            "rooms": {},
            "controls": {}
        });
        let map = build_state_uuid_map(&structure);
        assert!(map.is_empty());
    }

    #[test]
    fn test_parse_binary_payload_unknown_type() {
        let events = parse_binary_payload(0xFF, &[]).unwrap();
        assert!(events.is_empty());
    }

    #[test]
    fn test_parse_binary_payload_keepalive() {
        let events = parse_binary_payload(MSG_KEEPALIVE, &[]).unwrap();
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], StateEvent::Keepalive));
    }

    #[test]
    fn test_parse_binary_payload_out_of_service() {
        let events = parse_binary_payload(MSG_OUT_OF_SERVICE, &[]).unwrap();
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], StateEvent::OutOfService));
    }
}
