use serde_json::{json, Value};
use std::collections::BTreeMap;

use crate::args::sentinel::SentinelAction;
use crate::client::SocketClient;
use crate::cmd::print::{fail, print_json};

fn assignments(values: Vec<String>) -> Result<BTreeMap<String, String>, String> {
    let mut pairs = BTreeMap::new();
    for value in values {
        let (key, value) = value.split_once('=').ok_or("use bucket=value")?;
        if pairs.insert(key.into(), value.into()).is_some() {
            return Err(format!("duplicate assignment for bucket {key}"));
        }
    }
    Ok(pairs)
}

fn render(job: &Value) {
    println!(
        "{} [{}] bucket={} every={}ms{}",
        job["id"].as_str().unwrap_or("?"),
        if job
            .pointer("/machine/stopped")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            "stopped"
        } else {
            "watching"
        },
        job.pointer("/machine/last_bucket")
            .and_then(Value::as_str)
            .unwrap_or("unclassified"),
        job["interval_ms"],
        job["last_reason"]
            .as_str()
            .map(|reason| format!(" reason={reason}"))
            .unwrap_or_default()
    );
}

pub fn handle_sentinel(client: &mut SocketClient, json_mode: bool, action: SentinelAction) {
    let mut params = json!({ "caller_tab_id": std::env::var("CODETERM_TAB_ID").ok(), "session_binding_nonce": std::env::var("CODETERM_SESSION_BINDING_NONCE").ok() });
    let method = match action {
        SentinelAction::Create {
            probe,
            question,
            buckets,
            every,
            routes,
            until,
            timeout,
            matches,
        } => {
            params["create"] = json!({ "probe": probe, "question": question, "buckets": buckets, "every": every, "routes": assignments(routes).unwrap_or_else(|error| fail(2, error)), "matches": assignments(matches).unwrap_or_else(|error| fail(2, error)), "until": until, "timeout": timeout });
            "sentinel.create"
        }
        SentinelAction::List => "sentinel.list",
        SentinelAction::Show { id } => {
            params["id"] = json!(id);
            "sentinel.show"
        }
        SentinelAction::Stop { id } => {
            params["id"] = json!(id);
            "sentinel.stop"
        }
    };
    let result = client
        .call(method, Some(params))
        .unwrap_or_else(|error| fail(1, error.to_string()));
    if json_mode {
        print_json(&result);
        return;
    }
    if let Some(jobs) = result.as_array() {
        if jobs.is_empty() {
            println!("No sentinels.");
        }
        for job in jobs {
            render(job);
        }
    } else {
        render(&result);
        if method == "sentinel.show" {
            println!(
                "Probe: {}\nQuestion: {}\nCreator: {}",
                result["probe"].as_str().unwrap_or("?"),
                result["question"].as_str().unwrap_or("?"),
                result["creator_tab_id"].as_str().unwrap_or("?")
            );
        }
    }
}
