use serde_json::{json, Value};

use crate::args::orchestration::OrchestrationAction;
use crate::client::SocketClient;

pub fn handle(
    client: &mut SocketClient,
    json_output: bool,
    action: OrchestrationAction,
) -> Result<Option<Value>, String> {
    match action {
        OrchestrationAction::New {
            orchestrator,
            planner,
            workspace,
            title,
        } => new_pair(client, json_output, orchestrator, planner, workspace, title),
    }
}

/// Splits a `provider[:model]` spec. A trailing empty model (`"claude:"`) is
/// treated the same as no model — let the provider descriptor default apply.
pub(crate) fn split_provider_model(spec: &str) -> (String, Option<String>) {
    match spec.split_once(':') {
        Some((provider, model)) if !model.trim().is_empty() => {
            (provider.trim().to_string(), Some(model.trim().to_string()))
        }
        _ => (
            spec.split(':').next().unwrap_or(spec).trim().to_string(),
            None,
        ),
    }
}

fn new_pair(
    client: &mut SocketClient,
    json_output: bool,
    orchestrator: String,
    planner: String,
    workspace: Option<String>,
    title: Option<String>,
) -> Result<Option<Value>, String> {
    let (orchestrator_agent, orchestrator_model) = split_provider_model(&orchestrator);
    let (planner_agent, planner_model) = split_provider_model(&planner);

    let mut params = json!({
        "orchestrator_agent": orchestrator_agent,
        "planner_agent": planner_agent,
    });
    if let Some(m) = orchestrator_model {
        params["orchestrator_model"] = json!(m);
    }
    if let Some(m) = planner_model {
        params["planner_model"] = json!(m);
    }
    if let Some(w) = workspace {
        params["workspace_id"] = json!(w);
    }
    if let Some(t) = title {
        params["title"] = json!(t);
    }
    if let Ok(spawner) = std::env::var("CODETERM_TAB_ID") {
        if !spawner.is_empty() {
            params["spawned_by_tab_id"] = json!(spawner);
        }
    }

    let result = client.call("orchestration.new", Some(params))?;
    if json_output {
        return Ok(Some(result));
    }
    print_result(&result);
    Ok(None)
}

fn print_result(value: &Value) {
    let plan_id = value.get("plan_id").and_then(|v| v.as_str()).unwrap_or("?");
    let orch_tab = value
        .get("orchestrator_tab_id")
        .and_then(|v| v.as_str())
        .unwrap_or("?");
    let planner_tab = value
        .get("planner_tab_id")
        .and_then(|v| v.as_str())
        .unwrap_or("?");
    println!("Plan: {plan_id}");
    println!("Orchestrator: Tab {orch_tab}");
    println!("Planner: Tab {planner_tab}");
    if let Some(warning) = value.get("warning").and_then(|v| v.as_str()) {
        eprintln!("Warning: {warning}");
    }
}

#[cfg(test)]
mod tests {
    use super::split_provider_model;

    #[test]
    fn splits_provider_and_model() {
        assert_eq!(
            split_provider_model("claude:fable"),
            ("claude".to_string(), Some("fable".to_string()))
        );
    }

    #[test]
    fn provider_only_has_no_model() {
        assert_eq!(split_provider_model("codex"), ("codex".to_string(), None));
    }

    #[test]
    fn trailing_colon_with_no_model_falls_back_to_provider_only() {
        assert_eq!(
            split_provider_model("claude:"),
            ("claude".to_string(), None)
        );
    }
}
