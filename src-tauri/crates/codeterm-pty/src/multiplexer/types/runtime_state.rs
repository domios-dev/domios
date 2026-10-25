use serde::Serialize;
use serde_json::Value;

pub(super) fn serialize<S: serde::Serializer>(
    state: &Option<Value>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    project(state.clone(), now).serialize(serializer)
}

fn project(mut state: Option<Value>, now: u64) -> Option<Value> {
    if let Some(login) = state
        .as_mut()
        .and_then(|state| state.get_mut("login"))
        .and_then(Value::as_object_mut)
    {
        let pending = login.get("status").and_then(Value::as_str) == Some("pending");
        let expired = login
            .get("expiresAtMs")
            .and_then(Value::as_u64)
            .is_some_and(|expiry| expiry <= now);
        if !pending || expired {
            login.remove("url");
            login.remove("userCode");
            if expired && pending {
                login.insert("status".into(), Value::String("failed".into()));
                login.insert(
                    "error".into(),
                    Value::String("Sign-in expired. Cancel it and try again.".into()),
                );
            }
        }
    }
    state
}

#[cfg(test)]
mod tests;
