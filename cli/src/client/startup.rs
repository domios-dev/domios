use std::io::{BufRead, BufReader};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

const STARTUP_DEADLINE: Duration = Duration::from_secs(30);
const PHASES: [&str; 8] = [
    "listener",
    "integrity",
    "states",
    "providers",
    "service",
    "plans",
    "background",
    "platform",
];

pub(super) fn target_url() -> Result<String, String> {
    let resolved = crate::resolve_api_config();
    let explicit_url = std::env::var("CODETERM_API_URL").ok();
    let socket = std::env::var("CODETERM_SOCKET_PATH").ok();
    let explicit_port = std::env::var("CODETERM_API_PORT").ok();
    if explicit_url.is_none() && explicit_port.is_none() {
        if socket.as_deref().is_some_and(|socket| {
            super::socket_dev_class(&super::normalize_socket_path(socket)).is_none()
        }) {
            return Err("Cannot derive an HTTP target from CODETERM_SOCKET_PATH".into());
        }
        let dev = crate::resolved_instance().value.is_some()
            || socket
                .as_deref()
                .is_some_and(|path| super::is_dev_socket(&super::normalize_socket_path(path)));
        if dev && resolved.port_source == super::ResolutionSource::Default {
            return Err("Development API port is absent from the selected configuration".into());
        }
    }
    let configured = format!("http://{}:{}", resolved.host, resolved.port);
    super::http_base_url_from(Some(explicit_url.as_deref().unwrap_or(&configured)), None)
}

pub(super) fn refusal(response: &Value) -> Option<String> {
    let snapshot = response.get("startup")?;
    let state = snapshot.get("state")?.as_str()?;
    let (code, message) = match state {
        "starting" => {
            let phase = current_phase(snapshot).unwrap_or("unknown");
            ("starting", format!("Domios is starting (phase {phase})"))
        }
        "failed" => ("startup_failed", "Domios could not finish starting".into()),
        _ => return None,
    };
    Some(format!(
        "HTTP 503: {}",
        json!({
            "error": {"code": code}, "message": message, "startup": snapshot,
        })
    ))
}

fn current_phase(snapshot: &Value) -> Option<&'static str> {
    PHASES
        .into_iter()
        .find(|phase| snapshot["phases"][phase] == "starting")
}

fn may_proceed(snapshot: &Value, wait: bool) -> Result<bool, String> {
    match snapshot["state"].as_str() {
        Some("ready") => Ok(true),
        Some("starting") if wait => Ok(false),
        _ => Err(snapshot_error(snapshot)),
    }
}

pub(super) fn probe_with_readiness(
    mut probe: impl FnMut() -> Result<(reqwest::StatusCode, Value), String>,
    wait_ready: impl FnOnce(Value) -> Result<(), String>,
) -> Result<(), String> {
    let (mut status, mut response) = probe()?;
    if let Some(snapshot) = response
        .get("startup")
        .filter(|snapshot| snapshot["state"] == "starting")
    {
        wait_ready(snapshot.clone())?;
        (status, response) = probe()?;
    }
    super::http_rpc_result(status, response)
        .map(|_| ())
        .map_err(|error| error.to_string())
}

pub(super) fn after_refusal(
    client: &reqwest::blocking::Client,
    base_url: &str,
    snapshot: Value,
) -> Result<(), String> {
    wait_for_ready(
        client,
        base_url,
        snapshot,
        true,
        Instant::now() + STARTUP_DEADLINE,
    )
    .map(|_| ())
}

// No command has been sent: only an observed startup snapshot permits waiting.
pub(super) fn before_fallback(wait: bool) -> Result<bool, String> {
    let Ok(base_url) = target_url() else {
        return Ok(false);
    };
    let client = reqwest::blocking::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|error| error.to_string())?;
    let deadline = Instant::now() + STARTUP_DEADLINE;
    let health = client
        .get(format!("{base_url}/api/health"))
        .timeout(
            deadline
                .saturating_duration_since(Instant::now())
                .min(Duration::from_secs(2)),
        )
        .send()
        .and_then(|response| response.error_for_status())
        .and_then(|response| response.json::<Value>());
    let Ok(health) = health else { return Ok(false) };
    let Some(snapshot) = health.get("startup").cloned() else {
        return Ok(false);
    };
    wait_for_ready(&client, &base_url, snapshot, wait, deadline)
}

fn wait_for_ready(
    client: &reqwest::blocking::Client,
    base_url: &str,
    mut snapshot: Value,
    wait: bool,
    deadline: Instant,
) -> Result<bool, String> {
    if snapshot["state"] == "starting" && wait && !crate::cmd::error_contract::json_mode() {
        eprintln!(
            "{}",
            crate::cmd::print::CliError::new(snapshot_error(&snapshot)).message
        );
    }
    loop {
        if may_proceed(&snapshot, wait)? {
            return Ok(true);
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(snapshot_error(&snapshot));
        }
        let response = client
            .get(format!("{base_url}/api/startup/events"))
            .timeout(remaining)
            .send()
            .and_then(|response| response.error_for_status())
            .map_err(|_| snapshot_error(&snapshot))?;
        if !response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.starts_with("text/event-stream"))
        {
            return Err(snapshot_error(&snapshot));
        }
        if read_events(BufReader::new(response), &mut snapshot)
            .map_err(|_| snapshot_error(&snapshot))?
        {
            continue;
        }
        // EOF means listener replacement; re-subscribe within the original deadline.
    }
}

fn snapshot_error(snapshot: &Value) -> String {
    refusal(&json!({"startup": snapshot}))
        .unwrap_or_else(|| "Invalid Domios startup snapshot".into())
}

fn read_events(mut reader: impl BufRead, snapshot: &mut Value) -> Result<bool, String> {
    let mut event = String::new();
    let mut data = String::new();
    loop {
        let mut line = String::new();
        if reader
            .read_line(&mut line)
            .map_err(|error| error.to_string())?
            == 0
        {
            return Ok(false);
        }
        let line = line.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            if event == "startup-state" {
                *snapshot = serde_json::from_str(&data).map_err(|error| error.to_string())?;
                if snapshot["state"] != "starting" {
                    return Ok(true);
                }
            }
            event.clear();
            data.clear();
        } else if let Some(value) = line.strip_prefix("event:") {
            event = value.trim_start().to_owned();
        } else if let Some(value) = line.strip_prefix("data:") {
            if !data.is_empty() {
                data.push('\n');
            }
            data.push_str(value.trim_start());
        }
    }
}

#[cfg(test)]
mod tests;
