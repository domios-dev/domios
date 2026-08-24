use crate::args::misc::NotifyCategory;

/// `notification.create` params. The body has exactly one source — the calling
/// tab's pad — so a shell can never mangle a question before the user reads it.
pub fn send_params(
    title: &str,
    level: &str,
    category: Option<NotifyCategory>,
    event_key: Option<&str>,
    from_pad: Option<&str>,
) -> serde_json::Value {
    let body = from_pad.map(|path| {
        crate::cmd::payload_source::read_from_pad(path).unwrap_or_else(|e| {
            eprintln!("Error: {e}");
            std::process::exit(1);
        })
    });
    params_from_body(title, level, category, event_key, body)
}

fn params_from_body(
    title: &str,
    level: &str,
    category: Option<NotifyCategory>,
    event_key: Option<&str>,
    body: Option<String>,
) -> serde_json::Value {
    let mut params = serde_json::json!({ "title": title, "level": level });
    if let Some(body) = body {
        params["body"] = serde_json::Value::String(body);
    }
    if let Some(category) = category {
        params["category"] = serde_json::Value::String(category.as_str().to_string());
    }
    if let Some(event_key) = event_key {
        params["event_key"] = serde_json::Value::String(event_key.to_string());
    }
    params
}

#[cfg(test)]
mod tests;
