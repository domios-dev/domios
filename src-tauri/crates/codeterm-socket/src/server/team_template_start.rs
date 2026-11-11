use serde_json::Value;

use codeterm_service_core::service::team_template_start;
use codeterm_service_core::service::ServiceState;

use super::JsonRpcError;

pub(super) const METHODS: [&str; 3] = [
    "team.template.list",
    "team.template.preview",
    "team.template.start",
];

pub(super) fn handles(method: &str) -> bool {
    METHODS.contains(&method)
}

pub(super) fn dispatch(
    method: &str,
    params: &Option<Value>,
    svc: &ServiceState,
) -> Result<Value, JsonRpcError> {
    let outcome = match method {
        "team.template.list" => team_template_start::list(svc),
        "team.template.preview" => team_template_start::preview(svc, params.as_ref()),
        _ => team_template_start::start(svc, params.as_ref()),
    };
    outcome.map_err(super::error_codes::template_unavailable)
}
