pub(crate) mod graph;
use tauri::Manager;

use super::ServiceContext;
use crate::sentinel::{
    classify, routes,
    scheduler::Handle,
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
            && job.team_id == self.team
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
        {
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
            handle.store.lock().insert(job.clone())?;
        }
        handle.notify();
        Ok(job)
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
        let result = {
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
            store.stop(id, "stopped_by_creator")
        };
        handle.notify();
        result
    }
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
            probe: "status".into(),
            question: "state".into(),
            buckets: vec!["done".into()],
            every: "10s".into(),
            routes: Default::default(),
            matches: Default::default(),
            until: None,
            timeout: None,
            paused: false,
            handlers: None,
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
