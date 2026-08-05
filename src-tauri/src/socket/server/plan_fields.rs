use serde_json::Value;

use crate::multiplexer::types::AgentRole;
use crate::plan::{tag_facets_from_value, MilestoneKind, TagFacet, TaskExecutionIntent, TaskGroup};
use crate::service::ServiceState;
use codeterm_types::agent::role_kind::builtins::ORCHESTRATOR;

use super::JsonRpcError;

pub(super) fn execution_intent_field(
    params: Option<&Value>,
) -> Result<TaskExecutionIntent, JsonRpcError> {
    params
        .and_then(|value| value.get("execution_intent"))
        .cloned()
        .map(serde_json::from_value)
        .transpose()
        .map(|intent| intent.unwrap_or_default())
        .map_err(|error| JsonRpcError {
            code: -32602,
            message: format!("invalid execution_intent: {error}"),
            data: Some(serde_json::json!({ "type": "invalid_execution_intent" })),
        })
}

pub(super) fn kinds_from_csv(raw: &str) -> Vec<MilestoneKind> {
    raw.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|key| MilestoneKind {
            key: key.into(),
            label: key.into(),
            color: None,
        })
        .collect()
}

pub(super) fn groups_from_csv(raw: &str) -> Vec<TaskGroup> {
    raw.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|item| {
            let (key, label) = item.split_once(':').unwrap_or((item, item));
            TaskGroup {
                key: key.trim().into(),
                label: label.trim().into(),
                color: None,
            }
        })
        .collect()
}

pub(super) fn tag_facets_field(params: Option<&Value>, key: &str) -> Result<Vec<TagFacet>, String> {
    let value = params
        .and_then(|x| x.get(key))
        .ok_or_else(|| format!("missing required field '{key}'"))?;
    tag_facets_from_value(value)
}

pub(super) fn string_array_field(params: Option<&Value>, key: &str) -> Vec<String> {
    params
        .and_then(|x| x.get(key))
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
}

pub(super) fn optional_string_field<'a>(
    params: Option<&'a Value>,
    key: &str,
) -> Option<Option<&'a str>> {
    params.and_then(|x| x.get(key).map(|v| v.as_str()))
}

pub(super) fn tab_role(svc: &ServiceState, tab_id: &str) -> Option<&'static str> {
    svc.mux
        .read()
        .get_tab(tab_id)
        .and_then(|tab| tab.role.map(|role| (role, tab.role_profile.clone())))
        .map(|(role, profile)| match role {
            AgentRole::Manager if profile.as_deref() == Some(ORCHESTRATOR) => "orchestrator",
            AgentRole::Manager => "manager",
            AgentRole::Worker => "worker",
        })
}

pub(super) fn tab_and_ancestors(
    svc: &ServiceState,
    tab_id: &str,
) -> std::collections::HashSet<String> {
    let mux = svc.mux.read();
    let mut out = std::collections::HashSet::new();
    let mut current = Some(tab_id.to_string());
    while let Some(id) = current {
        if !out.insert(id.clone()) {
            break;
        }
        current = mux.get_tab(&id).and_then(|tab| tab.parent_tab_id.clone());
    }
    out
}
