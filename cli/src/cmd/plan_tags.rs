use serde_json::Value;

pub fn render_tag_report(val: &Value) -> String {
    let mut out = String::new();
    if let Some(facets) = val.get("facets").and_then(|v| v.as_array()) {
        if !facets.is_empty() {
            out.push_str("Facets:\n");
            for facet in facets {
                let key = facet.get("key").and_then(|v| v.as_str()).unwrap_or("?");
                let label = facet
                    .get("label")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                    .unwrap_or(key);
                out.push_str(&format!("  {label} ({key})\n"));
                if let Some(values) = facet.get("values").and_then(|v| v.as_array()) {
                    for value in values {
                        let v = value.get("value").and_then(|x| x.as_str()).unwrap_or("?");
                        let tag = format!("{key}:{v}");
                        let display = value
                            .get("label")
                            .and_then(|x| x.as_str())
                            .filter(|s| !s.is_empty())
                            .unwrap_or(v);
                        let sections = value.get("sections").and_then(|x| x.as_u64()).unwrap_or(0);
                        let tasks = value.get("tasks").and_then(|x| x.as_u64()).unwrap_or(0);
                        out.push_str(&format!(
                            "    {tag}\t{display}\tsections={sections} tasks={tasks}\n"
                        ));
                    }
                }
            }
        }
    }
    if let Some(topics) = val.get("topics").and_then(|v| v.as_array()) {
        if !topics.is_empty() {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str("Topics:\n");
            for topic in topics {
                let tag = topic.get("tag").and_then(|v| v.as_str()).unwrap_or("?");
                let sections = topic.get("sections").and_then(|v| v.as_u64()).unwrap_or(0);
                let tasks = topic.get("tasks").and_then(|v| v.as_u64()).unwrap_or(0);
                out.push_str(&format!("  {tag}\tsections={sections} tasks={tasks}\n"));
            }
        }
    }
    if let Some(warnings) = val.get("duplicate_warnings").and_then(|v| v.as_array()) {
        if !warnings.is_empty() {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str("Duplicate warnings:\n");
            for warning in warnings {
                let a = warning.get("a").and_then(|v| v.as_str()).unwrap_or("?");
                let b = warning.get("b").and_then(|v| v.as_str()).unwrap_or("?");
                out.push_str(&format!("  {a} ~ {b}\n"));
            }
        }
    }
    if out.is_empty() {
        out.push_str("(no tags)\n");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn render_tag_report_lists_facets_topics_and_warnings() {
        let rendered = render_tag_report(&json!({
            "facets": [{
                "key": "area",
                "label": "Area",
                "color_lane": true,
                "values": [{
                    "value": "nav",
                    "label": "Navigation",
                    "color": "#38bdf8",
                    "sections": 1,
                    "tasks": 2
                }]
            }],
            "topics": [{ "tag": "pan-zoom", "sections": 0, "tasks": 1 }],
            "duplicate_warnings": [{ "a": "foo", "b": "Foo" }]
        }));
        assert!(rendered.contains("Facets:"));
        assert!(rendered.contains("area:nav"));
        assert!(rendered.contains("Navigation"));
        assert!(rendered.contains("Topics:"));
        assert!(rendered.contains("pan-zoom"));
        assert!(rendered.contains("Duplicate warnings:"));
        assert!(rendered.contains("foo ~ Foo"));
    }

    #[test]
    fn render_tag_report_empty_plan() {
        assert_eq!(
            render_tag_report(&json!({
                "facets": [],
                "topics": [],
                "duplicate_warnings": []
            })),
            "(no tags)\n"
        );
    }
}
