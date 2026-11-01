use super::agent_models::select_provider;
use crate::cmd::print::CliError;

pub(super) fn handle(
    local_http: &crate::local_api::LocalApiClient,
    base: &str,
    tab_id: String,
    name: Option<String>,
    force: bool,
    speed: Option<String>,
    list_speeds: bool,
    reasoning_effort: Option<String>,
    runtime_mode: Option<String>,
    json: bool,
) {
    if list_speeds {
        let response = local_http
            .get(format!("{base}/api/tabs/{tab_id}/agent/service-tiers"))
            .send()
            .unwrap_or_else(|error| crate::cmd::print::fail(1, error.to_string()));
        let status = response.status();
        let text = response.text().unwrap_or_default();
        let catalogue = match model_switch_result(status, &text) {
            Ok(value) => value,
            Err(error) => crate::cmd::print::exit_with_cli_error(json, &error),
        };
        let mut listed = catalogue.clone();
        if let Some(tiers) = listed["tiers"].as_array_mut() {
            tiers.retain(|tier| tier.get("unavailableReason").is_none());
        }
        if json {
            crate::cmd::print::print_json(&listed);
        } else {
            for tier in listed["tiers"].as_array().into_iter().flatten() {
                println!(
                    "{}\t{}",
                    tier["id"].as_str().unwrap_or("?"),
                    tier["name"].as_str().unwrap_or("")
                );
            }
        }
        return;
    }
    let selection = if let Some(speed) = speed.as_deref() {
        let response = local_http
            .get(format!("{base}/api/tabs/{tab_id}/agent/service-tiers"))
            .send()
            .unwrap_or_else(|error| crate::cmd::print::fail(1, error.to_string()));
        let status = response.status();
        let text = response.text().unwrap_or_default();
        let catalogue = model_switch_result(status, &text)
            .unwrap_or_else(|error| crate::cmd::print::exit_with_cli_error(json, &error));
        Some(
            requested_service_tier(&catalogue, speed, &tab_id)
                .unwrap_or_else(|error| crate::cmd::print::exit_with_cli_error(json, &error)),
        )
    } else {
        None
    };
    let mut current_model = None;
    if reasoning_effort.is_some() || runtime_mode.is_some() {
        let encoded_tab = urlencoding::encode(&tab_id);
        let response = local_http
            .get(format!("{base}/api/tabs/inspect?tab_ids={encoded_tab}"))
            .send()
            .unwrap_or_else(|error| crate::cmd::print::fail(1, error.to_string()));
        let status = response.status();
        let inspected: serde_json::Value = response.json().unwrap_or_default();
        if !status.is_success() {
            crate::cmd::print::fail_http(1, status.as_u16(), &inspected.to_string());
        }
        let tab_record = inspected
            .as_array()
            .and_then(|tabs| tabs.first())
            .unwrap_or_else(|| crate::cmd::print::fail(1, "tab inspect returned no tab"));
        let provider_id = tab_record
            .get("agent_type")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_else(|| crate::cmd::print::fail(1, "tab has no agent provider"));
        current_model = name
            .clone()
            .or_else(|| {
                tab_record
                    .pointer("/agent_runtime_config/model")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned)
            })
            .or_else(|| {
                tab_record
                    .get("agent_session_model")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned)
            })
            .or_else(|| {
                tab_record
                    .get("agent_provider_resolved_model")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned)
            });
        let model = current_model
            .as_deref()
            .unwrap_or_else(|| crate::cmd::print::fail(1, "tab has no current model to validate"));
        let response = local_http
            .get(format!("{base}/api/agents/providers"))
            .send()
            .unwrap_or_else(|error| crate::cmd::print::fail(1, error.to_string()));
        let status = response.status();
        let providers: serde_json::Value = response.json().unwrap_or_default();
        if !status.is_success() {
            crate::cmd::print::fail_http(1, status.as_u16(), &providers.to_string());
        }
        let provider = select_provider(&providers, provider_id)
            .unwrap_or_else(|error| crate::cmd::print::exit_with_cli_error(json, &error));
        validate_controls(
            provider,
            model,
            reasoning_effort.as_deref(),
            runtime_mode.as_deref(),
        )
        .unwrap_or_else(|error| crate::cmd::print::exit_with_cli_error(json, &error));
    }
    if name.is_none() && speed.is_none() && reasoning_effort.is_none() && runtime_mode.is_none() {
        crate::cmd::print::fail(
            2,
            "provide MODEL, --speed, --reasoning-effort, or --runtime-mode",
        );
    }
    let selected_model = name.or(current_model).unwrap_or_default();
    let mut body = serde_json::json!({
        "model": selected_model,
        "force": force,
        "reasoning_effort": reasoning_effort,
        "runtime_mode": runtime_mode,
    });
    if let Some(selection) = &selection {
        body["service_tier"] = selection.clone();
    }
    let result = local_http
        .post(format!("{base}/api/tabs/{tab_id}/agent/model"))
        .json(&body)
        .send()
        .map_err(|error| crate::cmd::print::CliError::new(error.to_string()))
        .and_then(|response| {
            let status = response.status();
            let text = response.text().unwrap_or_default();
            model_switch_result(status, &text)
        });
    match result {
        Ok(mut value) => {
            if let Some(selection) = selection {
                value["requested_service_tier"] = selection.clone();
                let readback = local_http
                    .get(format!("{base}/api/tabs/{tab_id}/agent/service-tiers"))
                    .send()
                    .ok()
                    .and_then(|response| response.json::<serde_json::Value>().ok());
                let confirmed = readback
                    .as_ref()
                    .and_then(|catalogue| catalogue.get("confirmed"));
                let matches =
                    confirmed.is_some_and(|confirmed| confirmed.get("id") == selection.get("id"));
                value["confirmed_service_tier"] =
                    confirmed.cloned().unwrap_or(serde_json::Value::Null);
                value["service_tier_outcome"] =
                    if matches { "confirmed" } else { "pending" }.into();
            }
            if json {
                crate::cmd::print::print_json(&value);
            } else {
                println!("{}", render_model_switch(&value));
            }
        }
        Err(error) => crate::cmd::print::exit_with_cli_error(json, &error),
    }
}

pub(super) fn validate_controls(
    provider: &serde_json::Value,
    model: &str,
    effort: Option<&str>,
    runtime_mode: Option<&str>,
) -> Result<(), CliError> {
    if let Some(requested) = effort {
        let model_levels = provider
            .get("available_models")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .find(|entry| {
                entry
                    .get("id")
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|id| id.eq_ignore_ascii_case(model))
            })
            .and_then(|entry| entry.get("reasoning_efforts"))
            .and_then(serde_json::Value::as_array)
            .filter(|levels| !levels.is_empty());
        let provider_levels = provider
            .get("reasoning_efforts")
            .and_then(serde_json::Value::as_array);
        let supported = model_levels.or(provider_levels);
        let ids = supported
            .into_iter()
            .flatten()
            .filter_map(|level| level.get("id").and_then(serde_json::Value::as_str))
            .collect::<Vec<_>>();
        if !ids.iter().any(|level| *level == requested) {
            return Err(CliError {
                message: format!(
                    "Model '{model}' does not support reasoning effort '{requested}'; supported: {}",
                    if ids.is_empty() { "none".into() } else { ids.join(", ") }
                ),
                code: None,
                data: Some(serde_json::json!({
                    "reason": "unsupported_reasoning_effort",
                    "model": model,
                    "requested": requested,
                    "supported": ids,
                })),
            });
        }
    }
    if let Some(requested) = runtime_mode {
        let ids = provider
            .get("agent_runtime")
            .and_then(|control| control.get("runtimeModes"))
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|mode| mode.get("id").and_then(serde_json::Value::as_str))
            .collect::<Vec<_>>();
        if !ids.iter().any(|mode| *mode == requested) {
            return Err(CliError {
                message: format!(
                    "Unsupported runtime mode '{requested}'; supported: {}",
                    if ids.is_empty() {
                        "none".into()
                    } else {
                        ids.join(", ")
                    }
                ),
                code: None,
                data: Some(serde_json::json!({
                    "reason": "unsupported_runtime_mode",
                    "requested": requested,
                    "supported": ids,
                })),
            });
        }
    }
    Ok(())
}

pub(super) fn requested_service_tier(
    catalogue: &serde_json::Value,
    requested: &str,
    tab_id: &str,
) -> Result<serde_json::Value, CliError> {
    if requested.eq_ignore_ascii_case("default") {
        return Ok(serde_json::json!({"id": null}));
    }
    catalogue["tiers"].as_array().into_iter().flatten()
        .find(|tier| tier["id"].as_str() == Some(requested) && tier.get("unavailableReason").is_none())
        .map(|tier| serde_json::json!({"id": tier["id"]}))
        .ok_or_else(|| CliError { message: format!("Unsupported or unavailable service tier: {requested}"), code: None,
            data: Some(serde_json::json!({"reason":"unsupported_service_tier","tier":requested,"tab_id":tab_id})) })
}

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
    fn service_tier_selection_uses_only_available_catalogue_entries() {
        let catalogue = serde_json::json!({"tiers":[
            {"id":"opaque-fast","name":"Fast"},
            {"id":"opaque-ultra","name":"Ultrafast","unavailableReason":"plan"}
        ]});
        assert_eq!(
            requested_service_tier(&catalogue, "opaque-fast", "tab-a").unwrap(),
            serde_json::json!({"id":"opaque-fast"})
        );
        assert_eq!(
            requested_service_tier(&catalogue, "default", "tab-a").unwrap(),
            serde_json::json!({"id":null})
        );
        let error = requested_service_tier(&catalogue, "opaque-ultra", "tab-a").unwrap_err();
        assert_eq!(error.data.unwrap()["reason"], "unsupported_service_tier");
        assert_eq!(
            requested_service_tier(&serde_json::json!({"tiers":[]}), "opaque-fast", "tab-b")
                .unwrap_err()
                .data
                .unwrap()["tab_id"],
            "tab-b"
        );
    }

    #[test]
    fn effort_and_runtime_mode_validation_uses_model_and_provider_catalogues() {
        let provider = serde_json::json!({
            "reasoning_efforts":[{"id":"low"},{"id":"medium"}],
            "available_models":[
                {"id":"model-a","reasoning_efforts":[{"id":"high"}]},
                {"id":"model-b","reasoning_efforts":[]}
            ],
            "agent_runtime":{"runtimeModes":[{"id":"auto"},{"id":"full-access"}]}
        });
        assert!(validate_controls(&provider, "model-a", Some("high"), Some("auto")).is_ok());
        assert!(validate_controls(&provider, "model-b", Some("medium"), None).is_ok());
        let effort_error =
            validate_controls(&provider, "model-a", Some("medium"), None).unwrap_err();
        assert_eq!(
            effort_error.data.as_ref().unwrap()["reason"],
            "unsupported_reasoning_effort"
        );
        assert_eq!(
            effort_error.data.as_ref().unwrap()["supported"],
            serde_json::json!(["high"])
        );
        let mode_error =
            validate_controls(&provider, "model-a", None, Some("approval-required")).unwrap_err();
        assert_eq!(
            mode_error.data.as_ref().unwrap()["reason"],
            "unsupported_runtime_mode"
        );
        assert_eq!(
            mode_error.data.as_ref().unwrap()["supported"],
            serde_json::json!(["auto", "full-access"])
        );
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
        assert_eq!(value["mode_used"], "live");
        assert_eq!(value["model_confirmation"], "observed");
        assert_eq!(value["evidence"], "live_session_record");
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
