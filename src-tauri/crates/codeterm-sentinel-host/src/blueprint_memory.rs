use crate::{
    scheduler::Handle,
    service_adapter::{caller_is_live, SentinelCaller},
    BlueprintApplyExt,
};
use codeterm_blueprint_types::blueprint_memory::Evidence;
use codeterm_service_core::service::ServiceContext;
use serde_json::{json, Value};
mod cards;
pub(crate) mod curator;

fn area(workspace: &str) -> String {
    format!("blueprints:{workspace}")
}

pub trait BlueprintMemoryExt {
    fn blueprint_save_solution_host(
        &self,
        workspace: &str,
        team: Option<&str>,
        id: &str,
        when: &str,
        tags: &[String],
    ) -> Result<Value, String>;
    fn blueprint_solutions_host(&self, workspace: &str) -> Result<Value, String>;
    fn blueprint_solution_show_host(&self, workspace: &str, id: &str) -> Result<Value, String>;
    fn blueprint_archive_solution_host(&self, workspace: &str, id: &str) -> Result<Value, String>;

    fn blueprint_keep(
        &self,
        caller: &SentinelCaller,
        id: &str,
        when: &[String],
        tags: &[String],
    ) -> Result<Value, String>;
    fn blueprint_recall_cards(
        &self,
        caller: &SentinelCaller,
        intent: &str,
        limit: usize,
    ) -> Result<Value, String>;
    fn blueprint_keep_callable(
        &self,
        caller: &SentinelCaller,
        document: codeterm_blueprint_types::blueprint_document::Document,
        local_callees: &[codeterm_blueprint_types::blueprint_document::calls::LocalCallee],
        when: &[String],
        tags: &[String],
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
    fn blueprint_save_solution_host(
        &self,
        workspace: &str,
        team: Option<&str>,
        id: &str,
        when: &str,
        tags: &[String],
    ) -> Result<Value, String> {
        let tags = codeterm_blueprint_types::blueprint_tags::validate(tags)
            .map_err(|error| error.to_string())?;
        crate::graph::validate_scope(&self.mux.read(), workspace, team)?;
        let handle = self
            .host_extension::<Handle>()
            .ok_or("blueprints not initialized")?;
        let job = {
            let store = handle.store.lock();
            store.ensure_available()?;
            store
                .get(id)
                .filter(|job| job.workspace_id == workspace && job.team_id.as_deref() == team)
                .cloned()
                .ok_or("Blueprint not found in this scope")?
        };
        let mut evidence = evidence(&handle, &job)?;
        evidence.tags = tags.to_vec();
        if evidence.successes == 0 {
            return Err("Only a successful Blueprint can be saved as a solution".into());
        }
        let document = codeterm_blueprint_types::blueprint_document::Document::from_definition(
            &crate::portable::definition(&job),
        )?;
        let saved = self.mem.save_blueprint_scoped(
            &area(workspace),
            team.ok_or("Saved callable requires a Team")?,
            document,
            &[when.to_owned()],
            evidence,
        )?;
        self.mem.blueprint_recipe(&area(workspace), &saved.id)?;
        Ok(json!({"id":saved.id,"evicted_ids":saved.evicted_ids,"status":"saved","tags":tags}))
    }
    fn blueprint_solutions_host(&self, workspace: &str) -> Result<Value, String> {
        crate::graph::validate_scope(&self.mux.read(), workspace, None)?;
        let entries = self.mem.list_blueprint_solutions(&area(workspace))?;
        Ok(json!(entries.into_iter().map(|entry| Ok(json!({"id":entry.id,"tags":self.mem.blueprint_tags(&entry.id)?,"when_to_use":entry.when_to_use(),"intents":entry.intents,"how":entry.document.note,"uses":entry.uses,"successes":entry.successes,"last_used":entry.last_used}))).collect::<Result<Vec<_>, String>>()?))
    }
    fn blueprint_solution_show_host(&self, workspace: &str, id: &str) -> Result<Value, String> {
        crate::graph::validate_scope(&self.mux.read(), workspace, None)?;
        let graph = self.mem.blueprint_solution_show(&area(workspace), id)?;
        Ok(json!({"document":graph.root,"expanded_calls":graph.calls}))
    }
    fn blueprint_archive_solution_host(&self, workspace: &str, id: &str) -> Result<Value, String> {
        crate::graph::validate_scope(&self.mux.read(), workspace, None)?;
        self.mem.archive_blueprint_solution(&area(workspace), id)?;
        Ok(json!({"id":id,"status":"archived"}))
    }

    fn blueprint_keep(
        &self,
        caller: &SentinelCaller,
        id: &str,
        when: &[String],
        tags: &[String],
    ) -> Result<Value, String> {
        let tags = codeterm_blueprint_types::blueprint_tags::validate(tags)
            .map_err(|error| error.to_string())?;
        let when = &codeterm_blueprint_types::blueprint_memory::validate_intents(when)?;
        if self
            .role_policy_for_tab(&caller.tab)
            .is_some_and(|policy| policy.conservative_save())
        {
            return curator::keep(self, caller, id, when, &tags);
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
        let mut evidence = evidence(&handle, &job)?;
        evidence.tags = tags.to_vec();
        let saved = self.mem.enqueue_blueprint_scoped(
            area(&caller.workspace),
            caller
                .team
                .clone()
                .ok_or("Saved callable requires a Team")?,
            crate::portable::definition(&job),
            when.clone(),
            evidence,
        )?;
        Ok(json!({"id":saved.id,"evicted_ids":saved.evicted_ids,"status":"saved","tags":tags}))
    }
    fn blueprint_recall_cards(
        &self,
        caller: &SentinelCaller,
        intent: &str,
        limit: usize,
    ) -> Result<Value, String> {
        cards::recall(self, caller, intent, limit)
    }
    fn blueprint_keep_callable(
        &self,
        caller: &SentinelCaller,
        document: codeterm_blueprint_types::blueprint_document::Document,
        local_callees: &[codeterm_blueprint_types::blueprint_document::calls::LocalCallee],
        when: &[String],
        tags: &[String],
    ) -> Result<Value, String> {
        use codeterm_blueprint_types::blueprint_document::{calls, checker, Kind};
        let tags = codeterm_blueprint_types::blueprint_tags::validate(tags)
            .map_err(|error| error.to_string())?;
        let when = codeterm_blueprint_types::blueprint_memory::validate_intents(when)?;
        {
            let mux = self.mux.read();
            if !caller_is_live(&mux, caller)
                || caller.team.is_none()
                || !mux
                    .get_tab(&caller.tab)
                    .is_some_and(|tab| tab.has_team_orchestrator_authority())
            {
                return Err("Only the Team Orchestrator may keep callable solutions".into());
            }
        }
        let team = caller
            .team
            .as_deref()
            .ok_or("Callable save requires a Team")?;
        if document.kind != Kind::Pipeline
            || document.trigger.is_some()
            || document.result.port.is_none()
            || document.result.from.is_none()
            || document.result.to.is_some()
        {
            return Err("keep --file requires a triggerless typed callable Blueprint".into());
        }
        if calls::has_unsealed_code_files(&document) {
            return Err("Callable code files must be frozen by the CLI before saving".into());
        }
        let document = self.mem.pin_blueprint_bundle(
            &area(&caller.workspace),
            team,
            document,
            local_callees,
        )?;
        let closure = calls::FrozenClosure::from_document(document.clone())?;
        let context = calls::check_context(&closure, &Default::default());
        let diagnostics = checker::check_ir(&closure.root.ir(), &context);
        if checker::has_errors(&diagnostics) {
            return Err(checker::render(&diagnostics, true));
        }
        let saved = self.mem.save_blueprint_scoped(
            &area(&caller.workspace),
            team,
            document,
            &when,
            Evidence {
                kept: true,
                tags: tags.clone(),
                ..Default::default()
            },
        )?;
        let id = format!("mem:{}", saved.id);
        let mut body = self
            .mem
            .blueprint_callable(&area(&caller.workspace), team, &id)?;
        body.dependencies.clear();
        let hash = codeterm_blueprint_types::blueprint_document::projection::identity(&body)?;
        Ok(
            json!({"id":id,"hash":hash,"status":"saved","tags":tags,"evicted_ids":saved.evicted_ids}),
        )
    }
    fn blueprint_recall(&self, caller: &SentinelCaller, intent: &str) -> Result<Value, String> {
        if intent.len() > 4096 {
            return Err("Blueprint intent exceeds 4096 bytes".into());
        }
        let entries = self
            .mem
            .recall_blueprints(&area(&caller.workspace), intent, 5)?;
        let offers=entries.into_iter().map(|hit| {
            let entry=hit.entry;
            let yaml=entry.document.yaml()?;
            Ok(json!({"id":entry.id,"tags":self.mem.blueprint_tags(&entry.id)?,"name":entry.document.name,"when_to_use":entry.when_to_use(),"intents":entry.intents,"how":entry.document.note,"uses":entry.uses,"successes":entry.successes,"last_used":entry.last_used,"yaml":yaml,"why":hit.why,"enrichment_pending":hit.enrichment_pending}))
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
        let result = self
            .blueprint_apply_recipe_document(caller, definition, dry_run, id)
            .map_err(|error| error.to_string())?;
        if !dry_run {
            self.mem.record_blueprint_reuse(id);
        }
        Ok(result)
    }
}

fn evidence(handle: &Handle, job: &crate::store::Sentinel) -> Result<Evidence, String> {
    let mut successes = job
        .transitions
        .iter()
        .filter(|transition| {
            transition.bucket != "other"
                && (job.until.as_ref() == Some(&transition.bucket)
                    || transition
                        .handlers
                        .iter()
                        .any(|handler| handler.status == crate::handlers::Status::Completed))
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
    Ok(Evidence {
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
    })
}
