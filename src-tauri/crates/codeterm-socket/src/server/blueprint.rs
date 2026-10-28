use serde_json::{json, Value};

use super::{get_string_param, JsonRpcError};
use codeterm_sentinel_host::{
    BlueprintApplyExt, BlueprintExt, BlueprintMemoryExt, BlueprintPortableExt, SentinelDeleteExt,
    SentinelExt,
};
use codeterm_service_core::service::ServiceState;

fn error(message: impl Into<String>) -> JsonRpcError {
    let message = message.into();
    let version_conflict = message
        .strip_prefix("blueprint_version_conflict: current_version=")
        .and_then(|version| version.parse::<u64>().ok());
    let code = if version_conflict.is_some() {
        "blueprint_version_conflict"
    } else if message.starts_with("blueprint_name_conflict:") {
        "blueprint_name_conflict"
    } else if message.starts_with("blueprint_store_unsupported_format:") {
        "blueprint_store_unsupported_format"
    } else if message.starts_with("blueprint_store_load_failed:") {
        "blueprint_store_load_failed"
    } else if message.starts_with("blueprint_store_persistence_failed:") {
        "blueprint_store_persistence_failed"
    } else {
        "blueprint_refused"
    };
    JsonRpcError {
        code: -32602,
        message,
        data: Some(match version_conflict {
            Some(current_version) => json!({
                "code": code,
                "current_version": current_version
            }),
            None => json!({"code": code}),
        }),
    }
}

fn apply_error(failure: codeterm_blueprint_types::blueprint_contract::ApplyError) -> JsonRpcError {
    match serde_json::to_value(&failure) {
        Ok(data) => JsonRpcError {
            code: -32602,
            message: failure.to_string(),
            data: Some(data),
        },
        Err(failure) => error(format!("Could not serialize Blueprint error: {failure}")),
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
        "blueprint.keep" => {
            let args = params
                .as_ref()
                .ok_or_else(|| error("parameters required"))?;
            let tags = keep_tags(args)?;
            let when = serde_json::from_value::<Vec<String>>(
                args.get("when").cloned().unwrap_or(json!([])),
            )
            .map_err(|e| error(e.to_string()))?;
            if let Some(definition) = args.get("definition") {
                if args.get("id").is_some() {
                    return Err(error("keep id and file definition are mutually exclusive"));
                }
                let document =
                    codeterm_blueprint_types::blueprint_document::Document::from_definition(
                        definition,
                    )
                    .map_err(error)?;
                let local = local_callees(params)?;
                svc.blueprint_keep_callable(&caller, document, &local, &when, &tags)
                    .map_err(error)
            } else {
                svc.blueprint_keep(&caller, &get_string_param(params, "id")?, &when, &tags)
                    .map_err(error)
            }
        }
        "blueprint.recall" => {
            let intent = get_string_param(params, "intent")?;
            let args = params
                .as_ref()
                .ok_or_else(|| error("parameters required"))?;
            if args["cards"].as_bool().unwrap_or(false) {
                let limit = args.get("limit").map_or(Ok(3), |value| {
                    value
                        .as_u64()
                        .filter(|v| (1..=20).contains(v))
                        .ok_or_else(|| error("Recall limit must be between 1 and 20"))
                })?;
                svc.blueprint_recall_cards(&caller, &intent, limit as usize)
                    .map_err(error)
            } else {
                svc.blueprint_recall(&caller, &intent).map_err(error)
            }
        }
        "blueprint.apply_recipe" => {
            let args = params
                .as_ref()
                .ok_or_else(|| error("parameters required"))?;
            let set = serde_json::from_value::<Vec<String>>(
                args.get("set").cloned().unwrap_or(json!([])),
            )
            .map_err(|e| error(e.to_string()))?;
            svc.blueprint_apply_recipe(
                &caller,
                &get_string_param(params, "id")?,
                &set,
                args["note"].as_str(),
                args["dry_run"].as_bool().unwrap_or(false),
            )
            .map_err(error)
        }
        "blueprint.apply" => {
            let definition = params
                .as_ref()
                .and_then(|p| p.get("definition"))
                .cloned()
                .ok_or_else(|| error("definition required"))?;
            let dry_run = params
                .as_ref()
                .and_then(|p| p.get("dry_run"))
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let document = codeterm_blueprint_types::blueprint_document::Document::from_definition(
                &definition,
            )
            .map_err(error)?;
            let local = local_callees(params)?;
            let document =
                codeterm_sentinel_host::blueprint_calls::pin_bundle(svc, &caller, document, &local)
                    .map_err(error)?;
            let definition = serde_json::to_value(document).map_err(|e| error(e.to_string()))?;
            svc.blueprint_apply(&caller, definition, dry_run)
                .map_err(apply_error)
        }
        "blueprint.export" => {
            let id = get_string_param(params, "id")?;
            serde_json::to_value(svc.blueprint_export(&caller, &id).map_err(error)?)
                .map_err(|e| error(e.to_string()))
        }
        "blueprint.import" => {
            let scope = params.as_ref().and_then(|p| p.get("scope"));
            let scope = match scope {
                None | Some(Value::Null) => None,
                Some(Value::String(scope)) => Some(scope.as_str()),
                _ => return Err(error("Blueprint scope must be a string")),
            };
            svc.blueprint_import_destination(&caller, scope)
                .map_err(error)?;
            let recipe = params
                .as_ref()
                .and_then(|p| p.get("recipe"))
                .cloned()
                .ok_or_else(|| error("recipe required"))?;
            let mut recipe: codeterm_blueprint_types::blueprint_recipe::Recipe =
                serde_json::from_value(recipe)
                    .map_err(|e| error(format!("invalid recipe: {e}")))?;
            let document = codeterm_blueprint_types::blueprint_document::Document::from_definition(
                &recipe.definition,
            )
            .map_err(error)?;
            let document = codeterm_sentinel_host::blueprint_calls::pin_bundle(
                svc,
                &caller,
                document,
                &local_callees(params)?,
            )
            .map_err(error)?;
            recipe.definition = serde_json::to_value(document).map_err(|e| error(e.to_string()))?;
            let assignments: Vec<String> = serde_json::from_value(
                params
                    .as_ref()
                    .and_then(|p| p.get("set"))
                    .cloned()
                    .unwrap_or(json!([])),
            )
            .map_err(|e| error(e.to_string()))?;
            let stopped = params
                .as_ref()
                .and_then(|p| p.get("stopped"))
                .and_then(Value::as_bool)
                .unwrap_or(true);
            svc.blueprint_import(&caller, recipe, &assignments, stopped)
                .map_err(error)
        }
        "blueprint.input" => {
            let id = get_string_param(params, "id")?;
            let text = get_string_param(params, "text")?;
            svc.blueprint_input(&caller, &id, text).map_err(error)
        }
        "blueprint.pipeline.nodes" => Ok(svc.blueprint_pipeline_nodes()),
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
        "blueprint.list" => {
            let mut listed = serde_json::to_value(svc.sentinel_list(&caller).map_err(error)?)
                .map_err(|e| error(e.to_string()))?;
            let quarantined = svc.sentinel_quarantined(&caller).map_err(error)?;
            if let Some(entries) = listed.as_array_mut() {
                for entry in quarantined {
                    entries.push(serde_json::to_value(entry).map_err(|e| error(e.to_string()))?);
                }
            }
            Ok(listed)
        }
        "blueprint.show" => {
            let id = get_string_param(params, "id")?;
            if codeterm_blueprint_types::blueprint_document::calls::saved_reference(&id) {
                return codeterm_sentinel_host::blueprint_calls::show_callable(svc, &caller, &id)
                    .map_err(error);
            }
            let job = match svc.sentinel_show(&caller, &id) {
                Ok(job) => job,
                Err(message) => {
                    let quarantined = svc.sentinel_quarantined(&caller).map_err(error)?;
                    return match quarantined.into_iter().find(|entry| entry.id == id) {
                        Some(entry) => {
                            serde_json::to_value(entry).map_err(|e| error(e.to_string()))
                        }
                        None => Err(error(message)),
                    };
                }
            };
            let mut value = serde_json::to_value(&job).map_err(|e| error(e.to_string()))?;
            if let Some(spec) = &job.pipeline().cloned() {
                let entries = svc.blueprint_log(&caller, &id, 200).map_err(error)?;
                add_pipeline_show(&mut value, spec, &entries);
                let recorded_version = entries.first().and_then(|row| row.definition_version);
                let document =
                    if entries.is_empty() || recorded_version == Some(job.definition_version) {
                        job.document.clone()
                    } else if let Some(version) = recorded_version {
                        svc.sentinel_history(&caller, &id)
                            .ok()
                            .and_then(|history| {
                                history
                                    .versions
                                    .into_iter()
                                    .find(|row| row.version == version)
                            })
                            .and_then(|row| row.definition.document)
                    } else {
                        None
                    };
                let document = document.and_then(|raw| {
                    serde_json::from_value::<
                            codeterm_blueprint_types::blueprint_document::Document,
                        >(raw)
                        .ok()
                });
                let projection = codeterm_sentinel_host::blueprint_calls::project_retained(
                    document.as_ref(),
                    &entries,
                    recorded_version,
                )
                .map_err(error)?;
                if let (Some(value), Some(fields)) = (value.as_object_mut(), projection.as_object())
                {
                    value.extend(fields.clone());
                }
            }
            Ok(value)
        }
        "blueprint.stop" => {
            let id = get_string_param(params, "id")?;
            serde_json::to_value(svc.sentinel_stop(&caller, &id).map_err(error)?)
                .map_err(|e| error(e.to_string()))
        }
        "blueprint.delete" => {
            let flag = |name: &str| {
                params
                    .as_ref()
                    .and_then(|value| value.get(name))
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
            };
            let id = params
                .as_ref()
                .and_then(|value| value.get("id"))
                .and_then(Value::as_str)
                .map(str::to_owned);
            let target = match (&id, flag("stopped")) {
                (Some(id), false) => codeterm_sentinel::delete::DeleteTarget::One(id),
                (None, true) => codeterm_sentinel::delete::DeleteTarget::Stopped,
                _ => return Err(error("pass exactly one of id or stopped")),
            };
            let deleted = svc
                .blueprint_delete(&caller, target, flag("stop"))
                .map_err(error)?;
            Ok(json!({ "deleted": deleted }))
        }
        "blueprint.update" => {
            let args = params
                .as_ref()
                .ok_or_else(|| error("parameters required"))?;
            let id = get_string_param(params, "id")?;
            let version = args
                .get("expect_version")
                .and_then(Value::as_u64)
                .ok_or_else(|| error("expect_version must be an unsigned integer"))?;
            let definition = args
                .get("definition")
                .cloned()
                .ok_or_else(|| error("native v2 definition required"))?;
            let document = codeterm_blueprint_types::blueprint_document::Document::from_definition(
                &definition,
            )
            .map_err(error)?;
            let document = codeterm_sentinel_host::blueprint_calls::pin_bundle(
                svc,
                &caller,
                document,
                &local_callees(params)?,
            )
            .map_err(error)?;
            let definition = serde_json::to_value(document).map_err(|e| error(e.to_string()))?;
            let cause = args.get("cause").and_then(Value::as_str).map(str::to_owned);
            svc.blueprint_apply_document(
                &caller,
                definition,
                args["dry_run"].as_bool().unwrap_or(false),
                None,
                Some((id, version, cause)),
            )
            .map_err(apply_error)
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

fn add_pipeline_show(
    value: &mut Value,
    spec: &codeterm_blueprint_types::blueprint_pipeline::BlueprintSpec,
    entries: &[codeterm_service_core::storage::BlueprintJournalEntry],
) {
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
            let accepted = step.kind.outcomes().unwrap_or_default().into_iter().collect::<Vec<_>>();
            let outcome = run_entries.iter().find(|entry| {
                root_step_entry(step,entry)
                    && entry.outcome.as_ref().is_some_and(|outcome| accepted.iter().any(|value| value == outcome.as_str()))
            }).or_else(|| {
                run_entries.iter().find(|entry| {
                    root_step_entry(step,entry)
                        && matches!(entry.state.as_str(), "running" | "skipped")
                })
            });
            let summary = run_entries
                .iter()
                .find(|entry| root_step_entry(step,entry) && entry.result_summary.is_some());
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

fn root_step_entry(
    step: &codeterm_blueprint_types::blueprint_pipeline::Step,
    entry: &codeterm_service_core::storage::BlueprintJournalEntry,
) -> bool {
    entry.step_name == step.name
        && if matches!(
            step.kind,
            codeterm_blueprint_types::blueprint_pipeline::StepKind::Call { .. }
        ) {
            entry.call_stack.len() == 1
                && entry
                    .call_stack
                    .first()
                    .is_some_and(|frame| frame.node == step.name)
        } else {
            entry.call_stack.is_empty()
        }
}

fn step_kind_name(kind: &codeterm_blueprint_types::blueprint_pipeline::StepKind) -> &'static str {
    kind.name()
}

#[cfg(test)]
fn stored_definition(value: Value) -> Result<Value, JsonRpcError> {
    codeterm_sentinel::public_definition::stored_definition(value).map_err(error)
}

fn exposed_definition(value: Value) -> Value {
    codeterm_sentinel::public_definition::exposed_definition(value)
}

#[cfg(test)]
mod tests;

fn local_callees(
    params: &Option<Value>,
) -> Result<Vec<codeterm_blueprint_types::blueprint_document::calls::LocalCallee>, JsonRpcError> {
    serde_json::from_value(
        params
            .as_ref()
            .and_then(|args| args.get("local_callees"))
            .cloned()
            .unwrap_or(json!([])),
    )
    .map_err(|e| error(format!("Invalid local callee bundle: {e}")))
}

fn keep_tags(args: &Value) -> Result<Vec<String>, JsonRpcError> {
    let tags =
        serde_json::from_value::<Vec<String>>(args.get("tags").cloned().unwrap_or(json!([])))
            .map_err(|e| error(e.to_string()))?;
    codeterm_blueprint_types::blueprint_tags::validate(&tags).map_err(|failure| JsonRpcError {
        code: -32602,
        message: failure.to_string(),
        data: Some(json!(failure)),
    })
}
