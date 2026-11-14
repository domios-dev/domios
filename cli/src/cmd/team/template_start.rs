use std::collections::BTreeMap;

use codeterm_team_package::contracts::{
    ManagerModelChoice, TemplateRef, TemplateStartRequest, TemplateStartResult,
};
use codeterm_team_package::is_role_name;
use serde_json::{json, Value};

use crate::args::team::template_start::{TemplateAction, TemplateStartArgs};
use crate::client::RpcClient;

/// `name[@version]` as a template reference.
pub(crate) fn template_ref(spec: &str) -> TemplateRef {
    match spec.rsplit_once('@') {
        Some((name, version)) if !name.is_empty() && !version.is_empty() => TemplateRef {
            name: name.to_string(),
            version: Some(version.to_string()),
        },
        _ => TemplateRef {
            name: spec.to_string(),
            version: None,
        },
    }
}

fn request(spec: &str) -> TemplateStartRequest {
    TemplateStartRequest {
        template: template_ref(spec),
        team_id: None,
        inputs: BTreeMap::new(),
        opt_out: Vec::new(),
        decision_id: None,
        secret_bindings: BTreeMap::new(),
        manager_models: BTreeMap::new(),
        role_placements: BTreeMap::new(),
    }
}

/// The pair request's `template` for a named template with no inputs or choices yet.
pub(crate) fn start_request(spec: &str) -> Value {
    serde_json::to_value(request(spec)).unwrap_or(Value::Null)
}

fn pairs(values: &[String], flag: &str) -> Result<BTreeMap<String, String>, String> {
    values
        .iter()
        .map(|value| {
            value
                .split_once('=')
                .filter(|(key, _)| !key.trim().is_empty())
                .map(|(key, value)| (key.trim().to_string(), value.to_string()))
                .ok_or_else(|| format!("--{flag} expects KEY=VALUE, got `{value}`"))
        })
        .collect()
}

fn start_params(args: TemplateStartArgs) -> Result<Value, String> {
    let mut template = request(&args.name);
    template.inputs = pairs(&args.inputs, "input")?;
    template.secret_bindings = pairs(&args.secrets, "secret")?;
    template.opt_out = args.opt_out;
    template.decision_id = args.decision;
    template.team_id = args.team;
    for (role, model) in pairs(&args.manager_models, "manager-model")? {
        if !is_role_name(&role) {
            return Err(format!("--manager-model names no role: `{role}`"));
        }
        template.manager_models.insert(
            role,
            ManagerModelChoice {
                model,
                reasoning_effort: None,
            },
        );
    }
    let mut params = json!({ "template": template });
    for (field, spec) in [
        ("orchestrator", args.orchestrator.as_deref()),
        ("planner", args.planner.as_deref()),
    ] {
        if let Some(spec) = spec {
            let (agent, model) = super::split_provider_model(spec);
            params[format!("{field}_agent")] = json!(agent);
            if let Some(model) = model {
                params[format!("{field}_model")] = json!(model);
            }
        }
    }
    if let Some(workspace) = args.workspace {
        params["workspace_id"] = json!(workspace);
    }
    if let Some(title) = args.title {
        params["title"] = json!(title);
    }
    if let Some(spawner) = crate::nonempty_env("CODETERM_TAB_ID") {
        params["spawned_by_tab_id"] = json!(spawner);
    }
    if args.acknowledge_leaf {
        params["leaf_creation_acknowledged"] = json!(true);
    }
    Ok(params)
}

fn fail(message: impl std::fmt::Display) -> ! {
    eprintln!("Error: {message}");
    std::process::exit(crate::cmd::print::CLI_ERROR_EXIT_CODE);
}

/// A peer start carries the template, this machine's pair choices and the peer workspace
/// `--workspace` names; the Team is new there and the peer's focus never moves.
fn peer_start_body(peer: String, params: &Value) -> Result<Value, String> {
    let workspace = params
        .get("workspace_id")
        .and_then(Value::as_str)
        .filter(|workspace| !workspace.trim().is_empty())
        .ok_or_else(|| {
            format!(
                "--workspace names the workspace on {peer}; list them with `domios workspace list --mesh {peer}`"
            )
        })?;
    let mut start = json!({ "template": params["template"], "workspace_id": workspace });
    for field in [
        "orchestrator_agent",
        "orchestrator_model",
        "planner_agent",
        "planner_model",
    ] {
        if let Some(value) = params.get(field) {
            start[field] = value.clone();
        }
    }
    Ok(json!({
        "peer": peer,
        "start": start,
        "caller_tab_id": crate::nonempty_env("CODETERM_TAB_ID"),
    }))
}

/// Peer hosting runs on this machine's API, which owns the mesh client and the answer watch.
fn post_local(json_output: bool, path: &str, body: &Value) {
    let api = crate::build_local_api().unwrap_or_else(|error| fail(error));
    let response = api
        .post(path)
        .json(body)
        .send()
        .unwrap_or_else(|error| fail(error));
    let status = response.status();
    let value: Value = response.json().unwrap_or(Value::Null);
    if !status.is_success() {
        if let Ok(TemplateStartResult::Refused { refusal }) =
            serde_json::from_value::<TemplateStartResult>(value.clone())
        {
            eprintln!("Error: the peer refused the template start");
            crate::cmd::print::print_json(&json!({ "refused": refusal }));
            std::process::exit(crate::cmd::print::CLI_ERROR_EXIT_CODE);
        }
        let detail = value
            .get("error")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| value.to_string());
        fail(format!("{status}: {detail}"));
    }
    if json_output {
        crate::cmd::print::print_json(&value);
    } else {
        crate::cmd::print::print_human(&value);
    }
}

/// Whether `--mesh` makes this template verb an origin command for a peer Main rather than a
/// command executed on that peer.
pub(crate) fn hosted_on_peer(action: &TemplateAction) -> bool {
    matches!(
        action,
        TemplateAction::Start(_) | TemplateAction::Transfer { .. } | TemplateAction::Outcome { .. }
    )
}

/// `team template start|transfer|outcome --mesh <peer>`: this machine asks the peer.
pub(crate) fn handle_peer(json_output: bool, action: TemplateAction, peer: &str) {
    match action {
        TemplateAction::Start(args) => {
            let params = start_params(args).unwrap_or_else(|message| fail(message));
            let body =
                peer_start_body(peer.to_string(), &params).unwrap_or_else(|message| fail(message));
            post_local(json_output, "/api/team-templates/peer-start", &body);
        }
        TemplateAction::Transfer { name, tab } => {
            let body = json!({
                "template": template_ref(&name),
                "peer": peer,
                "format": "team",
                "tab": tab,
            });
            post_local(json_output, "/api/team-templates/transfer", &body);
        }
        TemplateAction::Outcome { request } => {
            let body = json!({ "peer": peer, "request_id": request });
            post_local(json_output, "/api/team-templates/peer-outcome", &body);
        }
        TemplateAction::List | TemplateAction::Preview { .. } => {}
    }
}

pub fn handle(client: &mut dyn RpcClient, json_output: bool, action: TemplateAction) {
    let (method, params) = match action {
        TemplateAction::List => ("team.template.list", json!({})),
        TemplateAction::Preview { name } => (
            "team.template.preview",
            json!({ "template": template_ref(&name) }),
        ),
        TemplateAction::Start(args) => match start_params(args) {
            Ok(params) => ("team.template.start", params),
            Err(message) => fail(message),
        },
        TemplateAction::Transfer { .. } | TemplateAction::Outcome { .. } => {
            fail("this template verb needs --mesh <peer>")
        }
    };
    super::finish(json_output, client.call_detailed(method, Some(params)));
}
