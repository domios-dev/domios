use serde_json::{json, Value};

use crate::args::orchestrator::{OrchestratorAction, OrchestratorHeartbeatAction};
use crate::client::{SocketCallError, SocketClient};
use crate::cmd::plan::{call_plan_rpc, owner_from_env, resolve_text};
use crate::resolve_tab;

pub fn handle(
    client: &mut SocketClient,
    json_output: bool,
    action: OrchestratorAction,
) -> Result<Option<Value>, String> {
    match action {
        OrchestratorAction::Status { plan, tab } => status(client, json_output, plan, tab),
        OrchestratorAction::Inspect { plan, tab } => inspect(client, json_output, plan, tab),
        OrchestratorAction::Learn { text, plan, tag } => {
            learn(client, json_output, text, plan, tag)
        }
        OrchestratorAction::Heartbeat { action } => match action {
            OrchestratorHeartbeatAction::Show { plan } => heartbeat_show(client, json_output, plan),
        },
        OrchestratorAction::Refresh { plan, tab } => refresh(client, json_output, plan, tab),
        OrchestratorAction::Handoff {
            plan,
            task,
            tab,
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
            tab,
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

fn refresh(
    client: &mut SocketClient,
    json_output: bool,
    plan: Option<String>,
    tab: Option<String>,
) -> Result<Option<Value>, String> {
    let plan_id = plan.ok_or("--plan or CODETERM_PLAN_ID is required")?;
    let tab_id = resolve_tab(client, tab);
    let result = client
        .call_detailed(
            "orchestrator.refresh",
            Some(json!({ "tab_id": tab_id, "plan_id": plan_id })),
        )
        .map_err(|err| {
            format!(
                "cannot start orchestrator refresh for tab {tab_id}.\nfix: run `codeterm tab list --agents` and pass an agent tab with `--tab <tab-id>`.\ncause: {}",
                concise_socket_error(err)
            )
        })?;
    if json_output {
        return Ok(Some(result));
    }
    let status = result
        .get("status")
        .and_then(|v| v.as_str())
        .unwrap_or("started");
    println!("orchestrator refresh {status} for {plan_id} (tab {tab_id})");
    Ok(None)
}

fn heartbeat_params(plan: Option<String>) -> Value {
    json!({
        "plan_id": plan,
        "tab_id": std::env::var("CODETERM_TAB_ID").ok(),
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

fn status(
    client: &mut SocketClient,
    json_output: bool,
    plan: Option<String>,
    tab: Option<String>,
) -> Result<Option<Value>, String> {
    let out = status_snapshot(client, plan, tab)?;
    let compact = compact_status(&out);
    if json_output {
        return Ok(Some(compact));
    }
    print!("{}", render_status(&compact));
    Ok(None)
}

fn inspect(
    client: &mut SocketClient,
    json_output: bool,
    plan: Option<String>,
    tab: Option<String>,
) -> Result<Option<Value>, String> {
    let out = status_snapshot(client, plan, tab)?;
    if json_output {
        return Ok(Some(out));
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&out).map_err(|err| err.to_string())?
    );
    Ok(None)
}

fn status_snapshot(
    client: &mut SocketClient,
    plan: Option<String>,
    tab: Option<String>,
) -> Result<Value, String> {
    let plan_id = plan.ok_or("--plan or CODETERM_PLAN_ID is required")?;
    let tab_id = resolve_tab(client, tab);
    let state = call_plan_rpc(client, "plan.state", json!({ "plan_id": plan_id }))?;
    let out = json!({
        "plan_id": plan_id,
        "tab_id": tab_id,
        "live_lanes": {
            "open": state.get("open_lanes").cloned().unwrap_or_else(|| json!([])),
            "blocked": state.get("blocked_lanes").cloned().unwrap_or_else(|| json!([])),
            "agents": state.get("live_agents").cloned().unwrap_or_else(|| json!([])),
            "heartbeat": state.get("heartbeat").cloned().unwrap_or_else(|| json!({})),
        },
        "drift": state.get("drift").cloned().unwrap_or_else(|| json!({})),
        "next_actions": state.get("next_actions").cloned().unwrap_or_else(|| json!([])),
        "reminder": state.get("reminder").cloned().unwrap_or_else(|| json!({})),
    });
    Ok(out)
}

fn compact_status(value: &Value) -> Value {
    let count = |path: &[&str]| {
        path.iter()
            .try_fold(value, |current, key| current.get(*key))
            .and_then(Value::as_array)
            .map_or(0, Vec::len)
    };
    let drift = value
        .get("drift")
        .and_then(|v| v.get("total"))
        .and_then(Value::as_u64)
        .unwrap_or_else(|| count(&["drift", "items"]) as u64);
    let actions = value
        .get("next_actions")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let compact_actions: Vec<Value> = actions
        .iter()
        .take(3)
        .map(|action| {
            json!({
                "kind": action.get("kind").cloned().unwrap_or(Value::Null),
                "task_id": action.get("task_id").cloned().unwrap_or(Value::Null),
            })
        })
        .collect();
    json!({
        "plan_id": value.get("plan_id").cloned().unwrap_or(Value::Null),
        "tab_id": value.get("tab_id").cloned().unwrap_or(Value::Null),
        "live_lanes": {
            "open": count(&["live_lanes", "open"]),
            "blocked": count(&["live_lanes", "blocked"]),
            "agents": count(&["live_lanes", "agents"]),
        },
        "drift": drift,
        "next_actions": compact_actions,
        "next_actions_total": actions.len(),
        "reminder": value.get("reminder").cloned().unwrap_or_else(|| json!({})),
    })
}

fn render_status(value: &Value) -> String {
    let plan_id = value.get("plan_id").and_then(|v| v.as_str()).unwrap_or("?");
    let tab_id = value.get("tab_id").and_then(|v| v.as_str()).unwrap_or("?");
    let drift = value.get("drift").and_then(Value::as_u64).unwrap_or(0);
    let actions = value
        .get("next_actions")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let action_total = value
        .get("next_actions_total")
        .and_then(Value::as_u64)
        .unwrap_or(actions.len() as u64);
    let live_agents = value
        .get("live_lanes")
        .and_then(|v| v.get("agents"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let open = value
        .get("live_lanes")
        .and_then(|v| v.get("open"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let blocked = value
        .get("live_lanes")
        .and_then(|v| v.get("blocked"))
        .and_then(Value::as_u64)
        .unwrap_or(0);

    let mut out = format!("Orchestrator status for {plan_id} (tab {tab_id})\n");
    out.push_str(&format!(
        "live_lanes: open={open} blocked={blocked} agents={live_agents}\n"
    ));
    out.push_str(&format!("drift: {drift}\n"));
    let reminder = value.get("reminder").cloned().unwrap_or_default();
    out.push_str(&format!(
        "reminder: last_tick={:?} last_due={:?} last_delivery={:?} retry_reason={:?}\n",
        reminder.get("last_tick"),
        reminder.get("last_due"),
        reminder.get("last_delivery"),
        reminder.get("delivery_failure_retry_reason"),
    ));
    if actions.is_empty() {
        out.push_str("next_actions: none\n");
    } else {
        out.push_str("next_actions:\n");
        for action in actions {
            let kind = action
                .get("kind")
                .and_then(Value::as_str)
                .unwrap_or("action");
            let task = action
                .get("task_id")
                .and_then(Value::as_str)
                .map(|id| format!(" {id}"))
                .unwrap_or_default();
            let command = action_command(plan_id, &action);
            out.push_str(&format!("- {kind}{task}\n  {command}\n"));
        }
        let hidden = action_total.saturating_sub(3);
        if hidden > 0 {
            out.push_str(&format!(
                "- +{hidden} more; use `codeterm orchestrator inspect --plan {plan_id}`\n"
            ));
        }
    }
    out
}

fn action_command(plan_id: &str, action: &Value) -> String {
    let kind = action.get("kind").and_then(|v| v.as_str()).unwrap_or("");
    let task_id = action.get("task_id").and_then(|v| v.as_str());
    match (kind, task_id) {
        ("gate", Some(task)) => {
            format!("codeterm task show {task} && codeterm plan task close {plan_id} {task}")
        }
        ("advance", Some(task)) => {
            format!("codeterm task show {task} && codeterm plan record {plan_id} --task {task} --step decision:<next-instruction>")
        }
        ("dispatch", Some(task)) => format!("codeterm task show {task}"),
        ("reconcile", _) => format!("codeterm plan reconcile {plan_id}"),
        ("consider_stop", _) => {
            format!("codeterm plan record {plan_id} --step decision:<loop-decision>")
        }
        _ => format!("codeterm plan get {plan_id}"),
    }
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
    let plan_id = value.get("plan_id").and_then(|v| v.as_str()).unwrap_or("?");
    println!("Heartbeat for {plan_id}");
    let Some(heartbeat) = value.get("heartbeat") else {
        println!("(empty)");
        return;
    };
    let mut printed = false;
    if let Some(focus) = heartbeat.get("current_focus").and_then(|v| v.as_str()) {
        if !focus.trim().is_empty() {
            println!("focus: {}", focus.trim());
            printed = true;
        }
    }
    printed |= print_string_array(heartbeat, "active_lanes", "lanes");
    printed |= print_string_array(heartbeat, "rules", "rules");
    if let Some(learnings) = heartbeat.get("learnings").and_then(|v| v.as_array()) {
        let texts: Vec<&str> = learnings
            .iter()
            .filter_map(|learning| learning.get("text").and_then(|v| v.as_str()))
            .filter(|text| !text.trim().is_empty())
            .collect();
        if !texts.is_empty() {
            println!("learnings:");
            for text in texts {
                println!("- {}", text.trim());
            }
            printed = true;
        }
    }
    if !printed {
        println!("(empty)");
    }
}

fn print_string_array(value: &Value, key: &str, label: &str) -> bool {
    let Some(items) = value.get(key).and_then(|v| v.as_array()) else {
        return false;
    };
    let items: Vec<&str> = items
        .iter()
        .filter_map(|v| v.as_str())
        .filter(|s| !s.trim().is_empty())
        .collect();
    if items.is_empty() {
        return false;
    }
    println!("{label}:");
    for item in items {
        println!("- {}", item.trim());
    }
    true
}

#[allow(clippy::too_many_arguments)]
fn handoff(
    client: &mut SocketClient,
    json_output: bool,
    plan: Option<String>,
    task: Option<String>,
    tab: Option<String>,
    reseed: bool,
    mode: String,
    dry_run: bool,
    force: bool,
    goal: Option<String>,
    next_actions: Vec<String>,
    from_file: Option<String>,
) -> Result<Option<Value>, String> {
    let plan_id = plan.ok_or("--plan or CODETERM_PLAN_ID is required")?;
    let tab_id = resolve_tab(client, tab);
    let source_session_id = if dry_run {
        tab_session_id(client, &tab_id).unwrap_or_else(|_| "<dry-run-source-session>".into())
    } else {
        tab_session_id(client, &tab_id)?
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
            "tab_id": tab_id,
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
            "tab_id": tab_id,
            "source_session_id": source_session_id,
            "mode": mode,
            "force": force,
        }),
    )
    .map(Some)
}

fn tab_session_id(client: &mut SocketClient, tab_id: &str) -> Result<String, String> {
    let tabs = client
        .call_detailed("tab.list", None)
        .map_err(|err| render_reseed_tab_lookup_error(tab_id, err))?;
    let tab = tabs
        .as_array()
        .and_then(|items| {
            items
                .iter()
                .find(|item| item.get("id").and_then(|v| v.as_str()) == Some(tab_id))
        })
        .ok_or_else(|| render_reseed_tab_session_error(tab_id))?;
    tab.get("agent_session_id")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .ok_or_else(|| render_reseed_tab_session_error(tab_id))
}

fn render_reseed_tab_lookup_error(tab_id: &str, err: SocketCallError) -> String {
    format!(
        "cannot inspect tab {tab_id} for handoff reseed.\nfix: run `codeterm tab list --agents` and pass an agent tab with `--tab <tab-id>`.\ncause: {}",
        concise_socket_error(err)
    )
}

fn render_reseed_tab_session_error(tab_id: &str) -> String {
    format!(
        "cannot reseed because tab {tab_id} is not an agent or has no session.\nfix: run `codeterm tab list --agents` and pass an agent tab with `--tab <tab-id>`."
    )
}

fn concise_socket_error(err: SocketCallError) -> String {
    match err {
        SocketCallError::Rpc(rpc) => rpc.message,
        SocketCallError::Transport(message) => message,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        action_command, compact_status, render_reseed_tab_lookup_error,
        render_reseed_tab_session_error, render_status,
    };
    use crate::client::{RpcError, SocketCallError};

    #[test]
    fn status_renderer_includes_exact_commands() {
        let rendered = render_status(&compact_status(&json!({
            "plan_id": "pl_1",
            "tab_id": "tab1",
            "live_lanes": {
                "open": [{"task_id": "task_a"}],
                "blocked": [],
                "agents": [{"tab_id": "worker1"}],
                "heartbeat": {"active_lanes": ["L2"]}
            },
            "drift": {"total": 2, "items": []},
            "next_actions": [
                {"kind": "gate", "task_id": "task_a", "message": "gate task_a"},
                {"kind": "reconcile", "message": "reconcile"}
            ]
        })));

        assert!(rendered.contains("live_lanes: open=1 blocked=0 agents=1"));
        assert!(rendered.contains("drift: 2"));
        assert!(
            rendered.contains("codeterm task show task_a && codeterm plan task close pl_1 task_a")
        );
        assert!(rendered.contains("codeterm plan reconcile pl_1"));
    }

    #[test]
    fn compact_status_omits_lane_bodies() {
        let compact = compact_status(&json!({
            "plan_id": "pl_1",
            "tab_id": "tab1",
            "live_lanes": {
                "open": [{"task_id": "task_a", "large": "payload"}],
                "blocked": [],
                "agents": [{"tab_id": "worker1", "transcript": "large"}]
            },
            "drift": {"items": [{"reason": "drift body"}]},
            "next_actions": [{"kind": "dispatch", "task_id": "task_a", "message": "very long action body"}],
            "reminder": {"last_tick": 1}
        }));

        assert_eq!(compact["live_lanes"]["open"], 1);
        assert_eq!(compact["live_lanes"]["agents"], 1);
        assert_eq!(compact["drift"], 1);
        assert_eq!(compact["next_actions_total"], 1);
        let encoded = compact.to_string();
        assert!(!encoded.contains("transcript"));
        assert!(!encoded.contains("drift body"));
        assert!(!encoded.contains("very long action body"));
    }

    #[test]
    fn action_commands_cover_task_and_plan_context() {
        assert_eq!(
            action_command("pl_1", &json!({"kind": "dispatch", "task_id": "task_b"})),
            "codeterm task show task_b"
        );
        assert_eq!(
            action_command("pl_1", &json!({"kind": "reconcile"})),
            "codeterm plan reconcile pl_1"
        );
    }

    #[test]
    fn reseed_tab_lookup_error_includes_agent_recovery_command() {
        let msg = render_reseed_tab_lookup_error(
            "tab1",
            SocketCallError::Rpc(RpcError {
                code: -32603,
                message: "Tab not found".into(),
                data: None,
            }),
        );

        assert!(msg.contains("cannot inspect tab tab1 for handoff reseed"));
        assert!(msg.contains("codeterm tab list --agents"));
        assert!(msg.contains("--tab <tab-id>"));
        assert!(!msg.contains("RPC error"));
        assert!(!msg.contains("-32603"));
    }

    #[test]
    fn reseed_session_error_includes_agent_recovery_command() {
        let msg = render_reseed_tab_session_error("tab1");

        assert!(msg.contains("cannot reseed because tab tab1 is not an agent or has no session"));
        assert!(msg.contains("codeterm tab list --agents"));
        assert!(msg.contains("--tab <tab-id>"));
    }
}
