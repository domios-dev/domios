use std::collections::BTreeMap;

use codeterm_team_package::contracts::{TemplateRef, TemplateStartRequest};
use serde_json::{json, Value};

use crate::args::team::template_start::TemplateAction;
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

/// The pair request's `template` for a named template with no inputs or choices yet.
pub(crate) fn start_request(spec: &str) -> Value {
    serde_json::to_value(TemplateStartRequest {
        template: template_ref(spec),
        team_id: None,
        inputs: BTreeMap::new(),
        opt_out: Vec::new(),
        decision_id: None,
        manager_models: BTreeMap::new(),
    })
    .unwrap_or(Value::Null)
}

pub fn handle(client: &mut dyn RpcClient, json_output: bool, action: TemplateAction) {
    let (method, params) = match action {
        TemplateAction::List => ("team.template.list", json!({})),
        TemplateAction::Preview { name } => (
            "team.template.preview",
            json!({ "template": template_ref(&name) }),
        ),
        TemplateAction::Start {
            name,
            workspace,
            title,
        } => (
            "team.template.start",
            json!({
                "template": start_request(&name),
                "workspace_id": workspace,
                "title": title,
            }),
        ),
    };
    super::finish(json_output, client.call_detailed(method, Some(params)));
}
