mod calibration;
mod graph;
mod item;
mod skill;
mod tail;
mod worker;

use crate::cmd::print::handle_result;
use crate::{build_http, read_api_config, MemAction};

pub fn handle_mem(json_mode: bool, action: MemAction) {
    let (base, token) = read_api_config();
    let http = match build_http(&token) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error: failed to build HTTP client: {}", e);
            std::process::exit(1);
        }
    };

    let result: Result<serde_json::Value, String> = match action {
        action @ (MemAction::Search {
            query: _,
            kind: _,
            area: _,
            scope: _,
            limit: _,
            rerank: _,
        }
        | MemAction::Get { id: _ }
        | MemAction::Delete { id: _ }
        | MemAction::Forget { area: _ }
        | MemAction::Update {
            id: _,
            title: _,
            summary: _,
            area: _,
            tags: _,
            body: _,
            importance: _,
            changed_by: _,
        }
        | MemAction::Pin { id: _ }
        | MemAction::Unpin { id: _ }
        | MemAction::Pinned { limit: _ }
        | MemAction::Save {
            kind: _,
            title: _,
            body: _,
            summary: _,
            tags: _,
            area: _,
            importance: _,
            scope: _,
            workspace: _,
            pin: _,
            source_path: _,
            replace: _,
            skip_extract: _,
        }
        | MemAction::ExtractPending { limit: _ }
        | MemAction::WakeUp {
            budget: _,
            l0_only: _,
            no_l0: _,
            explain: _,
            workspace: _,
        }
        | MemAction::Stats
        | MemAction::Learn {
            text: _,
            kind: _,
            tags: _,
            error: _,
            feature: _,
        }
        | MemAction::LearnList
        | MemAction::LearnPromote { id: _ }
        | MemAction::Flush { pane: _, title: _ }
        | MemAction::Sync { path: _ }) => item::handle_mem_items(&http, &base, action),
        MemAction::History { id } => match item::handle_mem_history(&http, &base, id, json_mode) {
            Ok(None) => return,
            Ok(Some(v)) => Ok(v),
            Err(e) => Err(e),
        },
        MemAction::Taxonomy { workspace } => {
            match item::handle_mem_taxonomy(&http, &base, workspace, json_mode) {
                Ok(None) => return,
                Ok(Some(v)) => Ok(v),
                Err(e) => Err(e),
            }
        }
        MemAction::Tail { follow, limit } => {
            return tail::run_mem_tail(&http, &base, json_mode, follow, limit);
        }
        MemAction::Skill { action } => skill::handle_mem_skill(&http, &base, action),
        MemAction::Graph { action } => graph::handle_mem_graph(&http, &base, action),
        MemAction::Worker { action } => worker::handle_mem_worker(&http, &base, action),
        MemAction::RunWorker { .. } => unreachable!("RunWorker is short-circuited in main()"),
        MemAction::Dream { .. } => unreachable!("Dream is short-circuited in main()"),
        MemAction::Reembed { scope } => mem_post_request(
            &http,
            &format!(
                "{}/api/mem/reembed?scope={}",
                base,
                urlencoding::encode(&scope)
            ),
            &serde_json::json!({}),
        ),
        MemAction::Calibration { action } => {
            calibration::handle_mem_calibration(&http, &base, action)
        }
        MemAction::Secret { action } => {
            return crate::cmd::mem_secret::handle_mem_secret(json_mode, &http, &base, action);
        }
    };

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
    let resp = http.get(url).send().map_err(|e| e.to_string())?;
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
        .map_err(|e| e.to_string())?;
    let status = resp.status();
    let text = resp.text().unwrap_or_default();
    let val: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
    if !status.is_success() {
        return Err(if text.is_empty() {
            val.to_string()
        } else {
            text
        });
    }
    Ok(val)
}

#[cfg(test)]
mod tests;
