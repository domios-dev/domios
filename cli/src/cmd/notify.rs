use codeterm_types::notification::{answer_actions, validate_title, CallerAction, NotifyLevel};

/// `notification.create` params. The body has exactly one source — the calling
/// tab's pad — so a shell can never mangle a question before the user reads it.
/// The surface a notification offers to turn on, and the choice that accepts
/// it. Both or neither: an offer with nothing to accept it would never apply,
/// and an accepting label with no surface would apply nothing.
#[derive(Debug, Clone, Copy, Default)]
pub struct Offer<'a> {
    pub surface: Option<&'a str>,
    pub accept: Option<&'a str>,
}

#[allow(clippy::too_many_arguments)]
pub fn send_params(
    title: &str,
    severity: &str,
    level: Option<NotifyLevel>,
    event_key: Option<&str>,
    from_pad: Option<&str>,
    choices: &[String],
    offer: Offer<'_>,
) -> serde_json::Value {
    let body = from_pad.map(|path| {
        crate::cmd::payload_source::read_from_pad(path).unwrap_or_else(|e| {
            eprintln!("Error: {e}");
            std::process::exit(1);
        })
    });
    params_from_body(
        title,
        severity,
        level,
        event_key,
        body,
        choices,
        offer,
        crate::nonempty_env("CODETERM_TAB_ID").as_deref(),
    )
    .unwrap_or_else(|e| {
        eprintln!("Error: {e}");
        std::process::exit(1);
    })
}

/// The offered choices, or nothing when none were named. A label is the whole
/// payload of a choice, so an unusable set is refused before it is sent.
pub fn choice_actions(choices: &[String]) -> Result<Option<Vec<CallerAction>>, String> {
    if choices.is_empty() {
        return Ok(None);
    }
    answer_actions(choices).map(Some)
}

#[allow(clippy::too_many_arguments)]
fn params_from_body(
    title: &str,
    severity: &str,
    level: Option<NotifyLevel>,
    event_key: Option<&str>,
    body: Option<String>,
    choices: &[String],
    offer: Offer<'_>,
    caller_tab_id: Option<&str>,
) -> Result<serde_json::Value, String> {
    let title = validate_title(title)?;
    let mut params = serde_json::json!({ "title": title, "severity": severity });
    if let Some(body) = body {
        params["body"] = serde_json::Value::String(body);
    }
    if let Some(level) = level {
        params["level"] = serde_json::Value::String(level.as_str().to_string());
    }
    if let Some(event_key) = event_key {
        params["event_key"] = serde_json::Value::String(event_key.to_string());
    }
    // Every record needs its asking tab: it answers questions and places the record in its team.
    if let Some(tab_id) = caller_tab_id {
        params["caller_tab_id"] = serde_json::Value::String(tab_id.to_string());
    }
    if let Some(actions) = choice_actions(choices)? {
        params["actions"] = serde_json::to_value(actions)
            .map_err(|e| format!("choices are not serializable: {e}"))?;
    }
    if let Some(surface) = offer.surface {
        params["offer"] = serde_json::Value::String(surface.to_string());
    }
    if let Some(accept) = offer.accept {
        params["offer_accept"] = serde_json::Value::String(accept.to_string());
    }
    Ok(params)
}

/// A named jump target must name something; whether that tab exists is the
/// daemon's fact, checked there.
pub fn vet_target_tab(target_tab: Option<&str>) -> Result<(), String> {
    match target_tab {
        Some(tab) if tab.trim().is_empty() => Err("--target-tab must name a tab".to_string()),
        _ => Ok(()),
    }
}

/// The chat message this notification jumps to. Both ids travel as navigation
/// hints; the daemon never treats them as provenance.
pub fn with_target(
    mut params: serde_json::Value,
    target_tab: Option<&str>,
    chat_message: Option<&str>,
) -> serde_json::Value {
    if let Err(e) = vet_target_tab(target_tab) {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
    if let Some(tab) = target_tab {
        params["target_tab_id"] = serde_json::Value::String(tab.to_string());
        if let Some(message) = chat_message {
            params["chat_message_id"] = serde_json::Value::String(message.to_string());
        }
    }
    params
}

/// One surface that will not show the notification, as it travels on the
/// `notification.create` result.
struct ClosedSurface {
    surface: String,
    reason: String,
}

/// The daemon's answer to "will anyone see this", read off the result it
/// already returns. `None` is the ordinary case: something will show it, so
/// there is nothing to say. The CLI reads no config of its own — whether a
/// surface is open is the daemon's fact, never a guess made out here.
fn unseen_surfaces(result: &serde_json::Value) -> Option<Vec<ClosedSurface>> {
    let closed = result.get("unseen")?.get("closed")?.as_array()?;
    let surfaces: Vec<ClosedSurface> = closed
        .iter()
        .filter_map(|entry| {
            Some(ClosedSurface {
                surface: entry.get("surface")?.as_str()?.to_string(),
                reason: entry.get("reason")?.as_str()?.to_string(),
            })
        })
        .collect();
    (!surfaces.is_empty()).then_some(surfaces)
}

/// What the caller reads under the id. Absent on the ordinary case — a line
/// that always prints is worth exactly as much as no line at all.
pub fn unseen_notice(result: &serde_json::Value) -> Option<String> {
    let surfaces = unseen_surfaces(result)?;
    let width = surfaces
        .iter()
        .map(|closed| closed.surface.len())
        .max()
        .unwrap_or(0);
    let mut notice = String::from("Recorded, but no surface will show it:");
    for closed in &surfaces {
        notice.push_str(&format!(
            "\n  {:width$}  {}",
            closed.surface,
            closed.reason,
            width = width
        ));
    }
    Some(notice)
}

/// The result minus the report, so the human rendering stays the id it has
/// always been and the report gets its own prose underneath.
fn without_unseen(result: &serde_json::Value) -> serde_json::Value {
    let mut value = result.clone();
    if let Some(object) = value.as_object_mut() {
        object.remove("unseen");
    }
    value
}

/// `notify send`. The record was accepted either way: the id still prints and
/// the exit stays 0. The notice is information printed beside it, never a
/// failure — a caller that treated it as one would start retrying.
pub fn handle_send(
    client: &mut impl crate::client::RpcClient,
    json: bool,
    params: serde_json::Value,
) {
    let result = client.call("notification.create", Some(params));
    // `--json` hands the caller the whole result, report included, so a machine
    // reader sees the same fact the prose does.
    if json {
        crate::cmd::print::handle_result(json, result);
        return;
    }
    match result {
        Ok(value) => {
            crate::cmd::print::print_human(&without_unseen(&value));
            if let Some(notice) = unseen_notice(&value) {
                println!("{notice}");
            }
        }
        Err(error) => crate::cmd::print::handle_result(json, Err(error)),
    }
}

#[cfg(test)]
mod tests;
