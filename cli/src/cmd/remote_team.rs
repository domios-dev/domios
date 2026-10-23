use super::plan::caller_context::CallerContext;
use crate::client::{RpcClient, SocketCallError};
use serde_json::{json, Value};

pub(super) fn call(
    client: &mut dyn RpcClient,
    method: &str,
    mut params: Value,
) -> Result<Value, SocketCallError> {
    if client.is_remote() {
        return Err(SocketCallError::Transport("Team adoption and reparenting must run on the local origin; use --peer instead of --mesh".into()));
    }
    CallerContext::from_env().apply_trusted_session(&mut params);
    client.call_detailed(method, Some(params))
}

pub(super) fn reparent(
    client: &mut dyn RpcClient,
    json_output: bool,
    tab: String,
    to: String,
    peer: Option<String>,
) {
    match call(
        client,
        "agent.reparent",
        json!({ "tab_id": tab, "to": to, "peer": peer }),
    ) {
        Ok(value) => super::print::handle_result(json_output, Ok(value)),
        Err(error) => super::model_error::exit_with_socket_error(json_output, error),
    }
}

#[cfg(test)]
mod tests;
