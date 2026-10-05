pub(crate) mod graph;
use tauri::Manager;

use super::ServiceContext;
use crate::sentinel::{
    amend::{self, Update},
    classify, routes,
    scheduler::Handle,
    source,
    store::{Create, Sentinel},
};

#[derive(Clone)]
pub(crate) struct SentinelCaller {
    tab: String,
    team: Option<String>,
    workspace: String,
    cwd: String,
    nonce: Option<String>,
}

impl SentinelCaller {
    fn owns(&self, job: &Sentinel) -> bool {
        job.creator_tab_id == self.tab
            && (job.team_id == self.team || (job.team_id.is_none() && self.team.is_some()))
            && job.workspace_id == self.workspace
    }

    fn can_amend(&self, job: &Sentinel) -> bool {
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
    let tab = mux.get_tab(tab_id).ok_or("sentinel caller tab not found")?;
    if tab.session_binding_nonce.is_some() && tab.session_binding_nonce.as_deref() != nonce {
        return Err("sentinel requires the calling tab's current session binding nonce".into());
    }
    let workspace = mux
        .find_workspace_for_tab(tab_id)
        .ok_or("sentinel caller workspace not found")?;
    let cwd = tab
        .cwd
        .clone()
        .or_else(|| tab.provisioned_cwd.clone())
        .filter(|cwd| !cwd.is_empty())
        .ok_or("sentinel creator cwd is unavailable")?;
    if tab
        .team_id
        .as_ref()
        .is_some_and(|id| mux.get_team(id).is_none())
    {
        return Err("sentinel creator Team is gone".into());
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
        let handle = self
            .app_handle
            .try_state::<Handle>()
            .ok_or("sentinels not initialized")?;
        let connected = classify::connected_model(self).is_some();
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
        if let Some(binding) = job.observe.clone() {
            job.observe = Some(source::resolve_binding(self, &job, &binding)?);
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
        let staged = {
            let mux = self.mux.read();
            let live = mux.get_tab(&caller.tab).is_some_and(|tab| {
                tab.team_id == caller.team && tab.session_binding_nonce == caller.nonce
            }) && mux.find_workspace_for_tab(&caller.tab).as_deref()
                == Some(&caller.workspace)
                && caller
                    .team
                    .as_ref()
                    .is_none_or(|id| mux.get_team(id).is_some());
            if !live {
                return Err("sentinel creator or Team is gone".into());
            }
            if let Some(crate::sentinel::source::TabBinding::Tab(tab_id)) = &job.observe {
                source::validate_tab_scope(&mux, &job, tab_id)?;
            }
            for handler in job.routes.values().flatten() {
                if let crate::sentinel::machine::Route::Send(id) = &handler.action {
                    let target = mux.get_tab(id).ok_or("sentinel send target disappeared")?;
                    if target.agent_type.is_none()
                        || target.team_id != job.team_id
                        || mux.find_workspace_for_tab(id).as_deref() != Some(&job.workspace_id)
                    {
                        return Err("sentinel send target scope changed".into());
                    }
                }
            }
            let tab = mux.get_tab(&caller.tab).ok_or("sentinel creator gone")?;
            job.agent_authored = tab.agent_type.is_some() && tab.session_binding_nonce.is_some();
            handle.store.lock().stage_insert(job.clone())
        };
        staged.settle(&handle.store)?;
        handle.notify();
        Ok(job)
    }

    pub(crate) fn sentinel_update(
        &self,
        caller: &SentinelCaller,
        id: &str,
        expect_version: u64,
        update: Update,
    ) -> Result<Sentinel, String> {
        let handle = self
            .app_handle
            .try_state::<Handle>()
            .ok_or("sentinels not initialized")?;
        let current = {
            let store = handle.store.lock();
            store.ensure_available()?;
            store
                .get(id)
                .filter(|job| caller.can_amend(job))
                .cloned()
                .ok_or_else(|| format!("sentinel not found in this workspace and Team: {id}"))?
        };
        if current.definition_version != expect_version {
            return Err(amend::version_conflict(current.definition_version));
        }
        let connected = classify::connected_model(self).is_some();
        let mut update = update.prepare(&current, &caller.tab, connected)?;
        let note = update.note.clone();
        let cause = update.cause_transition_id.clone();
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
        let authored_at = handle.store.lock().clock.now_ms();
        let updated = {
            let mux = self.mux.read();
            if !caller_is_live(&mux, caller) {
                return Err("sentinel caller scope changed".into());
            }
            for handler in update
                .job
                .routes
                .values()
                .flatten()
                .chain(&update.job.on_other)
            {
                if let crate::sentinel::machine::Route::Send(id) = &handler.action {
                    let target = mux.get_tab(id).ok_or("sentinel send target disappeared")?;
                    if target.agent_type.is_none()
                        || target.team_id != update.job.team_id
                        || mux.find_workspace_for_tab(id).as_deref()
                            != Some(&update.job.workspace_id)
                    {
                        return Err("sentinel send target scope changed".into());
                    }
                }
            }
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
        if let Err(error) = crate::service::supporter::runtime_with_notifications(self)
            .sentinel_event(serde_json::json!({
                "time": chrono::Utc::now().timestamp(),
                "kind": "sentinel",
                "sentinel_id": &updated.id,
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
            log::error!("sentinel definition update journal failed: {error}");
        }
        Ok(updated)
    }

    pub(crate) fn sentinel_history(
        &self,
        caller: &SentinelCaller,
        id: &str,
    ) -> Result<amend::History, String> {
        let handle = self
            .app_handle
            .try_state::<Handle>()
            .ok_or("sentinels not initialized")?;
        if !caller_is_live(&self.mux.read(), caller) {
            return Err("sentinel caller scope changed".into());
        }
        let (job, typed_history) = {
            let store = handle.store.lock();
            store.ensure_available()?;
            let job = store
                .get(id)
                .filter(|job| caller.can_amend(job))
                .cloned()
                .ok_or_else(|| format!("sentinel not found in this workspace and Team: {id}"))?;
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
                    return Err(format!("sentinel history not found: {id}"));
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
            .ok_or("sentinels not initialized")?;
        let staged = {
            let mux = self.mux.read();
            if !caller_is_live(&mux, caller) {
                return Err("sentinel caller scope changed".into());
            }
            let mut store = handle.store.lock();
            store.ensure_available()?;
            if !store.get(id).is_some_and(|job| caller.can_amend(job)) {
                return Err(format!(
                    "sentinel not found in this workspace and Team: {id}"
                ));
            }
            store.stage_resolve_escalation(id, transition_id)
        };
        if !staged.settle(&handle.store)? {
            return Err("sentinel escalation resolution was not applied".into());
        }
        handle.store.lock().wake_stage(id);
        handle.notify();
        let resolved = handle
            .store
            .lock()
            .get(id)
            .cloned()
            .ok_or_else(|| format!("sentinel not found: {id}"))?;
        if let Err(error) = crate::service::supporter::runtime_with_notifications(self)
            .sentinel_event(serde_json::json!({
                "time": chrono::Utc::now().timestamp(),
                "kind": "sentinel",
                "sentinel_id": id,
                "transition_id": transition_id,
                "tab": caller.tab,
                "team_id": caller.team,
                "workspace_id": caller.workspace,
                "action": "escalation_resolved"
            }))
        {
            log::error!("sentinel escalation resolution journal failed: {error}");
        }
        Ok(resolved)
    }

    pub(crate) fn sentinel_list(&self, caller: &SentinelCaller) -> Result<Vec<Sentinel>, String> {
        let handle = self
            .app_handle
            .try_state::<Handle>()
            .ok_or("sentinels not initialized")?;
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
            .ok_or("sentinels not initialized")?;
        let store = handle.store.lock();
        store.ensure_available()?;
        let job = store
            .get(id)
            .filter(|job| caller.owns(job))
            .cloned()
            .ok_or_else(|| format!("sentinel not found for this creator and Team: {id}"))?;
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
            .ok_or("sentinels not initialized")?;
        let staged = {
            let mux = self.mux.read();
            let live = mux.get_tab(&caller.tab).is_some_and(|tab| {
                tab.team_id == caller.team && tab.session_binding_nonce == caller.nonce
            }) && mux.find_workspace_for_tab(&caller.tab).as_deref()
                == Some(&caller.workspace);
            if !live {
                return Err("sentinel caller scope changed".into());
            }
            crate::sentinel::graph::validate_scope(
                &mux,
                &caller.workspace,
                caller.team.as_deref(),
            )?;
            let mut store = handle.store.lock();
            store.ensure_available()?;
            if !store.get(id).is_some_and(|job| caller.owns(job)) {
                return Err("sentinel not found for this creator and Team".into());
            }
            store.stage_stop(id, "stopped_by_creator")
        };
        let result = staged.settle(&handle.store);
        handle.notify();
        result
    }
}

fn caller_is_live(
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
            name: None,
            probe: Some("status".into()),
            observe: None,
            question: "state".into(),
            buckets: vec!["done".into()],
            every: "10s".into(),
            routes: Default::default(),
            matches: Default::default(),
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
}
