use serde_json::{json, Value};

use super::JsonRpcError;

/// Validate the whole draft before any task is persisted.
pub(super) fn normalize(
    params: &Option<Value>,
    store: &codeterm_service_core::service::plan::PlanStore,
    plan_id: &str,
) -> Result<Value, JsonRpcError> {
    let mut value = params.clone().unwrap_or_else(|| json!({}));
    let mut errors = Vec::new();
    for field in ["name", "executor_role", "role_guidance"] {
        if !value
            .get(field)
            .and_then(Value::as_str)
            .is_some_and(|text| !text.trim().is_empty())
        {
            errors.push(format!(
                "--{} is required and cannot be blank",
                field.replace('_', "-")
            ));
        }
    }
    let name = value["name"].as_str().unwrap_or_default().to_owned();
    match codeterm_types::plan::require_executor_name(
        value.get("personality").and_then(Value::as_str),
    ) {
        Ok(personality) => value["personality"] = json!(personality),
        Err(error) => errors.push(format!("--personality: {error}")),
    }
    let work_facet = store
        .plan_work_facet_projection(plan_id)
        .map_err(super::plan_rpc_err)?;
    let sections = super::csv_string_field(Some(&value), "sections");
    for section in &sections {
        if store
            .resolve_task_section_ids(plan_id, std::slice::from_ref(section))
            .map_err(super::plan_rpc_err)?
            .is_empty()
        {
            errors.push(format!(
                "--sections: unknown or ambiguous section {section:?}"
            ));
        }
    }
    let mut tags = super::tags_field(Some(&value));
    let inferred = codeterm_types::plan::work_facet::infer_work_tag(
        work_facet.then_some("work"),
        &mut tags,
        &format!(
            "{name} {} {}",
            value["body"].as_str().unwrap_or_default(),
            value["files"]
        ),
    );
    if let Err(error) = codeterm_types::plan::work_facet::validate_work_facet(
        work_facet.then_some("work"),
        tags.iter().map(String::as_str),
    ) {
        errors.push(format!("--tag: {error}"));
    }
    value["tags"] = json!(tags);
    if let Some(tag) = inferred {
        value["inferred_work_warning"] = json!(format!(
            "WARNING: inferred {tag}; set --tag explicitly to override"
        ));
    }
    if errors.is_empty() {
        Ok(value)
    } else {
        Err(JsonRpcError {
            code: -32602,
            message: errors.join("\n"),
            data: Some(json!({"type":"invalid_task_add", "errors": errors})),
        })
    }
}

#[cfg(test)]
mod tests;
