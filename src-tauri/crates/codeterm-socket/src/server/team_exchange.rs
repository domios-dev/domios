use serde_json::Value;

use codeterm_service_core::service::team_exchange::{self, ExchangeError, INVALID_REQUEST};
use codeterm_service_core::service::{team_template_recall, ServiceState};

use super::error_codes::INVALID_PARAMS;
use super::JsonRpcError;

pub(super) const METHODS: [&str; 6] = [
    "team.distill",
    "team.export",
    "team.import",
    "team.template.recall",
    "team.template.delete",
    "team.template.card",
];

pub(super) fn handles(method: &str) -> bool {
    METHODS.contains(&method)
}

/// A bad request is invalid params; a refused package carries its diagnostics as data.
fn refused(error: ExchangeError) -> JsonRpcError {
    JsonRpcError {
        code: if error.code == INVALID_REQUEST {
            INVALID_PARAMS
        } else {
            -32000
        },
        message: error.to_string(),
        data: serde_json::to_value(&error).ok(),
    }
}

pub(super) fn dispatch(
    method: &str,
    params: &Option<Value>,
    svc: &ServiceState,
) -> Result<Value, JsonRpcError> {
    let params = params.as_ref();
    let outcome = match method {
        "team.distill" => team_exchange::distill(svc, params),
        "team.export" => team_exchange::export(svc, params),
        "team.import" => team_exchange::import(svc, params),
        "team.template.delete" => team_exchange::delete(svc, params),
        "team.template.card" => team_exchange::share_card(params),
        _ => team_template_recall::recall(svc, params),
    };
    outcome.map_err(refused)
}
