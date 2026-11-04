use serde_json::Value;

use codeterm_service_core::plan::OrchestratorRef;

pub(super) fn csv_string_field(params: Option<&Value>, key: &str) -> Vec<String> {
    params
        .and_then(|x| x.get(key))
        .and_then(|v| v.as_str())
        .map(|s| {
            s.split(',')
                .map(str::trim)
                .filter(|x| !x.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

pub(super) fn owner_from_params(p: &Value) -> OrchestratorRef {
    p.get("owner")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .or_else(|| {
            p.get("tab_id")
                .and_then(|v| v.as_str())
                .map(OrchestratorRef::from_tab)
        })
        .or_else(|| {
            p.get("caller_tab_id")
                .and_then(|v| v.as_str())
                .map(OrchestratorRef::from_tab)
        })
        .unwrap_or_else(|| OrchestratorRef::from_tab("unknown"))
}

pub(super) fn tags_field(params: Option<&Value>) -> Vec<String> {
    params
        .and_then(|x| x.get("tags"))
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
}
