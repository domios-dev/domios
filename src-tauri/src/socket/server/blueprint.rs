use serde_json::{json, Value};

use super::{get_string_param, JsonRpcError};
use crate::service::ServiceState;

fn blueprint_message(message: &str) -> String {
    message
        .replace("Sentinels", "Blueprints")
        .replace("Sentinel", "Blueprint")
        .replace("sentinels ", "blueprints ")
        .replace("sentinel ", "blueprint ")
}

fn error(message: impl Into<String>) -> JsonRpcError {
    let message = message.into();
    let version_conflict = message
        .strip_prefix("sentinel_version_conflict: current_version=")
        .and_then(|version| version.parse::<u64>().ok());
    let code = if version_conflict.is_some() {
        "sentinel_version_conflict"
    } else if message.starts_with("sentinel_store_unsupported_format:") {
        "sentinel_store_unsupported_format"
    } else if message.starts_with("sentinel_store_load_failed:") {
        "sentinel_store_load_failed"
    } else if message.starts_with("sentinel_store_persistence_failed:") {
        "sentinel_store_persistence_failed"
    } else {
        "sentinel_refused"
    };
    JsonRpcError {
        code: -32602,
        message: blueprint_message(&message),
        data: Some(match version_conflict {
            Some(current_version) => json!({
                "code": code,
                "current_version": current_version
            }),
            None => json!({"code": code}),
        }),
    }
}

pub(super) fn dispatch(
    method: &str,
    params: &Option<Value>,
    svc: &ServiceState,
) -> Result<Value, JsonRpcError> {
    let tab = get_string_param(params, "caller_tab_id")?;
    let nonce = params
        .as_ref()
        .and_then(|p| p.get("session_binding_nonce"))
        .and_then(Value::as_str);
    let caller = svc.sentinel_caller(&tab, nonce).map_err(error)?;
    let result = match method {
        "blueprint.input" => {
            let id = get_string_param(params, "id")?;
            let text = get_string_param(params, "text")?;
            svc.blueprint_input(&caller, &id, text).map_err(error)
        }
        "blueprint.pipeline.nodes" => Ok(svc.blueprint_pipeline_nodes()),
        "blueprint.pipeline.validate" => {
            let spec = pipeline_spec_param(params)?;
            svc.blueprint_pipeline_validate(&caller, spec)
                .map_err(error)
        }
        "blueprint.pipeline.validate.update" => {
            let id = get_string_param(params, "id")?;
            let expect_version = params
                .as_ref()
                .and_then(|value| value.get("expect_version"))
                .and_then(Value::as_u64)
                .ok_or_else(|| error("expect_version must be an unsigned integer"))?;
            let spec = pipeline_spec_param(params)?;
            svc.blueprint_pipeline_validate_update(&caller, &id, expect_version, spec)
                .map_err(error)
        }
        "blueprint.pipeline.create" => {
            let spec = pipeline_spec_param(params)?;
            svc.blueprint_pipeline_create(&caller, spec).map_err(error)
        }
        "blueprint.pipeline.update" => {
            let id = get_string_param(params, "id")?;
            let expect_version = params
                .as_ref()
                .and_then(|value| value.get("expect_version"))
                .and_then(Value::as_u64)
                .ok_or_else(|| error("expect_version must be an unsigned integer"))?;
            let spec = pipeline_spec_param(params)?;
            let cause = params
                .as_ref()
                .and_then(|value| value.get("cause"))
                .and_then(Value::as_str)
                .map(str::to_owned);
            serde_json::to_value(
                svc.blueprint_pipeline_update(&caller, &id, expect_version, spec, cause)
                    .map_err(error)?,
            )
            .map_err(|e| error(e.to_string()))
        }
        "blueprint.log" => {
            let id = get_string_param(params, "id")?;
            let limit = params
                .as_ref()
                .and_then(|value| value.get("limit"))
                .and_then(Value::as_u64)
                .unwrap_or(50)
                .clamp(1, 200) as u16;
            serde_json::to_value(svc.blueprint_log(&caller, &id, limit).map_err(error)?)
                .map_err(|e| error(e.to_string()))
        }
        "blueprint.create" => {
            let create = params
                .as_ref()
                .and_then(|p| p.get("create"))
                .ok_or_else(|| error("create params required"))?;
            let create = serde_json::from_value(stored_definition(create.clone())?)
                .map_err(|e| error(format!("invalid blueprint params: {e}")))?;
            serde_json::to_value(svc.sentinel_create(&caller, create).map_err(error)?)
                .map_err(|e| error(e.to_string()))
        }
        "blueprint.validate.create" => {
            let create = params
                .as_ref()
                .and_then(|p| p.get("create"))
                .ok_or_else(|| error("create params required"))?;
            let create = serde_json::from_value(stored_definition(create.clone())?)
                .map_err(|e| error(format!("invalid blueprint params: {e}")))?;
            let mut preview = serde_json::to_value(
                svc.sentinel_validate_create(&caller, create)
                    .map_err(error)?,
            )
            .map_err(|e| error(e.to_string()))?;
            if let Some(fields) = preview.as_object_mut() {
                fields.remove("id");
                fields.remove("scenario_id");
            }
            Ok(preview)
        }
        "blueprint.list" => serde_json::to_value(svc.sentinel_list(&caller).map_err(error)?)
            .map_err(|e| error(e.to_string())),
        "blueprint.show" => {
            let id = get_string_param(params, "id")?;
            let job = svc.sentinel_show(&caller, &id).map_err(error)?;
            let mut value = serde_json::to_value(&job).map_err(|e| error(e.to_string()))?;
            if let Some(spec) = &job.pipeline {
                let entries = svc.blueprint_log(&caller, &id, 200).map_err(error)?;
                add_pipeline_show(&mut value, spec, &entries);
            }
            Ok(value)
        }
        "blueprint.stop" => {
            let id = get_string_param(params, "id")?;
            serde_json::to_value(svc.sentinel_stop(&caller, &id).map_err(error)?)
                .map_err(|e| error(e.to_string()))
        }
        "blueprint.update" => {
            let id = get_string_param(params, "id")?;
            let expect_version = params
                .as_ref()
                .and_then(|value| value.get("expect_version"))
                .and_then(Value::as_u64)
                .ok_or_else(|| error("expect_version must be an unsigned integer"))?;
            let update = params
                .as_ref()
                .and_then(|value| value.get("update"))
                .ok_or_else(|| error("update params required"))?;
            let update = stored_definition(update.clone())?;
            if update.get("versions").is_some() {
                serde_json::to_value(
                    svc.sentinel_update_from_history(&caller, &id, expect_version, &update)
                        .map_err(error)?,
                )
                .map_err(|e| error(e.to_string()))
            } else {
                let update = serde_json::from_value(update)
                    .map_err(|e| error(format!("invalid blueprint update: {e}")))?;
                serde_json::to_value(
                    svc.sentinel_update(&caller, &id, expect_version, update)
                        .map_err(error)?,
                )
                .map_err(|e| error(e.to_string()))
            }
        }
        "blueprint.validate.update" => {
            let id = get_string_param(params, "id")?;
            let expect_version = params
                .as_ref()
                .and_then(|value| value.get("expect_version"))
                .and_then(Value::as_u64)
                .ok_or_else(|| error("expect_version must be an unsigned integer"))?;
            let update = params
                .as_ref()
                .and_then(|value| value.get("update"))
                .ok_or_else(|| error("update params required"))?;
            let update = stored_definition(update.clone())?;
            if update.get("versions").is_some() {
                svc.sentinel_validate_history_update(&caller, &id, expect_version, &update)
                    .map_err(error)
            } else {
                let update = serde_json::from_value(update)
                    .map_err(|e| error(format!("invalid blueprint update: {e}")))?;
                serde_json::to_value(
                    svc.sentinel_validate_update(&caller, &id, expect_version, update)
                        .map_err(error)?,
                )
                .map_err(|e| error(e.to_string()))
            }
        }
        "blueprint.history" => {
            let id = get_string_param(params, "id")?;
            serde_json::to_value(svc.sentinel_history(&caller, &id).map_err(error)?)
                .map_err(|e| error(e.to_string()))
        }
        "blueprint.escalation.resolve" => {
            let id = get_string_param(params, "id")?;
            let transition_id = get_string_param(params, "transition_id")?;
            serde_json::to_value(
                svc.sentinel_resolve_escalation(&caller, &id, &transition_id)
                    .map_err(error)?,
            )
            .map_err(|e| error(e.to_string()))
        }
        _ => Err(JsonRpcError {
            code: -32601,
            message: "Method not found".into(),
            data: None,
        }),
    }?;
    if method.starts_with("blueprint.pipeline.") || method == "blueprint.log" {
        Ok(result)
    } else {
        Ok(exposed_definition(result))
    }
}

fn pipeline_spec_param(
    params: &Option<Value>,
) -> Result<codeterm_types::blueprint_pipeline::BlueprintSpec, JsonRpcError> {
    let value = params
        .as_ref()
        .and_then(|params| params.get("spec"))
        .cloned()
        .ok_or_else(|| error("spec params required"))?;
    serde_json::from_value(value)
        .map_err(|parse_error| error(format!("invalid pipeline spec: {parse_error}")))
}

fn add_pipeline_show(
    value: &mut Value,
    spec: &codeterm_types::blueprint_pipeline::BlueprintSpec,
    entries: &[crate::storage::BlueprintJournalEntry],
) {
    use codeterm_types::blueprint_pipeline::StepKind;
    let latest_run = entries.first().map(|entry| entry.run_id.as_str());
    let run_entries = latest_run
        .map(|run_id| {
            entries
                .iter()
                .filter(|entry| entry.run_id == run_id)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let closed = run_entries.iter().any(|entry| {
        entry.step_name == "$run"
            && matches!(
                entry.outcome.as_deref(),
                Some("done" | "failed" | "skipped")
            )
    });
    let current_step = if latest_run.is_none() || closed {
        None
    } else {
        run_entries
            .iter()
            .find(|entry| entry.step_index >= 0 && entry.state == "running")
            .map(|entry| entry.step_index as usize)
            .or_else(|| {
                (0..spec.steps.len()).find(|index| {
                    !run_entries.iter().any(|entry| {
                        entry.step_index == *index as i64
                            && matches!(entry.state.as_str(), "done" | "failed" | "skipped")
                    })
                })
            })
    };
    value["run_id"] = json!(latest_run);
    value["current_step"] = json!(current_step);
    value["last_event_at_ms"] = json!(entries
        .iter()
        .map(|entry| entry.ended_at_ms.unwrap_or(entry.started_at_ms))
        .max());
    value["steps"] = json!(spec
        .steps
        .iter()
        .enumerate()
        .map(|(index, step)| {
            let accepted = match &step.kind {
                StepKind::Command { .. } | StepKind::Convert { .. } => {
                    vec!["done".to_string(), "failed".to_string()]
                }
                StepKind::Ask { buckets, .. } => buckets
                    .iter()
                    .cloned()
                    .chain(std::iter::once("other".into()))
                    .collect(),
                StepKind::Filter { bucket, .. } => bucket
                    .iter()
                    .cloned()
                    .chain(["done".into(), "failed".into()])
                    .collect(),
            };
            let outcome = run_entries.iter().find(|entry| {
                entry.step_index == index as i64
                    && entry.outcome.as_ref().is_some_and(|outcome| accepted.contains(outcome))
            }).or_else(|| {
                run_entries.iter().find(|entry| {
                    entry.step_index == index as i64
                        && matches!(entry.state.as_str(), "running" | "skipped")
                })
            });
            let summary = run_entries
                .iter()
                .find(|entry| entry.step_index == index as i64 && entry.result_summary.is_some());
            json!({
                "index":index,
                "name":step.name,
                "kind":step_kind_name(&step.kind),
                "title":format!("{}: {}", step_kind_name(&step.kind), step.name),
                "state":outcome.map_or("waiting", |entry| entry.state.as_str()),
                "outcome":outcome.filter(|entry| !entry.outcome.as_deref().is_some_and(|value| value.starts_with("skip:"))).and_then(|entry| entry.outcome.clone()),
                "started_at_ms":outcome.map(|entry| entry.started_at_ms),
                "ended_at_ms":outcome.and_then(|entry| entry.ended_at_ms),
                "result_summary":summary.and_then(|entry| entry.result_summary.clone()),
                "reason":outcome.and_then(|entry| entry.reason.clone())
            })
        })
        .collect::<Vec<_>>());
}

fn step_kind_name(kind: &codeterm_types::blueprint_pipeline::StepKind) -> &'static str {
    use codeterm_types::blueprint_pipeline::StepKind;
    match kind {
        StepKind::Command { .. } => "command",
        StepKind::Convert { .. } => "convert",
        StepKind::Ask { .. } => "ask",
        StepKind::Filter { .. } => "filter",
    }
}

fn stored_definition(mut value: Value) -> Result<Value, JsonRpcError> {
    translate_definition(&mut value, true)?;
    Ok(value)
}

fn exposed_definition(mut value: Value) -> Value {
    let _ = translate_definition(&mut value, false);
    value
}

fn translate_definition(value: &mut Value, incoming: bool) -> Result<(), JsonRpcError> {
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
                        return Err(error("action kind must be blueprint"));
                    }
                    if kind == if incoming { "blueprint" } else { "sentinel" } {
                        *kind = if incoming { "sentinel" } else { "blueprint" }.into();
                    }
                }
            }
            if fields.contains_key("parameters") {
                if let Some(Value::String(kind)) = fields.get_mut("type") {
                    if incoming && kind == "sentinel" {
                        return Err(error("handler type must be blueprint"));
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
                            return Err(error("route must use blueprint:<exact-id>"));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blueprint_handlers_round_trip_without_changing_stored_identity_or_content() {
        let public = json!({
            "id": "sentinel_existing", "probe": "sentinel is opaque probe content",
            "routes": {"done": "blueprint:sentinel_child"},
            "handlers": {"done": [{"id":"join", "type":"blueprint", "parameters":{"target":"sentinel_child"}}]}
        });
        let stored = stored_definition(public.clone()).unwrap();
        assert_eq!(stored["id"], "sentinel_existing");
        assert_eq!(stored["probe"], public["probe"]);
        assert_eq!(stored["routes"]["done"], "sentinel:sentinel_child");
        assert_eq!(stored["handlers"]["done"][0]["type"], "sentinel");
        assert_eq!(exposed_definition(stored), public);
        let persisted = json!({"routes":{"done":[{"id":"join","action":{"kind":"sentinel","target":"sentinel_child"},"target":{"kind":"sentinel","id":"sentinel_child"}}]}});
        let exposed = exposed_definition(persisted.clone());
        assert_eq!(exposed["routes"]["done"][0]["action"]["kind"], "blueprint");
        assert_eq!(exposed["routes"]["done"][0]["target"]["kind"], "sentinel");
        assert_eq!(stored_definition(exposed).unwrap(), persisted);
    }

    #[test]
    fn blueprint_handlers_refuse_removed_public_names() {
        assert!(stored_definition(json!({"routes":{"done":"sentinel:child"}})).is_err());
        assert!(stored_definition(
            json!({"handlers":{"done":[{"type":"sentinel","parameters":{"target":"child"}}]}})
        )
        .is_err());
    }

    #[test]
    fn sentinel_update_rpc_version_conflict_carries_current_version() {
        let error = error(crate::sentinel::amend::version_conflict(9));
        assert_eq!(
            error.data.as_ref().unwrap()["code"],
            "sentinel_version_conflict"
        );
        assert_eq!(error.data.as_ref().unwrap()["current_version"], 9);
    }
}
