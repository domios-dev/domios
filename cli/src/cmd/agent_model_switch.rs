use crate::cmd::print::CliError;

/// The daemon answers every switch with a JSON body; success is only an `ok`
/// body on a success status, and every refusal keeps that body as error data.
pub(super) fn model_switch_result(
    status: reqwest::StatusCode,
    text: &str,
) -> Result<serde_json::Value, CliError> {
    let mut value = serde_json::from_str::<serde_json::Value>(text).unwrap_or_else(|_| {
        let message = if text.trim().is_empty() {
            status.to_string()
        } else {
            text.trim().to_string()
        };
        serde_json::json!({ "ok": false, "error": message })
    });
    if status.is_success() && value.get("ok").and_then(|ok| ok.as_bool()) == Some(true) {
        return Ok(value);
    }
    let message = value
        .get("error")
        .and_then(|error| error.as_str())
        .map(str::to_string)
        .unwrap_or_else(|| format!("model switch failed: {status}"));
    if let Some(body) = value.as_object_mut() {
        body.insert("http_status".into(), status.as_u16().into());
    }
    Err(CliError {
        message,
        code: None,
        data: Some(value),
    })
}

pub(super) fn render_model_switch(value: &serde_json::Value) -> String {
    let field = |key: &str| value.get(key).and_then(|v| v.as_str()).unwrap_or("-");
    format!(
        "Switched Tab {} to {} (was {}); resumed session {}, model confirmed by the provider",
        field("tab_id"),
        field("model"),
        field("previous_model"),
        field("session_id"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rendered(json: bool, error: &CliError) -> (String, String) {
        let mut out = Vec::new();
        let mut err = Vec::new();
        crate::cmd::print::write_cli_error(json, error, &mut out, &mut err);
        (
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
        )
    }

    #[test]
    fn agent_model_json_on_error() {
        let refusal = serde_json::json!({
            "ok": false,
            "error": "opaque-refusal-4c1",
            "reason": "model_switch_unsupported",
            "provider": "opaque-provider-4c1",
        });
        let error = model_switch_result(reqwest::StatusCode::CONFLICT, &refusal.to_string())
            .expect_err("a refusal is an error");
        let (out, err) = rendered(true, &error);
        assert!(err.is_empty(), "--json keeps the failure on stdout: {err}");
        let envelope: serde_json::Value = serde_json::from_str(&out).expect("one JSON envelope");
        assert_eq!(envelope["ok"], serde_json::Value::Bool(false));
        assert!(
            out.contains("opaque-refusal-4c1") && out.contains("model_switch_unsupported"),
            "the daemon's refusal travels in the envelope: {out}"
        );

        let plain = model_switch_result(reqwest::StatusCode::BAD_GATEWAY, "opaque-proxy-text")
            .expect_err("a non-JSON failure is still an error");
        let (out, err) = rendered(true, &plain);
        assert!(err.is_empty());
        let envelope: serde_json::Value = serde_json::from_str(&out).expect("one JSON envelope");
        assert_eq!(envelope["ok"], serde_json::Value::Bool(false));
        assert!(out.contains("opaque-proxy-text"), "{out}");
    }

    #[test]
    fn agent_model_success_requires_an_ok_body() {
        let success = serde_json::json!({ "ok": true, "model": "opaque-model-9e" });
        let value = model_switch_result(reqwest::StatusCode::OK, &success.to_string())
            .expect("an ok body on 200 is success");
        assert_eq!(value["model"], "opaque-model-9e");
        let bare = model_switch_result(reqwest::StatusCode::OK, "")
            .expect_err("a 200 without an ok body proves nothing");
        assert!(bare.data.is_some());
    }
}
