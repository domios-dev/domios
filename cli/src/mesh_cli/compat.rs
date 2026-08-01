#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MeshApiDialect {
    Current,
    PaneEra,
}

pub(crate) fn dialect_from_identify(value: &serde_json::Value) -> MeshApiDialect {
    let Some(version) = value.get("version").and_then(|value| value.as_str()) else {
        return MeshApiDialect::Current;
    };
    let mut parts = version
        .split_once('-')
        .map(|(base, _)| base)
        .unwrap_or(version)
        .split('.')
        .filter_map(|part| part.parse::<u64>().ok());
    let parsed = (
        parts.next().unwrap_or(u64::MAX),
        parts.next().unwrap_or(u64::MAX),
        parts.next().unwrap_or(u64::MAX),
    );
    if parsed < (1, 7, 8) {
        MeshApiDialect::PaneEra
    } else {
        MeshApiDialect::Current
    }
}

pub(crate) fn request_for_dialect(
    dialect: MeshApiDialect,
    path: &str,
    body: Option<&str>,
) -> (String, Option<String>) {
    if dialect == MeshApiDialect::Current {
        return (path.to_string(), body.map(str::to_string));
    }
    let path = path
        .strip_prefix("/api/tabs")
        .map(|suffix| format!("/api/panes{suffix}"))
        .unwrap_or_else(|| path.to_string())
        .replace("tab_ids=", "pane_ids=");
    let body = body.map(|raw| {
        serde_json::from_str::<serde_json::Value>(raw)
            .map(|value| translate_value(value, Direction::ToPaneEra).to_string())
            .unwrap_or_else(|_| raw.to_string())
    });
    (path, body)
}

pub(crate) fn response_for_dialect(
    dialect: MeshApiDialect,
    value: serde_json::Value,
) -> serde_json::Value {
    match dialect {
        MeshApiDialect::Current => value,
        MeshApiDialect::PaneEra => translate_value(value, Direction::FromPaneEra),
    }
}

#[derive(Clone, Copy)]
enum Direction {
    ToPaneEra,
    FromPaneEra,
}

fn translate_value(value: serde_json::Value, direction: Direction) -> serde_json::Value {
    match value {
        serde_json::Value::Array(values) => serde_json::Value::Array(
            values
                .into_iter()
                .map(|value| translate_value(value, direction))
                .collect(),
        ),
        serde_json::Value::Object(values) => {
            let mut translated = serde_json::Map::new();
            for (key, value) in &values {
                if shadowed_legacy_alias(&values, key, direction) {
                    continue;
                }
                translated.insert(
                    translate_key(key, direction),
                    translate_value(value.clone(), direction),
                );
            }
            serde_json::Value::Object(translated)
        }
        value => value,
    }
}

fn shadowed_legacy_alias(
    values: &serde_json::Map<String, serde_json::Value>,
    key: &str,
    direction: Direction,
) -> bool {
    match direction {
        Direction::FromPaneEra => {
            let pane_key = translate_key(key, Direction::ToPaneEra);
            pane_key != key && values.contains_key(&pane_key)
        }
        Direction::ToPaneEra => {
            let tab_key = translate_key(key, Direction::FromPaneEra);
            tab_key != key && values.contains_key(&tab_key)
        }
    }
}

fn translate_key(key: &str, direction: Direction) -> String {
    match direction {
        Direction::ToPaneEra => key
            .replace("tab_id", "pane_id")
            .strip_suffix("_tab")
            .map(|prefix| format!("{prefix}_pane"))
            .unwrap_or_else(|| key.replace("tab_id", "pane_id")),
        Direction::FromPaneEra => key
            .replace("pane_id", "tab_id")
            .strip_suffix("_pane")
            .map(|prefix| format!("{prefix}_tab"))
            .unwrap_or_else(|| key.replace("pane_id", "tab_id")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn identifies_the_pane_era_without_downgrading_current_peers() {
        assert_eq!(
            dialect_from_identify(&json!({"version": "1.7.7"})),
            MeshApiDialect::PaneEra
        );
        assert_eq!(
            dialect_from_identify(&json!({"version": "1.7.8"})),
            MeshApiDialect::Current
        );
        assert_eq!(
            dialect_from_identify(&json!({"version": "2.0.0-beta.1"})),
            MeshApiDialect::Current
        );
        assert_eq!(dialect_from_identify(&json!({})), MeshApiDialect::Current);
    }

    #[test]
    fn pane_era_request_translates_paths_queries_and_identity_fields() {
        let (path, body) = request_for_dialect(
            MeshApiDialect::PaneEra,
            "/api/tabs/inspect?tab_ids=one,two",
            Some(
                &json!({
                    "parent_tab_id": "parent",
                    "source_tab": "source",
                    "nested": {"creator_tab_id": "creator"},
                    "tab_title": "keep"
                })
                .to_string(),
            ),
        );
        assert_eq!(path, "/api/panes/inspect?pane_ids=one,two");
        let body: serde_json::Value = serde_json::from_str(body.as_deref().unwrap()).unwrap();
        assert_eq!(body["parent_pane_id"], "parent");
        assert_eq!(body["source_pane"], "source");
        assert_eq!(body["nested"]["creator_pane_id"], "creator");
        assert_eq!(body["tab_title"], "keep");
    }

    #[test]
    fn pane_era_response_normalizes_nested_ids_for_current_consumers() {
        let response = response_for_dialect(
            MeshApiDialect::PaneEra,
            json!({
                "pane_id": "worker",
                "tab_id": "obsolete-internal-tab",
                "parent_pane_id": "parent",
                "children": [{"worker_pane_id": "child"}],
                "remote_pane": "remote",
                "tab_title": "keep"
            }),
        );
        assert_eq!(response["tab_id"], "worker");
        assert_ne!(response["tab_id"], "obsolete-internal-tab");
        assert_eq!(response["parent_tab_id"], "parent");
        assert_eq!(response["children"][0]["worker_tab_id"], "child");
        assert_eq!(response["remote_tab"], "remote");
        assert_eq!(response["tab_title"], "keep");
    }
}
