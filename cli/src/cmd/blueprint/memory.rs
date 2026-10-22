use serde_json::Value;

pub(super) fn offers(mut stored: Value) -> Value {
    for offer in stored.as_array_mut().into_iter().flatten() {
        if let Some(id) = offer["id"].as_str() {
            offer["id"] = Value::String(format!("memory:{id}"));
        }
    }
    stored
}

pub(super) fn save_receipt(saved: &Value) -> String {
    let mut text = format!(
        "Saved {} in Blueprint memory.",
        saved["id"].as_str().unwrap_or("?")
    );
    for id in saved["evicted_ids"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
    {
        text.push_str(&format!(
            "\nArchived Blueprint memory entry {id} (area storage cap)."
        ));
    }
    text
}

