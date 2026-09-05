use super::plan_fields::{groups_from_csv, kinds_from_csv, tag_facets_field};
use super::plan_linking::{owner_from_params, tags_field};
use super::plan_refs;
use super::{get_string_param, JsonRpcError};
use crate::plan::{OrchestratorRef, PlanStatus, PlannerRef};
use crate::service::plan::adopt::AdoptPlanExt;
use crate::service::plan::batch::{BatchRequest, BatchResult};
use crate::service::plan::{hints, PlanError, PlanStore};
use crate::service::plan_batch_authority::{apply_batch_from_socket, BatchWriteError};
use crate::service::{ServiceState, TeamPlanCreationError};
use serde_json::{json, Value};
use tauri::AppHandle;
pub(super) fn plan_rpc_err(e: PlanError) -> JsonRpcError {
    let data = e.to_json();
    JsonRpcError {
        code: e.rpc_code(),
        message: e.to_string(),
        data: Some(data),
    }
}

fn batch_write_rpc_err(error: BatchWriteError) -> JsonRpcError {
    match error {
        BatchWriteError::Plan(e) => plan_rpc_err(e),
        BatchWriteError::Refused(refusal) => JsonRpcError {
            code: -32003,
            data: Some(refusal.to_json()),
            message: refusal.message,
        },
    }
}

/// The `plan.batch` verb's whole write: parse the ops, cross the authority
/// boundary, apply. The dispatch arm adds only plan resolution and the event.
pub(crate) fn apply_batch_rpc(
    store: &PlanStore,
    plan_id: &str,
    body: &Value,
    now_ms: u64,
) -> Result<BatchResult, JsonRpcError> {
    let request = BatchRequest {
        key: body.get("key").and_then(|v| v.as_str()).map(str::to_string),
        ops: body
            .get("ops")
            .cloned()
            .ok_or(JsonRpcError {
                code: -32602,
                message: "Missing required parameter: ops".to_string(),
                data: None,
            })
            .and_then(|v| {
                serde_json::from_value(v).map_err(|e| JsonRpcError {
                    code: -32602,
                    message: format!("invalid batch ops: {e}"),
                    data: None,
                })
            })?,
    };
    apply_batch_from_socket(store, plan_id, Some(body), request, now_ms)
        .map_err(batch_write_rpc_err)
}

pub(super) fn emit_plan_changed(app: &AppHandle, plan_id: &str) {
    crate::api::events::emit_everywhere(app, "plan-changed", json!({ "plan_id": plan_id }));
}

pub(super) fn dispatch(
    method: &str,
    params: &Option<Value>,
    app: &AppHandle,
    svc: &ServiceState,
) -> Result<Value, JsonRpcError> {
    let now = super::plan_time::epoch_ms();
    if method.starts_with("plan.handoff.") {
        return super::plan_handoff::dispatch(method, params, app, svc);
    }
    if method.starts_with("plan.heartbeat.") {
        return super::plan_heartbeat::dispatch(method, params, app, svc);
    }
    if method.starts_with("plan.goal.") {
        return super::plan_goal::dispatch(method, params, app, svc);
    }
    if method.starts_with("plan.qa.") {
        return super::plan_qa::dispatch(method, params, app, svc);
    }
    if method.starts_with("plan.review.") {
        return super::plan_review::dispatch(method, params, app, svc);
    }
    if method.starts_with("plan.decision.") {
        return super::plan_gates::dispatch(method, params, app, svc);
    }
    if super::plan_read::handles(method) {
        return super::plan_read::dispatch(method, params, svc);
    }
    if super::plan_elements::handles(method) {
        return super::plan_elements::dispatch(method, params, app, svc);
    }
    if super::plan_loops::handles(method) {
        return super::plan_loops::dispatch(method, params, app, svc);
    }
    if super::plan_lifecycle::handles(method) {
        return super::plan_lifecycle::dispatch(method, params, app, svc);
    }
    if super::plan_attachments::handles(method) {
        return super::plan_attachments::dispatch(method, params, app, svc);
    }
    if super::plan_task_rpc::handles(method) {
        return super::plan_task_rpc::dispatch(method, params, app, svc, now);
    }
    if super::plan_skills_rpc::handles(method) {
        return super::plan_skills_rpc::dispatch(method, params, svc, now);
    }
    let p = params.as_ref();
    let store = &svc.plan_store;

    match method {
        "plan.binding.resolve" => {
            let tab_id = params
                .as_ref()
                .and_then(|value| value.get("tab_id"))
                .and_then(|value| value.as_str());
            let team_id = tab_id.and_then(|tab_id| {
                svc.mux
                    .read()
                    .get_tab(tab_id)
                    .and_then(|tab| tab.team_id.clone())
            });
            plan_refs::task_binding(store, params, team_id.as_deref())
        }
        "plan.batch" => {
            let id = plan_refs::resolve_plan(store, params)?;
            let body = p.ok_or(JsonRpcError {
                code: -32602,
                message: "params required".to_string(),
                data: None,
            })?;
            let result = apply_batch_rpc(store, &id, body, now)?;
            emit_plan_changed(app, &id);
            serde_json::to_value(result).map_err(|e| JsonRpcError {
                code: -32603,
                message: e.to_string(),
                data: None,
            })
        }

        "plan.create" => {
            let body = p.ok_or(JsonRpcError {
                code: -32602,
                message: "params required".to_string(),
                data: None,
            })?;
            let title = get_string_param(params, "title")?;
            let project = body
                .get("project_path")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| {
                    std::env::current_dir()
                        .map(|d| d.display().to_string())
                        .unwrap_or_default()
                });
            let kinds = body
                .get("kinds")
                .and_then(|v| v.as_str())
                .map(kinds_from_csv)
                .unwrap_or_default();
            let from_spec = body
                .get("from_spec")
                .and_then(|v| v.as_str())
                .filter(|s| !s.trim().is_empty())
                .map(str::to_string);
            let owner = owner_from_params(body);
            let outcome = svc
                .create_team_plan_for_caller(
                    title,
                    project,
                    owner,
                    crate::service::TeamPlanCaller {
                        tab_id: body
                            .get("caller_tab_id")
                            .and_then(|value| value.as_str())
                            .map(str::to_string),
                        session_id: body
                            .get("session_id")
                            .and_then(|value| value.as_str())
                            .map(str::to_string),
                        binding_nonce: body
                            .get("session_binding_nonce")
                            .and_then(|value| value.as_str())
                            .map(str::to_string),
                    },
                    kinds,
                    from_spec.clone(),
                    now,
                )
                .map_err(|error| match error {
                    TeamPlanCreationError::Plan(error) => plan_rpc_err(error),
                    TeamPlanCreationError::Team(message) => JsonRpcError {
                        code: -32602,
                        message,
                        data: None,
                    },
                })?;
            let plan = outcome.plan.clone();
            let plan_id = plan.id.clone();
            let empty_sections = plan.sections.is_empty();
            let mut response = json!({
                "plan_id": plan_id.clone(),
                "plan": plan,
                "from_spec": from_spec.unwrap_or_default()
            });
            if empty_sections {
                response["_hints"] = json!([hints::plan_sections_hint(&plan_id)]);
            }
            outcome.apply_to_response(&mut response);
            emit_plan_changed(app, &plan_id);
            Ok(response)
        }

        "plan.adopt" => {
            let body = p.ok_or(JsonRpcError {
                code: -32602,
                message: "params required".to_string(),
                data: None,
            })?;
            let source = get_string_param(params, "source")?;
            let target = body
                .get("project")
                .or_else(|| body.get("to"))
                .and_then(|v| v.as_str())
                .ok_or(JsonRpcError {
                    code: -32602,
                    message: "project required".into(),
                    data: None,
                })?;
            let target_project = svc
                .ensure_plan_project_loaded(std::path::Path::new(target))
                .map_err(plan_rpc_err)?;
            let result = store
                .adopt_plan(
                    crate::service::plan::adopt::AdoptPlanRequest {
                        source: std::path::PathBuf::from(source),
                        target_project,
                        owner: owner_from_params(body),
                        dry_run: body
                            .get("dry_run")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false),
                        overwrite: body
                            .get("overwrite")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false),
                        remove_source: body
                            .get("remove_source")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false),
                    },
                    now,
                )
                .map_err(plan_rpc_err)?;
            if !result.dry_run {
                emit_plan_changed(app, &result.plan_id);
            }
            Ok(serde_json::to_value(result).unwrap_or_default())
        }

        "plan.list" => {
            svc.validate_active_team_plan_bindings()
                .map_err(super::team::team_plan_rpc_err)?;
            let want =
                if let Some(project) = p.and_then(|x| x.get("project")).and_then(|v| v.as_str()) {
                    Some(
                        svc.ensure_plan_project_loaded(std::path::Path::new(project))
                            .map_err(plan_rpc_err)?
                            .display()
                            .to_string(),
                    )
                } else {
                    None
                };
            let mut entries = store.list_index();
            if let Some(want) = &want {
                entries.retain(|e| {
                    store
                        .get(&e.plan_id)
                        .map(|pl| pl.project_path == *want)
                        .unwrap_or(false)
                });
            }
            if p.and_then(|x| x.get("unowned")).and_then(|v| v.as_bool()) == Some(true) {
                entries.retain(|e| e.owner.tab_id.is_none() && e.owner.session_id.is_none());
            }
            if let Some(status) = p.and_then(|x| x.get("status")).and_then(|v| v.as_str()) {
                entries.retain(|e| match status {
                    "active" => e.status == PlanStatus::Active,
                    "archived" => e.status == PlanStatus::Archived,
                    _ => true,
                });
            }
            Ok(json!({ "plans": entries }))
        }

        "plan.update" => {
            let id = plan_refs::resolve_plan(store, params)?;
            let status = p
                .and_then(|x| x.get("status"))
                .and_then(|v| v.as_str())
                .map(|s| serde_json::from_value::<PlanStatus>(Value::String(s.into())))
                .transpose()
                .map_err(|_| plan_rpc_err(PlanError::BadStatus))?;
            let mode = p.and_then(|x| x.get("mode")).and_then(|v| v.as_str());
            store
                .update_plan(
                    &id,
                    p.and_then(|x| x.get("title")).and_then(|v| v.as_str()),
                    status,
                    mode,
                    now,
                )
                .map_err(plan_rpc_err)?;
            if let Some(planner_val) = p.and_then(|x| x.get("planner")) {
                let planner_ref: Option<PlannerRef> = serde_json::from_value(planner_val.clone())
                    .map_err(|e| JsonRpcError {
                    code: -32602,
                    message: format!("invalid planner: {e}"),
                    data: None,
                })?;
                svc.transition_plan_planner(&id, planner_ref)
                    .map_err(|message| JsonRpcError {
                        code: -32603,
                        message,
                        data: None,
                    })?;
            }
            emit_plan_changed(app, &id);
            Ok(json!({ "ok": true, "plan_id": id }))
        }
        "plan.archive" => super::plan_archive::archive(store, svc, app, params, now),
        "plan.unarchive" => super::plan_archive::unarchive(store, svc, params, now),
        "plan.delete" => {
            let id = plan_refs::resolve_plan(store, params)?;
            store.delete(&id).map_err(|e| JsonRpcError {
                code: -32603,
                message: e.to_string(),
                data: None,
            })?;
            emit_plan_changed(app, &id);
            Ok(json!({ "ok": true, "plan_id": id }))
        }

        "plan.transfer" => {
            let id = plan_refs::resolve_plan(store, params)?;
            let to: OrchestratorRef = p
                .and_then(|x| x.get("to"))
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .ok_or(JsonRpcError {
                    code: -32602,
                    message: "to required".into(),
                    data: None,
                })?;
            let reason = p
                .and_then(|x| x.get("reason"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let caller = transfer_caller(params)?;
            svc.transfer_team_plan(&id, to, reason, caller, now)
                .map_err(super::team::team_plan_rpc_err)?;
            Ok(json!({ "ok": true, "plan_id": id }))
        }

        "plan.disown" => {
            let id = plan_refs::resolve_plan(store, params)?;
            store.disown(&id, now).map_err(plan_rpc_err)?;
            emit_plan_changed(app, &id);
            Ok(json!({ "ok": true, "plan_id": id }))
        }

        "plan.claim" => {
            let id = plan_refs::resolve_plan(store, params)?;
            let params_val = p.cloned().unwrap_or(json!({}));
            let owner: OrchestratorRef = params_val
                .get("owner")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_else(|| owner_from_params(&params_val));
            store.claim(&id, owner.clone(), now).map_err(plan_rpc_err)?;
            emit_plan_changed(app, &id);
            let mut response_hints = Vec::new();
            if matches!(
                store.resolve_planner_tab(&id, |pid| svc.tab_probe(pid)),
                crate::plan::peer::PeerResolution::Unbound
            ) {
                let spawn_cmd = crate::service::plan::peer_mandate::planner_spawn_cmd_hint(
                    owner.tab_id.as_deref(),
                    &id,
                );
                response_hints.push(crate::prompts::peer_pairing_mandate_full(&spawn_cmd));
            }
            Ok(json!({ "ok": true, "plan_id": id, "_hints": response_hints }))
        }

        "plan.ingest" => {
            let id = plan_refs::resolve_plan(store, params)?;
            super::plan_task_rpc::authorize_draft_mutation(svc, &id, p)?;
            let markdown = get_string_param(params, "markdown")?;
            let result = store
                .ingest_plan_sections(&id, &markdown, now)
                .map_err(plan_rpc_err)?;
            emit_plan_changed(app, &id);
            Ok(serde_json::to_value(result).unwrap_or_else(|_| json!({ "sections": [] })))
        }

        "plan.section.add" => {
            let id = plan_refs::resolve_plan(store, params)?;
            super::plan_task_rpc::authorize_draft_mutation(svc, &id, p)?;
            let title =
                get_string_param(params, "title").or_else(|_| get_string_param(params, "name"))?;
            let body = p
                .and_then(|x| x.get("body"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let parent = p.and_then(|x| x.get("parent")).and_then(|v| v.as_str());
            let sid = store
                .add_section(
                    &id,
                    &title,
                    body,
                    parent,
                    &tags_field(p),
                    p.and_then(|x| x.get("primary")).and_then(|v| v.as_str()),
                    p.and_then(|x| x.get("handle")).and_then(|v| v.as_str()),
                    now,
                )
                .map_err(plan_rpc_err)?;
            emit_plan_changed(app, &id);
            let caller_tab = p
                .and_then(|x| x.get("caller_tab_id"))
                .and_then(|v| v.as_str());
            let mut resp = json!({ "section_id": sid });
            if let Some(nudge) = store
                .get(&id)
                .as_ref()
                .and_then(|plan| hints::planner_ownership_nudge(plan, caller_tab))
            {
                resp["_planner_nudge"] = json!(nudge);
            }
            Ok(resp)
        }

        "plan.section.update" => {
            let (id, sid) = plan_refs::resolve_section(store, params)?;
            super::plan_task_rpc::authorize_draft_mutation(svc, &id, p)?;
            store
                .update_section(
                    &id,
                    &sid,
                    p.and_then(|x| x.get("title").or_else(|| x.get("name")))
                        .and_then(|v| v.as_str()),
                    p.and_then(|x| x.get("body")).and_then(|v| v.as_str()),
                    &tags_field(p),
                    p.and_then(|x| x.get("primary")).and_then(|v| v.as_str()),
                    p.and_then(|x| x.get("handle")).and_then(|v| v.as_str()),
                    now,
                )
                .map_err(plan_rpc_err)?;
            emit_plan_changed(app, &id);
            let caller_tab = p
                .and_then(|x| x.get("caller_tab_id"))
                .and_then(|v| v.as_str());
            let mut resp = json!({ "ok": true, "section_id": sid });
            if let Some(nudge) = store
                .get(&id)
                .as_ref()
                .and_then(|plan| hints::planner_ownership_nudge(plan, caller_tab))
            {
                resp["_planner_nudge"] = json!(nudge);
            }
            Ok(resp)
        }

        "plan.section.rm" => {
            let (id, sid) = plan_refs::resolve_section(store, params)?;
            super::plan_task_rpc::authorize_draft_mutation(svc, &id, p)?;
            store.rm_section(&id, &sid, now).map_err(plan_rpc_err)?;
            emit_plan_changed(app, &id);
            Ok(json!({ "ok": true, "section_id": sid }))
        }

        "plan.section.show" => {
            let (id, sid) = plan_refs::resolve_section(store, params)?;
            let result = store.show_section(&id, &sid).map_err(plan_rpc_err)?;
            serde_json::to_value(result).map_err(|e| JsonRpcError {
                code: -32603,
                message: e.to_string(),
                data: None,
            })
        }

        "plan.kinds.set" => {
            let id = plan_refs::resolve_plan(store, params)?;
            let kinds = get_string_param(params, "kinds")?;
            store
                .set_kinds(&id, kinds_from_csv(&kinds), now)
                .map_err(plan_rpc_err)?;
            emit_plan_changed(app, &id);
            Ok(json!({ "ok": true, "plan_id": id }))
        }

        "plan.tag_facets.set" => {
            let id = plan_refs::resolve_plan(store, params)?;
            let facets =
                tag_facets_field(params.as_ref(), "facets").map_err(|message| JsonRpcError {
                    code: -32602,
                    message,
                    data: None,
                })?;
            store
                .set_tag_facets(&id, facets, now)
                .map_err(plan_rpc_err)?;
            emit_plan_changed(app, &id);
            Ok(json!({ "ok": true, "plan_id": id }))
        }

        "plan.coverage" => {
            let id = plan_refs::resolve_plan(store, params)?;
            let report = store
                .coverage_report(&id)
                .ok_or_else(|| plan_rpc_err(PlanError::NotFound))?;
            Ok(serde_json::to_value(report).unwrap_or_default())
        }

        "plan.state" => {
            let id = plan_refs::resolve_plan(store, params)?;
            let snapshot = svc.build_plan_state(&id, now).map_err(plan_rpc_err)?;
            let mut value = serde_json::to_value(snapshot).unwrap_or_default();
            let tab_id = store
                .get(&id)
                .and_then(|plan| plan.owner.tab_id)
                .unwrap_or_default();
            value["reminder"] =
                serde_json::to_value(svc.plan_turn_counter.time_reminder_telemetry(&tab_id))
                    .unwrap_or_default();
            Ok(value)
        }

        "plan.next" => {
            let id = plan_refs::resolve_plan(store, params)?;
            let snapshot = svc.build_plan_state(&id, now).map_err(plan_rpc_err)?;
            Ok(json!({
                "plan_id": id,
                "next_actions": snapshot.next_actions,
                "blocked_or_gated_count": snapshot.blocked_or_gated_count,
            }))
        }

        "plan.reconcile" => {
            let id = plan_refs::resolve_plan(store, params)?;
            let fix = p.and_then(|x| x.get("fix")).and_then(|v| v.as_bool()) == Some(true);
            let caller = p
                .and_then(|x| x.get("caller_tab_id"))
                .and_then(|v| v.as_str());
            let report = svc
                .reconcile_plan(&id, fix, caller, now)
                .map_err(plan_rpc_err)?;
            Ok(serde_json::to_value(report).unwrap_or_default())
        }

        "plan.groups.set" => {
            let id = plan_refs::resolve_plan(store, params)?;
            let groups = get_string_param(params, "groups")?;
            store
                .set_task_groups(&id, groups_from_csv(&groups), now)
                .map_err(plan_rpc_err)?;
            emit_plan_changed(app, &id);
            Ok(json!({ "ok": true, "plan_id": id }))
        }

        "plan.groups.close" => {
            let id = plan_refs::resolve_plan(store, params)?;
            let key = get_string_param(params, "key")?;
            let note = p.and_then(|x| x.get("note")).and_then(|v| v.as_str()).unwrap_or("");
            let from = p
                .and_then(|x| x.get("from"))
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .or_else(|| {
                    p.and_then(|x| x.get("caller_tab_id"))
                        .and_then(|v| v.as_str())
                        .map(crate::plan::AgentRef::from_tab)
                })
                .unwrap_or_else(|| crate::plan::AgentRef::from_tab("unknown"));
            store
                .close_group(&id, &key, note, from, now)
                .map_err(plan_rpc_err)?;
            emit_plan_changed(app, &id);
            Ok(json!({ "ok": true, "plan_id": id, "key": key }))
        }

        "plan.link" => {
            let id = plan_refs::resolve_plan(store, params)?;
            let to = get_string_param(params, "to")?;
            store.link_plan(&id, &to, now).map_err(plan_rpc_err)?;
            emit_plan_changed(app, &id);
            Ok(json!({ "ok": true, "plan_id": id }))
        }

        "plan.unlink" => {
            let id = plan_refs::resolve_plan(store, params)?;
            let to = get_string_param(params, "to")?;
            store.unlink_plan(&id, &to, now).map_err(plan_rpc_err)?;
            emit_plan_changed(app, &id);
            Ok(json!({ "ok": true, "plan_id": id }))
        }

        "plan.retrospective" => {
            let id = plan_refs::resolve_plan(store, params)?;
            let body = p
                .and_then(|x| x.get("body"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim();
            if body.is_empty() {
                return Err(JsonRpcError {
                    code: -32602,
                    message: "body is required".to_string(),
                    data: None,
                });
            }
            let from = p
                .and_then(|x| x.get("from"))
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_else(|| crate::plan::AgentRef::from_tab("unknown"));
            let stride_id = store
                .add_retrospective(&id, body, from, now)
                .map_err(plan_rpc_err)?;
            emit_plan_changed(app, &id);
            Ok(json!({ "plan_id": id, "stride_id": stride_id }))
        }

        _ => Err(JsonRpcError {
            code: -32601,
            message: "Method not found".to_string(),
            data: None,
        }),
    }
}

fn transfer_caller(params: &Option<Value>) -> Result<String, JsonRpcError> {
    params
        .as_ref()
        .and_then(|value| value.get("caller_tab_id"))
        .and_then(|value| value.as_str())
        .filter(|tab_id| !tab_id.trim().is_empty())
        .map(str::to_string)
        .ok_or_else(|| JsonRpcError {
            code: -32602,
            message: "caller_tab_id is required for plan transfer".into(),
            data: None,
        })
}

#[cfg(test)]
mod tests;
