use serde_json::Value;

pub fn stored_definition(mut value: Value) -> Result<Value, String> {
    canonicalize(&mut value);
    Ok(value)
}

pub fn exposed_definition(mut value: Value) -> Value {
    canonicalize(&mut value);
    value
}

fn canonicalize(value: &mut Value) {
    match value {
        Value::Array(values) => values.iter_mut().for_each(canonicalize),
        Value::Object(fields) => {
            for (old, new) in [
                ("sentinel_id", "blueprint_id"),
                ("sentinel_definition", "blueprint_definition"),
            ] {
                if let Some(value) = fields.remove(old) {
                    fields.entry(new).or_insert(value);
                }
            }
            for key in ["kind", "type"] {
                if let Some(Value::String(kind)) = fields.get_mut(key) {
                    if kind == "sentinel" {
                        *kind = "blueprint".into();
                    }
                }
            }
            if let Some(Value::Object(routes)) = fields.get_mut("routes") {
                for route in routes.values_mut() {
                    if let Value::String(action) = route {
                        if let Some(target) = action.strip_prefix("sentinel:") {
                            *action = format!("blueprint:{target}");
                        }
                    }
                }
            }
            for (key, value) in fields {
                if !matches!(key.as_str(), "parameters" | "data" | "input" | "result") {
                    canonicalize(value);
                }
            }
        }
        _ => {}
    }
}
