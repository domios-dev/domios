use serde_json::Value;

pub(crate) fn tab_name(tab: &Value) -> &str {
    text(tab, "title")
}

fn text<'a>(tab: &'a Value, field: &str) -> &'a str {
    tab.get(field).and_then(Value::as_str).unwrap_or("").trim()
}

#[cfg(test)]
mod tests;
