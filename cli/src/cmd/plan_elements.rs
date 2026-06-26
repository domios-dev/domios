use serde_json::Value;

pub(crate) fn render_element_list(val: &Value) -> String {
    let Some(elements) = val.get("elements").and_then(|v| v.as_array()) else {
        return "(no elements)\n".into();
    };
    if elements.is_empty() {
        return "(no elements)\n".into();
    }
    let mut out = String::new();
    for element in elements {
        let id = element.get("id").and_then(|v| v.as_str()).unwrap_or("?");
        let kind = element.get("kind").and_then(|v| v.as_str()).unwrap_or("?");
        let title = element.get("title").and_then(|v| v.as_str()).unwrap_or("?");
        let links = element
            .get("links")
            .and_then(|v| v.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.as_str())
                    .collect::<Vec<_>>()
                    .join(",")
            })
            .unwrap_or_default();
        out.push_str(&format!("{id}\t{kind}\t{title}\t{links}\n"));
    }
    out
}

pub(crate) fn render_element_show(val: &Value) -> String {
    let element = val.get("element").unwrap_or(&Value::Null);
    let id = element.get("id").and_then(|v| v.as_str()).unwrap_or("?");
    let kind = element.get("kind").and_then(|v| v.as_str()).unwrap_or("?");
    let title = val.get("title").and_then(|v| v.as_str()).unwrap_or(id);
    let links = element
        .get("links")
        .and_then(|v| v.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default();
    let markdown = val
        .get("markdown")
        .and_then(|v| v.as_str())
        .unwrap_or_default();

    let mut out = String::new();
    out.push_str(&format!("# {title}\n\n"));
    out.push_str(&format!("id: {id}\n"));
    out.push_str(&format!("kind: {kind}\n"));
    if !links.is_empty() {
        out.push_str(&format!("links: {links}\n"));
    }
    if !markdown.trim().is_empty() {
        out.push('\n');
        out.push_str(markdown.trim_end());
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn renders_element_list() {
        let rendered = render_element_list(&json!({
            "elements": [{
                "id": "el_1",
                "kind": "github.issue",
                "title": "Fix crash",
                "links": ["task_1"]
            }]
        }));

        assert_eq!(rendered, "el_1\tgithub.issue\tFix crash\ttask_1\n");
    }

    #[test]
    fn renders_element_show() {
        let rendered = render_element_show(&json!({
            "title": "Decision",
            "markdown": "Use the generic element path.",
            "element": {
                "id": "el_1",
                "kind": "decision",
                "links": ["task_1"]
            }
        }));

        assert!(rendered.contains("# Decision"));
        assert!(rendered.contains("id: el_1"));
        assert!(rendered.contains("links: task_1"));
        assert!(rendered.contains("Use the generic element path."));
    }
}
