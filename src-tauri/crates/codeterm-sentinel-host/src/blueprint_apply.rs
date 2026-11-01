use crate::{
    amend::{DefinitionSnapshot, PreparedUpdate},
    blueprint_pipeline::BlueprintExt,
    scheduler::Handle,
    service_adapter::{SentinelCaller, SentinelExt},
    storage::{self, Staged},
    store::{Sentinel, Store},
};
use codeterm_blueprint_types::{
    blueprint_contract::{ApplyError, FieldCheck},
    blueprint_pipeline::BlueprintSpec,
};
use codeterm_service_core::service::ServiceContext;
use serde_json::{json, Value};

mod admission;
pub(crate) mod mux;

pub trait BlueprintApplyExt {
    fn blueprint_apply_document(
        &self,
        caller: &SentinelCaller,
        definition: Value,
        dry_run: bool,
        stopped: Option<bool>,
        update: Option<(String, u64, Option<String>)>,
    ) -> Result<Value, ApplyError>;

    fn blueprint_apply(
        &self,
        caller: &SentinelCaller,
        definition: Value,
        dry_run: bool,
    ) -> Result<Value, ApplyError>;
    fn blueprint_apply_with_state(
        &self,
        caller: &SentinelCaller,
        definition: Value,
        dry_run: bool,
        stopped: Option<bool>,
    ) -> Result<Value, ApplyError>;
    /// Applies a definition bound from saved recipe `recipe`, which is never its own duplicate.
    fn blueprint_apply_recipe_document(
        &self,
        caller: &SentinelCaller,
        definition: Value,
        dry_run: bool,
        recipe: &str,
    ) -> Result<Value, ApplyError>;
}
impl BlueprintApplyExt for ServiceContext {
    fn blueprint_apply_recipe_document(
        &self,
        caller: &SentinelCaller,
        definition: Value,
        dry_run: bool,
        recipe: &str,
    ) -> Result<Value, ApplyError> {
        apply_document(self, caller, definition, dry_run, None, None, Some(recipe))
    }
    fn blueprint_apply(
        &self,
        caller: &SentinelCaller,
        definition: Value,
        dry_run: bool,
    ) -> Result<Value, ApplyError> {
        self.blueprint_apply_with_state(caller, definition, dry_run, None)
    }
    fn blueprint_apply_with_state(
        &self,
        caller: &SentinelCaller,
        definition: Value,
        dry_run: bool,
        stopped: Option<bool>,
    ) -> Result<Value, ApplyError> {
        self.blueprint_apply_document(caller, definition, dry_run, stopped, None)
    }
    fn blueprint_apply_document(
        &self,
        caller: &SentinelCaller,
        definition: Value,
        dry_run: bool,
        stopped: Option<bool>,
        update: Option<(String, u64, Option<String>)>,
    ) -> Result<Value, ApplyError> {
        apply_document(self, caller, definition, dry_run, stopped, update, None)
    }
}

fn apply_document(
    service: &ServiceContext,
    caller: &SentinelCaller,
    definition: Value,
    dry_run: bool,
    stopped: Option<bool>,
    update: Option<(String, u64, Option<String>)>,
    recipe: Option<&str>,
) -> Result<Value, ApplyError> {
    {
        if let Some(refusal) = service.tab_write_lock(&caller.tab) {
            return Err(refusal.to_string().into());
        }
        let mut document: codeterm_blueprint_types::blueprint_document::Document =
            serde_json::from_value(definition.clone()).map_err(|error| ApplyError::Validation {
                checks: vec![FieldCheck::new("", Err(error.to_string()))],
            })?;
        let errors = document.template_errors();
        if !errors.is_empty() {
            return Err(ApplyError::Validation {
                checks: errors
                    .into_iter()
                    .map(|(field, message)| FieldCheck::new(field, Err(message)))
                    .collect(),
            });
        }
        codeterm_sentinel::seal_code::document(&mut document, std::path::Path::new(&caller.cwd))?;
        let document = crate::blueprint_calls::pin_document(service, caller, document)?;
        let definition = serde_json::to_value(&document).map_err(|error| error.to_string())?;
        let handle = service
            .host_extension::<Handle>()
            .ok_or("blueprints not initialized")?;
        let _apply = handle.apply_lock.lock();
        use codeterm_blueprint_types::blueprint_document::checker::DuplicateOracle;
        let closure =
            codeterm_blueprint_types::blueprint_document::calls::FrozenClosure::from_document(
                document.clone(),
            )?;
        let mut context = codeterm_blueprint_types::blueprint_document::calls::check_context(
            &closure,
            &Default::default(),
        );
        context.duplicates = admission::ScopedDuplicates {
            service,
            workspace: &caller.workspace,
            team: &caller.team,
            updating: update.as_ref().map(|(id, _, _)| id.as_str()),
            recipe,
        }
        .compare(&document);
        let ir = codeterm_blueprint_types::blueprint_document::projection::PositionedIr {
            canonical: codeterm_blueprint_types::blueprint_document::projection::canonical(
                &document,
            ),
            locations: std::collections::BTreeMap::new(),
        };
        let diagnostics =
            codeterm_blueprint_types::blueprint_document::checker::check_ir(&ir, &context);
        let checks = diagnostics
            .iter()
            .filter(|d| d.severity == "error")
            .map(|d| {
                FieldCheck::new(
                    d.field.clone(),
                    Err(
                        codeterm_blueprint_types::blueprint_document::checker::render(
                            std::slice::from_ref(d),
                            true,
                        ),
                    ),
                )
            })
            .collect::<Vec<_>>();
        if !checks.is_empty() {
            return Err(ApplyError::Validation { checks });
        }
        if document.kind == codeterm_blueprint_types::blueprint_document::Kind::Pipeline
            && document.trigger.is_none()
            && document.result.port.is_some()
        {
            if dry_run {
                let call_drift =
                    crate::blueprint_calls::call_drift(service, caller, document.clone())?;
                return Ok(
                    json!({"action":"check", "dry_run":true,"after":definition,"diagnostics":diagnostics,"call_drift":call_drift}),
                );
            }
            return Err(
                "Callable Blueprints are saved with `blueprint keep --file`; reuse them via call"
                    .into(),
            );
        }
        let call_drift = serde_json::to_value(crate::blueprint_calls::call_drift(
            service,
            caller,
            document.clone(),
        )?)
        .map_err(|error| error.to_string())?;
        let curator = service
            .role_policy_for_tab(&caller.tab)
            .is_some_and(|policy| policy.retarget_only());
        let authorized_id = if curator {
            let id = handle
                .store
                .lock()
                .jobs
                .iter()
                .find(|job| {
                    job.workspace_id == caller.workspace
                        && job.team_id == caller.team
                        && job.name.as_deref() == Some(&document.name)
                })
                .map(|job| job.id.clone());
            service.authorize_role_blueprint(
                &caller.tab,
                id.as_deref(),
                if id.is_some() {
                    codeterm_service_core::service::role_policy::BlueprintAction::Apply
                } else {
                    codeterm_service_core::service::role_policy::BlueprintAction::Create
                },
            )?;
            id
        } else {
            None
        };
        let runtime = document.lower().map_err(|errors| ApplyError::Validation {
            checks: errors
                .into_iter()
                .map(|(field, message)| FieldCheck::new(field, Err(message)))
                .collect(),
        })?;
        let spec = runtime
            .get("spec")
            .map(|value| serde_json::from_value::<BlueprintSpec>(value.clone()))
            .transpose()
            .map_err(|error| error.to_string())?;
        let mut candidate = if let Some(spec) = spec {
            let preview = service.blueprint_pipeline_validate(caller, spec.clone())?;
            let spec = preview.normalized_spec;
            let checks = preview
                .checks
                .into_iter()
                .filter(|check| !check.passed)
                .collect::<Vec<_>>();
            if !checks.is_empty() {
                return Err(ApplyError::Validation { checks });
            }
            handle.store.lock().prepare_pipeline(
                spec,
                crate::store::Owner {
                    creator_tab_id: caller.tab.clone(),
                    team_id: caller.team.clone(),
                    workspace_id: caller.workspace.clone(),
                    cwd: caller.cwd.clone(),
                },
                true,
            )?
        } else {
            service.prepare_sentinel_create(
                caller,
                serde_json::from_value(runtime["create"].clone())
                    .map_err(|error| error.to_string())?,
            )?
        };
        candidate.document = Some(definition.clone());
        with_apply_mux(service, caller, candidate, |mux, mut candidate| {
            crate::plan_lock::refuse(service, &candidate)?;
            validate_candidate_scope(mux, caller, &candidate)?;
            let tab = mux.get_tab(&caller.tab).ok_or("Blueprint caller is gone")?;
            candidate.agent_authored = stopped.is_some()
                || (tab.agent_type.is_some() && tab.session_binding_nonce.is_some());
            let mut result = storage::commit(&handle.store, move |store| {
                let staged = stage_apply(
                    store,
                    caller,
                    ApplyIntent {
                        candidate,
                        definition,
                        update,
                        dry_run,
                        stopped,
                        curator,
                        authorized_id,
                    },
                );
                match staged {
                    Ok(staged) => staged.map(Ok),
                    Err(error) => Staged::ready(Ok(Err(error))),
                }
            })??;
            result["call_drift"] = call_drift;
            result["diagnostics"] =
                serde_json::to_value(diagnostics).map_err(|error| error.to_string())?;
            handle.notify();
            if !dry_run && result["action"] == "create" {
                service.record_product_feature(codeterm_analytics::Feature::BlueprintCreated);
            }
            Ok(result)
        })
    }
}

fn validate_candidate_scope(
    mux: &dyn mux::ScopeView,
    caller: &SentinelCaller,
    candidate: &Sentinel,
) -> Result<(), ApplyError> {
    if !crate::service_adapter::caller_is_live_in_view(mux, caller) {
        return Err("Blueprint caller scope changed".into());
    }
    if let Some(spec) = candidate.pipeline() {
        crate::blueprint_pipeline::validate_result_scope(mux, candidate, &spec.result.to)?;
    } else {
        if let Some(crate::source::TabBinding::Tab(tab)) = candidate.observe() {
            let observed = mux.get_tab(tab).ok_or("observation tab not found")?;
            crate::source::validate_tab_scope_metadata(
                observed,
                mux.find_workspace_for_tab(tab).as_deref(),
                candidate,
            )?;
        }
        crate::service_adapter::validate_send_scope_in_view(mux, candidate)?;
    }
    Ok(())
}

fn with_apply_mux<T>(
    service: &ServiceContext,
    caller: &SentinelCaller,
    candidate: Sentinel,
    admit: impl FnOnce(&dyn mux::ScopeView, Sentinel) -> Result<T, ApplyError>,
) -> Result<T, ApplyError> {
    let references = mux::References::collect(caller, &candidate);
    let snapshot = {
        let guard = service.mux.read();
        mux::Snapshot::capture(&guard, caller, references)
    };
    admit(&snapshot, candidate)
}

struct ApplyIntent {
    update: Option<(String, u64, Option<String>)>,
    candidate: Sentinel,
    definition: Value,
    dry_run: bool,
    stopped: Option<bool>,
    curator: bool,
    authorized_id: Option<String>,
}
fn stage_apply(
    store: &mut Store,
    caller: &SentinelCaller,
    intent: ApplyIntent,
) -> Result<Staged<Value>, ApplyError> {
    let ApplyIntent {
        update,
        mut candidate,
        definition,
        dry_run,
        stopped,
        curator,
        authorized_id,
    } = intent;
    store.ensure_available()?;
    let mut matches = store.jobs.iter().filter(|job| {
        job.workspace_id == caller.workspace
            && job.team_id == caller.team
            && update
                .as_ref()
                .map_or(job.name == candidate.name, |(id, _, _)| &job.id == id)
    });
    let current = matches.next().cloned();
    if matches.next().is_some() {
        return Err("Ambiguous Blueprint name in this Team; delete or rename duplicates".into());
    }
    if let Some((_, version, _)) = &update {
        let job = current
            .as_ref()
            .ok_or("Blueprint update target not found in this Team")?;
        if job.definition_version != *version {
            return Err(format!(
                "blueprint_version_conflict: current_version={}",
                job.definition_version
            )
            .into());
        }
    }
    if let Some(job) = &current {
        if !caller.can_amend(job) {
            return Err("Blueprint exists but caller cannot amend it".into());
        }
        if job.pipeline().is_some() != candidate.pipeline().is_some() {
            return Err(
                "A watch and a pipeline cannot replace each other under the same name".into(),
            );
        }
        if let (Some(before), Some(after)) = (job.pipeline(), candidate.pipeline()) {
            if before.owner != after.owner {
                return Err("owner: pipeline owner is immutable".into());
            }
        }
        if job.pipeline().is_none() {
            let Some(before) = &job.creation_options else {
                return Err(ApplyError::CreationOnly {
                    field: "steps[0].timeout".into(),
                    message: "original creation settings are unavailable; delete and re-apply"
                        .into(),
                });
            };
            let after = candidate
                .creation_options
                .as_ref()
                .ok_or("Missing prepared creation settings")?;
            for (field, changed) in [
                ("timeout", before.timeout_ms != after.timeout_ms),
                ("paused", before.paused != after.paused),
            ] {
                if changed {
                    return Err(ApplyError::CreationOnly {
                        field: format!("steps[0].{field}"),
                        message: "applies only at creation; delete and re-apply".into(),
                    });
                }
            }
            if caller.tab != job.creator_tab_id
                && (candidate.probe() != job.probe()
                    || crate::amend::has_new_command(job, &candidate))
            {
                return Err("Only the Blueprint creator may introduce new command text".into());
            }
        }
    }
    let before = current.as_ref().map(crate::portable::definition);
    if curator
        && (current.as_ref().map(|job| job.id.as_str()) != authorized_id.as_deref()
            || before
                .as_ref()
                .is_none_or(|before| !retarget_only(before, &definition)))
    {
        return Err("Curator amendments may only retarget an existing granted Blueprint".into());
    }
    let unchanged = current.as_ref().is_some_and(|job| {
        DefinitionSnapshot::from_job(job) == DefinitionSnapshot::from_job(&candidate)
    });
    let reactivate = unchanged
        && current.as_ref().is_some_and(|job| job.machine.stopped)
        && stopped != Some(true);
    let action = if reactivate {
        "reactivate"
    } else if unchanged {
        "unchanged"
    } else if current.is_some() {
        "update"
    } else {
        "create"
    };
    let result = json!({"action":action,"name":candidate.name,"id":current.as_ref().map(|job|&job.id),"dry_run":dry_run,"before":before,"after":definition,"state":current.as_ref().map(state)});
    if dry_run {
        return Ok(Staged::ready(Ok(result)));
    }
    let staged = match current {
        Some(job) if unchanged && !reactivate => {
            if stopped == Some(true) && !job.machine.stopped {
                store.stage_stop(&job.id, "stopped_by_creator")
            } else {
                Staged::ready(Ok(job))
            }
        }
        Some(job) => {
            candidate.id = job.id.clone();
            candidate.creator_tab_id = job.creator_tab_id.clone();
            candidate.cwd = job.cwd.clone();
            store.stage_apply_definition(
                job.definition_version,
                &caller.tab,
                store.clock.now_ms(),
                PreparedUpdate {
                    job: candidate,
                    cause_transition_id: update.and_then(|(_, _, cause)| cause),
                    note: None,
                },
                stopped,
            )
        }
        None => {
            candidate.machine.stopped = stopped.unwrap_or(false);
            store.stage_insert(candidate)
        }
    };
    Ok(staged.map(move |job| {
        let mut result = result;
        result["id"] = json!(job.id);
        result["definition_version"] = json!(job.definition_version);
        result["state"] = json!(state(&job));
        result
    }))
}
fn state(job: &Sentinel) -> &'static str {
    if job.machine.stopped {
        "stopped"
    } else if job.machine.paused {
        "paused"
    } else {
        "running"
    }
}

pub(crate) fn retarget_only(before: &Value, after: &Value) -> bool {
    fn without_targets(value: &Value) -> Value {
        let mut value = value.clone();
        if let Some(result) = value.get_mut("result").and_then(Value::as_object_mut) {
            result.remove("to");
        }
        for field in ["steps", "other"] {
            if let Some(nodes) = value.get_mut(field).and_then(Value::as_array_mut) {
                for node in nodes {
                    let key = match node.get("uses").and_then(Value::as_str) {
                        Some("observe" | "terminal") => Some("id"),
                        Some("send" | "cron" | "blueprint" | "escalate" | "respond_prompt") => {
                            Some("target")
                        }
                        _ => None,
                    };
                    if let Some(key) = key {
                        if let Some(params) = node.get_mut("with").and_then(Value::as_object_mut) {
                            params.remove(key);
                        }
                    }
                }
            }
        }
        value
    }

    without_targets(before) == without_targets(after)
}

#[cfg(test)]
mod tests;
