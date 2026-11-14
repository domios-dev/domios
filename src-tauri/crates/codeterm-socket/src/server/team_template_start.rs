use codeterm_orchestration::{OrchestrationPairExt, OrchestrationPairParams};
use codeterm_team_package::contracts::{TemplateStartRequest, TemplateStartResult};
use serde::Deserialize;
use serde_json::Value;

use codeterm_service_core::service::team_template_start::{self, TemplateCallError};
use codeterm_service_core::service::ServiceState;

use super::JsonRpcError;

pub(super) const METHODS: [&str; 4] = [
    "team.channel",
    "team.template.list",
    "team.template.preview",
    "team.template.start",
];

pub(super) fn handles(method: &str) -> bool {
    METHODS.contains(&method)
}

/// `team.template.start` is the ordinary pair request with a template; it has no path of its own.
#[derive(Deserialize)]
struct StartParams {
    template: TemplateStartRequest,
    #[serde(default)]
    orchestrator_agent: String,
    #[serde(default)]
    orchestrator_model: Option<String>,
    #[serde(default)]
    planner_agent: String,
    #[serde(default)]
    planner_model: Option<String>,
    #[serde(default)]
    workspace_id: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    spawned_by_tab_id: Option<String>,
    #[serde(default)]
    leaf_creation_acknowledged: bool,
}

fn invalid_params(message: String) -> JsonRpcError {
    JsonRpcError {
        code: -32602,
        message,
        data: None,
    }
}

/// A typed template outcome that is not a started Team: the message keeps the JSON.
pub(super) fn template_result_error(message: String) -> JsonRpcError {
    let data = TemplateStartResult::from_error_message(&message)
        .and_then(|result| serde_json::to_value(result).ok());
    JsonRpcError {
        code: if data.is_some() { -32000 } else { -32603 },
        message,
        data,
    }
}

fn call_error(error: TemplateCallError) -> JsonRpcError {
    match error {
        TemplateCallError::InvalidParams(message) => invalid_params(message),
        TemplateCallError::Refused(refusal) => {
            template_result_error(TemplateStartResult::Refused { refusal }.error_message())
        }
    }
}

fn start(params: &Option<Value>, svc: &ServiceState) -> Result<Value, JsonRpcError> {
    let params: StartParams = serde_json::from_value(params.clone().unwrap_or(Value::Null))
        .map_err(|error| invalid_params(format!("invalid params: {error}")))?;
    let pair = OrchestrationPairParams {
        orchestrator_agent: params.orchestrator_agent,
        orchestrator_model: params.orchestrator_model,
        planner_agent: params.planner_agent,
        planner_model: params.planner_model,
        workspace_id: params.workspace_id,
        title: params.title,
        spawned_by_tab_id: params.spawned_by_tab_id,
        leaf_creation_acknowledged: params.leaf_creation_acknowledged,
        template: Some(params.template),
        ..Default::default()
    };
    let svc_ref = &**svc;
    let result =
        codeterm_service_core::async_runtime::block_on(svc_ref.create_orchestration_pair(pair))
            .map_err(template_result_error)?;
    serde_json::to_value(result).map_err(|error| JsonRpcError {
        code: -32603,
        message: error.to_string(),
        data: None,
    })
}

pub(super) fn dispatch(
    method: &str,
    params: &Option<Value>,
    svc: &ServiceState,
) -> Result<Value, JsonRpcError> {
    let encode = |value: Result<Value, serde_json::Error>| {
        value.map_err(|error| JsonRpcError {
            code: -32603,
            message: error.to_string(),
            data: None,
        })
    };
    match method {
        "team.channel" => {
            let action = serde_json::from_value(params.clone().unwrap_or(Value::Null))
                .map_err(|error| invalid_params(format!("invalid channel action: {error}")))?;
            codeterm_service_core::service::team_channels::command(svc, action).map_err(|message| {
                JsonRpcError {
                    code: -32603,
                    message,
                    data: None,
                }
            })
        }
        "team.template.list" => encode(serde_json::to_value(team_template_start::list(svc))),
        "team.template.preview" => {
            let preview = team_template_start::preview(svc, params.as_ref()).map_err(call_error)?;
            encode(serde_json::to_value(preview))
        }
        _ => start(params, svc),
    }
}
