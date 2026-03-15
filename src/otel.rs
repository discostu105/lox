//! OpenTelemetry exporter — pushes Loxone metrics, logs, and traces via OTLP.
//!
//! Supports both continuous daemon mode (`lox otel serve`) and one-shot push
//! (`lox otel push`). Uses WebSocket streaming for real-time control state
//! updates, plus HTTP polling for system/network diagnostics.
//!
//! Signals:
//! - **Metrics**: control state gauges, system diagnostics, network counters, weather
//! - **Logs**: state change events, text-state messages, Miniserver system log
//! - **Traces**: synthetic automation traces (autopilot rule correlations)
//!
//! Uses the experimental async PeriodicReader which spawns a tokio task
//! (not a std thread) for periodic export, avoiding the executor mismatch
//! between `futures_executor::block_on` and async HTTP clients.

use anyhow::{bail, Result};
use opentelemetry::logs::{LogRecord as _, Logger as _, LoggerProvider as _, Severity};
use opentelemetry::metrics::MeterProvider as _;
use opentelemetry::trace::{Span as _, Tracer as _, TracerProvider as _};
use opentelemetry::KeyValue;
use opentelemetry_otlp::{LogExporter, MetricExporter, Protocol, SpanExporter};
use opentelemetry_otlp::{WithExportConfig, WithHttpConfig};
use opentelemetry_sdk::logs::SdkLoggerProvider;
use opentelemetry_sdk::metrics::periodic_reader_with_async_runtime::PeriodicReader;
use opentelemetry_sdk::metrics::SdkMeterProvider;
use opentelemetry_sdk::trace::SdkTracerProvider;
use opentelemetry_sdk::Resource;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::client::LoxClient;
use crate::config::Config;
use crate::stream::{self, StateEvent, StateUuidInfo, WeatherEntry};

// ── Interval parsing ────────────────────────────────────────────────────────

/// Parse a human-friendly duration string (e.g. "30s", "5m", "1h").
pub fn parse_interval(s: &str) -> Result<Duration> {
    let s = s.trim();
    if let Some(n) = s.strip_suffix('s') {
        Ok(Duration::from_secs(n.parse()?))
    } else if let Some(n) = s.strip_suffix('m') {
        Ok(Duration::from_secs(n.parse::<u64>()? * 60))
    } else if let Some(n) = s.strip_suffix('h') {
        Ok(Duration::from_secs(n.parse::<u64>()? * 3600))
    } else {
        // Assume seconds if no suffix
        Ok(Duration::from_secs(s.parse()?))
    }
}

// ── OTLP exporter setup ────────────────────────────────────────────────────

/// Parse header strings like "Authorization=Bearer xxx" into (key, value) pairs.
fn parse_headers(headers: &[String]) -> Result<HashMap<String, String>> {
    let mut map = HashMap::new();
    for h in headers {
        if let Some((k, v)) = h.split_once('=') {
            map.insert(k.to_string(), v.to_string());
        } else {
            bail!("Invalid header format '{}'. Expected 'Key=Value'.", h);
        }
    }
    Ok(map)
}

/// Build the full OTLP metrics URL from the user-provided endpoint.
/// Appends `/v1/metrics` only if not already present.
fn otlp_metrics_url(endpoint: &str) -> String {
    let base = endpoint.trim_end_matches('/');
    if base.ends_with("/v1/metrics") {
        base.to_string()
    } else {
        format!("{}/v1/metrics", base)
    }
}

/// Build the full OTLP URL for a given signal path (e.g. "/v1/logs", "/v1/traces").
fn otlp_signal_url(endpoint: &str, signal_path: &str) -> String {
    let base = endpoint.trim_end_matches('/');
    if base.ends_with(signal_path) {
        base.to_string()
    } else {
        format!("{}{}", base, signal_path)
    }
}

/// Build an OTLP metric exporter with the given endpoint and headers.
///
/// Uses the async reqwest client so the PeriodicReader's tokio task can
/// drive exports without blocking. Log/trace exporters use the default
/// blocking client which works on the BatchProcessor's std threads.
fn build_metric_exporter(
    endpoint: &str,
    headers: &[String],
    delta: bool,
) -> Result<MetricExporter> {
    let header_map = parse_headers(headers)?;
    let mut builder = MetricExporter::builder()
        .with_http()
        .with_http_client(reqwest::Client::new())
        .with_protocol(Protocol::HttpBinary)
        .with_endpoint(otlp_metrics_url(endpoint));
    if !header_map.is_empty() {
        builder = builder.with_headers(header_map);
    }
    if delta {
        builder = builder.with_temporality(opentelemetry_sdk::metrics::Temporality::Delta);
    }
    builder
        .build()
        .map_err(|e| anyhow::anyhow!("Failed to build OTLP metric exporter: {}", e))
}

/// Build an OTLP log exporter.
fn build_log_exporter(endpoint: &str, headers: &[String]) -> Result<LogExporter> {
    let header_map = parse_headers(headers)?;
    let mut builder = LogExporter::builder()
        .with_http()
        .with_protocol(Protocol::HttpBinary)
        .with_endpoint(otlp_signal_url(endpoint, "/v1/logs"));
    if !header_map.is_empty() {
        builder = builder.with_headers(header_map);
    }
    builder
        .build()
        .map_err(|e| anyhow::anyhow!("Failed to build OTLP log exporter: {}", e))
}

/// Build an OTLP span exporter for traces.
fn build_span_exporter(endpoint: &str, headers: &[String]) -> Result<SpanExporter> {
    let header_map = parse_headers(headers)?;
    let mut builder = SpanExporter::builder()
        .with_http()
        .with_protocol(Protocol::HttpBinary)
        .with_endpoint(otlp_signal_url(endpoint, "/v1/traces"));
    if !header_map.is_empty() {
        builder = builder.with_headers(header_map);
    }
    builder
        .build()
        .map_err(|e| anyhow::anyhow!("Failed to build OTLP span exporter: {}", e))
}

/// Build the OTel resource with Miniserver metadata.
fn build_resource(cfg: &Config) -> Resource {
    let mut attrs = vec![
        KeyValue::new("service.name", "loxone-miniserver"),
        KeyValue::new("service.version", env!("CARGO_PKG_VERSION")),
        KeyValue::new("host.name", cfg.host.clone()),
    ];
    if !cfg.serial.is_empty() {
        attrs.push(KeyValue::new("device.id", cfg.serial.clone()));
    }
    Resource::builder().with_attributes(attrs).build()
}

/// Check if the OTLP endpoint is reachable by sending an empty export request.
fn check_endpoint(endpoint: &str, headers: &[String]) -> Result<()> {
    let url = otlp_metrics_url(endpoint);
    let header_map = parse_headers(headers)?;
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()?;
    let mut req = client
        .post(&url)
        .header("Content-Type", "application/x-protobuf")
        .body(Vec::new());
    for (k, v) in &header_map {
        req = req.header(k, v);
    }
    match req.send() {
        Ok(r) if r.status().is_success() || r.status().as_u16() == 415 => Ok(()),
        Ok(r) => bail!("OTLP endpoint {} returned HTTP {}", url, r.status()),
        Err(e) => bail!("Cannot reach OTLP endpoint {}: {}", url, e),
    }
}

/// Build the MeterProvider with async PeriodicReader.
///
/// Must be called from within a tokio runtime context (e.g. inside `block_on`
/// or with an active `rt.enter()` guard). The PeriodicReader spawns a tokio
/// task that periodically collects and exports metrics.
fn build_metric_provider(
    cfg: &Config,
    endpoint: &str,
    interval: Duration,
    headers: &[String],
    delta: bool,
) -> Result<SdkMeterProvider> {
    let exporter = build_metric_exporter(endpoint, headers, delta)?;
    let reader = PeriodicReader::builder(exporter, opentelemetry_sdk::runtime::Tokio)
        .with_interval(interval)
        .build();
    Ok(SdkMeterProvider::builder()
        .with_resource(build_resource(cfg))
        .with_reader(reader)
        .build())
}

/// Build the LoggerProvider with batch exporter.
fn build_log_provider(
    cfg: &Config,
    endpoint: &str,
    headers: &[String],
) -> Result<SdkLoggerProvider> {
    let exporter = build_log_exporter(endpoint, headers)?;
    Ok(SdkLoggerProvider::builder()
        .with_resource(build_resource(cfg))
        .with_batch_exporter(exporter)
        .build())
}

/// Build the TracerProvider with batch exporter.
fn build_trace_provider(
    cfg: &Config,
    endpoint: &str,
    headers: &[String],
) -> Result<SdkTracerProvider> {
    let exporter = build_span_exporter(endpoint, headers)?;
    Ok(SdkTracerProvider::builder()
        .with_resource(build_resource(cfg))
        .with_batch_exporter(exporter)
        .build())
}

// ── Shared types ────────────────────────────────────────────────────────────

/// Shared state for the latest values from WebSocket streaming.
type StateStore = Arc<Mutex<HashMap<String, (StateUuidInfo, f64)>>>;

/// Shared storage for the latest weather entries from WebSocket streaming.
type WeatherStore = Arc<Mutex<Vec<WeatherEntry>>>;

/// Tracks previous cumulative values for delta computation.
type PrevValues = HashMap<String, f64>;

// ── Metric recording ────────────────────────────────────────────────────────

/// Record system diagnostics as metrics.
fn record_system_metrics(lox: &LoxClient, meter: &opentelemetry::metrics::Meter) -> Result<()> {
    let gauge_metrics: &[(&str, &str, &str)] = &[
        ("/dev/sys/heap", "loxone.system.heap", "kBy"),
        ("/jdev/sys/lastcpu", "loxone.system.cpu", "%"),
        ("/jdev/sys/numtasks", "loxone.system.tasks", "{tasks}"),
    ];
    for (path, name, unit) in gauge_metrics {
        if let Ok(text) = lox.get_text(path) {
            if let Some(val) = extract_lox_value(&text) {
                meter
                    .f64_gauge(*name)
                    .with_unit(*unit)
                    .build()
                    .record(val, &[]);
            }
        }
    }

    Ok(())
}

/// Fetch a cumulative value from the Miniserver and record it as a gauge.
/// When `delta` is true, computes and records the difference since the last call.
fn record_cumulative(
    lox: &LoxClient,
    meter: &opentelemetry::metrics::Meter,
    prev: &mut PrevValues,
    path: &str,
    name: &str,
    delta: bool,
) {
    if let Ok(text) = lox.get_text(path) {
        if let Some(current) = extract_lox_value(&text) {
            let val = if delta {
                let prev_val = prev.get(name).copied().unwrap_or(current);
                let d = current - prev_val;
                prev.insert(name.to_string(), current);
                if d < 0.0 {
                    current
                } else {
                    d
                } // handle counter reset
            } else {
                current
            };
            meter.f64_gauge(name.to_string()).build().record(val, &[]);
        }
    }
}

/// Record CAN bus and LAN network metrics.
/// These are cumulative counters — with `delta` mode, only the increment
/// since the last cycle is reported.
fn record_network_metrics(
    lox: &LoxClient,
    meter: &opentelemetry::metrics::Meter,
    prev: &mut PrevValues,
    delta: bool,
) -> Result<()> {
    let counters: &[(&str, &str)] = &[
        ("/jdev/bus/packetssent", "loxone.can.packets_sent"),
        ("/jdev/bus/packetsreceived", "loxone.can.packets_received"),
        ("/jdev/bus/receiveerrors", "loxone.can.receive_errors"),
        ("/jdev/bus/frameerrors", "loxone.can.frame_errors"),
        ("/jdev/bus/overruns", "loxone.can.overruns"),
        ("/jdev/lan/txp", "loxone.lan.tx_packets"),
        ("/jdev/lan/txe", "loxone.lan.tx_errors"),
        ("/jdev/lan/txc", "loxone.lan.tx_collisions"),
        ("/jdev/lan/rxp", "loxone.lan.rx_packets"),
        ("/jdev/lan/rxo", "loxone.lan.rx_overflow"),
        ("/jdev/lan/eof", "loxone.lan.eof_errors"),
        (
            "/jdev/sys/contextswitches",
            "loxone.system.context_switches",
        ),
    ];
    for (path, name) in counters {
        record_cumulative(lox, meter, prev, path, name, delta);
    }

    Ok(())
}

/// Record the latest control state values as OTel gauge metrics.
fn record_control_metrics(
    store: &StateStore,
    meter: &opentelemetry::metrics::Meter,
    type_filter: Option<&str>,
    room_filter: Option<&str>,
) {
    let state = store.lock().unwrap();
    let gauge = meter.f64_gauge("loxone.control.value").build();

    for (uuid, (info, value)) in state.iter() {
        if let Some(tf) = type_filter {
            if !info
                .control_type
                .to_lowercase()
                .contains(&tf.to_lowercase())
            {
                continue;
            }
        }
        if let Some(rf) = room_filter {
            if !info
                .room
                .as_deref()
                .unwrap_or("")
                .to_lowercase()
                .contains(&rf.to_lowercase())
            {
                continue;
            }
        }

        let mut attrs = vec![
            KeyValue::new("control.name", info.control_name.clone()),
            KeyValue::new("control.type", info.control_type.clone()),
            KeyValue::new("control.uuid", uuid.clone()),
            KeyValue::new("state.name", info.state_name.clone()),
            KeyValue::new("control.room", info.room.clone().unwrap_or_default()),
            KeyValue::new(
                "control.category",
                info.category.clone().unwrap_or_default(),
            ),
        ];
        if let Some(unit) = &info.unit {
            attrs.push(KeyValue::new("unit", unit.clone()));
        }
        gauge.record(*value, &attrs);
    }
}

/// Record weather data as OTel gauge metrics from the shared weather store.
fn record_weather_metrics(store: &WeatherStore, meter: &opentelemetry::metrics::Meter) {
    let entries = store.lock().unwrap();
    // Use the first entry (current conditions) if available
    if let Some(entry) = entries.first() {
        let g = |name, unit| meter.f64_gauge(name).with_unit(unit).build();
        g("loxone.weather.temperature", "Cel").record(entry.temperature, &[]);
        g("loxone.weather.perceived_temperature", "Cel").record(entry.perceived_temperature, &[]);
        g("loxone.weather.dew_point", "Cel").record(entry.dew_point, &[]);
        g("loxone.weather.humidity", "%").record(entry.relative_humidity as f64, &[]);
        g("loxone.weather.wind_speed", "m/s").record(entry.wind_speed, &[]);
        g("loxone.weather.wind_direction", "deg").record(entry.wind_direction as f64, &[]);
        g("loxone.weather.pressure", "hPa").record(entry.barometric_pressure, &[]);
        g("loxone.weather.precipitation", "mm").record(entry.precipitation, &[]);
        g("loxone.weather.solar_radiation", "W/m2").record(entry.solar_radiation as f64, &[]);
    }
}

// ── Log recording ────────────────────────────────────────────────────────────

/// Emit an OTel log record for a numeric state change.
fn emit_state_change_log(
    provider: &SdkLoggerProvider,
    info: &StateUuidInfo,
    uuid: &str,
    value: f64,
    old_value: Option<f64>,
) {
    let logger = provider.logger("loxone-state");
    let mut record = logger.create_log_record();

    let body = if let Some(old) = old_value {
        format!(
            "{} {}: {} → {}",
            info.control_name, info.state_name, old, value
        )
    } else {
        format!("{} {}: {}", info.control_name, info.state_name, value)
    };

    record.set_body(body.into());
    let sev = Severity::Info;
    record.set_severity_number(sev);
    record.set_severity_text(sev.name());
    record.add_attribute("control.name", info.control_name.clone());
    record.add_attribute("control.type", info.control_type.clone());
    record.add_attribute("control.uuid", info.control_uuid.clone());
    record.add_attribute("state.name", info.state_name.clone());
    record.add_attribute("state.uuid", uuid.to_string());
    record.add_attribute("value", value);
    if let Some(old) = old_value {
        record.add_attribute("old_value", old);
    }
    if let Some(ref room) = info.room {
        record.add_attribute("control.room", room.clone());
    }
    if let Some(ref cat) = info.category {
        record.add_attribute("control.category", cat.clone());
    }

    logger.emit(record);
}

/// Emit an OTel log record for a text state update.
fn emit_text_state_log(
    provider: &SdkLoggerProvider,
    uuid: &str,
    text: &str,
    state_map: &HashMap<String, StateUuidInfo>,
) {
    let logger = provider.logger("loxone-state");
    let mut record = logger.create_log_record();

    let body = if let Some(info) = state_map.get(uuid) {
        format!("{}: {}", info.control_name, text)
    } else {
        format!("{}: {}", uuid, text)
    };

    record.set_body(body.into());
    let sev = Severity::Info;
    record.set_severity_number(sev);
    record.set_severity_text(sev.name());
    record.add_attribute("state.uuid", uuid.to_string());
    record.add_attribute("text", text.to_string());

    if let Some(info) = state_map.get(uuid) {
        record.add_attribute("control.name", info.control_name.clone());
        record.add_attribute("control.type", info.control_type.clone());
        record.add_attribute("control.uuid", info.control_uuid.clone());
        if let Some(ref room) = info.room {
            record.add_attribute("control.room", room.clone());
        }
        if let Some(ref cat) = info.category {
            record.add_attribute("control.category", cat.clone());
        }
    }

    logger.emit(record);
}

/// Emit an OTel log record for an out-of-service event.
fn emit_out_of_service_log(provider: &SdkLoggerProvider) {
    let logger = provider.logger("loxone-system");
    let mut record = logger.create_log_record();
    record.set_body(
        "Miniserver going offline (firmware update or maintenance)"
            .to_string()
            .into(),
    );
    let sev = Severity::Warn;
    record.set_severity_number(sev);
    record.set_severity_text(sev.name());
    record.add_attribute("event.type", "out_of_service");
    logger.emit(record);
}

/// Fetch and emit Miniserver system log lines as OTel log records.
/// Tracks the last seen line count to avoid re-emitting old lines.
fn emit_system_logs(provider: &SdkLoggerProvider, lox: &LoxClient, last_line_count: &mut usize) {
    let log = match lox.get_text("/dev/fsget/log/def.log") {
        Ok(t) => t,
        Err(_) => return,
    };
    if log.contains("<errorcode>") {
        return;
    }

    let lines: Vec<&str> = log.lines().collect();
    let new_lines = if lines.len() >= *last_line_count {
        &lines[*last_line_count..]
    } else {
        // Log rotated — emit all
        &lines[..]
    };

    if new_lines.is_empty() {
        *last_line_count = lines.len();
        return;
    }

    let logger = provider.logger("loxone-syslog");
    for line in new_lines {
        if line.trim().is_empty() {
            continue;
        }
        let severity = parse_log_severity(line);
        let mut record = logger.create_log_record();
        record.set_body(line.to_string().into());
        record.set_severity_number(severity);
        record.set_severity_text(severity.name());
        record.add_attribute("log.source", "miniserver-syslog");
        logger.emit(record);
    }

    *last_line_count = lines.len();
}

/// Parse log severity from a Miniserver log line.
fn parse_log_severity(line: &str) -> Severity {
    let lower = line.to_lowercase();
    if lower.contains("error") || lower.contains("fail") {
        Severity::Error
    } else if lower.contains("warn") {
        Severity::Warn
    } else if lower.contains("debug") {
        Severity::Debug
    } else {
        Severity::Info
    }
}

// ── Trace recording ────────────────────────────────────────────────────────

/// Build the set of autopilot state UUIDs for trace correlation.
fn build_autopilot_uuids(state_map: &HashMap<String, StateUuidInfo>) -> HashSet<String> {
    state_map
        .iter()
        .filter(|(_, info)| info.control_type == "AutopilotRule")
        .map(|(uuid, _)| uuid.clone())
        .collect()
}

/// Emit a synthetic automation trace when an autopilot rule fires.
///
/// Creates a root span for the autopilot rule and adds all other state
/// changes in the same event batch as span events (temporal correlation).
fn emit_automation_trace(
    provider: &SdkTracerProvider,
    trigger_info: &StateUuidInfo,
    trigger_uuid: &str,
    events: &[StateEvent],
    state_map: &HashMap<String, StateUuidInfo>,
) {
    let tracer = provider.tracer("loxone-automation");
    let mut span = tracer.start(format!("Autopilot: {}", trigger_info.control_name));

    span.set_attribute(KeyValue::new(
        "autopilot.name",
        trigger_info.control_name.clone(),
    ));
    span.set_attribute(KeyValue::new(
        "autopilot.uuid",
        trigger_info.control_uuid.clone(),
    ));
    span.set_attribute(KeyValue::new(
        "autopilot.state",
        trigger_info.state_name.clone(),
    ));

    // Add correlated state changes in the same batch as span events
    for event in events {
        match event {
            StateEvent::ValueState { uuid, value } => {
                if uuid == trigger_uuid {
                    continue;
                }
                if let Some(info) = state_map.get(uuid) {
                    span.add_event(
                        format!("{} {} = {}", info.control_name, info.state_name, value),
                        vec![
                            KeyValue::new("control.name", info.control_name.clone()),
                            KeyValue::new("control.type", info.control_type.clone()),
                            KeyValue::new("control.uuid", info.control_uuid.clone()),
                            KeyValue::new("state.name", info.state_name.clone()),
                            KeyValue::new("control.room", info.room.clone().unwrap_or_default()),
                            KeyValue::new("value", *value),
                        ],
                    );
                }
            }
            StateEvent::TextState { uuid, text, .. } => {
                if let Some(info) = state_map.get(uuid) {
                    span.add_event(
                        format!("{}: {}", info.control_name, text),
                        vec![
                            KeyValue::new("control.name", info.control_name.clone()),
                            KeyValue::new("text", text.clone()),
                        ],
                    );
                }
            }
            _ => {}
        }
    }

    span.end();
}

// ── Helper ──────────────────────────────────────────────────────────────────

/// Extract a numeric value from Loxone XML/JSON response.
/// Handles both JSON `{"LL":{"value":"42"}}` and plain XML `value="42"` formats.
/// Also handles values with unit suffixes like "352880/1016404kB" or "42.5%".
fn extract_lox_value(text: &str) -> Option<f64> {
    // Try JSON first
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(text) {
        if let Some(val) = v.pointer("/LL/value").and_then(|v| v.as_str()) {
            return parse_numeric_prefix(val);
        }
    }
    // Try XML attribute
    let key = "value=\"";
    if let Some(start) = text.find(key) {
        let rest = &text[start + key.len()..];
        if let Some(end) = rest.find('"') {
            return parse_numeric_prefix(&rest[..end]);
        }
    }
    None
}

/// Parse the leading numeric portion of a string, ignoring trailing
/// units or denominators (e.g. "352880/1016404kB" → 352880.0).
fn parse_numeric_prefix(s: &str) -> Option<f64> {
    if let Ok(v) = s.parse::<f64>() {
        return Some(v);
    }
    // Find the longest leading substring that parses as a number
    let end = s
        .find(|c: char| !c.is_ascii_digit() && c != '.' && c != '-')
        .unwrap_or(s.len());
    if end > 0 {
        s[..end].parse().ok()
    } else {
        None
    }
}

// ── Serve (continuous daemon) ───────────────────────────────────────────────

/// Run the OTel exporter in continuous mode:
/// - Connect to Miniserver WebSocket for real-time state changes
/// - Push metrics, logs, and traces via OTLP at the configured interval
/// - Also fetch system/network diagnostics and system logs on each interval
#[allow(clippy::too_many_arguments)]
pub fn serve(
    cfg: &Config,
    endpoint: &str,
    interval: Duration,
    type_filter: Option<&str>,
    room_filter: Option<&str>,
    headers: &[String],
    quiet: bool,
    delta: bool,
    no_logs: bool,
    no_traces: bool,
) -> Result<()> {
    // Load structure for UUID mapping
    let mut lox = LoxClient::new(cfg.clone());
    let structure = lox.get_structure()?.clone();
    let state_map = stream::build_state_uuid_map(&structure);

    // Shared state stores updated by WebSocket, read by metric callbacks
    let store: StateStore = Arc::new(Mutex::new(HashMap::new()));
    let store_ws = Arc::clone(&store);
    let weather_store: WeatherStore = Arc::new(Mutex::new(Vec::new()));
    let weather_ws = Arc::clone(&weather_store);

    // Autopilot UUIDs for trace correlation
    let autopilot_uuids = build_autopilot_uuids(&state_map);

    // Verify the OTLP endpoint is reachable before starting
    check_endpoint(endpoint, headers)?;

    // Create tokio runtime — the async PeriodicReader and batch exporters
    // spawn tokio tasks, so the runtime must be active for their lifetime.
    let rt = tokio::runtime::Runtime::new()?;

    // Build all providers inside the runtime context so background tasks
    // are spawned on this runtime.
    let (metric_provider, log_provider, trace_provider) = rt.block_on(async {
        let mp = build_metric_provider(cfg, endpoint, interval, headers, delta)?;
        let lp = if !no_logs {
            Some(build_log_provider(cfg, endpoint, headers)?)
        } else {
            None
        };
        let tp = if !no_traces {
            Some(build_trace_provider(cfg, endpoint, headers)?)
        } else {
            None
        };
        Ok::<_, anyhow::Error>((mp, lp, tp))
    })?;

    if !quiet {
        let signals: Vec<&str> = [
            Some("metrics"),
            if no_logs { None } else { Some("logs") },
            if no_traces { None } else { Some("traces") },
        ]
        .into_iter()
        .flatten()
        .collect();
        eprintln!(
            "Pushing {} to {} every {}s (Ctrl+C to stop)...",
            signals.join(" + "),
            endpoint,
            interval.as_secs()
        );
    }

    let tf = type_filter.map(|s| s.to_string());
    let rf = room_filter.map(|s| s.to_string());

    // Spawn system metrics polling on a separate thread.
    // Uses reqwest::blocking for HTTP calls — must run outside tokio context.
    let cfg_sys = cfg.clone();
    let meter_sys = metric_provider.meter("loxone");
    let tf_sys = tf.clone();
    let rf_sys = rf.clone();
    let store_sys = Arc::clone(&store);
    let weather_sys = Arc::clone(&weather_store);
    let log_provider_sys = log_provider.clone();
    let quiet_sys = quiet;
    std::thread::spawn(move || {
        let lox = LoxClient::new(cfg_sys);
        let mut prev = PrevValues::new();
        let mut last_log_lines: usize = 0;
        loop {
            // Record system & network diagnostics
            let _ = record_system_metrics(&lox, &meter_sys);
            let _ = record_network_metrics(&lox, &meter_sys, &mut prev, delta);

            // Record control state gauges from the shared store
            let control_count = store_sys.lock().unwrap().len();
            record_control_metrics(&store_sys, &meter_sys, tf_sys.as_deref(), rf_sys.as_deref());

            // Record weather gauges from the shared store
            record_weather_metrics(&weather_sys, &meter_sys);

            // Emit system log lines
            if let Some(ref lp) = log_provider_sys {
                emit_system_logs(lp, &lox, &mut last_log_lines);
            }

            if !quiet_sys {
                eprintln!(
                    "[otel] recorded {} control states + system metrics",
                    control_count
                );
            }

            std::thread::sleep(interval);
        }
    });

    // Clone providers for the WebSocket handler closure
    let log_provider_ws = log_provider.clone();
    let trace_provider_ws = trace_provider.clone();
    let state_map_ws = state_map.clone();
    let autopilot_ws = autopilot_uuids;

    // WebSocket streaming keeps the runtime alive. The PeriodicReader's
    // tokio task runs in the background, exporting metrics at each interval.
    rt.block_on(stream::stream_events(cfg, |events| {
        // Process value states (update store + emit logs)
        {
            let mut state = store_ws.lock().unwrap();
            for event in &events {
                if let StateEvent::ValueState { uuid, value } = event {
                    if let Some(info) = state_map_ws.get(uuid) {
                        let old_value = state.get(uuid).map(|(_, v)| *v);
                        state.insert(uuid.clone(), (info.clone(), *value));

                        if let Some(ref lp) = log_provider_ws {
                            emit_state_change_log(lp, info, uuid, *value, old_value);
                        }
                    }
                }
            }
        } // state lock released

        // Process text states → logs
        if let Some(ref lp) = log_provider_ws {
            for event in &events {
                if let StateEvent::TextState { uuid, text, .. } = event {
                    emit_text_state_log(lp, uuid, text, &state_map_ws);
                }
            }
        }

        // Process weather states → update shared store
        for event in &events {
            if let StateEvent::WeatherState { entries, .. } = event {
                *weather_ws.lock().unwrap() = entries.clone();
            }
        }

        // Process out-of-service → log
        if let Some(ref lp) = log_provider_ws {
            for event in &events {
                if let StateEvent::OutOfService = event {
                    emit_out_of_service_log(lp);
                }
            }
        }

        // Check for autopilot triggers → automation traces
        if let Some(ref tp) = trace_provider_ws {
            for event in &events {
                if let StateEvent::ValueState { uuid, value } = event {
                    if *value == 1.0 && autopilot_ws.contains(uuid) {
                        if let Some(info) = state_map_ws.get(uuid) {
                            emit_automation_trace(tp, info, uuid, &events, &state_map_ws);
                        }
                    }
                }
            }
        }

        Ok(())
    }))?;

    // Graceful shutdown
    metric_provider
        .shutdown()
        .map_err(|e| anyhow::anyhow!("OTel metric shutdown: {}", e))?;
    if let Some(ref lp) = log_provider {
        let _ = lp.shutdown();
    }
    if let Some(ref tp) = trace_provider {
        let _ = tp.shutdown();
    }

    Ok(())
}

// ── Push (one-shot) ─────────────────────────────────────────────────────────

/// Push current state once and exit. Connects to WebSocket, collects the
/// initial state dump, fetches system metrics, pushes everything, and exits.
#[allow(clippy::too_many_arguments)]
pub fn push(
    cfg: &Config,
    endpoint: &str,
    type_filter: Option<&str>,
    room_filter: Option<&str>,
    headers: &[String],
    quiet: bool,
    delta: bool,
    no_logs: bool,
) -> Result<()> {
    // Verify the OTLP endpoint is reachable before starting
    check_endpoint(endpoint, headers)?;

    // Load structure for UUID mapping
    let mut lox = LoxClient::new(cfg.clone());
    let structure = lox.get_structure()?.clone();
    let state_map = stream::build_state_uuid_map(&structure);

    if !quiet {
        eprintln!("Collecting current state...");
    }

    // Create tokio runtime — needed for both WS streaming and the async
    // PeriodicReader. The runtime stays alive for the entire function.
    let rt = tokio::runtime::Runtime::new()?;

    // Build providers inside the runtime context.
    let (metric_provider, log_provider) = rt.block_on(async {
        let mp = build_metric_provider(cfg, endpoint, Duration::from_secs(60), headers, delta)?;
        let lp = if !no_logs {
            Some(build_log_provider(cfg, endpoint, headers)?)
        } else {
            None
        };
        Ok::<_, anyhow::Error>((mp, lp))
    })?;
    let meter = metric_provider.meter("loxone");

    // Collect initial state dump via WebSocket with a timeout.
    let store: StateStore = Arc::new(Mutex::new(HashMap::new()));
    let store_ws = Arc::clone(&store);
    let weather_store: WeatherStore = Arc::new(Mutex::new(Vec::new()));
    let weather_ws = Arc::clone(&weather_store);
    let log_provider_ws = log_provider.clone();
    let state_map_ws = state_map.clone();

    let collect_result = rt.block_on(async {
        tokio::time::timeout(
            Duration::from_secs(10),
            stream::stream_events(cfg, |events| {
                let mut state = store_ws.lock().unwrap();
                let mut collected = false;
                for event in &events {
                    match event {
                        StateEvent::ValueState { uuid, value } => {
                            if let Some(info) = state_map_ws.get(uuid) {
                                state.insert(uuid.clone(), (info.clone(), *value));
                                collected = true;

                                // Emit state change log
                                if let Some(ref lp) = log_provider_ws {
                                    emit_state_change_log(lp, info, uuid, *value, None);
                                }
                            }
                        }
                        StateEvent::TextState { uuid, text, .. } => {
                            if let Some(ref lp) = log_provider_ws {
                                emit_text_state_log(lp, uuid, text, &state_map_ws);
                            }
                        }
                        StateEvent::WeatherState { entries, .. } => {
                            *weather_ws.lock().unwrap() = entries.clone();
                        }
                        _ => {}
                    }
                }
                if collected && !state.is_empty() {
                    return Err(anyhow::anyhow!("__done_collecting__"));
                }
                Ok(())
            }),
        )
        .await
    });

    // The "done collecting" error is expected — it means we got the initial dump.
    // WebSocket auth failures are non-fatal: we still push system metrics via HTTP.
    match collect_result {
        Ok(Ok(())) => {}
        Ok(Err(e)) if e.to_string().contains("__done_collecting__") => {}
        Ok(Err(e)) => {
            if !quiet {
                eprintln!("Warning: WebSocket streaming failed: {}", e);
                eprintln!("Falling back to system metrics only (no control states).");
            }
        }
        Err(_) => {} // Timeout — collected what we could
    }

    // Record system metrics on a std::thread (reqwest::blocking can't
    // run inside a tokio runtime context without panicking).
    {
        let cfg_sys = cfg.clone();
        let meter_sys = meter.clone();
        std::thread::spawn(move || {
            let lox_sys = LoxClient::new(cfg_sys);
            let mut prev = PrevValues::new();
            let _ = record_system_metrics(&lox_sys, &meter_sys);
            // For push mode, delta=false since there's no previous value
            let _ = record_network_metrics(&lox_sys, &meter_sys, &mut prev, false);
        })
        .join()
        .ok();
    }

    // Record control metrics from collected state
    record_control_metrics(&store, &meter, type_filter, room_filter);

    // Record weather metrics
    record_weather_metrics(&weather_store, &meter);

    // Emit system log (if logs enabled)
    if let Some(ref lp) = log_provider {
        let mut seen = 0;
        emit_system_logs(lp, &lox, &mut seen);
    }

    if !quiet {
        let count = store.lock().unwrap().len();
        let weather_count = weather_store.lock().unwrap().len();
        let signals = if no_logs { "metrics" } else { "metrics + logs" };
        eprintln!(
            "Pushing {} control states + {} weather entries + system ({}) to {}...",
            count, weather_count, signals, endpoint
        );
    }

    // Flush and shut down. Use spawn_blocking for the sync force_flush()
    // so we don't block a runtime worker thread (which would prevent the
    // PeriodicReader's tokio task from being polled).
    rt.block_on(async {
        tokio::time::sleep(Duration::from_secs(2)).await;
        let mp_clone = metric_provider.clone();
        let flush_result = tokio::task::spawn_blocking(move || mp_clone.force_flush()).await;
        match flush_result {
            Ok(Err(e)) => eprintln!("Warning: OTel metric flush: {}", e),
            Err(e) => eprintln!("Warning: OTel metric flush task: {}", e),
            _ => {}
        }
        if let Some(ref lp) = log_provider {
            let lp_clone = lp.clone();
            let _ = tokio::task::spawn_blocking(move || lp_clone.force_flush()).await;
        }
        let _ = metric_provider.shutdown();
        if let Some(ref lp) = log_provider {
            let _ = lp.shutdown();
        }
    });

    if !quiet {
        eprintln!("Done.");
    }

    Ok(())
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use opentelemetry::logs::Severity;

    #[test]
    fn test_parse_interval_seconds() {
        assert_eq!(parse_interval("30s").unwrap(), Duration::from_secs(30));
        assert_eq!(parse_interval("1s").unwrap(), Duration::from_secs(1));
    }

    #[test]
    fn test_parse_interval_minutes() {
        assert_eq!(parse_interval("5m").unwrap(), Duration::from_secs(300));
        assert_eq!(parse_interval("1m").unwrap(), Duration::from_secs(60));
    }

    #[test]
    fn test_parse_interval_hours() {
        assert_eq!(parse_interval("2h").unwrap(), Duration::from_secs(7200));
    }

    #[test]
    fn test_parse_interval_no_suffix() {
        assert_eq!(parse_interval("60").unwrap(), Duration::from_secs(60));
    }

    #[test]
    fn test_parse_interval_whitespace() {
        assert_eq!(parse_interval(" 30s ").unwrap(), Duration::from_secs(30));
    }

    #[test]
    fn test_parse_interval_invalid() {
        assert!(parse_interval("abc").is_err());
        assert!(parse_interval("").is_err());
    }

    #[test]
    fn test_parse_headers() {
        let headers = vec![
            "Authorization=Bearer token123".to_string(),
            "X-Custom=value".to_string(),
        ];
        let map = parse_headers(&headers).unwrap();
        assert_eq!(map.get("Authorization").unwrap(), "Bearer token123");
        assert_eq!(map.get("X-Custom").unwrap(), "value");
    }

    #[test]
    fn test_parse_headers_invalid() {
        let headers = vec!["no-equals-sign".to_string()];
        assert!(parse_headers(&headers).is_err());
    }

    #[test]
    fn test_extract_lox_value_json() {
        let text = r#"{"LL":{"value":"42.5","Code":"200"}}"#;
        assert_eq!(extract_lox_value(text), Some(42.5));
    }

    #[test]
    fn test_extract_lox_value_xml() {
        let text = r#"<LL value="21.5" />"#;
        assert_eq!(extract_lox_value(text), Some(21.5));
    }

    #[test]
    fn test_extract_lox_value_with_unit_suffix() {
        // Heap response: "352880/1016404kB"
        let text = r#"<LL control="dev/sys/heap" value="352880/1016404kB" Code="200"/>"#;
        assert_eq!(extract_lox_value(text), Some(352880.0));
    }

    #[test]
    fn test_extract_lox_value_none() {
        assert_eq!(extract_lox_value("no value here"), None);
    }

    #[test]
    fn test_parse_numeric_prefix() {
        assert_eq!(parse_numeric_prefix("42"), Some(42.0));
        assert_eq!(parse_numeric_prefix("42.5"), Some(42.5));
        assert_eq!(parse_numeric_prefix("352880/1016404kB"), Some(352880.0));
        assert_eq!(parse_numeric_prefix("-1.5"), Some(-1.5));
        assert_eq!(parse_numeric_prefix("abc"), None);
        assert_eq!(parse_numeric_prefix(""), None);
    }

    #[test]
    fn test_build_resource() {
        let cfg = Config {
            host: "https://192.168.1.100".to_string(),
            serial: "SERIAL123".to_string(),
            ..Default::default()
        };
        let resource = build_resource(&cfg);
        // Resource is opaque, but we can verify it was created without panic
        let _ = format!("{:?}", resource);
    }

    #[test]
    fn test_parse_log_severity() {
        assert!(matches!(
            parse_log_severity("2024-01-15 ERROR something broke"),
            Severity::Error
        ));
        assert!(matches!(
            parse_log_severity("connection failed at 14:00"),
            Severity::Error
        ));
        assert!(matches!(
            parse_log_severity("WARNING: disk space low"),
            Severity::Warn
        ));
        assert!(matches!(
            parse_log_severity("DEBUG: entering function"),
            Severity::Debug
        ));
        assert!(matches!(
            parse_log_severity("system started successfully"),
            Severity::Info
        ));
    }

    #[test]
    fn test_build_autopilot_uuids() {
        let mut state_map = HashMap::new();
        state_map.insert(
            "uuid-auto-1".to_string(),
            StateUuidInfo {
                control_name: "Arriving Home".to_string(),
                control_uuid: "rule-1".to_string(),
                state_name: "active".to_string(),
                control_type: "AutopilotRule".to_string(),
                room: None,
                category: None,
                unit: None,
            },
        );
        state_map.insert(
            "uuid-light-1".to_string(),
            StateUuidInfo {
                control_name: "Kitchen Light".to_string(),
                control_uuid: "ctrl-1".to_string(),
                state_name: "active".to_string(),
                control_type: "Switch".to_string(),
                room: Some("Kitchen".to_string()),
                category: None,
                unit: None,
            },
        );

        let autopilot = build_autopilot_uuids(&state_map);
        assert_eq!(autopilot.len(), 1);
        assert!(autopilot.contains("uuid-auto-1"));
        assert!(!autopilot.contains("uuid-light-1"));
    }

    #[test]
    fn test_otlp_signal_url() {
        assert_eq!(
            otlp_signal_url("http://localhost:4318", "/v1/logs"),
            "http://localhost:4318/v1/logs"
        );
        assert_eq!(
            otlp_signal_url("http://localhost:4318/", "/v1/traces"),
            "http://localhost:4318/v1/traces"
        );
        assert_eq!(
            otlp_signal_url("http://localhost:4318/v1/logs", "/v1/logs"),
            "http://localhost:4318/v1/logs"
        );
    }
}
