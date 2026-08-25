use codeterm_types::notification::{answer_actions, CallerAction, NotifyLevel};

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
    if let Some(actions) = choice_actions(choices)? {
        params["actions"] = serde_json::to_value(actions)
            .map_err(|e| format!("choices are not serializable: {e}"))?;
        // The answer goes back to the tab that asked, and the daemon records that
        // tab when the notification is created — so a question sent without it
        // renders its buttons and has nowhere to deliver the tap. Every other
        // tab-scoped CLI command already attaches this; notify did not.
        if let Some(tab_id) = caller_tab_id {
            params["caller_tab_id"] = serde_json::Value::String(tab_id.to_string());
        }
    }
    // The daemon resolves the surface against its own registry and ties the
    // accepting label to this notification's choices, so the names travel as
    // written and nothing here decides whether either is real.
    if let Some(surface) = offer.surface {
        params["offer"] = serde_json::Value::String(surface.to_string());
    }
    if let Some(accept) = offer.accept {
        params["offer_accept"] = serde_json::Value::String(accept.to_string());
    }
    Ok(params)
}

#[cfg(test)]
mod tests;
