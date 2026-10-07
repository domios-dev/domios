use serde_json::Value;

#[derive(Debug, PartialEq, Eq)]
pub enum Selection {
    Current,
    Verify(String),
    Ignore,
}

pub fn selection_for_event(bound_thread: &str, params: &Value) -> Selection {
    match params.get("threadId").and_then(Value::as_str) {
        Some(id) if id == bound_thread => Selection::Current,
        Some(id) if !id.is_empty() => Selection::Verify(id.to_owned()),
        _ => Selection::Ignore,
    }
}

pub fn verified_user_thread(candidate: &str, response: &Value) -> bool {
    let Some(thread) = response.get("thread") else {
        return false;
    };
    if thread.get("id").and_then(Value::as_str) != Some(candidate) {
        return false;
    }
    if thread
        .get("parentThreadId")
        .is_some_and(|parent| !parent.is_null())
    {
        return false;
    }
    if thread
        .get("source")
        .and_then(Value::as_object)
        .is_some_and(|source| source.contains_key("subAgent"))
    {
        return false;
    }
    matches!(
        thread.get("threadSource").and_then(Value::as_str),
        Some("user")
    )
}

#[cfg(test)]
mod tests;
