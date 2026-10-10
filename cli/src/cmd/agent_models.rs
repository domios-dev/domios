use crate::cmd::print::CliError;
use serde_json::Value;

pub(super) fn select_provider<'a>(
    descriptors: &'a Value,
    provider: &str,
) -> Result<&'a Value, CliError> {
    let providers = descriptors
        .as_array()
        .ok_or_else(|| CliError::new("Unexpected response shape"))?;
    providers
        .iter()
        .find(|entry| entry.get("id").and_then(Value::as_str) == Some(provider))
        .ok_or_else(|| {
            let known = providers
                .iter()
                .filter_map(|entry| entry.get("id").and_then(Value::as_str))
                .collect::<Vec<_>>();
            CliError {
                message: format!(
                    "Unknown provider: '{}'. Known: [{}]",
                    provider,
                    known.join(", ")
                ),
                code: None,
                data: Some(serde_json::json!({"code": "not_found"})),
            }
        })
}

pub(crate) fn render_agent_models(
    descriptors: &Value,
    provider: &str,
    json: bool,
) -> Result<String, CliError> {
    let target = select_provider(descriptors, provider)?;
    Ok(if json {
        format!("{}\n", format_agent_models_json(target))
    } else {
        format_agent_models(target, provider)
    })
}

pub(crate) fn render_agent_providers(descriptors: &Value, json: bool) -> Result<String, CliError> {
    if json {
        return Ok(format!(
            "{}\n",
            serde_json::to_string_pretty(descriptors).unwrap()
        ));
    }
    let providers = descriptors
        .as_array()
        .ok_or_else(|| CliError::new("Unexpected response shape"))?;
    if providers.is_empty() {
        return Ok("No agent providers registered.\n".to_string());
    }
    let mut output = String::new();
    for p in providers {
        let kind = p.get("transport").and_then(|v| v.as_str()).unwrap_or("pty");
        let id = p.get("id").and_then(|v| v.as_str()).unwrap_or("?");
        let name = p.get("display_name").and_then(|v| v.as_str()).unwrap_or(id);
        let default_model = p
            .get("default_model")
            .and_then(|v| v.as_str())
            .unwrap_or("-");
        let models_count = p
            .get("available_models")
            .and_then(|v| v.as_array())
            .map(|a| a.len())
            .unwrap_or(0);
        output.push_str(&format!(
            "{:<12} {:<6} {:<20} default={}  models={}{}\n",
            id,
            kind,
            name,
            default_model,
            models_count,
            provider_availability_marker(p)
        ));
        if let Some(line) = super::agent_compatibility::status_line(p) {
            output.push_str(&format!("  {line}\n"));
        }
        if let Some(status) = p.get("cli_status") {
            if status.get("installed").and_then(|v| v.as_bool()) != Some(true) {
                for line in crate::cmd::plugin::cli_status::status_lines(id, status) {
                    output.push_str(&format!("  {line}\n"));
                }
            }
        }
    }
    Ok(output)
}

pub(super) const AVAILABILITY_UNAVAILABLE: &str = " (unavailable)";
pub(super) const AVAILABILITY_UNKNOWN: &str = " (availability unknown)";

pub(super) fn provider_availability_marker(provider: &serde_json::Value) -> &'static str {
    match provider.get("availability").and_then(|v| v.as_str()) {
        Some("available") => "",
        Some("unavailable") => AVAILABILITY_UNAVAILABLE,
        _ => AVAILABILITY_UNKNOWN,
    }
}

pub(super) fn format_agent_models_json(target: &serde_json::Value) -> String {
    serde_json::to_string_pretty(
        target
            .get("available_models")
            .unwrap_or(&serde_json::Value::Null),
    )
    .unwrap()
}

fn availability_marker(model: &serde_json::Value) -> &'static str {
    match model.get("availability").and_then(|v| v.as_str()) {
        Some("unavailable") => AVAILABILITY_UNAVAILABLE,
        Some("unknown") => AVAILABILITY_UNKNOWN,
        Some(_) => "",
        None if model.get("available").and_then(|v| v.as_bool()) == Some(false) => {
            AVAILABILITY_UNAVAILABLE
        }
        None => "",
    }
}

pub(super) fn format_agent_models(target: &serde_json::Value, provider: &str) -> String {
    let models = target.get("available_models").and_then(|v| v.as_array());
    let Some(models) = models else {
        return format!("No models advertised for provider '{}'.\n", provider);
    };
    let default_model = target.get("default_model").and_then(|v| v.as_str());
    let mut out = String::new();
    for model in models {
        let id = model.get("id").and_then(|v| v.as_str()).unwrap_or("?");
        let name = model
            .get("display_name")
            .and_then(|v| v.as_str())
            .unwrap_or(id);
        let marker = if default_model == Some(id) {
            " (default)"
        } else {
            ""
        };
        let description = model
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let marker = format!("{marker}{}", availability_marker(model));
        if description.is_empty() {
            out.push_str(&format!("{:<10} {}{}\n", id, name, marker));
        } else {
            out.push_str(&format!(
                "{:<10} {}{}\n             {}\n",
                id, name, marker, description
            ));
        }
        if let Some(efforts) = format_model_reasoning_efforts(
            model,
            model
                .get("default_reasoning_effort")
                .and_then(|value| value.as_str())
                .or_else(|| default_reasoning_effort(target)),
        ) {
            out.push_str(&efforts);
        }
    }
    out.push_str(&format_reasoning_efforts(target));
    out
}

fn default_reasoning_effort(target: &serde_json::Value) -> Option<&str> {
    target
        .get("default_reasoning_effort")
        .and_then(|value| value.as_str())
}

fn format_model_reasoning_efforts(
    model: &serde_json::Value,
    default: Option<&str>,
) -> Option<String> {
    let efforts = model.get("reasoning_efforts")?.as_array()?;
    let levels = efforts
        .iter()
        .map(|effort| {
            let id = effort
                .get("id")
                .and_then(|value| value.as_str())
                .unwrap_or("?");
            if default == Some(id) {
                format!("{id} (default)")
            } else {
                id.to_string()
            }
        })
        .collect::<Vec<_>>();
    let levels = if levels.is_empty() {
        "none".to_string()
    } else {
        levels.join(", ")
    };
    Some(format!("             Reasoning effort: {levels}\n"))
}

fn format_reasoning_efforts(target: &serde_json::Value) -> String {
    let efforts = target
        .get("reasoning_efforts")
        .and_then(|v| v.as_array())
        .filter(|a| !a.is_empty());
    let Some(efforts) = efforts else {
        return String::new();
    };
    let default = default_reasoning_effort(target);
    let mut out = String::from("\nProvider fallback reasoning effort:\n");
    for effort in efforts {
        let id = effort.get("id").and_then(|v| v.as_str()).unwrap_or("?");
        let name = effort
            .get("display_name")
            .and_then(|v| v.as_str())
            .unwrap_or(id);
        let marker = if default == Some(id) {
            " (default)"
        } else {
            ""
        };
        out.push_str(&format!("{:<10} {}{}\n", id, name, marker));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn descriptors() -> Value {
        serde_json::json!([
            {"id": "other", "available_models": [{"id": "other-model"}]},
            {
                "id": "requested", "default_model": "model-b",
                "available_models": [
                    {"id": "model-a", "display_name": "Model A", "availability": "unknown"},
                    {"id": "model-b", "display_name": "Model B", "description": "Details",
                     "reasoning_efforts": [{"id": "high"}], "default_reasoning_effort": "high"}
                ]
            }
        ])
    }

    #[test]
    fn shared_models_renderer_selects_only_requested_provider_rows() {
        let descriptors = descriptors();
        let output = render_agent_models(&descriptors, "requested", true).unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&output).unwrap(),
            descriptors[1]["available_models"]
        );
        assert_eq!(
            output,
            format!("{}\n", format_agent_models_json(&descriptors[1]))
        );
        assert_eq!(
            render_agent_models(&descriptors, "requested", false).unwrap(),
            format_agent_models(&descriptors[1], "requested")
        );
    }

    #[test]
    fn shared_models_renderer_returns_typed_unknown_provider() {
        let descriptors = descriptors();
        for json in [false, true] {
            let error = render_agent_models(&descriptors, "missing", json).unwrap_err();
            assert_eq!(error, select_provider(&descriptors, "missing").unwrap_err());
            assert_eq!(error.data, Some(serde_json::json!({"code": "not_found"})));
            let mut stdout = Vec::new();
            let mut stderr = Vec::new();
            crate::cmd::print::write_cli_error(json, &error, &mut stdout, &mut stderr);
            if json {
                let envelope: Value = serde_json::from_slice(&stdout).unwrap();
                assert_eq!(envelope["ok"], false);
                assert_eq!(envelope["error"]["code"], "not_found");
                assert!(stderr.is_empty());
            } else {
                assert!(stdout.is_empty());
                assert!(String::from_utf8(stderr).unwrap().contains(&error.message));
            }
            assert_eq!(
                error.message,
                "Unknown provider: 'missing'. Known: [other, requested]"
            );
        }
    }

    #[test]
    fn shared_providers_renderer_preserves_descriptor_array_and_human_summary() {
        let descriptors = descriptors();
        assert_eq!(
            serde_json::from_str::<Value>(&render_agent_providers(&descriptors, true).unwrap())
                .unwrap(),
            descriptors
        );
        let output = render_agent_providers(&descriptors, false).unwrap();
        assert!(output.contains("default=model-b  models=2"));
        assert!(output.contains("pty"));
        assert_eq!(
            render_agent_providers(&serde_json::json!([]), false).unwrap(),
            "No agent providers registered.\n"
        );
    }
}
