use codeterm_provider_signals::{ApiErrorKind, RuntimeErrorSignal, RuntimeRetryableErrorKind};
use serde_json::Value;

pub(super) fn classify(record: &Value) -> Option<RuntimeErrorSignal> {
    classify_retry(record).or_else(|| {
        (record["type"] == "assistant"
            && record["isApiErrorMessage"] == true
            && record["isSidechain"] != true)
            .then(|| RuntimeErrorSignal {
                retryable: ApiErrorKind::retrying(record.get("error"), record),
                retryable_kind: None,
                retry_at_ms: None,
            })
    })
}

fn classify_retry(record: &Value) -> Option<RuntimeErrorSignal> {
    if record["type"] != "assistant" || record["isSidechain"] == true {
        return None;
    }
    if output_limit(&record["message"])
        || (record["isApiErrorMessage"] == true && output_limit(&record["error"]))
    {
        return Some(RuntimeErrorSignal {
            retryable: false,
            retryable_kind: Some(RuntimeRetryableErrorKind::OutputLimit),
            retry_at_ms: None,
        });
    }
    if record["isApiErrorMessage"] != true {
        return None;
    }
    let error = &record["error"];
    let kind = ApiErrorKind::from_error(Some(error), record);
    let mut fields = vec![
        error,
        &record["apiErrorCode"],
        &error["type"],
        &error["code"],
    ];
    if let Some(parts) = record["message"]["content"].as_array() {
        fields.extend(
            parts
                .iter()
                .filter(|part| part["type"] == "text")
                .map(|part| &part["text"]),
        );
    }
    let network = fields
        .into_iter()
        .filter_map(Value::as_str)
        .filter(|field| field.encode_utf16().count() <= 16384)
        .any(|field| {
            field
                .split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
                .any(|token| {
                    matches!(
                        token,
                        "ENOTFOUND"
                            | "ECONNRESET"
                            | "ECONNREFUSED"
                            | "ETIMEDOUT"
                            | "EAI_AGAIN"
                            | "EPIPE"
                            | "UND_ERR_CONNECT_TIMEOUT"
                            | "api_connection_error"
                            | "connection_error"
                    )
                })
        });
    let (retryable_kind, retry_at_ms) = match kind {
        ApiErrorKind::RateLimit => {
            let reset = record
                .get("apiErrorResetAtMs")
                .filter(|value| !value.is_null())
                .or_else(|| error.get("retryAtMs"));
            let due = if let Some(reset) = reset {
                reset.as_u64()?
            } else {
                let timestamp = chrono::DateTime::parse_from_rfc3339(record["timestamp"].as_str()?)
                    .ok()?
                    .timestamp_millis();
                u64::try_from(timestamp).ok()?.checked_add(
                    ApiErrorKind::retry_after(Some(error), record)?.checked_mul(1000)?,
                )?
            };
            if due > 9_007_199_254_740_991 {
                return None;
            }
            (RuntimeRetryableErrorKind::RateLimitedUntil, Some(due))
        }
        ApiErrorKind::ServerOverloaded => (RuntimeRetryableErrorKind::Transient, None),
        ApiErrorKind::Other if network => (RuntimeRetryableErrorKind::Transient, None),
        _ => {
            return Some(RuntimeErrorSignal {
                retryable: ApiErrorKind::retrying(Some(error), record),
                retryable_kind: None,
                retry_at_ms: None,
            })
        }
    };
    Some(RuntimeErrorSignal {
        retryable: ApiErrorKind::retrying(Some(error), record),
        retryable_kind: Some(retryable_kind),
        retry_at_ms,
    })
}

fn output_limit(value: &Value) -> bool {
    value["stop_reason"] == "max_tokens"
        || value["finish_reason"] == "length"
        || (value["status"] == "incomplete"
            && value["incomplete_details"]["reason"] == "max_output_tokens")
        || value["message"].as_str().is_some_and(|message| {
            message.encode_utf16().count() <= 16384
                && message.to_ascii_lowercase().split('\n').any(|line| {
                    line.split_once("incomplete response")
                        .is_some_and(|(_, tail)| {
                            tail.split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
                                .any(|token| token == "max_output_tokens")
                        })
                })
        })
}
