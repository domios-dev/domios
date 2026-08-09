//! Shell-tab spawning: visible Team terminals and hidden utility shells.

use super::spawn::{resolve_spawn_workspace_id, SpawnTabSpec, TabHandle};
use super::ServiceContext;

impl ServiceContext {
    pub(crate) fn spawn_visible_shell_impl(&self, spec: SpawnTabSpec) -> Result<TabHandle, String> {
        let (ws_id, ws_cwd) = {
            let mux = self.mux.write();
            let id = resolve_spawn_workspace_id(
                spec.workspace_id.clone(),
                spec.parent_tab_id.as_deref(),
                |pid| mux.find_workspace_for_tab(pid),
                mux.get_active_workspace().map(|w| w.id.clone()),
                mux.list_workspaces().first().map(|w| w.id.clone()),
            )?;
            let cwd = mux.get_workspace(&id).and_then(|ws| ws.cwd.clone());
            (id, cwd)
        };
        let team_membership = {
            let mux = self.mux.read();
            match (&spec.team_id, &spec.creator_tab_id) {
                (None, None) => None,
                (Some(team_id), Some(creator_tab_id)) => {
                    let creator = mux
                        .get_tab(creator_tab_id)
                        .ok_or_else(|| format!("Creator Tab {creator_tab_id} not found"))?;
                    if creator.team_id.as_deref() != Some(team_id.as_str()) {
                        return Err(format!(
                            "Creator Tab {creator_tab_id} does not belong to Team {team_id}"
                        ));
                    }
                    let team_workspace = mux
                        .team_workspace(team_id)
                        .ok_or_else(|| format!("Team {team_id} not found"))?;
                    if team_workspace != ws_id {
                        return Err(format!(
                            "Team {team_id} belongs to workspace {team_workspace}, not {ws_id}"
                        ));
                    }
                    let manager_tab_id = mux
                        .snapshot()
                        .tabs
                        .values()
                        .find(|tab| {
                            tab.team_id.as_deref() == Some(team_id.as_str()) && tab.anchors_team()
                        })
                        .map(|tab| tab.id.clone())
                        .ok_or_else(|| format!("Team {team_id} has no live Orchestrator"))?;
                    Some((team_id.clone(), creator_tab_id.clone(), manager_tab_id))
                }
                _ => {
                    return Err(
                        "team_id and creator_tab_id are required together for a Team terminal"
                            .into(),
                    )
                }
            }
        };
        let (resolved_cwd, warning) = Self::resolve_agent_cwd(&spec.cwd, &ws_cwd);

        let tab_id = {
            let mut mux = self.mux.write();
            let result = mux.create_tab(&ws_id)?;
            if let Some(tab) = mux.get_tab_mut(&result) {
                tab.audience = spec.audience;
                if let Some((team_id, creator_tab_id, manager_tab_id)) = &team_membership {
                    tab.team_id = Some(team_id.clone());
                    tab.spawned_by_tab_id = Some(creator_tab_id.clone());
                    tab.parent_tab_id = Some(manager_tab_id.clone());
                    tab.depth = 2;
                }
                if let Some(ref dir) = resolved_cwd {
                    tab.cwd = Some(dir.clone());
                }
            }
            if let Some(ref description) = spec.description {
                let _ = mux.set_tab_description(&result, Some(description.clone()));
            }
            if let Some(ref title) = spec.title {
                if let Err(e) = mux.rename_tab(&result, title) {
                    log::warn!("[spawn.shell] rename_tab failed: {}", e);
                }
            }
            result
        };
        self.emit_state_changed();

        let pty_id = if spec.eager {
            self.ensure_tab_pty_for_send(&tab_id)?;
            self.mux
                .write()
                .get_tab(&tab_id)
                .and_then(|p| p.pty_id.clone())
        } else {
            None
        };

        let budget_notice = self.workspace_tab_budget_notice(&ws_id);
        Ok(TabHandle {
            tab_id,
            pty_id,
            workspace_id: ws_id,
            description: spec.description,
            worker_id: None,
            warning,
            model_fallback: None,
            budget_notice,
        })
    }

    pub(crate) fn spawn_hidden_shell_impl(&self, spec: SpawnTabSpec) -> Result<TabHandle, String> {
        use std::collections::HashMap;

        let ws_id = {
            let mux = self.mux.write();
            spec.workspace_id
                .clone()
                .or_else(|| mux.snapshot().active_workspace_id.clone())
                .or_else(|| mux.list_workspaces().first().map(|w| w.id.clone()))
                .ok_or("No workspace for hidden shell")?
        };
        let ws_cwd = {
            let mux = self.mux.write();
            mux.get_workspace(&ws_id).and_then(|ws| ws.cwd.clone())
        };
        let resolved_cwd = spec.cwd.clone().or(ws_cwd);

        let tab_id = {
            let mut mux = self.mux.write();
            let result = mux.create_tab(&ws_id)?;
            if let Some(tab) = mux.get_tab_mut(&result) {
                tab.hidden = true;
                tab.show_in_tabbar = false;
                tab.audience = spec.audience;
                if let Some(ref dir) = resolved_cwd {
                    tab.cwd = Some(dir.clone());
                }
            }
            result
        };

        let mut env_vars: HashMap<String, String> = HashMap::new();
        env_vars.insert("CODETERM_TAB_ID".into(), tab_id.clone());
        env_vars.insert(
            "CODETERM_SESSION_BINDING_NONCE".into(),
            self.ensure_session_binding_nonce(&tab_id),
        );
        env_vars.insert(
            "CODETERM_SOCKET_PATH".into(),
            crate::socket::server::socket_display_path(),
        );
        let api_cfg = crate::config::load_config().api;
        let (api_url, api_port) = crate::config::api_callback_url(&api_cfg);
        env_vars.insert("CODETERM_API_URL".into(), api_url);
        env_vars.insert("CODETERM_API_PORT".into(), api_port);
        if let Some(ref tok) = api_cfg.token {
            env_vars.insert("CODETERM_API_TOKEN".into(), tok.clone());
        }

        let pty_id = {
            let mut pty = self.pty.lock();
            match pty.spawn(None, resolved_cwd, 120, 40, env_vars) {
                Ok((id, _)) => id,
                Err(error) => {
                    drop(pty);
                    let _ = self.revoke_agent_pad(&tab_id);
                    self.rollback_provisional_tab(&ws_id, &tab_id);
                    return Err(error);
                }
            }
        };
        if let Some(orphan) = self.mux.write().link_pty(&tab_id, pty_id.clone()) {
            self.kill_ptys(&[orphan]);
        }
        crate::service::orchestrator::tab_relink_signal::notify_tab_relinked();
        self.emit_state_changed();

        let svc_reader = std::sync::Arc::new(self.clone());
        let pty_reader = pty_id.clone();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("backend reader runtime");
            rt.block_on(async {
                match crate::service::pty_lifecycle::start_backend_reader(
                    &svc_reader,
                    &pty_reader,
                    true,
                ) {
                    Ok(handle) => {
                        let _ = handle.await;
                    }
                    Err(e) => log::warn!("[spawn.hidden_shell] backend reader: {}", e),
                }
            });
        });

        let budget_notice = self.workspace_tab_budget_notice(&ws_id);
        Ok(TabHandle {
            tab_id,
            pty_id: Some(pty_id),
            workspace_id: ws_id,
            description: spec.description,
            worker_id: None,
            warning: None,
            model_fallback: None,
            budget_notice,
        })
    }
}
