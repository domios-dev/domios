use serde_json::Value;

pub(super) fn offers(mut stored: Value) -> Value {
    for offer in stored.as_array_mut().into_iter().flatten() {
        if let Some(id) = offer["id"].as_str() {
            offer["id"] = Value::String(format!("memory:{id}"));
        }
    }
    stored
}
