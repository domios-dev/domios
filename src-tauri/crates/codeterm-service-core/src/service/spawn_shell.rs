//! Shell-tab spawning: visible Team terminals and hidden utility shells.

use super::spawn::{resolve_spawn_workspace_id, SpawnTabSpec, TabHandle};
use super::ServiceContext;
use crate::service::MuxEmitCause;

pub(crate) fn apply_team_shell_path(env: &mut std::collections::HashMap<String, String>) {
    #[cfg(windows)]
    {
        let profiles = crate::pty::shell_profiles::catalog().load_blocking(false);
        let inherited = std::env::var_os("PATH").unwrap_or_default();
        if let Some(path) = crate::pty::shell_profiles::path_with_git_bash(&profiles, &inherited) {
            env.insert("PATH".into(), path.to_string_lossy().into_owned());
        }
    }
    #[cfg(not(windows))]
    let _ = env;
}

impl ServiceContext {
    pub(crate) fn shell_job_policy_for_tab(
        &self,
        tab_id: &str,
    ) -> crate::pty::manager::WindowsJobPolicy {
        shell_job_policy_for_mux(&self.mux.read(), tab_id)
    }
}

fn shell_job_policy_for_mux(
    mux: &crate::multiplexer::state::MultiplexerState,
    tab_id: &str,
) -> crate::pty::manager::WindowsJobPolicy {
    let Some(tab) = mux.get_tab(tab_id) else {
        return crate::pty::manager::WindowsJobPolicy::Contained;
    };
    let team_member = tab.team_id.as_deref().is_some_and(|team_id| {
        mux.team_workspace(team_id).is_some()
            && mux
                .team_member_tab_ids(team_id)
                .iter()
                .any(|id| id == tab_id)
    });
    crate::pty::manager::shell_job_policy(team_member, tab.agent_type.is_some(), tab.purpose)
}

impl ServiceContext {
    pub(crate) fn resolve_shell_profile(
        &self,
        requested: Option<&str>,
    ) -> Result<Option<crate::pty::shell_profiles::ShellProfile>, String> {
        let default = self.configuration().terminal.default_shell;
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
            let mux = self.mux.read();
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
        let team_membership = super::team_membership::resolve_inherited_team_attachment(
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
            &super::utility_tabs::configured_limits(self.ext::<crate::config::ConfigState>()),
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
            self.restart_shell_tab(tab_id, resolved_cwd.clone(), None)?;
        }
        let gate = self.limit_gate();
        let tab_id = {
            let mut mux = self.mux.write();
            let result = match &reused {
                Some(id) => id.clone(),
                None => {
                    super::billing::admit_new_tab(&gate, &mux)?;
                    let tab_id = mux.create_tab(&ws_id)?;
                    crate::startup::boot_timing::bind_request(&tab_id);
                    tab_id
                }
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
                super::team_membership::apply_team_attachment_under(
                    &mut mux,
                    attachment,
                    &result,
                    super::team_membership::utility_shell_parent(Some(&attachment.caller_tab_id)),
                )?;
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
        self.emit_state_changed(MuxEmitCause::TabLifecycle);

        let pty_id = if spec.eager || spec.cmd.is_some() || reused.is_some() {
            self.ensure_tab_pty_for_send(&tab_id)?;
            self.mux
                .read()
                .get_tab(&tab_id)
                .and_then(|p| p.pty_id.clone())
        } else {
            None
        };

        let command_submission = if let Some(command) = spec.cmd.as_deref() {
            self.prepare_utility_command(&tab_id, command);
            let exact_pty = self.resolve_pty_id(&tab_id)?;
            match self.submit_shell_command_with_receipt(&tab_id, &exact_pty, command) {
                Ok(submission) => Some(submission),
                Err(error) => {
                    if let Some(tab) = self.mux.write().get_tab_mut(&tab_id) {
                        tab.utility_command_pending = false;
                    }
                    return Err(format!("Tab {tab_id} command failed: {error}"));
                }
            }
        } else { None };
        if reused.is_none() {
            self.record_kpi_tab_created(
                &tab_id,
                crate::agent::kpi_ledger::TabCreatedKind::Terminal,
            );
        }
        let budget_notice = self.workspace_tab_budget_notice(&ws_id);
        Ok(TabHandle {
            command_submission,
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
            let mux = self.mux.read();
            spec.workspace_id
                .clone()
                .or_else(|| mux.snapshot().active_workspace_id.clone())
                .or_else(|| mux.list_workspaces().first().map(|w| w.id.clone()))
                .ok_or("No workspace for hidden shell")?
        };
        let team_membership = super::team_membership::resolve_inherited_team_attachment(
            &self.mux.read(),
            spec.team_id.as_deref(),
            spec.creator_tab_id.as_deref(),
            &ws_id,
            spec.detach_from_team,
        )?;
        let ws_cwd = {
            let mux = self.mux.read();
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

        let gate = self.limit_gate();
        let tab_id = {
            let mut mux = self.mux.write();
            super::billing::admit_new_tab(&gate, &mux)?;
            let result = mux.create_tab(&ws_id)?;
            crate::startup::boot_timing::bind_request(&result);
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
            if let Some(attachment) = &team_membership {
                super::team_membership::apply_team_attachment_under(
                    &mut mux,
                    attachment,
                    &result,
                    super::team_membership::utility_shell_parent(Some(&attachment.caller_tab_id)),
                )?;
            }
            result
        };

        let job_policy = self.shell_job_policy_for_tab(&tab_id);
        let env_vars = self.tab_env(&tab_id);

        let pty_id = match self.link_hidden_shell_pty_with_builder(&tab_id, |gate| {
            crate::pty::manager::build_shell_session_with_policy(
                gate,
                None,
                resolved_cwd,
                120,
                40,
                env_vars,
                profile.as_ref(),
                job_policy,
            )
        }) {
            Ok(id) => id,
            Err(error) => {
                let _ = self.revoke_agent_pad(&tab_id);
                self.rollback_provisional_tab(&ws_id, &tab_id);
                return Err(error);
            }
        };

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
            command_submission: None,
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

    fn link_hidden_shell_pty_with_builder(
        &self,
        tab_id: &str,
        build: impl FnOnce(
            crate::pty::manager::PtySpawnGate,
        ) -> Result<crate::pty::manager::BuiltSession, String>,
    ) -> Result<String, String> {
        let claim = self.pty_ownership.acquire(tab_id);
        if let Ok(pty_id) = self.resolve_pty_id(tab_id) {
            return Ok(pty_id);
        }
        if self.mux.read().get_tab(tab_id).is_none() {
            return Err(format!("Tab {tab_id} closed before PTY build"));
        }
        let gate = self.pty.lock().spawn_gate();
        let built = build(gate)?;
        let candidate = self.pty.lock().register_session(None, built).0;
        let selection = self.link_pty_with_claim(&claim, tab_id, candidate, false);
        if !selection.linked {
            return Err(format!("Tab {tab_id} closed before PTY link"));
        }
        Ok(selection.pty_id)
    }
}

#[cfg(test)]
#[path = "spawn_shell/tests.rs"]
mod tests;
