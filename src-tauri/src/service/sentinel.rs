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
}

impl SentinelCaller {
    fn owns(&self, job: &Sentinel) -> bool {
        job.creator_tab_id == self.tab
            && job.team_id == self.team
            && job.workspace_id == self.workspace
    }
}

impl ServiceContext {
    pub(crate) fn sentinel_caller(
        &self,
        tab_id: &str,
        nonce: Option<&str>,
    ) -> Result<SentinelCaller, String> {
        let mux = self.mux.read();
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
        })
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
        job.routes = job
            .routes
            .iter()
            .map(|(bucket, route)| {
                routes::resolve(self, &job, route).map(|route| (bucket.clone(), route))
            })
            .collect::<Result<_, _>>()?;
        {
            let mux = self.mux.read();
            let live = mux
                .get_tab(&caller.tab)
                .is_some_and(|tab| tab.team_id == caller.team)
                && mux.find_workspace_for_tab(&caller.tab).as_deref() == Some(&caller.workspace)
                && caller
                    .team
                    .as_ref()
                    .is_none_or(|id| mux.get_team(id).is_some());
            if !live {
                return Err("sentinel creator or Team is gone".into());
            }
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
        let jobs = handle
            .store
            .lock()
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
        let job = handle
            .store
            .lock()
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
        self.sentinel_show(caller, id)?;
        let handle = self
            .app_handle
            .try_state::<Handle>()
            .ok_or("sentinels not initialized")?;
        let result = handle.store.lock().stop(id, "stopped_by_creator");
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
            probe: "status".into(),
            question: "state".into(),
            buckets: vec!["done".into()],
            every: "10s".into(),
            routes: Default::default(),
            matches: Default::default(),
            until: None,
            timeout: None,
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
}
