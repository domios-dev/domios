pub mod delete;
pub mod graph;

use crate::{
    amend, classify, routes,
    scheduler::Handle,
    source,
    store::{Create, Sentinel},
};
use codeterm_service_core::service::ServiceContext;

#[derive(Clone)]
pub struct SentinelCaller {
    pub(super) tab: String,
    pub(super) team: Option<String>,
    pub(super) workspace: String,
    pub(super) cwd: String,
    pub(super) nonce: Option<String>,
}

impl SentinelCaller {
    pub fn owns(&self, job: &Sentinel) -> bool {
        if let Some(pipeline) = &job.pipeline().cloned() {
            return match pipeline.owner {
                codeterm_blueprint_types::blueprint_pipeline::BlueprintOwner::Tab => {
                    job.creator_tab_id == self.tab
                        && (job.team_id == self.team
                            || (job.team_id.is_none() && self.team.is_some()))
                        && job.workspace_id == self.workspace
                }
                codeterm_blueprint_types::blueprint_pipeline::BlueprintOwner::Team => {
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

    pub fn may_list(&self, job: &Sentinel, team_owner: bool) -> bool {
        self.owns(job)
            || (team_owner
                && self.team.is_some()
                && job.team_id == self.team
                && job.workspace_id == self.workspace)
    }

    pub fn may_inspect(&self, job: &Sentinel, all_scopes: bool, team_owner: bool) -> bool {
        all_scopes || self.may_list(job, team_owner)
    }

    pub fn can_amend(&self, job: &Sentinel) -> bool {
        if job.pipeline().is_some() {
            return self.owns(job);
        }
        (job.team_id == self.team
            || (job.team_id.is_none() && self.team.is_some() && job.creator_tab_id == self.tab))
            && job.workspace_id == self.workspace
    }
}

fn caller_from_mux(
    mux: &codeterm_service_core::multiplexer::state::MultiplexerState,
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

pub trait SentinelExt {
    fn blueprint_input(
        &self,
        caller: &SentinelCaller,
        id: &str,
        text: String,
    ) -> Result<serde_json::Value, String>;

    fn sentinel_caller(&self, tab_id: &str, nonce: Option<&str>) -> Result<SentinelCaller, String>;

    fn prepare_sentinel_create(
        &self,
        caller: &SentinelCaller,
        create: Create,
    ) -> Result<Sentinel, String>;

    fn sentinel_history(&self, caller: &SentinelCaller, id: &str)
        -> Result<amend::History, String>;
    fn sentinel_resolve_escalation(
        &self,
        caller: &SentinelCaller,
        id: &str,
        transition_id: &str,
    ) -> Result<Sentinel, String>;
    fn sentinel_list(&self, caller: &SentinelCaller) -> Result<Vec<Sentinel>, String>;
    fn sentinel_quarantined(
        &self,
        caller: &SentinelCaller,
    ) -> Result<Vec<crate::storage::quarantine::Quarantined>, String>;
    fn sentinel_show(&self, caller: &SentinelCaller, id: &str) -> Result<Sentinel, String>;
    fn sentinel_inspect(&self, caller: &SentinelCaller, id: &str) -> Result<Sentinel, String>;
    fn sentinel_read_all_authority(&self, caller: &SentinelCaller) -> Result<bool, String>;
    fn sentinel_stop(&self, caller: &SentinelCaller, id: &str) -> Result<Sentinel, String>;
}

impl SentinelExt for ServiceContext {
    fn blueprint_input(
        &self,
        caller: &SentinelCaller,
        id: &str,
        text: String,
    ) -> Result<serde_json::Value, String> {
        self.authorize_role_blueprint(
            &caller.tab,
            Some(id),
            codeterm_service_core::service::role_policy::BlueprintAction::Input,
        )?;
        let job = self.sentinel_show(caller, id)?;
        crate::plan_lock::refuse(self, &job)?;
        let input = job.input().ok_or("blueprint is not a text input stage")?;
        if input.kind != codeterm_blueprint_types::sentinel_input::MediaKind::Text
            || input.convert_to.is_some()
        {
            return Err(
                "blueprint input delivery supports text without media conversion only".into(),
            );
        }
        let handle = self
            .host_extension::<Handle>()
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

    fn sentinel_caller(&self, tab_id: &str, nonce: Option<&str>) -> Result<SentinelCaller, String> {
        caller_from_mux(&self.mux.read(), tab_id, nonce)
    }

    fn prepare_sentinel_create(
        &self,
        caller: &SentinelCaller,
        create: Create,
    ) -> Result<Sentinel, String> {
        let handle = self
            .host_extension::<Handle>()
            .ok_or("blueprints not initialized")?;
        let connected = create.model_classification
            && create.matches.is_empty()
            && classify::connected_model(self).is_some();
        let mut job = handle.store.lock().prepare(
            create,
            crate::store::Owner {
                creator_tab_id: caller.tab.clone(),
                team_id: caller.team.clone(),
                workspace_id: caller.workspace.clone(),
                cwd: caller.cwd.clone(),
            },
            connected,
        )?;
        crate::watch_input::validate(self, &job)?;
        if let Some(binding) = job.observe().cloned() {
            job.set_observe(Some(source::resolve_binding(self, &job, &binding)?));
        }
        let mut resolved = std::collections::BTreeMap::new();
        for (bucket, handlers) in &job.routes {
            let mut admitted = Vec::new();
            for handler in handlers {
                let action = routes::resolve(self, &job, &handler.action)?;
                let target = graph::target_identity(self, &job, &action)?;
                admitted.push(crate::handlers::Handler {
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
            resolved_other.push(crate::handlers::Handler {
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
            if let Some(crate::source::TabBinding::Tab(tab_id)) = &job.observe().cloned() {
                source::validate_tab_scope(&mux, &job, tab_id)?;
            }
            validate_send_scope(&mux, &job)?;
        }
        Ok(job)
    }

    fn sentinel_history(
        &self,
        caller: &SentinelCaller,
        id: &str,
    ) -> Result<amend::History, String> {
        let handle = self
            .host_extension::<Handle>()
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
                    .wait(crate::storage::COMMIT_DEADLINE)
                    .map_err(|error| error.to_string())?
                    .value;
                let versions = crate::storage::history_versions(records);
                if versions.is_empty() {
                    return Err(format!("blueprint history not found: {id}"));
                }
                Ok(amend::from_storage_history(job.id, versions))
            }
            None => Ok(amend::history(&job)),
        }
    }

    fn sentinel_resolve_escalation(
        &self,
        caller: &SentinelCaller,
        id: &str,
        transition_id: &str,
    ) -> Result<Sentinel, String> {
        self.authorize_role_blueprint(
            &caller.tab,
            Some(id),
            codeterm_service_core::service::role_policy::BlueprintAction::Resolve,
        )?;
        let handle = self
            .host_extension::<Handle>()
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
        if let Err(error) =
            codeterm_service_core::service::supporter::runtime_with_notifications(self)
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

    fn sentinel_quarantined(
        &self,
        caller: &SentinelCaller,
    ) -> Result<Vec<crate::storage::quarantine::Quarantined>, String> {
        let team_owner = list_authority(self, caller)?;
        let handle = self
            .host_extension::<Handle>()
            .ok_or("blueprints not initialized")?;
        let store = handle.store.lock();
        store.ensure_available()?;
        Ok(store
            .quarantined_in(&caller.workspace, caller.team.as_deref())
            .into_iter()
            .filter(|entry| team_owner || entry.creator_tab_id == caller.tab)
            .collect())
    }

    fn sentinel_list(&self, caller: &SentinelCaller) -> Result<Vec<Sentinel>, String> {
        let team_owner = list_authority(self, caller)?;
        let all_scopes = self.sentinel_read_all_authority(caller)?;
        let handle = self
            .host_extension::<Handle>()
            .ok_or("blueprints not initialized")?;
        let store = handle.store.lock();
        store.ensure_available()?;
        let mut jobs: Vec<_> = store
            .list()
            .iter()
            .filter(|job| caller.may_inspect(job, all_scopes, team_owner))
            .cloned()
            .collect();
        drop(store);
        crate::plan_lock::project(self, &mut jobs);
        Ok(jobs)
    }

    fn sentinel_read_all_authority(&self, caller: &SentinelCaller) -> Result<bool, String> {
        let mux = self.mux.read();
        if !caller_is_live(&mux, caller) {
            return Err("blueprint caller scope changed".into());
        }
        Ok(mux.get_tab(&caller.tab).is_some_and(|tab| {
            tab.has_team_orchestrator_authority()
                || codeterm_service_core::service::general_agent::identity::is_general_agent(tab)
        }))
    }

    fn sentinel_inspect(&self, caller: &SentinelCaller, id: &str) -> Result<Sentinel, String> {
        self.sentinel_list(caller)?.into_iter().find(|job| job.id == id)
            .ok_or_else(|| format!("blueprint not found for this reader: {id}"))
    }

    fn sentinel_show(&self, caller: &SentinelCaller, id: &str) -> Result<Sentinel, String> {
        let curator = self.is_curator_tab(&caller.tab);
        let handle = self
            .host_extension::<Handle>()
            .ok_or("blueprints not initialized")?;
        let store = handle.store.lock();
        store.ensure_available()?;
        let job = store
            .get(id)
            .filter(|job| {
                caller.owns(job)
                    || (curator
                        && job.team_id == caller.team
                        && job.workspace_id == caller.workspace)
            })
            .cloned()
            .ok_or_else(|| format!("blueprint not found for this creator and Team: {id}"))?;
        drop(store);
        let mut job = job;
        crate::plan_lock::project(self, std::slice::from_mut(&mut job));
        Ok(job)
    }

    fn sentinel_stop(&self, caller: &SentinelCaller, id: &str) -> Result<Sentinel, String> {
        let curator = self.authorize_role_blueprint(
            &caller.tab,
            Some(id),
            codeterm_service_core::service::role_policy::BlueprintAction::Stop,
        )?;
        let handle = self
            .host_extension::<Handle>()
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
            crate::graph::validate_scope(&mux, &caller.workspace, caller.team.as_deref())?;
            let mut store = handle.store.lock();
            store.ensure_available()?;
            if !store.get(id).is_some_and(|job| {
                caller.owns(job)
                    || (curator
                        && job.team_id == caller.team
                        && job.workspace_id == caller.workspace)
            }) {
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
    mux: &codeterm_service_core::multiplexer::state::MultiplexerState,
    caller: &SentinelCaller,
) -> bool {
    caller_is_live_in_view(mux, caller)
}

pub(super) fn caller_is_live_in_view(
    mux: &dyn crate::graph::MuxView,
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

pub(crate) fn validate_send_scope(
    mux: &codeterm_service_core::multiplexer::state::MultiplexerState,
    job: &Sentinel,
) -> Result<(), String> {
    validate_send_scope_in_view(mux, job)
}

pub(crate) fn validate_send_scope_in_view(
    mux: &dyn crate::graph::MuxView,
    job: &Sentinel,
) -> Result<(), String> {
    for handler in job.routes.values().flatten().chain(&job.on_other) {
        if let crate::machine::Route::Send(id) = &handler.action {
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
pub mod tests;

fn list_authority(service: &ServiceContext, caller: &SentinelCaller) -> Result<bool, String> {
    let mux = service.mux.read();
    if !caller_is_live(&mux, caller) {
        return Err("blueprint caller scope changed".into());
    }
    Ok(mux.get_tab(&caller.tab).is_some_and(|tab| {
        tab.has_team_orchestrator_authority() || tab.role_profile.as_deref() == Some("curator")
    }))
}
