use serde_json::{json, Value};

use crate::args::orchestrator::{OrchestratorAction, OrchestratorHeartbeatAction};
use crate::client::SocketClient;
use crate::cmd::plan::{call_plan_rpc, owner_from_env, resolve_text};
use crate::resolve_pane;

pub fn handle(
    client: &mut SocketClient,
    json_output: bool,
    action: OrchestratorAction,
) -> Result<Option<Value>, String> {
    match action {
        OrchestratorAction::Learn { text, plan, tag } => learn(client, json_output, text, plan, tag),
        OrchestratorAction::Heartbeat { action } => match action {
            OrchestratorHeartbeatAction::Show { plan } => heartbeat_show(client, json_output, plan),
        },
        OrchestratorAction::Handoff {
            plan,
            task,
            pane,
            reseed,
            mode,
            dry_run,
            force,
            goal,
            next_actions,
            from_file,
        } => handoff(
            client,
            json_output,
            plan,
            task,
            pane,
            reseed,
            mode,
            dry_run,
            force,
            goal,
            next_actions,
            from_file,
        ),
    }
}

fn heartbeat_params(plan: Option<String>) -> Value {
    json!({
        "plan_id": plan,
        "pane_id": std::env::var("CODETERM_PANE_ID").ok(),
        "session_id": std::env::var("CODETERM_SESSION_ID").ok(),
    })
}

fn learn(
    client: &mut SocketClient,
    json_output: bool,
    text: String,
    plan: Option<String>,
    tag: Option<String>,
) -> Result<Option<Value>, String> {
    let mut params = heartbeat_params(plan);
    params["text"] = json!(text);
    if let Some(tag) = tag {
        params["tag"] = json!(tag);
    }
    let result = call_plan_rpc(client, "plan.heartbeat.learn", params)?;
    if json_output {
        return Ok(Some(result));
    }
    let plan_id = result
        .get("plan_id")
        .and_then(|v| v.as_str())
        .unwrap_or("?");
    let deduped = result
        .get("deduped")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let state = if deduped { "deduped" } else { "added" };
    println!("learning {state} for {plan_id}");
    Ok(None)
}

fn heartbeat_show(
    client: &mut SocketClient,
    json_output: bool,
    plan: Option<String>,
) -> Result<Option<Value>, String> {
    let result = call_plan_rpc(client, "plan.heartbeat.show", heartbeat_params(plan))?;
    if json_output {
        return Ok(Some(result));
    }
    print_heartbeat(&result);
    Ok(None)
}

fn print_heartbeat(value: &Value) {
    let plan_id = value
        .get("plan_id")
        .and_then(|v| v.as_str())
        .unwrap_or("?");
    println!("Heartbeat for {plan_id}");
    let Some(heartbeat) = value.get("heartbeat") else {
        println!("(empty)");
        return;
    };
    if let Some(focus) = heartbeat.get("current_focus").and_then(|v| v.as_str()) {
        if !focus.trim().is_empty() {
            println!("focus: {}", focus.trim());
        }
    }
    print_string_array(heartbeat, "active_lanes", "lanes");
    print_string_array(heartbeat, "rules", "rules");
    if let Some(learnings) = heartbeat.get("learnings").and_then(|v| v.as_array()) {
        if !learnings.is_empty() {
            println!("learnings:");
            for learning in learnings {
                if let Some(text) = learning.get("text").and_then(|v| v.as_str()) {
                    println!("- {}", text.trim());
                }
            }
        }
    }
    if value
        .get("reinjection")
        .and_then(|v| v.as_str())
        .map_or(true, |text| text.trim().is_empty())
    {
        println!("(empty)");
    }
}

fn print_string_array(value: &Value, key: &str, label: &str) {
    let Some(items) = value.get(key).and_then(|v| v.as_array()) else {
        return;
    };
    let items: Vec<&str> = items
        .iter()
        .filter_map(|v| v.as_str())
        .filter(|s| !s.trim().is_empty())
        .collect();
    if items.is_empty() {
        return;
    }
    println!("{label}:");
    for item in items {
        println!("- {}", item.trim());
    }
}

#[allow(clippy::too_many_arguments)]
fn handoff(
    client: &mut SocketClient,
    json_output: bool,
    plan: Option<String>,
    task: Option<String>,
    pane: Option<String>,
    reseed: bool,
    mode: String,
    dry_run: bool,
    force: bool,
    goal: Option<String>,
    next_actions: Vec<String>,
    from_file: Option<String>,
) -> Result<Option<Value>, String> {
    let plan_id = plan.ok_or("--plan or CODETERM_PLAN_ID is required")?;
    let pane_id = resolve_pane(client, pane);
    let source_session_id = if dry_run {
        pane_session_id(client, &pane_id).unwrap_or_else(|_| "<dry-run-source-session>".into())
    } else {
        pane_session_id(client, &pane_id)?
    };
    let narrative = resolve_text(None, from_file)?;
    let seed = format!(
        "run `codeterm plan handoff show {} --latest --markdown` FIRST, load orchestrator role (docs/contributing/agents.md + AGENTS.md), `codeterm plan get`, resume from next_actions; do not re-handoff.",
        plan_id
    );

    if dry_run {
        let sends = if mode == "compact" {
            json!([
                {"order": 1, "text": seed, "enter": true, "reason": "resume_seed"},
                {"order": 2, "text": "<provider compact command>", "enter": true, "reason": "provider_compact"}
            ])
        } else {
            json!([
                {"order": 1, "text": "<provider fresh-context command>", "enter": true, "reason": "provider_fresh_context"},
                {"order": 2, "text": seed, "enter": true, "reason": "session_start_resume_seed"}
            ])
        };
        let out = json!({
            "ok": true,
            "dry_run": true,
            "plan_id": plan_id,
            "pane_id": pane_id,
            "source_session_id": source_session_id,
            "mode": mode,
            "would_create_handoff": true,
            "would_reseed": reseed,
            "sends": sends,
        });
        if json_output {
            return Ok(Some(out));
        }
        println!("{}", serde_json::to_string_pretty(&out).unwrap_or_default());
        return Ok(None);
    }

    let mut create = json!({
        "plan_id": plan_id,
        "mode": mode,
        "next_actions": next_actions,
        "from": owner_from_env(),
    });
    if let Some(task) = task {
        create["task"] = json!(task);
    }
    if let Some(goal) = goal {
        create["goal"] = json!(goal);
    }
    if let Some(narrative) = narrative {
        create["narrative"] = json!(narrative);
    }
    let created = call_plan_rpc(client, "plan.handoff.create", create)?;
    if !reseed {
        return Ok(Some(created));
    }
    let handoff_id = created
        .get("handoff_id")
        .and_then(|v| v.as_str())
        .ok_or("handoff create did not return handoff_id")?;
    call_plan_rpc(
        client,
        "plan.handoff.reseed",
        json!({
            "plan_id": plan_id,
            "selector": handoff_id,
            "pane_id": pane_id,
            "source_session_id": source_session_id,
            "mode": mode,
            "force": force,
        }),
    )
    .map(Some)
}

fn pane_session_id(client: &mut SocketClient, pane_id: &str) -> Result<String, String> {
    let panes = client.call("pane.list", None)?;
    let pane = panes
        .as_array()
        .and_then(|items| {
            items
                .iter()
                .find(|item| item.get("id").and_then(|v| v.as_str()) == Some(pane_id))
        })
        .ok_or_else(|| format!("pane {pane_id} not found"))?;
    pane.get("agent_session_id")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .ok_or_else(|| format!("pane {pane_id} has no source session id"))
}
