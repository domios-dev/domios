use serde_json::Value;

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
