use super::*;
use crate::cmd::error_contract::{classify, ErrorCode};

#[test]
fn spawn_limit_refusal_uses_the_daemon_reason_and_refused_exit_contract() {
    for code in ["free_limit_reached", "plan_limit_locked"] {
        let reason = format!("{code}: quota reason (limit: agent.tabs)");
        let error = SocketCallError::Rpc(RpcError {
            code: -32603,
            message: reason.clone(),
            data: None,
        });
        assert_eq!(render_socket_error(&error), format!("Error: {reason}"));
        let mapped = CliError::from(error);
        assert_eq!(
            classify(&mapped.message, mapped.code, mapped.data.as_ref()),
            ErrorCode::Refused
        );
        assert_eq!(crate::cmd::print::CLI_ERROR_EXIT_CODE, 1);
    }
}
