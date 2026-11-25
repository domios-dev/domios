use crate::service::MuxEmitCause;
use serde::Deserialize;

use crate::multiplexer::state::MultiplexerState;
use crate::service::transport::AtomicSpawn;
use crate::service::{ServiceContext, SpawnAgentParams};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FreshGeneralSessionRequest {
    pub workspace_id: Option<String>,
    pub provider_id: Option<String>,
    #[serde(rename = "cwd")]
    _cwd: Option<String>,
    pub role: String,
    pub intent: String,
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub preset: Option<String>,
    pub account_id: Option<String>,
}

struct ResolvedFreshGeneralSessionRequest {
    workspace_id: String,
    provider_id: String,
    role: String,
    intent: String,
    model: Option<String>,
    reasoning_effort: Option<String>,
    preset: Option<String>,
    account_id: Option<String>,
}

impl FreshGeneralSessionRequest {
    fn resolve(
        self,
        mux: &MultiplexerState,
        config: &crate::config::schema::GeneralAgentConfig,
    ) -> Result<ResolvedFreshGeneralSessionRequest, String> {
        Ok(ResolvedFreshGeneralSessionRequest {
            workspace_id: self
                .workspace_id
                .or_else(|| super::general_workspace_id(mux))
                .ok_or("General workspace is unavailable")?,
            provider_id: self
                .provider_id
                .filter(|id| !id.trim().is_empty())
                .unwrap_or_else(|| config.agent_provider.clone()),
            role: self.role,
            intent: self.intent,
            model: self.model.or_else(|| config.model.clone()),
            reasoning_effort: self
                .reasoning_effort
                .or_else(|| config.reasoning_effort.clone()),
            preset: self.preset,
            account_id: self.account_id,
        })
    }
}

impl ResolvedFreshGeneralSessionRequest {
    fn account_launch_params(&self) -> SpawnAgentParams {
        SpawnAgentParams {
            agent: Some(self.provider_id.clone()),
            workspace_id: Some(self.workspace_id.clone()),
            account_id: self.account_id.clone(),
            ..Default::default()
        }
    }
}

fn validate_request(
    mux: &MultiplexerState,
    config: &crate::config::schema::GeneralAgentConfig,
    request: &ResolvedFreshGeneralSessionRequest,
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
    if request.provider_id.trim().is_empty() {
        return Err("No provider is configured for General sessions".into());
    }
    Ok(())
}

fn create_fresh_tab(
    mux: &mut MultiplexerState,
    workspace_id: &str,
    chat_view: bool,
    chat_backend_provider: Option<&str>,
    account: &SpawnAgentParams,
) -> Result<String, String> {
    let tab_id = mux.create_tab(workspace_id)?;
    if let Some(tab) = mux.get_tab_mut(&tab_id) {
        super::identity::apply_general_agent_kind(tab);
        let cwd = super::general_dir().to_string_lossy().to_string();
        tab.cwd = Some(cwd.clone());
        tab.provisioned_cwd = Some(cwd);
        tab.chat_view = chat_view || chat_backend_provider.is_some();
        if let Some(plugin_id) = chat_backend_provider {
            tab.pty_id = None;
            tab.backing = crate::multiplexer::types::TabBacking::ChatBackend {
                plugin_id: plugin_id.to_string(),
                session_id: String::new(),
            };
        }
        tab.restorable_launch.account_id = account.account_id.clone();
        tab.restorable_launch.account_namespace = account.account_namespace.clone();
        tab.agent_booting = true;
        crate::service::boot_stage::apply_boot_stage(
            tab,
            if chat_backend_provider.is_some() {
                codeterm_types::pty::BootStage::AgentLaunching
            } else {
                codeterm_types::pty::BootStage::PtyStarting
            },
            None,
        );
    }
    Ok(tab_id)
}

fn published_spawn_result(
    service: &ServiceContext,
    tab_id: String,
    workspace_id: String,
) -> AtomicSpawn<serde_json::Value> {
    service.emit_state_changed(MuxEmitCause::GeneralAgent);
    super::super::mux_emitter::emit_snapshot(service, MuxEmitCause::GeneralAgent.bit());
    AtomicSpawn {
        snapshot: service
            .ext::<std::sync::Arc<super::super::PublishedMuxState>>()
            .read_value(),
        tab_id,
        workspace_id,
        launch_command: None,
        warning: None,
    }
}

impl ServiceContext {
    pub fn spawn_fresh_general_session(
        &self,
        config: &crate::config::ConfigState,
        request: FreshGeneralSessionRequest,
    ) -> Result<AtomicSpawn<serde_json::Value>, String> {
        let general = config.lock().general_agent.clone();
        let mux = self.mux.read();
        let request = request.resolve(&mux, &general)?;
        let mut selection = super::session_contract::FreshLaunchSelection {
            model: request.model.clone(),
            reasoning_effort: request.reasoning_effort.clone(),
            ..Default::default()
        };
        validate_request(&mux, &general, &request)?;
        drop(mux);
        crate::service::execution_sandbox::enforce_delegation_ceiling(
            self,
            &crate::service::SpawnAgentParams::default(),
            None,
        )?;
        self.require_agent_provider_ready(&request.provider_id)?;
        let permission_bypass = crate::service::permission_policy::authorize_permission_bypass(
            &self.agent_registry,
            &request.provider_id,
            true,
        )
        .is_some();
        let chat_view = super::default_chat_view_for_provider(
            &config.lock(),
            &self.agent_registry,
            &request.provider_id,
        );
        if let Some(name) = request.preset.as_deref() {
            let adapter = self
                .agent_registry
                .with_provider(&request.provider_id, |provider| {
                    provider.model_provider_config()
                })
                .flatten()
                .ok_or("provider does not support model presets")?;
            let (model, args) = super::super::model_providers::presets::resolve(
                &*adapter,
                name,
                selection.model.as_deref(),
                &[],
            )
            .map_err(|error| error.to_string())?;
            selection.model = Some(model);
            selection.preset_args = args;
        }
        let (account, invocation, reservation) =
            crate::service::provider_accounts::spawn_params(self, request.account_launch_params())?;
        super::ensure_directories();
        super::state::mark_independent_sessions()?;
        super::instructions::generate_instructions(
            general.capability_hints,
            &request.provider_id,
            Some(&self.mem),
        );
        let chat_backend_provider =
            crate::service::agent_runtime::engine::requested(self, &request.provider_id)
                .then_some(request.provider_id.as_str());
        let gate = self.limit_gate();
        let tab_id = {
            let mut mux = self.mux.write();
            validate_request(&mux, &general, &request)?;
            crate::service::billing::admit_new_tab(&gate, &mux)?;
            create_fresh_tab(
                &mut mux,
                &request.workspace_id,
                chat_view,
                chat_backend_provider,
                &account,
            )?
        };
        super::super::supporter_notify::record_ui_origin(&tab_id);
        if let Err(error) = self.ensure_agent_pad(&tab_id).and_then(|_| {
            super::super::session_persist::write_session_snapshot(self, &self.mux).and_then(
                |outcome| match outcome {
                    crate::session::store::SaveOutcome::Saved => Ok(()),
                    crate::session::store::SaveOutcome::RefusedReduction(refusal) => Err(format!(
                        "desk persistence refused the General session: {refusal:?}"
                    )),
                },
            )
        }) {
            self.rollback_provisional_tab(&request.workspace_id, &tab_id);
            let _ = self.revoke_agent_pad(&tab_id);
            self.emit_state_changed(MuxEmitCause::GeneralAgent);
            return Err(error);
        }
        let service = std::sync::Arc::new(self.clone());
        let config = config.clone();
        let launch_tab_id = tab_id.clone();
        std::thread::spawn(move || {
            let _reservation = reservation;
            codeterm_plugin::provider_account::with_context(invocation, || {
                super::lifecycle::start_fresh_general_session(
                    &service,
                    &config,
                    launch_tab_id,
                    &request.provider_id,
                    permission_bypass,
                    selection,
                );
            });
        });
        Ok(published_spawn_result(self, tab_id, request.workspace_id))
    }
}

#[cfg(test)]
mod tests;
