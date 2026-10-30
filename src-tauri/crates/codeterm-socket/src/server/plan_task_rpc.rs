use super::plan::{emit_plan_changed, plan_rpc_err};
use super::plan_fields::{
    execution_intent_field, optional_string_field, string_array_field, tab_and_ancestors, tab_role,
};
use super::plan_lane_fields::{lane_metadata_field, lane_update_field};
use super::plan_linking::{csv_string_field, tags_field};
use super::plan_refs;
use super::plan_role_helpers::assign_task_with_profile;
pub(super) use super::plan_task_authority::authorize_draft_mutation;
use super::plan_task_authority::{task_mutation_authority, verify_owner_epoch_for_task_mutation};
#[cfg(test)]
use super::plan_task_dispatch::dispatch_with_spawn;
use super::plan_task_dispatch::set_local_task_name_description as set_task_description;
use super::{get_string_param, JsonRpcError};
use codeterm_service_core::plan::TaskStatus;
use codeterm_service_core::service::plan::task_queries::TaskListFilter;
use codeterm_service_core::service::plan::{hints, NewTask, PlanError};
use codeterm_service_core::service::ServiceState;
use serde_json::{json, Value};

mod add_fields;
mod respawn;
mod split;
mod task_fields;
use task_fields::task_files_field;

pub(super) fn authenticated_caller(
    params: Option<&Value>,
) -> codeterm_service_core::service::AuthenticatedPlanCaller {
    codeterm_service_core::service::AuthenticatedPlanCaller {
        tab_id: params
            .and_then(|v| v.get("caller_tab_id"))
            .and_then(Value::as_str)
            .map(str::to_string),
        session_id: params
            .and_then(|v| v.get("session_id"))
            .and_then(Value::as_str)
            .map(str::to_string),
        binding_nonce: params
            .and_then(|v| v.get("session_binding_nonce"))
            .and_then(Value::as_str)
            .map(str::to_string),
    }
}

pub(super) fn authorize_execution(
    svc: &ServiceState,
    plan_id: &str,
    params: Option<&Value>,
) -> Result<(), JsonRpcError> {
    svc.authorize_execution_action(plan_id, &authenticated_caller(params))
        .map_err(|message| JsonRpcError {
            code: -32003,
            message,
            data: Some(json!({ "type": "plan_execution_authorization_required" })),
        })
}
fn dependency_vec_field(p: Option<&Value>) -> Result<Option<Vec<String>>, JsonRpcError> {
    let Some(value) = p.and_then(|x| x.get("depends_on").or_else(|| x.get("dependency_task_ids")))
    else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(Some(Vec::new()));
    }
    serde_json::from_value(value.clone())
        .map(Some)
        .map_err(|_| plan_rpc_err(PlanError::BadStatus))
}

fn external_ref_values(p: Option<&Value>) -> Result<Vec<String>, JsonRpcError> {
    let Some(value) = p.and_then(|params| params.get("external_refs")) else {
        return Ok(Vec::new());
    };
    serde_json::from_value(value.clone()).map_err(|error| JsonRpcError {
        code: -32602,
        message: format!("invalid external_refs: expected an array of reference strings: {error}"),
        data: Some(json!({ "type": "invalid_external_ref_input" })),
    })
}

fn validate_task_update_params(p: Option<&Value>) -> Result<(), JsonRpcError> {
    let Some(params) = p else {
        return Ok(());
    };
    codeterm_service_core::plan::task_update_fields::validate_task_update_fields(
        params,
        codeterm_service_core::plan::task_update_fields::TaskUpdateSeam::Socket,
    )
    .map_err(|error| {
        let error_type = if error.unknown_key.is_some() {
            "unknown_task_update_key"
        } else {
            "invalid_task_update_params"
        };
        JsonRpcError {
            code: -32602,
            message: error.to_string(),
            data: Some(json!({
                "type": error_type,
                "key": error.unknown_key,
            })),
        }
    })
}


pub(super) fn handles(method: &str) -> bool {
    matches!(
        method,
        "task.list"
            | "task.show"
            | "task.take"
            | "task.drop"
            | "plan.task.add"
            | "plan.task.split"
            | "plan.task.update"
            | "plan.task.link"
            | "plan.task.relink"
            | "plan.task.assign"
            | "plan.task.dispatch"
            | "plan.task.dispatch_unplanned"
            | "plan.task.merge_remote_lane"
            | "plan.task.remote_ack"
            | "plan.task.remote_report"
            | "plan.task.attempts"
            | "plan.task.attempt.show"
            | "plan.task.respawn"
            | "plan.task.adopt_checkpoint"
            | "plan.task.compete"
            | "plan.task.judge"
            | "plan.task.rm"
    )
}

pub(super) fn dispatch(
    method: &str,
    params: &Option<Value>,
    svc: &ServiceState,
    now: u64,
) -> Result<Value, JsonRpcError> {
    let p = params.as_ref();
    let store = &svc.plan_store;
    if !super::remote_worker_policy::worker_authenticated_method(method) {
        verify_owner_epoch_for_task_mutation(method, params, svc)?;
    } else {
        super::plan_task_authority::verify_remote_worker_binding(params, svc)?;
    }
    match method {
        "plan.task.adopt_checkpoint" => {
            let (plan_id, task_id) = plan_refs::resolve_task(store, params)?;
            let tab_id = get_string_param(params, "caller_tab_id")?;
            let session_id = svc
                .mux
                .read()
                .get_tab(&tab_id)
                .and_then(|tab| tab.agent_session_id.clone())
                .ok_or_else(|| JsonRpcError {
                    code: -32603,
                    message: "adoption caller has no authenticated session".into(),
                    data: None,
                })?;
            let (owner_epoch, attempt_id) = svc
                .adopt_worker_checkpoint_sync(&task_id, &tab_id, &session_id, now)
                .map_err(|message| JsonRpcError {
                    code: -32603,
                    message,
                    data: None,
                })?;
            emit_plan_changed(svc, &plan_id);
            Ok(json!({
                "ok": true,
                "plan_id": plan_id,
                "task_id": task_id,
                "attempt_id": attempt_id,
                "owner_epoch": owner_epoch,
                "tab_id": tab_id,
                "session_id": session_id,
                "state": "awaiting_worker_ack"
            }))
        }
        "task.list" => {
            let caller = p
                .and_then(|x| x.get("caller_tab_id"))
                .and_then(|v| v.as_str());
            let mine = p.and_then(|x| x.get("mine")).and_then(|v| v.as_bool()) == Some(true);
            let require_plan_id = p
                .and_then(|x| x.get("require_plan_id"))
                .and_then(|v| v.as_bool())
                == Some(true);
            let status = p
                .and_then(|x| x.get("status"))
                .and_then(|v| v.as_str())
                .map(|status| {
                    serde_json::from_value::<TaskStatus>(Value::String(status.into()))
                        .map_err(|_| plan_rpc_err(PlanError::BadStatus))
                })
                .transpose()?;
            let plan_id = if require_plan_id {
                Some(plan_refs::resolve_plan(store, params)?)
            } else {
                p.and_then(|x| x.get("plan_id"))
                    .and_then(|v| v.as_str())
                    .map(str::to_string)
            };
            let listed = store.list_tasks_with_reason(TaskListFilter {
                project_path: p
                    .and_then(|x| x.get("project"))
                    .and_then(|v| v.as_str())
                    .map(str::to_string),
                active_only: status.is_none()
                    && p.and_then(|x| x.get("active")).and_then(|v| v.as_bool()) != Some(false),
                mine_tabs: if mine {
                    caller
                        .map(|id| tab_and_ancestors(svc, id))
                        .unwrap_or_default()
                } else {
                    Default::default()
                },
                unassigned: p
                    .and_then(|x| x.get("unassigned"))
                    .and_then(|v| v.as_bool())
                    == Some(true),
                plan_id,
                status,
            });
            Ok(json!(listed))
        }

        "task.show" => {
            let (_pid, task_id) = plan_refs::resolve_task(store, params)?;
            let detail = store
                .show_task(&task_id)
                .ok_or_else(|| plan_rpc_err(PlanError::TaskNotFound))?;
            Ok(serde_json::to_value(detail).unwrap_or_default())
        }

        "task.take" => {
            let (_pid, task_id) = plan_refs::resolve_task(store, params)?;
            let (plan_id, task_id) = svc
                .authorize_task_execution_action(&task_id, &authenticated_caller(p))
                .map_err(|message| JsonRpcError {
                    code: -32003,
                    message,
                    data: None,
                })?;
            let tab_id = get_string_param(params, "caller_tab_id")?;
            let role = tab_role(svc, &tab_id);
            store
                .set_task_assignment(&task_id, Some(&tab_id), role, now)
                .map_err(plan_rpc_err)?;
            let detail = store
                .show_task(&task_id)
                .ok_or_else(|| plan_rpc_err(PlanError::TaskNotFound))?;
            set_task_description(svc, p, &tab_id, &detail.task.name, false)?;
            emit_plan_changed(svc, &plan_id);
            Ok(json!({ "ok": true, "plan_id": plan_id, "task_id": task_id }))
        }

        "task.drop" => {
            let (_pid, task_id) = plan_refs::resolve_task(store, params)?;
            let (plan_id, task_id) = svc
                .authorize_task_execution_action(&task_id, &authenticated_caller(p))
                .map_err(|message| JsonRpcError {
                    code: -32003,
                    message,
                    data: None,
                })?;
            store
                .set_task_assignment(&task_id, None, None, now)
                .map_err(plan_rpc_err)?;
            emit_plan_changed(svc, &plan_id);
            Ok(json!({ "ok": true, "plan_id": plan_id, "task_id": task_id }))
        }

        "plan.task.add" => {
            let id = plan_refs::resolve_plan(store, params)?;
            let normalized = add_fields::normalize(params, store, &id)?;
            let p = Some(&normalized);
            let params = &Some(normalized.clone());
            let name = get_string_param(params, "name")?;
            let kind = p
                .and_then(|x| x.get("type"))
                .or_else(|| p.and_then(|x| x.get("kind")))
                .and_then(|v| v.as_str())
                .unwrap_or("implement")
                .to_string();
            let requested_sections = csv_string_field(p, "sections");
            let section_ids = store
                .resolve_task_section_ids(&id, &requested_sections)
                .map_err(plan_rpc_err)?;
            let author = svc
                .authorize_draft_structure_mutation(&id, &authenticated_caller(p))
                .map_err(|message| JsonRpcError {
                    code: -32003,
                    message,
                    data: None,
                })?;
            codeterm_service_core::service::ensure_planner_task_fields_are_structural(
                &author,
                p.unwrap_or(&Value::Null),
            )
            .map_err(|message| JsonRpcError {
                code: -32003,
                message,
                data: None,
            })?;
            let has_sections = !section_ids.is_empty();
            let has_model = p
                .and_then(|x| x.get("model"))
                .and_then(|v| v.as_str())
                .is_some_and(|s| !s.trim().is_empty());
            let has_why = p
                .and_then(|x| x.get("why"))
                .and_then(|v| v.as_str())
                .is_some_and(|s| !s.trim().is_empty());
            let acceptance_criteria = string_array_field(p, "acceptance_criteria");
            let has_criteria = acceptance_criteria.iter().any(|s| !s.trim().is_empty());
            let test_plan = p
                .and_then(|x| x.get("test_plan"))
                .and_then(|v| v.as_str())
                .map(str::to_string);
            let qa_plan = p
                .and_then(|x| x.get("qa_plan"))
                .and_then(|v| v.as_str())
                .map(str::to_string);
            let has_test_plan = test_plan.as_deref().is_some_and(|s| !s.trim().is_empty());
            let correctness = p
                .and_then(|x| x.get("correctness"))
                .and_then(|v| v.as_str())
                .map(str::to_string);
            let has_correctness = correctness.as_deref().is_some_and(|s| !s.trim().is_empty());
            let external_refs = external_ref_values(p)?;
            let tid = store
                .add_task_with_external_refs(
                    &id,
                    NewTask {
                        files: task_files_field(p)?.unwrap_or_default(),
                        name,
                        kind,
                        section_ids,
                        model: p
                            .and_then(|x| x.get("model"))
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string()),
                        model_rationale: p
                            .and_then(|x| x.get("why"))
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string()),
                        personality: p
                            .and_then(|x| x.get("personality"))
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string()),
                        personality_provenance: p
                            .and_then(|x| x.get("personality_provenance"))
                            .cloned()
                            .map(serde_json::from_value)
                            .transpose()
                            .map_err(|e| JsonRpcError {
                                code: -32602,
                                message: format!("invalid personality_provenance: {e}"),
                                data: None,
                            })?,
                        acceptance_criteria,
                        test_plan,
                        qa_plan,
                        correctness,
                        body: p
                            .and_then(|x| x.get("body"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                        role_guidance: p
                            .and_then(|x| x.get("role_guidance"))
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string()),
                        executor_role: p
                            .and_then(|x| x.get("executor_role"))
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string()),
                        competition: super::plan_competition::metadata_field(p)?,
                        intent: p
                            .and_then(|x| x.get("intent"))
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string()),
                        execution_intent: execution_intent_field(
                            p,
                            store.get(&id).map(|plan| plan.project_path).as_deref(),
                        )?,
                        parent_task_id: p
                            .and_then(|x| x.get("parent"))
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string()),
                        depends_on: dependency_vec_field(p)?.unwrap_or_default(),
                        assigned_tab_id: p
                            .and_then(|x| x.get("assign_tab"))
                            .and_then(|v| v.as_str())
                            .map(str::to_string),
                        lane: lane_metadata_field(p)?,
                        group: p
                            .and_then(|x| x.get("group"))
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string()),
                        priority: p
                            .and_then(|x| x.get("priority"))
                            .and_then(|v| v.as_str())
                            .map(|s| serde_json::from_value(serde_json::Value::String(s.into())))
                            .transpose()
                            .map_err(|_| plan_rpc_err(PlanError::BadPriority))?,
                        handle: p
                            .and_then(|x| x.get("handle"))
                            .and_then(|v| v.as_str())
                            .map(str::to_string),
                        tags: tags_field(p),
                    },
                    &external_refs,
                    now,
                )
                .map_err(plan_rpc_err)?;
            emit_plan_changed(svc, &id);
            let linked = store
                .get(&id)
                .and_then(|plan| {
                    plan.tasks
                        .iter()
                        .find(|task| task.id == tid)
                        .map(|task| task.section_ids.clone())
                })
                .unwrap_or_default();
            let linked_has_sections = !linked.is_empty();
            let mut response_hints = hints::task_create_hints(
                &id,
                &tid,
                has_sections || linked_has_sections,
                has_model,
                has_why,
                has_criteria,
                has_test_plan,
                has_correctness,
            );
            if let Some(hint) = hints::maybe_orchestration_reminder() {
                response_hints.push(hint);
            }
            if let Some(warning) = normalized["inferred_work_warning"].as_str() {
                response_hints.push(warning.to_owned());
            }
            let caller_tab = p
                .and_then(|x| x.get("caller_tab_id"))
                .and_then(|v| v.as_str());
            let mut resp = json!({
                "task_id": tid.clone(),
                "key": store.task_display_key(&id, &tid),
                "section_ids": linked,
                "_hints": response_hints
            });
            if let Some(nudge) = store
                .get(&id)
                .as_ref()
                .and_then(|plan| hints::planner_ownership_nudge(plan, caller_tab))
            {
                resp["_planner_nudge"] = json!(nudge);
            }
            Ok(resp)
        }
        "plan.task.split" => split::dispatch_split(params, svc, now),
        "plan.task.update" => {
            validate_task_update_params(p)?;
            let files = task_files_field(p)?;
            let (id, tid) = plan_refs::resolve_task(store, params)?;
            let previous_name = store
                .show_task(&tid)
                .ok_or_else(|| plan_rpc_err(PlanError::TaskNotFound))?
                .task
                .name;
            let author = svc
                .authorize_draft_structure_mutation(&id, &authenticated_caller(p))
                .map_err(|message| JsonRpcError {
                    code: -32003,
                    message,
                    data: None,
                })?;
            codeterm_service_core::service::ensure_planner_task_fields_are_structural(
                &author,
                p.unwrap_or(&Value::Null),
            )
            .map_err(|message| JsonRpcError {
                code: -32003,
                message,
                data: None,
            })?;
            if p.and_then(|value| value.get("status"))
                .and_then(Value::as_str)
                == Some("pending")
            {
                super::plan_task_dispatch::remote_recovery::recover_remote_attempts(
                    svc, &id, &tid, now, true,
                )?;
            }
            if let Some(files) = files {
                store
                    .update_task_files(&id, &tid, files, now)
                    .map_err(plan_rpc_err)?;
            }
            let sections = p.and_then(|x| x.get("sections")).map(|_| {
                store
                    .resolve_task_section_ids(&id, &csv_string_field(p, "sections"))
                    .map_err(plan_rpc_err)
            });
            let sections = match sections {
                Some(result) => Some(result?),
                None => None,
            };
            // Append by default; Replace only when explicitly requested — codeterm#372.
            let acceptance_criteria = p
                .and_then(|x| x.get("acceptance_criteria"))
                .map(|_| string_array_field(p, "acceptance_criteria"))
                .map(|items| {
                    let replace = p
                        .and_then(|x| x.get("acceptance_criteria_replace"))
                        .and_then(Value::as_bool)
                        .unwrap_or(false);
                    if replace {
                        codeterm_service_core::service::plan::mutations::task_ops::AcceptanceCriteriaUpdate::Replace(items)
                    } else {
                        codeterm_service_core::service::plan::mutations::task_ops::AcceptanceCriteriaUpdate::Append(items)
                    }
                });
            let test_plan = optional_string_field(p, "test_plan");
            let qa_plan = optional_string_field(p, "qa_plan");
            let correctness = optional_string_field(p, "correctness");
            let lane = lane_update_field(p)?;
            let lane = codeterm_service_core::service::lane_removal::clear_vanished_lane_on_cleanup_done(
                store, &id, &tid, lane,
            )
            .map_err(|error| {
                let (code, message) = match error {
                    codeterm_service_core::service::lane_removal::CleanupStatusDoneError::Refused(message) => {
                        (-32602, message)
                    }
                    codeterm_service_core::service::lane_removal::CleanupStatusDoneError::Verification(message) => {
                        (-32603, message)
                    }
                };
                JsonRpcError {
                    code,
                    message,
                    data: None,
                }
            })?;
            let external_refs = external_ref_values(p)?;
            codeterm_service_core::service::plan::external_refs::canonicalize_external_refs(
                &external_refs,
            )
            .map_err(plan_rpc_err)?;
            if let Some(depends_on) = dependency_vec_field(p)? {
                store
                    .update_task_dependencies(&id, &tid, depends_on, now)
                    .map_err(plan_rpc_err)?;
            }
            store
                .update_task_with_lane(
                    &id,
                    &tid,
                    p.and_then(|x| x.get("name")).and_then(|v| v.as_str()),
                    p.and_then(|x| x.get("type"))
                        .or_else(|| p.and_then(|x| x.get("kind")))
                        .and_then(|v| v.as_str()),
                    optional_string_field(p, "model"),
                    optional_string_field(p, "why"),
                    p.and_then(|x| x.get("status")).and_then(|v| v.as_str()),
                    optional_string_field(p, "status_label"),
                    p.and_then(|x| x.get("priority")).and_then(|v| v.as_str()),
                    p.and_then(|x| x.get("body")).and_then(|v| v.as_str()),
                    optional_string_field(p, "intent"),
                    optional_string_field(p, "executor_role"),
                    sections,
                    p.and_then(|x| x.get("parent_task_id").or_else(|| x.get("parent")))
                        .map(|v| v.as_str()),
                    optional_string_field(p, "group"),
                    &tags_field(p),
                    p.and_then(|x| x.get("primary")).and_then(|v| v.as_str()),
                    p.and_then(|x| x.get("handle")).and_then(|v| v.as_str()),
                    lane,
                    now,
                )
                .map_err(plan_rpc_err)?;
            if let Some(speed) = optional_string_field(p, "speed") {
                store
                    .update_task_speed(&id, &tid, speed, now)
                    .map_err(plan_rpc_err)?;
            }
            super::plan_role_helpers::update_role_guidance_if_present(store, &id, &tid, p, now)?;
            super::plan_role_helpers::update_personality_if_present(store, &id, &tid, p, now)?;
            if let Some(competition) = super::plan_competition::update_field(p)? {
                store
                    .update_task_competition(&id, &tid, competition, now)
                    .map_err(plan_rpc_err)?;
            }
            let acceptance_criteria_is_replace = matches!(
                acceptance_criteria.as_ref(),
                Some(
                    codeterm_service_core::service::plan::mutations::task_ops::AcceptanceCriteriaUpdate::Replace(_)
                )
            );
            let criteria_counts = if acceptance_criteria.is_some()
                || test_plan.is_some()
                || qa_plan.is_some()
                || correctness.is_some()
            {
                store
                    .update_task_quality(
                        &id,
                        &tid,
                        acceptance_criteria,
                        test_plan,
                        qa_plan,
                        correctness,
                        now,
                    )
                    .map_err(plan_rpc_err)?
            } else {
                None
            };
            if !external_refs.is_empty() {
                store
                    .add_task_external_refs(&id, &tid, &external_refs, now)
                    .map_err(plan_rpc_err)?;
            }
            let updated_task = store
                .show_task(&tid)
                .ok_or_else(|| plan_rpc_err(PlanError::TaskNotFound))?
                .task;
            if previous_name != updated_task.name {
                if let Some(tab_id) = updated_task.assigned_tab_id.as_deref() {
                    set_task_description(svc, p, tab_id, &updated_task.name, true)?;
                }
            }
            emit_plan_changed(svc, &id);
            let caller_tab = p
                .and_then(|x| x.get("caller_tab_id"))
                .and_then(|v| v.as_str());
            let mut resp = json!({ "ok": true });
            // Reported only for the destructive path — an append that grows
            // the array by exactly its own item count needs no confirmation.
            if acceptance_criteria_is_replace {
                if let Some(counts) = criteria_counts {
                    resp["acceptance_criteria_counts"] = json!(counts);
                }
            }
            if let Some(nudge) = store
                .get(&id)
                .as_ref()
                .and_then(|plan| hints::planner_ownership_nudge(plan, caller_tab))
            {
                resp["_planner_nudge"] = json!(nudge);
            }
            Ok(resp)
        }

        "plan.task.link" => {
            let (id, tid) = plan_refs::resolve_task(store, params)?;
            let sections = csv_string_field(p, "sections");
            let linked = store
                .link_task_sections(&id, &tid, &sections, now)
                .map_err(plan_rpc_err)?;
            emit_plan_changed(svc, &id);
            Ok(json!({ "ok": true, "section_ids": linked }))
        }

        "plan.task.relink" => {
            let id = plan_refs::resolve_plan(store, params)?;
            let apply = p.and_then(|x| x.get("apply")).and_then(|v| v.as_bool()) == Some(true);
            let report = store
                .relink_unlinked_tasks(&id, apply, now)
                .map_err(plan_rpc_err)?;
            if apply && !report.applied.is_empty() {
                emit_plan_changed(svc, &id);
            }
            Ok(serde_json::to_value(report).unwrap_or_default())
        }

        "plan.task.assign" => {
            let (id, task_id) = plan_refs::resolve_task(store, params)?;
            authorize_execution(svc, &id, p)?;
            let owner_epoch = store
                .show_task(&task_id)
                .map(|detail| detail.task.interruption.owner_epoch)
                .unwrap_or(0);
            let id = if owner_epoch > 0 {
                let authority = task_mutation_authority(params, svc)?;
                let mux = svc.mux.read();
                let authenticated = mux.get_tab(&authority.tab_id).is_some_and(|tab| {
                    tab.agent_session_id.as_deref() == Some(&authority.session_id)
                        && tab.session_binding_nonce.as_deref()
                            == Some(&authority.session_binding_nonce)
                        && tab.reported_session_binding_nonce.as_deref()
                            == Some(&authority.session_binding_nonce)
                });
                if !authenticated {
                    return Err(JsonRpcError {
                        code: -32603,
                        message: "task mutation caller binding is stale or rebound".into(),
                        data: None,
                    });
                }
                let id = assign_task_with_profile(store, svc, params, p, Some(&authority), now)?;
                drop(mux);
                id
            } else {
                assign_task_with_profile(store, svc, params, p, None, now)?
            };
            let tab_id = get_string_param(params, "tab")?;
            let detail = store
                .show_task(&task_id)
                .ok_or_else(|| plan_rpc_err(PlanError::TaskNotFound))?;
            set_task_description(svc, p, &tab_id, &detail.task.name, false)?;
            emit_plan_changed(svc, &id);
            Ok(json!({ "ok": true }))
        }
        "plan.task.dispatch" => super::plan_task_dispatch::dispatch(params, svc, now),
        "plan.task.merge_remote_lane" => {
            super::plan_task_dispatch::remote::merge_lane(params, svc, now)
        }
        "plan.task.remote_ack" => {
            let (plan_id, task_id) = plan_refs::resolve_task(store, params)?;
            let attempt_id = get_string_param(params, "attempt_id")?;
            let worker_tab_id = get_string_param(params, "worker_tab_id")?;
            let peer_id = get_string_param(params, "authenticated_mesh_peer_id").map_err(|_| {
                JsonRpcError {
                    code: -32003,
                    message: "remote ACK requires authenticated mesh transport".into(),
                    data: None,
                }
            })?;
            let acknowledged = store
                .acknowledge_remote_dispatch(
                    &plan_id,
                    &task_id,
                    &attempt_id,
                    &peer_id,
                    &worker_tab_id,
                    now,
                )
                .map_err(plan_rpc_err)?;
            svc.acknowledge_remote_worker_ack(
                &plan_id,
                &task_id,
                &attempt_id,
                &peer_id,
                &worker_tab_id,
            );
            emit_plan_changed(svc, &plan_id);
            Ok(
                json!({"acknowledged": acknowledged, "plan_id": plan_id, "task_id": task_id, "attempt_id": attempt_id}),
            )
        }
        "plan.task.remote_report" => {
            let (plan_id, task_id) = plan_refs::resolve_task(store, params)?;
            let body = params.as_ref().ok_or_else(|| JsonRpcError {
                code: -32602,
                message: "remote report params required".into(),
                data: None,
            })?;
            let attempt_id = get_string_param(params, "attempt_id")?;
            let peer_id = get_string_param(params, "authenticated_mesh_peer_id").map_err(|_| {
                JsonRpcError {
                    code: -32003,
                    message: "remote report requires authenticated mesh transport".into(),
                    data: None,
                }
            })?;
            let report: codeterm_service_core::plan::TaskExecutionReportSnapshot =
                serde_json::from_value(body.get("report").cloned().ok_or_else(|| {
                    JsonRpcError {
                        code: -32602,
                        message: "remote report snapshot is required".into(),
                        data: None,
                    }
                })?)
                .map_err(|error| JsonRpcError {
                    code: -32602,
                    message: format!("invalid remote report snapshot: {error}"),
                    data: None,
                })?;
            if report.id.trim().is_empty() {
                return Err(JsonRpcError {
                    code: -32602,
                    message: "remote report id is required for idempotency".into(),
                    data: None,
                });
            }
            if report.status.as_deref() != Some("in_review") {
                return Err(JsonRpcError {
                    code: -32602,
                    message: "remote task reports must enter in_review for origin acceptance"
                        .into(),
                    data: None,
                });
            }
            let reported = store
                .ingest_remote_dispatch_report(
                    &plan_id,
                    &task_id,
                    &attempt_id,
                    &peer_id,
                    report,
                    now,
                )
                .map_err(plan_rpc_err)?;
            emit_plan_changed(svc, &plan_id);
            Ok(
                json!({"reported": reported, "plan_id": plan_id, "task_id": task_id, "attempt_id": attempt_id}),
            )
        }
        "plan.task.dispatch_unplanned" => {
            super::plan_task_dispatch::dispatch_unplanned(params, svc, now)
        }
        "plan.task.attempts" => {
            let (id, tid) = plan_refs::resolve_task(store, params)?;
            let attempts = store.list_task_attempts(&id, &tid).map_err(plan_rpc_err)?;
            Ok(json!({ "plan_id": id, "task_id": tid, "attempts": attempts }))
        }
        "plan.task.attempt.show" => {
            let (id, tid) = plan_refs::resolve_task(store, params)?;
            let attempt_id = get_string_param(params, "attempt_id")?;
            let attempt = store
                .show_task_attempt(&id, &tid, &attempt_id)
                .map_err(plan_rpc_err)?;
            Ok(json!({ "plan_id": id, "task_id": tid, "attempt": attempt }))
        }

        "plan.task.respawn" => respawn::respawn(params, svc, now),

        "plan.task.compete" => super::plan_competition::compete(params, svc, now),

        "plan.task.judge" => super::plan_competition::judge(params, svc, now),

        "plan.task.rm" => {
            let all = p.and_then(|x| x.get("all")).and_then(|v| v.as_bool()) == Some(true);
            let (id, count) = if all {
                let id = plan_refs::resolve_plan(store, params)?;
                authorize_draft_mutation(svc, &id, p)?;
                let count = store.rm_all_tasks(&id, now).map_err(plan_rpc_err)?;
                (id, count)
            } else {
                let (id, tid) = plan_refs::resolve_task(store, params)?;
                authorize_draft_mutation(svc, &id, p)?;
                store.rm_task(&id, &tid, now).map_err(plan_rpc_err)?;
                (id, 1)
            };
            emit_plan_changed(svc, &id);
            Ok(json!({ "ok": true, "removed": count }))
        }

        other => Err(JsonRpcError {
            code: -32601,
            message: format!("Method not found: {other}"),
            data: None,
        }),
    }
}

#[cfg(test)]
mod task_key_contract_tests;
#[cfg(test)]
mod tests;
