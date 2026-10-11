use super::JsonRpcError;
use crate::startup::{RestApiControl, RestApiPhase, RestApiStatus};

pub(crate) async fn execute(
    method: &str,
    control: &RestApiControl,
) -> Result<serde_json::Value, JsonRpcError> {
    let status = match method {
        "rest_api.status" => control.status(),
        "rest_api.restart" => control.restart_and_wait().await.map_err(internal)?,
        _ => {
            return Err(JsonRpcError {
                code: -32601,
                message: "Method not found".into(),
                data: None,
            })
        }
    };
    status_result(method == "rest_api.restart", &status)
}

fn status_result(
    restarting: bool,
    status: &RestApiStatus,
) -> Result<serde_json::Value, JsonRpcError> {
    let value = serde_json::to_value(status).map_err(|error| internal(error.to_string()))?;
    if restarting && status.phase != RestApiPhase::Listening {
        return Err(JsonRpcError {
            code: -32603,
            message: status
                .message
                .clone()
                .unwrap_or_else(|| "REST listener did not become ready".into()),
            data: Some(value),
        });
    }
    Ok(value)
}

fn internal(message: String) -> JsonRpcError {
    JsonRpcError {
        code: -32603,
        message,
        data: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::startup::RestApiDiagnostic;

    #[test]
    fn bind_failure_is_observable_but_never_a_successful_restart() {
        let status = RestApiStatus {
            generation: 1,
            phase: RestApiPhase::Failed,
            diagnostic: Some(RestApiDiagnostic::BindOwnershipFailed),
            message: Some("fixture occupied port".into()),
        };
        let value = status_result(false, &status).unwrap();
        assert_eq!(value["phase"], "failed");
        assert_eq!(value["diagnostic"], "bind_ownership_failed");
        assert_eq!(value["message"], status.message.as_deref().unwrap());
        let error = status_result(true, &status).unwrap_err();
        assert_eq!(error.message, status.message.unwrap());
        assert_eq!(error.data, Some(value));
    }
}
