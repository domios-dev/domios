use serde_json::Value;

pub fn sanitize_history_input(value: &mut Value) {
    match value {
        Value::String(text) => {
            if text.starts_with("data:image/") && text.contains(";base64,") {
                *text = format!("[image omitted: {} bytes]", text.len());
            } else if text.len() > 16 * 1024 {
                let bytes = text.len();
                let prefix: String = text.chars().take(2048).collect();
                *text = format!("{prefix}\n[content truncated: {bytes} bytes total]");
            }
        }
        Value::Array(values) => values.iter_mut().for_each(sanitize_history_input),
        Value::Object(fields) => {
            let image = fields.get("type").and_then(Value::as_str) == Some("image");
            let generated = fields.get("type").and_then(Value::as_str) == Some("imageGeneration");
            for (key, value) in fields.iter_mut() {
                if (image && key == "data") || (generated && key == "result") {
                    if let Some(data) = value.as_str() {
                        if !data.starts_with("[image omitted:") {
                            *value =
                                Value::String(format!("[image omitted: {} bytes]", data.len()));
                        }
                    }
                } else {
                    sanitize_history_input(value);
                }
            }
        }
        _ => {}
    }
}
