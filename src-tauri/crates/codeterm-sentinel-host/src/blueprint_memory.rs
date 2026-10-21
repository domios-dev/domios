use crate::{
    scheduler::Handle,
    service_adapter::{caller_is_live, SentinelCaller},
    BlueprintApplyExt,
};
use codeterm_blueprint_types::blueprint_memory::Evidence;
use codeterm_service_core::service::ServiceContext;
use serde_json::{json, Value};

fn area(workspace: &str) -> String {
    format!("blueprints:{workspace}")
}

pub trait BlueprintMemoryExt {
    fn blueprint_keep(
        &self,
        caller: &SentinelCaller,
        id: &str,
        when: &str,
    ) -> Result<Value, String>;
    fn blueprint_recall(&self, caller: &SentinelCaller, intent: &str) -> Result<Value, String>;
    fn blueprint_apply_recipe(
        &self,
        caller: &SentinelCaller,
        id: &str,
        set: &[String],
        note: Option<&str>,
        dry_run: bool,
    ) -> Result<Value, String>;
}

#[cfg(test)]
mod tests;

impl BlueprintMemoryExt for ServiceContext {
    fn blueprint_keep(
        &self,
        caller: &SentinelCaller,
        id: &str,
        when: &str,
    ) -> Result<Value, String> {
        if when.trim().is_empty()
            || when.chars().count() > 200
            || when.chars().any(char::is_control)
        {
            return Err("When to use must contain 1..200 plain-text characters".into());
        }
        let handle = self
            .host_extension::<Handle>()
            .ok_or("blueprints not initialized")?;
        let job = {
            let mux = self.mux.read();
            if !caller_is_live(&mux, caller)
                || caller.team.is_none()
                || !mux
                    .get_tab(&caller.tab)
                    .is_some_and(|tab| tab.has_team_orchestrator_authority())
            {
                return Err("Only the Team Orchestrator may keep Blueprint solutions".into());
            }
            let store = handle.store.lock();
            store.ensure_available()?;
            store
                .get(id)
                .filter(|job| job.team_id == caller.team && job.workspace_id == caller.workspace)
                .cloned()
                .ok_or("Blueprint not found in this Team")?
        };
        let mut successes =
            job.transitions
                .iter()
                .filter(|transition| {
                    transition.bucket != "other"
                        && (job.until.as_ref() == Some(&transition.bucket)
                            || transition.handlers.iter().any(|handler| {
                                handler.status == crate::handlers::Status::Completed
                            }))
                })
                .count() as u64;
        let mut failures = job
            .transitions
            .iter()
            .flat_map(|transition| &transition.handlers)
            .filter(|handler| {
                matches!(
                    handler.status,
                    crate::handlers::Status::Failed | crate::handlers::Status::Unconfirmed
                )
            })
            .count() as u64;
        if job.pipeline().is_some() {
            let completion = handle.store.lock().typed_pipeline_log(&job.id, 200)?;
            let journal = completion
                .wait(crate::storage::COMMIT_DEADLINE)
                .map_err(|error| error.to_string())?
                .value;
            successes = journal
                .iter()
                .filter(|entry| entry.step_name == "$run" && entry.state == "done")
                .count() as u64;
            failures = journal
                .iter()
                .filter(|entry| entry.step_name == "$run" && entry.state == "failed")
                .count() as u64;
        }
        let evidence = Evidence {
            origin: Some(format!("{}@{}", job.id, job.definition_version)),
            kept: true,
            successes,
            failures,
            recognized: job
                .transitions
                .iter()
                .filter(|transition| transition.bucket != "other")
                .count() as u64,
            unclassified: job
                .transitions
                .iter()
                .filter(|transition| transition.bucket == "other")
                .count() as u64,
            runs: job.transitions.len() as u64,
            name: job.name.clone().unwrap_or_default(),
            note: job.note.clone().unwrap_or_default(),
            ..Evidence::default()
        };
        self.mem.enqueue_blueprint(
            area(&caller.workspace),
            crate::portable::definition(&job),
            when.into(),
            evidence,
        )?;
        Ok(json!({"id":id,"status":"queued"}))
    }
    fn blueprint_recall(&self, caller: &SentinelCaller, intent: &str) -> Result<Value, String> {
        if intent.len() > 4096 {
            return Err("Blueprint intent exceeds 4096 bytes".into());
        }
        let entries = self
            .mem
            .recall_blueprints(&area(&caller.workspace), intent, 5)?;
        let offers=entries.into_iter().map(|entry| {
            let yaml=entry.document.yaml()?;
            Ok(json!({"id":entry.id,"name":entry.document.name,"when_to_use":entry.when_to_use,"how":entry.document.note,"uses":entry.uses,"successes":entry.successes,"last_used":entry.last_used,"yaml":yaml}))
        }).collect::<Result<Vec<_>,String>>()?;
        Ok(json!(offers))
    }
    fn blueprint_apply_recipe(
        &self,
        caller: &SentinelCaller,
        id: &str,
        set: &[String],
        note: Option<&str>,
        dry_run: bool,
    ) -> Result<Value, String> {
        let mut document = self.mem.blueprint_recipe(&area(&caller.workspace), id)?;
        if let Some(note) = note {
            document.note = Some(note.into());
        }
        let definition = document.bind(set).map_err(|errors| {
            errors
                .into_iter()
                .map(|(field, error)| format!("{field}: {error}"))
                .collect::<Vec<_>>()
                .join("; ")
        })?;
        let result = self.blueprint_apply(caller, definition, dry_run)?;
        if !dry_run {
            self.mem.record_blueprint_reuse(id);
        }
        Ok(result)
    }
}
