use std::collections::BTreeMap;

use codeterm_team_package::contracts::{ManagerModelChoice, TemplateRef, TemplateStartRequest};
use codeterm_team_package::ManagerSlot;
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
    for (slot, model) in pairs(&args.manager_models, "manager-model")? {
        let slot = ManagerSlot::parse(&slot)
            .ok_or_else(|| format!("--manager-model names no manager slot: `{slot}`"))?;
        template.manager_models.insert(
            slot,
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

pub fn handle(client: &mut dyn RpcClient, json_output: bool, action: TemplateAction) {
    let (method, params) = match action {
        TemplateAction::List => ("team.template.list", json!({})),
        TemplateAction::Preview { name } => (
            "team.template.preview",
            json!({ "template": template_ref(&name) }),
        ),
        TemplateAction::Start(args) => match start_params(args) {
            Ok(params) => ("team.template.start", params),
            Err(message) => {
                eprintln!("Error: {message}");
                std::process::exit(crate::cmd::print::CLI_ERROR_EXIT_CODE);
            }
        },
    };
    super::finish(json_output, client.call_detailed(method, Some(params)));
}
