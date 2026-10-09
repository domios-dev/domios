use serde_json::Value;

pub fn stored_definition(mut value: Value) -> Result<Value, String> {
    translate_definition(&mut value, true)?;
    Ok(value)
}

pub fn exposed_definition(mut value: Value) -> Value {
    let _ = translate_definition(&mut value, false);
    value
}

fn translate_definition(value: &mut Value, incoming: bool) -> Result<(), String> {
    match value {
        Value::Array(values) => {
            for value in values {
                translate_definition(value, incoming)?;
            }
        }
        Value::Object(fields) => {
            if let Some(Value::Object(action)) = fields.get_mut("action") {
                if let Some(Value::String(kind)) = action.get_mut("kind") {
                    if incoming && kind == "sentinel" {
                        return Err("action kind must be blueprint".into());
                    }
                    if kind == if incoming { "blueprint" } else { "sentinel" } {
                        *kind = if incoming { "sentinel" } else { "blueprint" }.into();
                    }
                }
            }
            if fields.contains_key("parameters") {
                if let Some(Value::String(kind)) = fields.get_mut("type") {
                    if incoming && kind == "sentinel" {
                        return Err("handler type must be blueprint".into());
                    }
                    if kind == if incoming { "blueprint" } else { "sentinel" } {
                        *kind = if incoming { "sentinel" } else { "blueprint" }.into();
                    }
                }
            }
            if let Some(Value::Object(routes)) = fields.get_mut("routes") {
                for route in routes.values_mut() {
                    if let Value::String(action) = route {
                        if incoming && action.starts_with("sentinel:") {
                            return Err("route must use blueprint:<exact-id>".into());
                        }
                        let prefix = if incoming { "blueprint:" } else { "sentinel:" };
                        if let Some(target) = action.strip_prefix(prefix) {
                            *action = format!(
                                "{}:{target}",
                                if incoming { "sentinel" } else { "blueprint" }
                            );
                        }
                    }
                }
            }
            for (key, value) in fields {
                if !matches!(
                    key.as_str(),
                    "parameters" | "data" | "input" | "result" | "action"
                ) {
                    translate_definition(value, incoming)?;
                }
            }
        }
        _ => {}
    }
    Ok(())
}
