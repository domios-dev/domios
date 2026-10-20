use super::engine::{self, Session};
use crate::service::ServiceContext;
use codeterm_plugin::agent_runtime::{RuntimeOperation, RuntimeResult};
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "camelCase", deny_unknown_fields)]
pub enum ResetCreditsRequest {
    Read,
    Redeem {
        #[serde(rename = "idempotencyKey")]
        idempotency_key: uuid::Uuid,
        confirmed: bool,
        #[serde(rename = "creditId")]
        credit_id: Option<String>,
    },
}

pub(super) async fn supported(session: &Session) -> bool {
    let capability = session
        .adapter
        .lock()
        .as_ref()
        .and_then(|provider| provider.agent_runtime());
    let Some(capability) = capability else {
        return false;
    };
    let Some(minimum) = capability.reset_credits_min_version else {
        return false;
    };
    tokio::task::spawn_blocking(move || {
        let version =
            codeterm_plugin::provider_version::observe(&capability.command.binary).version;
        version_supported(version.as_deref(), &minimum)
    })
    .await
    .unwrap_or(false)
}

pub async fn operate(
    svc: &ServiceContext,
    tab: &str,
    request: ResetCreditsRequest,
) -> Result<Value, String> {
    let session = engine::for_tab(tab).ok_or("Runtime binding unavailable")?;
    engine::validate_binding(svc, &session).map_err(|error| error.to_string())?;
    if !supported(&session).await {
        return match request {
            ResetCreditsRequest::Read => Ok(json!({"supported":false})),
            _ => Err("Reset credits are unsupported by the installed provider".into()),
        };
    }
    let before_count = super::projections::for_tab(svc, tab)
        .ok()
        .and_then(|usage| usage["runtime_usage"][0]["resets"]["availableCount"].as_u64());
    let (action, value) = operation(request)?;
    let _permit = session
        .gate
        .try_acquire()
        .map_err(|_| "Runtime operation already in progress")?;
    let process = session
        .process
        .lock()
        .clone()
        .ok_or("Runtime unavailable")?;
    let thread_id = session.context.lock().thread_id.clone();
    let result = engine::run(
        svc,
        &session.provider,
        &session,
        &process,
        RuntimeOperation::Control {
            thread_id,
            action: action.into(),
            value,
        },
    )
    .await
    .map_err(|error| error.to_string())?;
    engine::validate_binding(svc, &session).map_err(|error| error.to_string())?;
    match result {
        RuntimeResult::ResetCreditConsumed { outcome } => {
            let remaining_count = super::projections::for_tab(svc, tab)
                .ok()
                .and_then(|usage| usage["runtime_usage"][0]["resets"]["availableCount"].as_u64());
            Ok(
                json!({"outcome":outcome,"beforeCount":before_count,"remainingCount":remaining_count}),
            )
        }
        RuntimeResult::Done if action == "readResetCredits" => Ok(json!({"supported":true})),
        _ => Err("Unexpected reset credit response".into()),
    }
}

fn version_supported(installed: Option<&str>, minimum: &str) -> bool {
    installed
        .and_then(|value| semver::Version::parse(value).ok())
        .zip(semver::Version::parse(minimum).ok())
        .is_some_and(|(installed, minimum)| installed >= minimum)
}
fn operation(request: ResetCreditsRequest) -> Result<(&'static str, Value), String> {
    let selected = match request {
        ResetCreditsRequest::Read => ("readResetCredits", json!({"details":true})),
        ResetCreditsRequest::Redeem {
            idempotency_key,
            confirmed: true,
            credit_id,
        } if idempotency_key.get_version_num() == 4 => {
            let mut value = json!({"idempotencyKey":idempotency_key});
            if let Some(id) = credit_id {
                if id.trim().is_empty() {
                    return Err("Reset credit identifier is empty".into());
                }
                value["creditId"] = json!(id);
            }
            ("redeemResetCredit", value)
        }
        _ => return Err("Reset requires explicit confirmation and a confirmation UUID".into()),
    };
    Ok(selected)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reset_version_gate_is_absent_for_old_or_unknown_versions() {
        for installed in [None, Some("0.160.0"), Some("invalid")] {
            assert!(!version_supported(installed, "0.162.1"));
        }
        assert!(version_supported(Some("0.162.1"), "0.162.1"));
        assert!(version_supported(Some("0.163.0"), "0.162.1"));
    }
    #[test]
    fn reset_operation_requires_confirmation_and_preserves_retry_key() {
        let key = uuid::Uuid::parse_str("12345678-1234-4123-8123-123456789abc").unwrap();
        assert!(operation(ResetCreditsRequest::Redeem {
            idempotency_key: key,
            confirmed: false,
            credit_id: None,
        })
        .is_err());
        assert!(operation(ResetCreditsRequest::Redeem {
            idempotency_key: uuid::Uuid::nil(),
            confirmed: true,
            credit_id: None,
        })
        .is_err());
        let a = operation(ResetCreditsRequest::Redeem {
            idempotency_key: key,
            confirmed: true,
            credit_id: None,
        })
        .unwrap();
        let b = operation(ResetCreditsRequest::Redeem {
            idempotency_key: key,
            confirmed: true,
            credit_id: None,
        })
        .unwrap();
        assert_eq!(a, b);
        assert_eq!(a.1["idempotencyKey"], key.to_string());
        assert_eq!(
            operation(ResetCreditsRequest::Read).unwrap().1,
            json!({"details":true})
        );
    }
    #[test]
    fn selected_reset_credit_round_trips_and_empty_selection_is_refused() {
        let request = json!({"action":"redeem","idempotencyKey":"12345678-1234-4123-8123-123456789abc","confirmed":true,"creditId":"opaque-credit"});
        let selected: ResetCreditsRequest = serde_json::from_value(request.clone()).unwrap();
        let (_, value) = operation(selected).unwrap();
        assert_eq!(value["creditId"], "opaque-credit");
        assert_eq!(value["idempotencyKey"], request["idempotencyKey"]);
        let mut empty = request;
        empty["creditId"] = json!("");
        assert!(operation(serde_json::from_value(empty).unwrap()).is_err());
    }
}
