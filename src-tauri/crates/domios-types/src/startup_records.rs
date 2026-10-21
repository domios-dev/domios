pub fn normalize_pane_id_fields(value: &mut serde_json::Value) -> Result<usize, String> {
    match value {
        serde_json::Value::Array(items) => {
            let mut normalized = 0;
            for item in items {
                normalized += normalize_pane_id_fields(item)?;
            }
            Ok(normalized)
        }
        serde_json::Value::Object(object) => {
            let legacy_fields: Vec<String> = object
                .keys()
                .filter(|key| *key == "pane_id" || key.ends_with("_pane_id"))
                .cloned()
                .collect();
            let mut normalized = 0;
            for legacy_field in legacy_fields {
                let current_field = if legacy_field == "pane_id" {
                    "tab_id".to_string()
                } else {
                    format!("{}_tab_id", legacy_field.trim_end_matches("_pane_id"))
                };
                let Some(legacy_value) = object.get(&legacy_field).cloned() else {
                    continue;
                };
                if let Some(current_value) = object.get(&current_field) {
                    if current_value != &legacy_value {
                        return Err(format!(
                            "ambiguous fields {} and {}",
                            legacy_field, current_field
                        ));
                    }
                } else {
                    object.insert(current_field, legacy_value);
                }
                object.remove(&legacy_field);
                normalized += 1;
            }
            for child in object.values_mut() {
                normalized += normalize_pane_id_fields(child)?;
            }
            Ok(normalized)
        }
        _ => Ok(0),
    }
}
