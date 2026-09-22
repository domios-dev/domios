use serde_json::Value;

pub(crate) fn tab_name(tab: &Value) -> &str {
    let legacy = text(tab, "tab_title");
    if legacy.is_empty() {
        text(tab, "title")
    } else {
        legacy
    }
}

fn text<'a>(tab: &'a Value, field: &str) -> &'a str {
    tab.get(field).and_then(Value::as_str).unwrap_or("").trim()
}

#[cfg(test)]
mod tests {
    use super::tab_name;
    use serde_json::json;

    #[test]
    fn reads_the_authoritative_title_and_accepts_legacy_tab_title() {
        assert_eq!(tab_name(&json!({"title":"Scout"})), "Scout");
        assert_eq!(
            tab_name(&json!({"title":"provider","tab_title":"Scout"})),
            "Scout"
        );
        assert_eq!(tab_name(&json!({"tab_title":"Scout"})), "Scout");
    }
}
