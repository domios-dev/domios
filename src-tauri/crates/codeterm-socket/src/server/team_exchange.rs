use serde_json::Value;

use codeterm_service_core::service::{team_exchange, team_template_recall, ServiceState};

use super::error_codes::template_unavailable;
use super::JsonRpcError;

pub(super) const METHODS: [&str; 4] = [
    "team.distill",
    "team.export",
    "team.import",
    "team.template.recall",
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
        "team.distill" => team_exchange::distill(svc, params.as_ref()),
        "team.export" => team_exchange::export(svc, params.as_ref()),
        "team.import" => team_exchange::import(svc, params.as_ref()),
        _ => team_template_recall::recall(svc, params.as_ref()),
    };
    outcome.map_err(template_unavailable)
}
