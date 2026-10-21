use crate::{
    amend::{DefinitionSnapshot, PreparedUpdate},
    blueprint_pipeline::BlueprintExt,
    scheduler::Handle,
    service_adapter::{SentinelCaller, SentinelExt},
};
use codeterm_blueprint_types::blueprint_pipeline::BlueprintSpec;
use codeterm_service_core::service::ServiceContext;
use serde_json::{json, Value};

pub trait BlueprintApplyExt {
    fn blueprint_apply(
        &self,
        caller: &SentinelCaller,
        definition: Value,
        dry_run: bool,
    ) -> Result<Value, String>;
    fn blueprint_apply_with_state(
        &self,
        caller: &SentinelCaller,
        definition: Value,
        dry_run: bool,
        stopped: Option<bool>,
    ) -> Result<Value, String>;
}
impl BlueprintApplyExt for ServiceContext {
    fn blueprint_apply(
        &self,
        caller: &SentinelCaller,
        definition: Value,
        dry_run: bool,
    ) -> Result<Value, String> {
        self.blueprint_apply_with_state(caller, definition, dry_run, None)
    }
    fn blueprint_apply_with_state(
        &self,
        caller: &SentinelCaller,
        definition: Value,
        dry_run: bool,
        stopped: Option<bool>,
    ) -> Result<Value, String> {
        if caller.team.is_none() {
            return Err("Blueprint apply requires a caller in a Team".into());
        }
        let document =
            codeterm_blueprint_types::blueprint_document::Document::from_definition(&definition)?;
        let errors = codeterm_blueprint_types::blueprint_document::definition_errors(&definition);
        if !errors.is_empty() {
            return Err(errors
                .into_iter()
                .map(|(field, message)| format!("{field}: {message}"))
                .collect::<Vec<_>>()
                .join("; "));
        }
        let handle = self
            .host_extension::<Handle>()
            .ok_or("blueprints not initialized")?;
        let _apply = handle.apply_lock.lock();
        let current = {
            let store = handle.store.lock();
            store.ensure_available()?;
            let matches = store
                .jobs
                .iter()
                .filter(|job| {
                    job.workspace_id == caller.workspace
                        && job.team_id == caller.team
                        && job.name.as_deref() == Some(document.name.as_str())
                })
                .cloned()
                .collect::<Vec<_>>();
            if matches.len() > 1 {
                return Err(
                    "Ambiguous Blueprint name in this Team; delete or rename duplicates".into(),
                );
            }
            matches.into_iter().next()
        };
        if let Some(current) = &current {
            if !caller.can_amend(current) {
                return Err("Blueprint exists but caller cannot amend it".into());
            }
        }
        let spec = definition
            .get("spec")
            .map(|value| serde_json::from_value::<BlueprintSpec>(value.clone()))
            .transpose()
            .map_err(|error| error.to_string())?;
        if let Some(spec) = &spec {
            let preview = self.blueprint_pipeline_validate(caller, spec.clone())?;
            if let Some(check) = preview["checks"]
                .as_array()
                .and_then(|checks| checks.iter().find(|check| check["passed"] == false))
            {
                return Err(format!("{}: {}", check["field"], check["reason"]));
            }
            if current.as_ref().is_some_and(|job| job.pipeline().is_none()) {
                return Err("A watch cannot be replaced by a pipeline under the same name".into());
            }
        } else if current.as_ref().is_some_and(|job| job.pipeline().is_some()) {
            return Err("A pipeline cannot be replaced by a watch under the same name".into());
        }
        let mut prepared_watch = if spec.is_none() {
            let create = serde_json::from_value(definition["create"].clone())
                .map_err(|error| error.to_string())?;
            Some(self.prepare_sentinel_create(caller, create)?)
        } else {
            None
        };
        if let (Some(current), Some(next)) = (&current, &prepared_watch) {
            match (&current.creation_options,&next.creation_options) {
                (Some(before),Some(after))=> {
                    for (field,changed) in [("timeout",before.timeout_ms!=after.timeout_ms),("paused",before.paused!=after.paused)] {
                        if changed {return Err(format!("blueprint_creation_only:watch.{field}:applies only at creation; delete and re-apply"))}
                    }
                },
                _=>return Err("blueprint_creation_only:watch.timeout:original creation settings are unavailable; delete and re-apply".into()),
            }
        }
        let before = current.as_ref().map(crate::portable::definition);
        let unchanged = match (&current, &spec, &prepared_watch) {
            (Some(job), Some(spec), _) => job.pipeline() == Some(spec),
            (Some(job), _, Some(next)) => {
                DefinitionSnapshot::from_job(job) == DefinitionSnapshot::from_job(next)
            }
            _ => false,
        };
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
        let mut result = json!({"action":action,"name":document.name,"id":current.as_ref().map(|job|&job.id),"dry_run":dry_run,"before":before,"after":definition,"state":current.as_ref().map(|job|if job.machine.stopped{"stopped"}else if job.machine.paused{"paused"}else{"running"})});
        if dry_run {
            return Ok(result);
        }
        if unchanged && !reactivate {
            if stopped == Some(true) {
                if let Some(job) = &current {
                    if !job.machine.stopped {
                        self.sentinel_stop(caller, &job.id)?;
                        result["state"] = json!("stopped");
                    }
                }
            }
            return Ok(result);
        }
        let id = match (current, spec) {
            (None, Some(spec)) => {
                self.blueprint_pipeline_create_stopped(caller, spec, stopped.unwrap_or(false))?
                    ["id"]
                    .as_str()
                    .ok_or("Missing created Blueprint ID")?
                    .to_owned()
            }
            (None, None) => {
                self.sentinel_create_stopped(
                    caller,
                    serde_json::from_value(definition["create"].clone())
                        .map_err(|e| e.to_string())?,
                    stopped.unwrap_or(false),
                )?
                .id
            }
            (Some(job), Some(spec)) => {
                self.blueprint_pipeline_update(caller, &job.id, job.definition_version, spec, None)?
                    .id
            }
            (Some(job), None) => {
                let mut next = prepared_watch.take().ok_or("Missing prepared watch")?;
                if caller.tab != job.creator_tab_id
                    && (next.probe() != job.probe() || crate::amend::has_new_command(&job, &next))
                {
                    return Err("Only the Blueprint creator may introduce new command text".into());
                }
                next.id = job.id.clone();
                next.creator_tab_id = job.creator_tab_id.clone();
                next.cwd = job.cwd.clone();
                let now = handle.store.lock().clock.now_ms();
                let staged = {
                    let mux = self.mux.read();
                    if !crate::service_adapter::caller_is_live(&mux, caller) {
                        return Err("Blueprint caller scope changed".into());
                    }
                    handle.store.lock().stage_apply_definition(
                        job.definition_version,
                        &caller.tab,
                        now,
                        PreparedUpdate {
                            job: next,
                            cause_transition_id: None,
                            note: None,
                        },
                    )
                };
                staged.settle(&handle.store)?.id
            }
        };
        if stopped == Some(true)
            && handle
                .store
                .lock()
                .get(&id)
                .is_some_and(|job| !job.machine.stopped)
        {
            self.sentinel_stop(caller, &id)?;
        }
        handle.notify();
        let job = {
            handle
                .store
                .lock()
                .get(&id)
                .cloned()
                .ok_or("Applied Blueprint disappeared")?
        };
        result["state"] = json!(if job.machine.stopped {
            "stopped"
        } else if job.machine.paused {
            "paused"
        } else {
            "running"
        });
        result["id"] = json!(id);
        Ok(result)
    }
}
