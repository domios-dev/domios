//! Shell-tab spawning: visible Team terminals and hidden utility shells.

use super::spawn::{resolve_spawn_workspace_id, SpawnTabSpec, TabHandle};
use super::ServiceContext;

impl ServiceContext {
    pub(crate) fn resolve_shell_profile(
        &self,
        requested: Option<&str>,
    ) -> Result<Option<crate::pty::shell_profiles::ShellProfile>, String> {
        let default = crate::config::load_config().terminal.default_shell;
        let Some(id) = requested.or(default.as_deref()).filter(|id| !id.is_empty()) else {
            return Ok(None);
        };
        let profiles = crate::pty::shell_profiles::catalog().load_blocking(false);
        crate::pty::shell_profiles::select(&profiles, Some(id), None)
    }

    pub(crate) fn spawn_visible_shell_impl(&self, spec: SpawnTabSpec) -> Result<TabHandle, String> {
        if let Some(timeout) = spec.timeout.as_deref() {
            super::utility_deadline_model::parse_timeout(timeout)?;
            let owner = spec
                .creator_tab_id
                .as_deref()
                .ok_or("timeout requires an owning agent")?;
            if !self
                .mux
                .read()
                .get_tab(owner)
                .is_some_and(|tab| tab.agent_type.is_some())
            {
                return Err("timeout requires a live owning agent".into());
            }
        }
        let profile = self.resolve_shell_profile(spec.shell.as_deref())?;
        let _spawn_guard = super::utility_tabs::SPAWN_LOCK.lock();
        let (ws_id, ws_cwd) = {
            let mux = self.mux.write();
            let id = resolve_spawn_workspace_id(
                spec.workspace_id.clone(),
                spec.parent_tab_id.as_deref(),
                spec.creator_tab_id.as_deref(),
                |pid| mux.find_workspace_for_tab(pid),
                mux.get_active_workspace().map(|w| w.id.clone()),
                mux.list_workspaces().first().map(|w| w.id.clone()),
            )?;
            let cwd = mux.get_workspace(&id).and_then(|ws| ws.cwd.clone());
            (id, cwd)
        };
        let team_membership = super::team::resolve_inherited_team_attachment(
            &self.mux.read(),
            spec.team_id.as_deref(),
            spec.creator_tab_id.as_deref(),
            &ws_id,
            spec.detach_from_team,
        )?;
        let (resolved_cwd, warning) = if let Some(profile) = profile
            .as_ref()
            .filter(|profile| profile.wsl_distro.is_some())
        {
            let cwd = spec
                .cwd
                .as_ref()
                .or(ws_cwd.as_ref())
                .map(|cwd| {
                    if cwd.starts_with('/') {
                        profile.host_cwd(cwd)
                    } else {
                        Ok(Self::normalize_resolved_agent_cwd(cwd))
                    }
                })
                .transpose()?;
            profile.launch(cwd.as_deref())?;
            (cwd, None)
        } else {
            Self::resolve_agent_cwd(&spec.cwd, &ws_cwd)
        };

        let limit = super::utility_tabs::limit_for(
            &super::utility_tabs::configured_limits(&self.app_handle),
            spec.purpose,
        );
        let reused = if let Some(attachment) = &team_membership {
            let mux = self.mux.read();
            super::utility_tabs::select_slot(
                mux.team_member_tab_ids(&attachment.team_id)
                    .iter()
                    .filter_map(|id| mux.get_tab(id)),
                &attachment.team_id,
                spec.purpose,
                limit,
            )?
        } else {
            None
        };
        if let Some(tab_id) = reused.as_deref() {
            if let Some(tab) = self.mux.write().get_tab_mut(tab_id) {
                tab.shell_profile = profile.clone();
            }
            self.restart_shell_tab(tab_id, resolved_cwd.clone())?;
        }
        let tab_id = {
            let mut mux = self.mux.write();
            let result = match &reused {
                Some(id) => id.clone(),
                None => mux.create_tab(&ws_id)?,
            };
            if let Some(tab) = mux.get_tab_mut(&result) {
                tab.shell_profile = profile.clone();
                tab.audience = spec.audience;
                tab.purpose = spec.purpose;
                tab.close_restore = Some(crate::multiplexer::types::CloseRestoreTabState {
                    utility_owner_tab_id: spec.creator_tab_id.clone(),
                    command: spec.cmd.clone(),
                    ..Default::default()
                });
                tab.icon = spec.icon.clone();
                if let Some(ref dir) = resolved_cwd {
                    tab.cwd = Some(dir.clone());
                }
            }
            let _ = mux.set_tab_description(&result, spec.description.clone());
            if let Some(attachment) = &team_membership {
                super::team::apply_team_attachment(&mut mux, attachment, &result)?;
            }
            if let Some(title) = spec.title.as_deref().filter(|_| reused.is_none()) {
                if let Err(e) = mux.rename_tab(&result, title) {
                    log::warn!("[spawn.shell] rename_tab failed: {}", e);
                }
            }
            result
        };
        if let Some(timeout) = spec.timeout.as_deref() {
            if let Err(error) = self.set_utility_timeout(
                &tab_id,
                spec.creator_tab_id.as_deref().unwrap_or_default(),
                timeout,
            ) {
                let _ = self.close_tab(&tab_id);
                return Err(error);
            }
        } else if reused.is_some() {
            self.cancel_utility_timeout(&tab_id)?;
        }
        self.emit_state_changed();

        let pty_id = if spec.eager || spec.cmd.is_some() || reused.is_some() {
            self.ensure_tab_pty_for_send(&tab_id)?;
            self.mux
                .write()
                .get_tab(&tab_id)
                .and_then(|p| p.pty_id.clone())
        } else {
            None
        };

        if let Some(command) = spec.cmd.as_deref() {
            self.prepare_utility_command(&tab_id, command);
            if let Err(error) = self.send_shell_cmd(&tab_id, command) {
                if let Some(tab) = self.mux.write().get_tab_mut(&tab_id) {
                    tab.utility_command_pending = false;
                }
                return Err(format!("Tab {tab_id} command failed: {error}"));
            }
        }
        if reused.is_none() {
            self.record_kpi_tab_created(
                &tab_id,
                crate::agent::kpi_ledger::TabCreatedKind::Terminal,
            );
        }
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
        let profile = self.resolve_shell_profile(spec.shell.as_deref())?;

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
        let resolved_cwd = spec
            .cwd
            .clone()
            .or(ws_cwd)
            .map(|cwd| Self::normalize_resolved_agent_cwd(&cwd));
        let resolved_cwd = if let Some(profile) = profile
            .as_ref()
            .filter(|profile| profile.wsl_distro.is_some())
        {
            let cwd = resolved_cwd
                .map(|cwd| {
                    if cwd.starts_with('/') {
                        profile.host_cwd(&cwd)
                    } else {
                        Ok(cwd)
                    }
                })
                .transpose()?;
            profile.launch(cwd.as_deref())?;
            cwd
        } else {
            resolved_cwd
        };

        let tab_id = {
            let mut mux = self.mux.write();
            let result = mux.create_tab(&ws_id)?;
            if let Some(tab) = mux.get_tab_mut(&result) {
                tab.hidden = true;
                tab.show_in_tabbar = false;
                tab.shell_profile = profile.clone();
                tab.audience = spec.audience;
                tab.purpose = spec.purpose;
                if let Some(ref dir) = resolved_cwd {
                    tab.cwd = Some(dir.clone());
                }
            }
            result
        };

        let env_vars = self.tab_env(&tab_id);

        let pty_id = {
            let mut pty = self.pty.lock();
            let built = crate::pty::manager::build_session_with_profile(
                pty.spawn_gate(),
                None,
                resolved_cwd,
                120,
                40,
                env_vars,
                profile.as_ref(),
            );
            match built.map(|built| pty.register_session(None, built)) {
                Ok((id, _)) => id,
                Err(error) => {
                    drop(pty);
                    let _ = self.revoke_agent_pad(&tab_id);
                    self.rollback_provisional_tab(&ws_id, &tab_id);
                    return Err(error);
                }
            }
        };
        let orphan = self.mux.write().link_pty(&tab_id, pty_id.clone());
        if let Some(orphan) = orphan {
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

        self.record_kpi_tab_created(&tab_id, crate::agent::kpi_ledger::TabCreatedKind::Terminal);
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
