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
    let fallback = value
        .get("fallback_reason")
        .and_then(|v| v.as_str())
        .map(|reason| format!("; Live unavailable: {reason}"))
        .unwrap_or_default();
    if field("mode_used") == "live" {
        if field("model_confirmation") == "pending" {
            return format!(
                "Requested live model switch for Tab {} to {}; session confirmation pending{}",
                field("tab_id"),
                field("requested_model"),
                value
                    .get("confirmation_detail")
                    .and_then(|v| v.as_str())
                    .map(|detail| format!(" ({detail})"))
                    .unwrap_or_default(),
            );
        }
        let observed_model = value
            .get("observed_model")
            .and_then(|v| v.as_str())
            .unwrap_or_else(|| field("requested_model"));
        return format!(
            "Switched Tab {} to {} (was {}); model confirmed from its session record without a restart",
            field("tab_id"),
            observed_model,
            field("previous_model"),
        );
    }
    if field("model_confirmation") == "pending" {
        return format!(
            "Relaunched Tab {} in session {} with --model {} using RestartResume; agent ready, model confirmation pending{}",
            field("tab_id"),
            field("session_id"),
            field("launched_model"),
            fallback,
        );
    }
    let observed_model = value
        .get("observed_model")
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| field("launched_model"));
    format!(
        "Switched Tab {} to {} using RestartResume (was {}); resumed session {}, model confirmed by the provider{}",
        field("tab_id"),
        observed_model,
        field("previous_model"),
        field("session_id"),
        fallback,
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
        let success = serde_json::json!({ "ok": true, "observed_model": "opaque-model-9e" });
        let value = model_switch_result(reqwest::StatusCode::OK, &success.to_string())
            .expect("an ok body on 200 is success");
        assert_eq!(value["observed_model"], "opaque-model-9e");
        let bare = model_switch_result(reqwest::StatusCode::OK, "")
            .expect_err("a 200 without an ok body proves nothing");
        assert!(bare.data.is_some());
    }

    #[test]
    fn agent_model_pending_success_keeps_relaunch_evidence_in_json_and_human_output() {
        let success = serde_json::json!({
            "ok": true,
            "tab_id": "tab-4f3a",
            "mode_used": "restart_resume",
            "launched_model": "opaque-model-9e",
            "model_confirmation": "pending",
            "session_id": "session-8b2c",
            "relaunch_evidence": {
                "session_id": "session-8b2c",
                "launched_model": "opaque-model-9e",
                "agent_ready": true,
            },
        });
        let value = model_switch_result(reqwest::StatusCode::OK, &success.to_string())
            .expect("pending confirmation is a successful relaunch");
        let rendered = render_model_switch(&value);

        assert_eq!(value["ok"], true);
        assert_eq!(value["model_confirmation"], "pending");
        assert_eq!(value["relaunch_evidence"]["session_id"], "session-8b2c");
        assert_eq!(
            value["relaunch_evidence"]["launched_model"],
            "opaque-model-9e"
        );
        assert_eq!(value["relaunch_evidence"]["agent_ready"], true);
        assert!(rendered.contains("--model opaque-model-9e"), "{rendered}");
        assert!(rendered.contains("session-8b2c"), "{rendered}");
        assert!(
            rendered.contains("agent ready, model confirmation pending"),
            "{rendered}"
        );
        assert!(rendered.contains("using RestartResume"), "{rendered}");
    }

    #[test]
    fn agent_model_live_success_names_mode_and_session_confirmation() {
        let success = serde_json::json!({
            "ok": true,
            "tab_id": "tab-4f3a",
            "mode_used": "live",
            "requested_model": "provider/model-next",
            "observed_model": "provider/model-next",
            "previous_model": "provider/model-old",
            "model_confirmation": "observed",
            "evidence": "live_session_record",
            "session_id": "session-8b2c",
        });
        let value = model_switch_result(reqwest::StatusCode::OK, &success.to_string())
            .expect("live model switch is successful");
        let rendered = render_model_switch(&value);
        assert!(rendered.contains("model confirmed from its session record"));
        assert!(rendered.contains("without a restart"));
    }

    #[test]
    fn agent_model_fallback_reports_restart_reason() {
        let success = serde_json::json!({
            "ok": true,
            "tab_id": "tab-4f3a",
            "mode_used": "restart_resume",
            "launched_model": "provider/model-next",
            "model_confirmation": "observed",
            "fallback_reason": "the session route is absent",
            "session_id": "session-8b2c",
        });
        let value = model_switch_result(reqwest::StatusCode::OK, &success.to_string())
            .expect("fallback switch is successful");
        let rendered = render_model_switch(&value);
        assert!(rendered.contains("using RestartResume"));
        assert!(rendered.contains("Live unavailable: the session route is absent"));
    }
}
