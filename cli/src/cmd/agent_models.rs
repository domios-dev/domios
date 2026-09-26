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
            let id = effort.get("id").and_then(|value| value.as_str()).unwrap_or("?");
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
