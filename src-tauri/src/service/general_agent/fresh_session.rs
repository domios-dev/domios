use serde::Deserialize;

use crate::multiplexer::state::MultiplexerState;
use crate::service::transport::AtomicSpawn;
use crate::service::ServiceContext;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FreshGeneralSessionRequest {
    pub workspace_id: String,
    pub provider_id: String,
    pub cwd: String,
    pub role: String,
    pub intent: String,
}

fn validate_request(
    mux: &MultiplexerState,
    config: &crate::config::schema::GeneralAgentConfig,
    request: &FreshGeneralSessionRequest,
) -> Result<(), String> {
    if !config.enabled || !config.first_run_complete {
        return Err("Finish Get started before creating another General session".into());
    }
    if request.intent != "fresh_general" || request.role != super::GENERAL_AGENT_KIND {
        return Err("A fresh General session requires General launch intent and role".into());
    }
    let main = mux
        .resolve_tab_id(super::GENERAL_ALIAS)
        .and_then(|id| mux.get_tab(&id));
    if !main.is_some_and(|tab| tab.protected)
        || super::general_workspace_id(mux).as_deref() != Some(request.workspace_id.as_str())
    {
        return Err("Fresh General sessions belong to the General workspace".into());
    }
    if request.provider_id != config.agent_provider
        || crate::paths::canonical_cwd(&request.cwd)
            != crate::paths::canonical_cwd(&super::general_dir().to_string_lossy())
    {
        return Err("General session configuration changed; refresh and try again".into());
    }
    Ok(())
}

fn create_fresh_tab(
    mux: &mut MultiplexerState,
    workspace_id: &str,
    chat_view: bool,
) -> Result<String, String> {
    let tab_id = mux.create_tab(workspace_id)?;
    if let Some(tab) = mux.get_tab_mut(&tab_id) {
        super::identity::apply_general_agent_kind(tab);
        let cwd = super::general_dir().to_string_lossy().to_string();
        tab.cwd = Some(cwd.clone());
        tab.provisioned_cwd = Some(cwd);
        tab.chat_view = chat_view;
        tab.agent_booting = true;
        crate::service::boot_stage::apply_boot_stage(
            tab,
            codeterm_types::pty::BootStage::PtyStarting,
            None,
        );
    }
    Ok(tab_id)
}

impl ServiceContext {
    pub fn spawn_fresh_general_session(
        &self,
        config: &crate::config::ConfigState,
        request: FreshGeneralSessionRequest,
    ) -> Result<AtomicSpawn, String> {
        let general = config.lock().general_agent.clone();
        validate_request(&self.mux.read(), &general, &request)?;
        self.require_agent_provider_ready(&general.agent_provider)?;
        let permission_bypass = crate::service::permission_policy::configured_permission_bypass(
            &self.agent_registry,
            &general.agent_provider,
        )?
        .is_some();
        let chat_view = super::default_chat_view_for_provider(
            &config.lock(),
            &self.agent_registry,
            &general.agent_provider,
        );
        super::ensure_directories();
        super::state::mark_independent_sessions()?;
        super::instructions::generate_instructions(&general.agent_provider, Some(&self.mem));
        let tab_id = {
            let mut mux = self.mux.write();
            validate_request(&mux, &general, &request)?;
            create_fresh_tab(&mut mux, &request.workspace_id, chat_view)?
        };
        super::super::supporter_notify::record_ui_origin(&tab_id);
        if let Err(error) = self.ensure_agent_pad(&tab_id).and_then(|_| {
            super::super::session_persist::write_session_snapshot(&self.app_handle, &self.mux)
                .and_then(|outcome| match outcome {
                    crate::session::store::SaveOutcome::Saved => Ok(()),
                    crate::session::store::SaveOutcome::RefusedReduction(refusal) => Err(format!(
                        "desk persistence refused the General session: {refusal:?}"
                    )),
                })
        }) {
            self.rollback_provisional_tab(&request.workspace_id, &tab_id);
            let _ = self.revoke_agent_pad(&tab_id);
            self.emit_state_changed();
            return Err(error);
        }
        let service = std::sync::Arc::new(self.clone());
        let config = config.clone();
        let launch_tab_id = tab_id.clone();
        std::thread::spawn(move || {
            super::lifecycle::start_fresh_general_session(
                &service,
                &config,
                launch_tab_id,
                &general.agent_provider,
                permission_bypass,
            );
        });
        self.emit_state_changed();
        Ok(AtomicSpawn {
            snapshot: self.mux.read().snapshot(),
            tab_id,
            workspace_id: request.workspace_id,
            launch_command: None,
            warning: None,
        })
    }
}

#[cfg(test)]
mod tests;
