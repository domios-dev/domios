mod delete;
pub(crate) mod graph;
use tauri::Manager;

use super::ServiceContext;
use crate::sentinel::{
    amend::{self, DefinitionSnapshot, InputPatch, PreparedUpdate, Update},
    classify, routes,
    scheduler::Handle,
    source,
    store::{Create, Sentinel},
};

#[derive(Clone)]
pub(crate) struct SentinelCaller {
    pub(super) tab: String,
    pub(super) team: Option<String>,
    pub(super) workspace: String,
    pub(super) cwd: String,
    pub(super) nonce: Option<String>,
}

impl SentinelCaller {
    pub(super) fn owns(&self, job: &Sentinel) -> bool {
        if let Some(pipeline) = &job.pipeline().cloned() {
            return match pipeline.owner {
                codeterm_types::blueprint_pipeline::BlueprintOwner::Tab => {
                    job.creator_tab_id == self.tab
                        && (job.team_id == self.team
                            || (job.team_id.is_none() && self.team.is_some()))
                        && job.workspace_id == self.workspace
                }
                codeterm_types::blueprint_pipeline::BlueprintOwner::Team => {
                    job.team_id.is_some()
                        && job.team_id == self.team
                        && job.workspace_id == self.workspace
                }
            };
        }
        job.creator_tab_id == self.tab
            && (job.team_id == self.team || (job.team_id.is_none() && self.team.is_some()))
            && job.workspace_id == self.workspace
    }

    pub(super) fn can_amend(&self, job: &Sentinel) -> bool {
        if job.pipeline().is_some() {
            return self.owns(job);
        }
        (job.team_id == self.team
            || (job.team_id.is_none() && self.team.is_some() && job.creator_tab_id == self.tab))
            && job.workspace_id == self.workspace
    }
}

fn caller_from_mux(
    mux: &crate::multiplexer::state::MultiplexerState,
    tab_id: &str,
    nonce: Option<&str>,
) -> Result<SentinelCaller, String> {
    let tab = mux
        .get_tab(tab_id)
        .ok_or("blueprint caller tab not found")?;
    if tab.session_binding_nonce.is_some() && tab.session_binding_nonce.as_deref() != nonce {
        return Err("blueprint requires the calling tab's current session binding nonce".into());
    }
    let workspace = mux
        .find_workspace_for_tab(tab_id)
        .ok_or("blueprint caller workspace not found")?;
    let cwd = tab
        .cwd
        .clone()
        .or_else(|| tab.provisioned_cwd.clone())
        .filter(|cwd| !cwd.is_empty())
        .ok_or("blueprint creator cwd is unavailable")?;
    if tab
        .team_id
        .as_ref()
        .is_some_and(|id| mux.get_team(id).is_none())
    {
        return Err("blueprint creator Team is gone".into());
    }
    Ok(SentinelCaller {
        tab: tab_id.into(),
        team: tab.team_id.clone(),
        workspace,
        cwd,
        nonce: tab.session_binding_nonce.clone(),
    })
}

impl ServiceContext {
    pub(crate) fn blueprint_input(
        &self,
        caller: &SentinelCaller,
        id: &str,
        text: String,
    ) -> Result<serde_json::Value, String> {
        let job = self.sentinel_show(caller, id)?;
        let input = job.input().ok_or("blueprint is not a text input stage")?;
        if input.kind != codeterm_types::sentinel_input::MediaKind::Text
            || input.convert_to.is_some()
        {
            return Err(
                "blueprint input delivery supports text without media conversion only".into(),
            );
        }
        let handle = self
            .app_handle
            .try_state::<Handle>()
            .ok_or("blueprints not initialized")?;
        {
            let mux = self.mux.read();
            if !caller_is_live(&mux, caller) {
                return Err("blueprint caller scope changed".into());
            }
            handle.enqueue_text(&job, text)?;
        }
        handle.notify();
        Ok(serde_json::json!({"id": id, "accepted": true}))
    }

    pub(crate) fn sentinel_update_from_history(
        &self,
        caller: &SentinelCaller,
        id: &str,
        expect_version: u64,
        history: &serde_json::Value,
    ) -> Result<Sentinel, String> {
        let definition = history_definition(history, id, expect_version)?;
        let snapshot: DefinitionSnapshot = serde_json::from_value(definition)
            .map_err(|error| format!("invalid history definition: {error}"))?;
        if let Some(pipeline) = snapshot.pipeline.clone() {
            let cause = history
                .get("cause")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned);
            return self.blueprint_pipeline_update(caller, id, expect_version, pipeline, cause);
        }
        let prepared =
            self.prepare_history_update(caller, id, expect_version, snapshot, history)?;
        let handle = self
            .app_handle
            .try_state::<Handle>()
            .ok_or("blueprints not initialized")?;
        let authored_at = handle.store.lock().clock.now_ms();
        let staged = {
            let mux = self.mux.read();
            if !caller_is_live(&mux, caller) {
                return Err("blueprint caller scope changed".into());
            }
            validate_send_scope(&mux, &prepared.job)?;
            handle.store.lock().stage_update_definition(
                expect_version,
                &caller.tab,
                authored_at,
                prepared,
            )
        };
        let updated = staged.settle(&handle.store)?;
        handle.notify();
        Ok(updated)
    }

    pub(crate) fn sentinel_validate_history_update(
        &self,
        caller: &SentinelCaller,
        id: &str,
        expect_version: u64,
        history: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let definition = history_definition(history, id, expect_version)?;
        let snapshot: DefinitionSnapshot = serde_json::from_value(definition)
            .map_err(|error| format!("invalid history definition: {error}"))?;
        if let Some(pipeline) = snapshot.pipeline.clone() {
            return self.blueprint_pipeline_validate_update(caller, id, expect_version, pipeline);
        }
        let prepared =
            self.prepare_history_update(caller, id, expect_version, snapshot, history)?;
        serde_json::to_value(prepared.job).map_err(|error| error.to_string())
    }

    fn prepare_history_update(
        &self,
        caller: &SentinelCaller,
        id: &str,
        expect_version: u64,
        snapshot: DefinitionSnapshot,
        history: &serde_json::Value,
    ) -> Result<PreparedUpdate, String> {
        let handle = self
            .app_handle
            .try_state::<Handle>()
            .ok_or("blueprints not initialized")?;
        let current = {
            let store = handle.store.lock();
            store.ensure_available()?;
            let current = store
                .get(id)
                .filter(|job| caller.can_amend(job))
                .cloned()
                .ok_or_else(|| format!("blueprint not found in this workspace and Team: {id}"))?;
            if current.definition_version != expect_version {
                return Err(amend::version_conflict(current.definition_version));
            }
            current
        };
        if snapshot.interval_ms % 1000 != 0 {
            return Err("interval_ms must be representable as seconds".into());
        }
        let input = snapshot.input.clone().map(|input| InputPatch {
            kind: Some(input.kind),
            data: input.data,
            convert_to: input.convert_to,
            convert_timeout_ms: input.convert_timeout_ms,
        });
        let cause = history
            .get("cause")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned);
        let update = Update {
            name: snapshot.name.clone(),
            question: snapshot.question.clone(),
            buckets: snapshot.buckets.clone(),
            every: Some(format!("{}s", snapshot.interval_ms / 1000)),
            routes: Default::default(),
            matches: snapshot.matches.clone(),
            model_classification: snapshot.model_classification,
            rearm_on_change: snapshot.rearm_on_change.clone(),
            until: snapshot.until.clone(),
            input,
            handlers: None,
            on_other: Vec::new(),
            escalation_timeout_ms: snapshot.escalation_timeout_ms,
            max_transitions: Some(snapshot.max_transitions),
            max_actions: Some(snapshot.max_actions),
            cause,
            note: None,
        };
        let connected = update.model_classification
            && update.matches.is_empty()
            && classify::connected_model(self).is_some();
        let mut prepared = update.prepare(&current, &caller.tab, connected)?;
        prepared.job.source = crate::sentinel::stage_source::StageSource::from_parts(
            snapshot.probe,
            snapshot.observe,
            snapshot.input,
            snapshot.terminal,
            snapshot.pipeline,
        )?;
        prepared.job.session = snapshot.session;
        prepared.job.routes = snapshot.routes;
        prepared.job.on_other = snapshot.on_other;
        if caller.tab != current.creator_tab_id && amend::has_new_command(&current, &prepared.job) {
            return Err("only the blueprint creator may introduce new command text".into());
        }
        crate::sentinel::handlers::validate(&prepared.job.buckets, &prepared.job.routes)?;
        crate::sentinel::source::validate(&prepared.job)?;
        crate::sentinel::escalation::validate(&prepared.job)?;
        let resolution_source = prepared.job.clone();
        for handler in prepared
            .job
            .routes
            .values_mut()
            .flatten()
            .chain(&mut prepared.job.on_other)
        {
            let action = routes::resolve(self, &resolution_source, &handler.action)?;
            handler.target = graph::target_identity(self, &resolution_source, &action)?;
            handler.action = action;
        }
        let size = serde_json::to_vec(&DefinitionSnapshot::from_job(&prepared.job))
            .map_err(|error| error.to_string())?
            .len();
        if size > 65_536 {
            return Err("blueprint definition exceeds 64 KiB".into());
        }
        Ok(prepared)
    }

    pub(crate) fn sentinel_caller(
        &self,
        tab_id: &str,
        nonce: Option<&str>,
    ) -> Result<SentinelCaller, String> {
        caller_from_mux(&self.mux.read(), tab_id, nonce)
    }

    pub(crate) fn sentinel_create(
        &self,
        caller: &SentinelCaller,
        create: Create,
    ) -> Result<Sentinel, String> {
        self.sentinel_create_in_state(caller, create, None)
    }

    pub(crate) fn sentinel_create_stopped(
        &self,
        caller: &SentinelCaller,
        create: Create,
        stopped: bool,
    ) -> Result<Sentinel, String> {
        self.sentinel_create_in_state(caller, create, Some(stopped))
    }

    fn sentinel_create_in_state(
        &self,
        caller: &SentinelCaller,
        create: Create,
        imported_stopped: Option<bool>,
    ) -> Result<Sentinel, String> {
        let mut job = self.prepare_sentinel_create(caller, create)?;
        job.machine.stopped = imported_stopped.unwrap_or(false);
        let handle = self
            .app_handle
            .try_state::<Handle>()
            .ok_or("blueprints not initialized")?;
        let staged = {
            let mux = self.mux.read();
            if !caller_is_live(&mux, caller) {
                return Err("blueprint creator or Team is gone".into());
            }
            if let Some(crate::sentinel::source::TabBinding::Tab(tab_id)) = &job.observe().cloned()
            {
                source::validate_tab_scope(&mux, &job, tab_id)?;
            }
            validate_send_scope(&mux, &job)?;
            let tab = mux.get_tab(&caller.tab).ok_or("blueprint creator gone")?;
            job.agent_authored = imported_stopped.is_some()
                || (tab.agent_type.is_some() && tab.session_binding_nonce.is_some());
            handle.store.lock().stage_insert(job.clone())
        };
        staged.settle(&handle.store)?;
        handle.notify();
        Ok(job)
    }

    pub(crate) fn sentinel_validate_create(
        &self,
        caller: &SentinelCaller,
        create: Create,
    ) -> Result<Sentinel, String> {
        self.prepare_sentinel_create(caller, create)
    }

    fn prepare_sentinel_create(
        &self,
        caller: &SentinelCaller,
        create: Create,
    ) -> Result<Sentinel, String> {
        let handle = self
            .app_handle
            .try_state::<Handle>()
            .ok_or("blueprints not initialized")?;
        let connected = create.model_classification
            && create.matches.is_empty()
            && classify::connected_model(self).is_some();
        let mut job = handle.store.lock().prepare(
            create,
            crate::sentinel::store::Owner {
                creator_tab_id: caller.tab.clone(),
                team_id: caller.team.clone(),
                workspace_id: caller.workspace.clone(),
                cwd: caller.cwd.clone(),
            },
            connected,
        )?;
        if let Some(binding) = job.observe().cloned() {
            job.set_observe(Some(source::resolve_binding(self, &job, &binding)?));
        }
        let mut resolved = std::collections::BTreeMap::new();
        for (bucket, handlers) in &job.routes {
            let mut admitted = Vec::new();
            for handler in handlers {
                let action = routes::resolve(self, &job, &handler.action)?;
                let target = graph::target_identity(self, &job, &action)?;
                admitted.push(crate::sentinel::handlers::Handler {
                    id: handler.id.clone(),
                    action,
                    target,
                });
            }
            resolved.insert(bucket.clone(), admitted);
        }
        job.routes = resolved;
        let mut resolved_other = Vec::new();
        for handler in &job.on_other {
            let action = routes::resolve(self, &job, &handler.action)?;
            resolved_other.push(crate::sentinel::handlers::Handler {
                id: handler.id.clone(),
                target: graph::target_identity(self, &job, &action)?,
                action,
            });
        }
        job.on_other = resolved_other;
        {
            let mux = self.mux.read();
            if !caller_is_live(&mux, caller) {
                return Err("blueprint creator or Team is gone".into());
            }
            if let Some(crate::sentinel::source::TabBinding::Tab(tab_id)) = &job.observe().cloned()
            {
                source::validate_tab_scope(&mux, &job, tab_id)?;
            }
            validate_send_scope(&mux, &job)?;
        }
        Ok(job)
    }

    pub(crate) fn sentinel_update(
        &self,
        caller: &SentinelCaller,
        id: &str,
        expect_version: u64,
        update: Update,
    ) -> Result<Sentinel, String> {
        let (current, update) = self.prepare_sentinel_update(caller, id, expect_version, update)?;
        let handle = self
            .app_handle
            .try_state::<Handle>()
            .ok_or("blueprints not initialized")?;
        let note = update.note.clone();
        let cause = update.cause_transition_id.clone();
        let authored_at = handle.store.lock().clock.now_ms();
        let updated = {
            let mux = self.mux.read();
            if !caller_is_live(&mux, caller) {
                return Err("blueprint caller scope changed".into());
            }
            validate_send_scope(&mux, &update.job)?;
            let staged = handle.store.lock().stage_update_definition(
                expect_version,
                &caller.tab,
                authored_at,
                update,
            );
            staged
        }
        .settle(&handle.store)?;
        handle.notify();
        if !updated.model_classification {
            return Ok(updated);
        }
        if let Err(error) = crate::service::supporter::runtime_with_notifications(self)
            .sentinel_event(serde_json::json!({
                "time": chrono::Utc::now().timestamp(),
                "kind": "blueprint",
                "blueprint_id": &updated.id,
                "tab": &caller.tab,
                "team_id": &caller.team,
                "workspace_id": &caller.workspace,
                "action": "definition_update",
                "from": current.definition_version,
                "to": updated.definition_version,
                "author": &caller.tab,
                "cause": cause,
                "note": note
            }))
        {
            log::error!("blueprint definition update journal failed: {error}");
        }
        Ok(updated)
    }

    pub(crate) fn sentinel_validate_update(
        &self,
        caller: &SentinelCaller,
        id: &str,
        expect_version: u64,
        update: Update,
    ) -> Result<Sentinel, String> {
        self.prepare_sentinel_update(caller, id, expect_version, update)
            .map(|(_, update)| update.job)
    }

    fn prepare_sentinel_update(
        &self,
        caller: &SentinelCaller,
        id: &str,
        expect_version: u64,
        update: Update,
    ) -> Result<(Sentinel, amend::PreparedUpdate), String> {
        let handle = self
            .app_handle
            .try_state::<Handle>()
            .ok_or("blueprints not initialized")?;
        let current = {
            let store = handle.store.lock();
            store.ensure_available()?;
            store
                .get(id)
                .filter(|job| caller.can_amend(job))
                .cloned()
                .ok_or_else(|| format!("blueprint not found in this workspace and Team: {id}"))?
        };
        if current.pipeline().is_some() {
            return Err("pipeline updates require a pipeline spec".into());
        }
        if current.definition_version != expect_version {
            return Err(amend::version_conflict(current.definition_version));
        }
        let connected = update.model_classification
            && update.matches.is_empty()
            && classify::connected_model(self).is_some();
        let mut update = update.prepare(&current, &caller.tab, connected)?;
        let resolution_source = update.job.clone();
        for handler in update
            .job
            .routes
            .values_mut()
            .flatten()
            .chain(&mut update.job.on_other)
        {
            let action = routes::resolve(self, &resolution_source, &handler.action)?;
            handler.target = graph::target_identity(self, &resolution_source, &action)?;
            handler.action = action;
        }
        if let Some(binding) = update.job.observe().cloned() {
            update
                .job
                .set_observe(Some(source::resolve_binding(self, &update.job, &binding)?));
        }
        {
            let mux = self.mux.read();
            if !caller_is_live(&mux, caller) {
                return Err("blueprint caller scope changed".into());
            }
            if let Some(crate::sentinel::source::TabBinding::Tab(tab_id)) =
                &update.job.observe().cloned()
            {
                source::validate_tab_scope(&mux, &update.job, tab_id)?;
            }
            validate_send_scope(&mux, &update.job)?;
        }
        Ok((current, update))
    }

    pub(crate) fn sentinel_history(
        &self,
        caller: &SentinelCaller,
        id: &str,
    ) -> Result<amend::History, String> {
        let handle = self
            .app_handle
            .try_state::<Handle>()
            .ok_or("blueprints not initialized")?;
        if !caller_is_live(&self.mux.read(), caller) {
            return Err("blueprint caller scope changed".into());
        }
        let (job, typed_history) = {
            let store = handle.store.lock();
            store.ensure_available()?;
            let job = store
                .get(id)
                .filter(|job| caller.can_amend(job))
                .cloned()
                .ok_or_else(|| format!("blueprint not found in this workspace and Team: {id}"))?;
            let typed = store.typed_history(id)?;
            (job, typed)
        };
        match typed_history {
            Some(history) => {
                let records = history
                    .wait(crate::sentinel::storage::COMMIT_DEADLINE)
                    .map_err(|error| error.to_string())?
                    .value;
                let versions = crate::sentinel::storage::history_versions(records);
                if versions.is_empty() {
                    return Err(format!("blueprint history not found: {id}"));
                }
                Ok(amend::from_storage_history(job.id, versions))
            }
            None => Ok(amend::history(&job)),
        }
    }

    pub(crate) fn sentinel_resolve_escalation(
        &self,
        caller: &SentinelCaller,
        id: &str,
        transition_id: &str,
    ) -> Result<Sentinel, String> {
        let handle = self
            .app_handle
            .try_state::<Handle>()
            .ok_or("blueprints not initialized")?;
        let staged = {
            let mux = self.mux.read();
            if !caller_is_live(&mux, caller) {
                return Err("blueprint caller scope changed".into());
            }
            let mut store = handle.store.lock();
            store.ensure_available()?;
            if !store.get(id).is_some_and(|job| caller.can_amend(job)) {
                return Err(format!(
                    "blueprint not found in this workspace and Team: {id}"
                ));
            }
            store.stage_resolve_escalation(id, transition_id)
        };
        if !staged.settle(&handle.store)? {
            return Err("blueprint escalation resolution was not applied".into());
        }
        handle.store.lock().wake_stage(id);
        handle.notify();
        let resolved = handle
            .store
            .lock()
            .get(id)
            .cloned()
            .ok_or_else(|| format!("blueprint not found: {id}"))?;
        if !resolved.model_classification {
            return Ok(resolved);
        }
        if let Err(error) = crate::service::supporter::runtime_with_notifications(self)
            .sentinel_event(serde_json::json!({
                "time": chrono::Utc::now().timestamp(),
                "kind": "blueprint",
                "blueprint_id": id,
                "transition_id": transition_id,
                "tab": caller.tab,
                "team_id": caller.team,
                "workspace_id": caller.workspace,
                "action": "escalation_resolved"
            }))
        {
            log::error!("blueprint escalation resolution journal failed: {error}");
        }
        Ok(resolved)
    }

    pub(crate) fn sentinel_list(&self, caller: &SentinelCaller) -> Result<Vec<Sentinel>, String> {
        let handle = self
            .app_handle
            .try_state::<Handle>()
            .ok_or("blueprints not initialized")?;
        let store = handle.store.lock();
        store.ensure_available()?;
        let jobs = store
            .list()
            .iter()
            .filter(|job| caller.owns(job))
            .cloned()
            .collect();
        Ok(jobs)
    }

    pub(crate) fn sentinel_show(
        &self,
        caller: &SentinelCaller,
        id: &str,
    ) -> Result<Sentinel, String> {
        let handle = self
            .app_handle
            .try_state::<Handle>()
            .ok_or("blueprints not initialized")?;
        let store = handle.store.lock();
        store.ensure_available()?;
        let job = store
            .get(id)
            .filter(|job| caller.owns(job))
            .cloned()
            .ok_or_else(|| format!("blueprint not found for this creator and Team: {id}"))?;
        Ok(job)
    }

    pub(crate) fn sentinel_stop(
        &self,
        caller: &SentinelCaller,
        id: &str,
    ) -> Result<Sentinel, String> {
        let handle = self
            .app_handle
            .try_state::<Handle>()
            .ok_or("blueprints not initialized")?;
        let staged = {
            let mux = self.mux.read();
            let live = mux.get_tab(&caller.tab).is_some_and(|tab| {
                tab.team_id == caller.team && tab.session_binding_nonce == caller.nonce
            }) && mux.find_workspace_for_tab(&caller.tab).as_deref()
                == Some(&caller.workspace);
            if !live {
                return Err("blueprint caller scope changed".into());
            }
            crate::sentinel::graph::validate_scope(
                &mux,
                &caller.workspace,
                caller.team.as_deref(),
            )?;
            let mut store = handle.store.lock();
            store.ensure_available()?;
            if !store.get(id).is_some_and(|job| caller.owns(job)) {
                return Err("blueprint not found for this creator and Team".into());
            }
            store.stage_stop(id, "stopped_by_creator")
        };
        let result = staged.settle(&handle.store);
        handle.notify();
        result
    }
}

pub(super) fn caller_is_live(
    mux: &crate::multiplexer::state::MultiplexerState,
    caller: &SentinelCaller,
) -> bool {
    mux.get_tab(&caller.tab)
        .is_some_and(|tab| tab.team_id == caller.team && tab.session_binding_nonce == caller.nonce)
        && mux.find_workspace_for_tab(&caller.tab).as_deref() == Some(&caller.workspace)
        && caller
            .team
            .as_ref()
            .is_none_or(|team| mux.get_team(team).is_some())
}

fn history_definition(
    history: &serde_json::Value,
    id: &str,
    expect_version: u64,
) -> Result<serde_json::Value, String> {
    if history
        .get("blueprint_id")
        .or_else(|| history.get("sentinel_id"))
        .and_then(serde_json::Value::as_str)
        != Some(id)
    {
        return Err("history blueprint_id does not match update id".into());
    }
    let current_version = history
        .get("current_version")
        .and_then(serde_json::Value::as_u64)
        .ok_or("history current_version is required")?;
    if current_version != expect_version {
        return Err(amend::version_conflict(current_version));
    }
    history
        .get("versions")
        .and_then(serde_json::Value::as_array)
        .and_then(|versions| {
            versions.iter().find(|version| {
                version.get("version").and_then(serde_json::Value::as_u64) == Some(expect_version)
            })
        })
        .and_then(|version| version.get("definition"))
        .cloned()
        .ok_or_else(|| "history current definition is missing".into())
}

fn validate_send_scope(
    mux: &crate::multiplexer::state::MultiplexerState,
    job: &Sentinel,
) -> Result<(), String> {
    for handler in job.routes.values().flatten().chain(&job.on_other) {
        if let crate::sentinel::machine::Route::Send(id) = &handler.action {
            let target = mux.get_tab(id).ok_or("blueprint send target disappeared")?;
            if target.agent_type.is_none()
                || target.team_id != job.team_id
                || mux.find_workspace_for_tab(id).as_deref() != Some(&job.workspace_id)
            {
                return Err("blueprint send target scope changed".into());
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sentinel_creator_team_and_workspace_scope_all_read_and_stop_operations() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = crate::sentinel::store::Store::new(
            dir.path().join("sentinels.json"),
            crate::cron::clock::MockClock::starting_at(1000),
        );
        let params = crate::sentinel::store::Create {
            terminal: None,
            session: false,
            name: Some("service-state-watch".into()),
            probe: Some("status".into()),
            observe: None,
            input: None,
            question: "state".into(),
            buckets: vec!["done".into()],
            every: "10s".into(),
            routes: Default::default(),
            matches: Default::default(),
            model_classification: false,
            rearm_on_change: Default::default(),
            until: None,
            timeout: None,
            paused: false,
            handlers: None,
            escalation_timeout_ms: None,
            max_transitions: None,
            max_actions: None,
        };
        let job = store
            .create(
                params,
                crate::sentinel::store::Owner {
                    creator_tab_id: "creator".into(),
                    team_id: Some("team".into()),
                    workspace_id: "workspace".into(),
                    cwd: "/tmp".into(),
                },
                true,
            )
            .unwrap();
        let caller = SentinelCaller {
            tab: "creator".into(),
            team: Some("team".into()),
            workspace: "workspace".into(),
            cwd: "/tmp".into(),
            nonce: None,
        };
        assert!(caller.owns(&job));
        assert!(SentinelCaller {
            tab: "editor".into(),
            ..caller.clone()
        }
        .can_amend(&job));
        for changed in [
            SentinelCaller {
                tab: "other".into(),
                ..caller.clone()
            },
            SentinelCaller {
                team: None,
                ..caller.clone()
            },
            SentinelCaller {
                workspace: "elsewhere".into(),
                ..caller.clone()
            },
        ] {
            assert!(!changed.owns(&job));
        }
        assert!(!SentinelCaller {
            workspace: "elsewhere".into(),
            ..caller.clone()
        }
        .can_amend(&job));

        let solo = store
            .create(
                serde_json::from_value(serde_json::json!({
                    "name": "solo-state-watch",
                    "probe": "status",
                    "question": "state",
                    "buckets": ["done"],
                    "every": "10s"
                }))
                .unwrap(),
                crate::sentinel::store::Owner {
                    creator_tab_id: "creator".into(),
                    team_id: None,
                    workspace_id: "workspace".into(),
                    cwd: "/tmp".into(),
                },
                true,
            )
            .unwrap();
        let formed_team_creator = SentinelCaller {
            team: Some("formed-team".into()),
            ..caller
        };
        assert!(formed_team_creator.owns(&solo));
        assert!(formed_team_creator.can_amend(&solo));
        assert!(!SentinelCaller {
            tab: "worker".into(),
            ..formed_team_creator
        }
        .can_amend(&solo));
    }
    #[test]
    fn sentinel_graph_tab_caller_keeps_current_nonce_and_creator_scope() {
        let (mut mux, _) = crate::multiplexer::state::MultiplexerState::new();
        let ws = mux.snapshot().active_workspace_id.unwrap();
        let creator = mux.create_tab(&ws).unwrap();
        let tab = mux.get_tab_mut(&creator).unwrap();
        tab.cwd = Some("/tmp".into());
        tab.session_binding_nonce = Some("supplied-current-nonce".into());
        assert!(caller_from_mux(&mux, &creator, None).is_err());
        assert!(caller_from_mux(&mux, &creator, Some("stale-nonce")).is_err());
        let caller = caller_from_mux(&mux, &creator, Some("supplied-current-nonce")).unwrap();
        assert_eq!(caller.tab, creator);
        assert_eq!(caller.workspace, ws);
        assert_eq!(caller.team, None);
        assert_eq!(caller.nonce.as_deref(), Some("supplied-current-nonce"));
        assert!(caller_from_mux(&mux, "foreign", Some("supplied-current-nonce")).is_err());
    }

    #[test]
    fn blueprint_pipeline_access_follows_tab_or_team_owner() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = crate::sentinel::store::Store::new(
            dir.path().join("sentinels.json"),
            crate::cron::clock::MockClock::starting_at(1000),
        );
        let legacy = store
            .create(
                serde_json::from_value(serde_json::json!({
                    "name":"owned-pipeline",
                    "probe":"status",
                    "question":"ready?",
                    "buckets":["done"],
                    "every":"10s"
                }))
                .unwrap(),
                crate::sentinel::store::Owner {
                    creator_tab_id: "creator".into(),
                    team_id: Some("team".into()),
                    workspace_id: "workspace".into(),
                    cwd: "/tmp".into(),
                },
                true,
            )
            .unwrap();
        let mut pipeline = legacy;
        pipeline.set_pipeline(Some(codeterm_types::blueprint_pipeline::BlueprintSpec {
            name: "owned-pipeline".into(),
            owner: codeterm_types::blueprint_pipeline::BlueprintOwner::Team,
            trigger: codeterm_types::blueprint_pipeline::Trigger::Schedule {
                every: "10s".into(),
            },
            steps: vec![codeterm_types::blueprint_pipeline::Step {
                name: "emit".into(),
                input: codeterm_types::blueprint_pipeline::IoType::Text,
                kind: codeterm_types::blueprint_pipeline::StepKind::Command {
                    command: "printf ready".into(),
                },
                on: Default::default(),
            }],
            result: Default::default(),
        }));
        let teammate = SentinelCaller {
            tab: "teammate".into(),
            team: Some("team".into()),
            workspace: "workspace".into(),
            cwd: "/tmp".into(),
            nonce: None,
        };
        assert!(teammate.owns(&pipeline));
        assert!(teammate.can_amend(&pipeline));
        assert!(!SentinelCaller {
            team: Some("other-team".into()),
            ..teammate.clone()
        }
        .owns(&pipeline));
    }
}
