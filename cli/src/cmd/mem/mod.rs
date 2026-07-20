mod calibration;
mod graph;
pub(crate) mod item;
mod skill;
mod socket;
mod tail;
pub(crate) mod transport;
mod worker;

use crate::client::SocketClient;
use crate::cmd::print::handle_result;
use crate::{build_http, read_api_config, MemAction};

pub fn handle_mem(client: &mut SocketClient, json_mode: bool, action: MemAction) {
    if let Some(result) = socket::try_dispatch(client, &action) {
        print_mem_result(json_mode, result);
        return;
    }

    // History/Taxonomy/Tail have daemon socket parity too, but keep their
    // own pretty-print (timeline / taxonomy tree) and --follow handling
    // instead of the generic try_dispatch -> handle_result path.
    match action {
        MemAction::History { id } => {
            let result = match item::handle_mem_history(client, id, json_mode) {
                Ok(None) => return,
                Ok(Some(v)) => Ok(v),
                Err(e) => Err(e),
            };
            handle_result(json_mode, result);
            return;
        }
        MemAction::Taxonomy { workspace } => {
            let result = match item::handle_mem_taxonomy(client, workspace, json_mode) {
                Ok(None) => return,
                Ok(Some(v)) => Ok(v),
                Err(e) => Err(e),
            };
            handle_result(json_mode, result);
            return;
        }
        MemAction::Tail { follow, limit } => {
            tail::run_mem_tail(client, json_mode, follow, limit);
            return;
        }
        _ => {}
    }

    // Everything left (skill/graph/worker/calibration/secret management)
    // is a separate, larger subsystem that still speaks HTTP.
    let (base, token) = read_api_config();
    let http = match build_http(&token) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error: failed to build HTTP client: {}", e);
            std::process::exit(1);
        }
    };

    let result: Result<serde_json::Value, String> = match action {
        MemAction::Skill { action } => skill::handle_mem_skill(&http, &base, action),
        MemAction::Graph { action } => graph::handle_mem_graph(&http, &base, action),
        MemAction::Worker { action } => worker::handle_mem_worker(&http, &base, action),
        MemAction::RunWorker { .. } => unreachable!("RunWorker is short-circuited in main()"),
        MemAction::Dream { .. } => unreachable!("Dream is short-circuited in main()"),
        MemAction::Calibration { action } => {
            calibration::handle_mem_calibration(&http, &base, action)
        }
        MemAction::Secret { action } => {
            return crate::cmd::mem_secret::handle_mem_secret(json_mode, &http, &base, action);
        }
        _ => unreachable!("all other mem actions have socket transport parity"),
    };

    print_mem_result(json_mode, result);
}

/// `{"text": "..."}` results (e.g. `mem wake-up`) print as raw text instead
/// of the generic `key: value` object dump — this is direct-injection
/// context, not structured data.
fn print_mem_result(json_mode: bool, result: Result<serde_json::Value, String>) {
    if let Ok(ref val) = result {
        if let Some(text) = val.get("text").and_then(|v: &serde_json::Value| v.as_str()) {
            println!("{}", text);
            return;
        }
    }
    handle_result(json_mode, result);
}

fn mem_get_request(
    http: &reqwest::blocking::Client,
    url: &str,
) -> Result<serde_json::Value, String> {
    let resp = http
        .get(url)
        .send()
        .map_err(|e| transport::mem_transport_error(url, &e))?;
    let status = resp.status();
    let val: serde_json::Value = resp.json().unwrap_or_default();
    if !status.is_success() {
        return Err(val
            .get("message")
            .and_then(|v| v.as_str())
            .map(String::from)
            .unwrap_or_else(|| format!("HTTP {}", status)));
    }
    Ok(val)
}

fn mem_post_request(
    http: &reqwest::blocking::Client,
    url: &str,
    body: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let resp = http
        .post(url)
        .json(body)
        .send()
        .map_err(|e| transport::mem_transport_error(url, &e))?;
    let status = resp.status();
    let text = resp.text().unwrap_or_default();
    let val: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
    if !status.is_success() {
        let body = text.trim();
        return Err(if body.is_empty() {
            status.to_string()
        } else {
            format!("{} {}", status, body)
        });
    }
    Ok(val)
}

#[cfg(test)]
mod tests;
