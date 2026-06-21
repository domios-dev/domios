//! Host-side structured prompt handles (PUSH model).
//!
//! Some providers expose pending prompts as a structured PUSH stream rather
//! than ephemeral PTY UI. OpenCode runs a local HTTP server with an SSE event
//! bus (`/event`) carrying `question.asked` / `permission.asked`, and accepts
//! replies on documented endpoints. This module owns:
//!
//! - a per-pane endpoint registry (`base_url` + `cwd`), populated at spawn;
//! - a background subscriber thread per pane that turns SSE prompt events into
//!   `pane.pending_prompt` (the PTY stays display-only for these classes);
//! - the active-prompt store the hot path reads ([`pending_for_pane`]);
//! - the HTTP reply round-trip ([`respond`]), wired through the same
//!   option-index contract the PTY send-back uses.
//!
//! All capability-gated by the caller (`supports_structured_prompts`) — this
//! module never branches on a provider id. The opencode-specific event/endpoint
//! shapes are isolated in [`opencode_sse`].

pub mod opencode_sse;
#[cfg(test)]
mod tests;

use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader};
use std::sync::LazyLock;
use std::time::Duration;

use parking_lot::Mutex;

use opencode_sse::{parse_event, reply_body, reply_endpoint, ParsedEvent, ResponseRoute};

use crate::multiplexer::types::PendingPrompt;
use crate::service::ServiceContext;

struct Endpoint {
    base_url: String,
    cwd: String,
}

struct Active {
    prompt: PendingPrompt,
    route: ResponseRoute,
}

static ENDPOINTS: LazyLock<Mutex<HashMap<String, Endpoint>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static ACTIVE: LazyLock<Mutex<HashMap<String, Active>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static SUBSCRIBED: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(|| Mutex::new(HashSet::new()));

/// Allocate a free loopback port for a provider's structured-prompt server by
/// binding `127.0.0.1:0`, reading the assigned port, then releasing it so the
/// agent can bind it. A small TOCTOU window, acceptable for a local dev server.
pub fn allocate_server() -> Option<crate::agent::provider::StructuredPromptServer> {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).ok()?;
    let port = listener.local_addr().ok()?.port();
    drop(listener);
    Some(crate::agent::provider::StructuredPromptServer {
        hostname: "127.0.0.1".to_string(),
        port,
    })
}

/// Record the structured-prompt server for a pane (called at spawn, once the
/// host has allocated the fixed port). `base_url` is e.g. `http://127.0.0.1:PORT`.
pub fn register_endpoint(pane_id: &str, base_url: String, cwd: String) {
    ENDPOINTS
        .lock()
        .insert(pane_id.to_string(), Endpoint { base_url, cwd });
}

/// The live structured prompt for a pane, if one is pending. Cheap in-memory
/// read on the hot path.
pub fn pending_for_pane(pane_id: &str) -> Option<PendingPrompt> {
    ACTIVE.lock().get(pane_id).map(|a| a.prompt.clone())
}

fn route_for_pane(pane_id: &str) -> Option<ResponseRoute> {
    ACTIVE.lock().get(pane_id).map(|a| a.route.clone())
}

/// Drop all structured-prompt state for a pane (pane closed / agent exited).
pub fn clear_pane(pane_id: &str) {
    ENDPOINTS.lock().remove(pane_id);
    ACTIVE.lock().remove(pane_id);
    SUBSCRIBED.lock().remove(pane_id);
}

/// Start the SSE subscriber for a pane if it has a registered endpoint and none
/// is running yet. Idempotent — safe to call on every session-bind. The caller
/// has already gated on `supports_structured_prompts`.
pub fn maybe_start_subscriber(svc: &ServiceContext, pane_id: &str) {
    let (base_url, cwd) = match ENDPOINTS.lock().get(pane_id) {
        Some(ep) => (ep.base_url.clone(), ep.cwd.clone()),
        None => return,
    };
    {
        let mut subscribed = SUBSCRIBED.lock();
        if !subscribed.insert(pane_id.to_string()) {
            return; // already running
        }
    }
    let svc = svc.clone();
    let pane_id = pane_id.to_string();
    std::thread::Builder::new()
        .name(format!("oc-sse-{}", &pane_id[..pane_id.len().min(8)]))
        .spawn(move || run_subscriber(svc, pane_id, base_url, cwd))
        .ok();
}

/// Answer a pending structured prompt over HTTP. Returns `Some(result)` when the
/// pane has a structured route (the PTY send-back must then be skipped), or
/// `None` when there is no structured handle (caller falls back to PTY).
pub fn respond(svc: &ServiceContext, pane_id: &str, value: &str) -> Option<Result<(), String>> {
    let route = route_for_pane(pane_id)?;
    let base_url = ENDPOINTS.lock().get(pane_id).map(|e| e.base_url.clone())?;
    let result = send_reply(&base_url, &route, value);
    // Clear locally regardless — a failed reply leaves the agent blocked, but a
    // stale prompt in the UI is worse; the next SSE event re-surfaces if needed.
    ACTIVE.lock().remove(pane_id);
    svc.push_structured_prompt(pane_id, None);
    Some(result)
}

fn send_reply(base_url: &str, route: &ResponseRoute, value: &str) -> Result<(), String> {
    let body = reply_body(route, value).ok_or_else(|| "option index out of range".to_string())?;
    let url = format!("{base_url}{}", reply_endpoint(route));
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client
        .post(&url)
        .json(&body)
        .send()
        .map_err(|e| e.to_string())?;
    if resp.status().is_success() {
        Ok(())
    } else {
        Err(format!("opencode reply {}: HTTP {}", url, resp.status()))
    }
}

fn run_subscriber(svc: ServiceContext, pane_id: String, base_url: String, cwd: String) {
    // A handful of reconnect attempts — the server is local and comes up within
    // a second or two of spawn; give up (and let a later bind restart us) if it
    // never appears or the pane goes away.
    for attempt in 0..30u32 {
        if pane_gone(&svc, &pane_id) {
            break;
        }
        if connect_and_stream(&svc, &pane_id, &base_url, &cwd) {
            break; // pane closed mid-stream
        }
        std::thread::sleep(Duration::from_millis(500 + (attempt.min(10) as u64) * 200));
    }
    SUBSCRIBED.lock().remove(&pane_id);
}

/// Connect once and stream until the connection drops or the pane closes.
/// Returns true if the pane is gone (stop reconnecting), false to retry.
fn connect_and_stream(svc: &ServiceContext, pane_id: &str, base_url: &str, cwd: &str) -> bool {
    let client = match reqwest::blocking::Client::builder().build() {
        Ok(c) => c,
        Err(_) => return false,
    };
    let resp = match client
        .get(format!("{base_url}/event"))
        .query(&[("directory", cwd)])
        .header("Accept", "text/event-stream")
        .send()
    {
        Ok(r) if r.status().is_success() => r,
        _ => return false,
    };
    let reader = BufReader::new(resp);
    for line in reader.lines() {
        if pane_gone(svc, pane_id) {
            return true;
        }
        let Ok(line) = line else {
            return false; // stream error → reconnect
        };
        let Some(payload) = line.strip_prefix("data:") else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(payload.trim()) else {
            continue;
        };
        match parse_event(&value) {
            ParsedEvent::Prompt { prompt, route } => {
                ACTIVE.lock().insert(
                    pane_id.to_string(),
                    Active {
                        prompt: prompt.clone(),
                        route,
                    },
                );
                svc.push_structured_prompt(pane_id, Some(prompt));
            }
            ParsedEvent::Replied { request_id } => {
                let cleared = {
                    let mut active = ACTIVE.lock();
                    match active.get(pane_id) {
                        Some(a) if a.route.request_id == request_id => {
                            active.remove(pane_id);
                            true
                        }
                        _ => false,
                    }
                };
                if cleared {
                    svc.push_structured_prompt(pane_id, None);
                }
            }
            ParsedEvent::Ignored => {}
        }
    }
    false
}

fn pane_gone(svc: &ServiceContext, pane_id: &str) -> bool {
    svc.mux.read().get_pane(pane_id).is_none()
}
